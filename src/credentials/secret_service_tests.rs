use super::*;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
#[derive(Default)]
struct State {
    values: Mutex<HashMap<String, Vec<u8>>>,
    locked: AtomicBool,
    content_type: Mutex<String>,
    invalid_parameters: AtomicBool,
    corrupt_read: AtomicBool,
}
struct Service(Arc<State>);
struct Collection(Arc<State>);
struct Item {
    key: String,
    state: Arc<State>,
}
fn object(path: &str) -> OwnedObjectPath {
    OwnedObjectPath::try_from(path).unwrap()
}
#[zbus::dbus_interface(name = "org.freedesktop.Secret.Service")]
impl Service {
    fn open_session(&self, algorithm: &str, input: OwnedValue) -> (OwnedValue, OwnedObjectPath) {
        assert_eq!(algorithm, "plain");
        assert_eq!(String::try_from(input).unwrap(), "");
        (
            OwnedValue::from(zbus::zvariant::Str::from("")),
            object("/session/test"),
        )
    }
    fn read_alias(&self, alias: &str) -> OwnedObjectPath {
        assert_eq!(alias, "default");
        object("/collection/test")
    }
    fn search_items(
        &self,
        attributes: HashMap<String, String>,
    ) -> (Vec<OwnedObjectPath>, Vec<OwnedObjectPath>) {
        assert_eq!(
            attributes.get("application").map(String::as_str),
            Some("rocks.tidalforces.Player")
        );
        let key = &attributes["entry"];
        let found = if self.0.values.lock().unwrap().contains_key(key) {
            vec![object(&format!("/item/k{key}"))]
        } else {
            vec![]
        };
        if self.0.locked.load(Ordering::Relaxed) {
            (vec![], found)
        } else {
            (found, vec![])
        }
    }
}
#[zbus::dbus_interface(name = "org.freedesktop.Secret.Collection")]
impl Collection {
    #[dbus_interface(property)]
    fn locked(&self) -> bool {
        self.0.locked.load(Ordering::Relaxed)
    }
    async fn create_item(
        &self,
        mut properties: HashMap<String, OwnedValue>,
        secret: Secret,
        replace: bool,
        #[zbus(object_server)] server: &zbus::ObjectServer,
    ) -> zbus::fdo::Result<(OwnedObjectPath, OwnedObjectPath)> {
        assert!(!replace);
        assert_eq!(secret.content_type, "application/json");
        assert!(secret.parameters.is_empty());
        let attributes: HashMap<String, String> = properties
            .remove("org.freedesktop.Secret.Item.Attributes")
            .unwrap()
            .try_into()
            .unwrap();
        assert_eq!(attributes["application"], "rocks.tidalforces.Player");
        let key = attributes["entry"].clone();
        let path = object(&format!("/item/k{key}"));
        self.0
            .values
            .lock()
            .unwrap()
            .insert(key.clone(), secret.value);
        server
            .at(
                path.clone(),
                Item {
                    key,
                    state: self.0.clone(),
                },
            )
            .await
            .unwrap();
        Ok((path, object("/")))
    }
}
#[zbus::dbus_interface(name = "org.freedesktop.Secret.Item")]
impl Item {
    fn get_secret(&self, session: OwnedObjectPath) -> Secret {
        Secret {
            session,
            parameters: if self.state.invalid_parameters.load(Ordering::Relaxed) {
                vec![1]
            } else {
                vec![]
            },
            value: if self.state.corrupt_read.load(Ordering::Relaxed) {
                b"changed reply".to_vec()
            } else {
                self.state.values.lock().unwrap()[&self.key].clone()
            },
            content_type: self.state.content_type.lock().unwrap().clone(),
        }
    }
    fn delete(&self) -> OwnedObjectPath {
        self.state.values.lock().unwrap().remove(&self.key);
        object("/")
    }
}
#[test]
fn secret_mime_is_advisory_but_plain_session_and_size_guards_remain_strict() {
    let session = object("/session/test");
    let reply = |mime: &str| Secret {
        session: session.clone(),
        parameters: vec![],
        value: vec![0, 255, 1, 127],
        content_type: mime.into(),
    };
    for mime in [
        "application/json",
        "text/plain",
        "text/plain; charset=utf-8",
        "application/octet-stream",
        "",
    ] {
        assert_eq!(reply(mime).into_value(&session).unwrap(), [0, 255, 1, 127]);
    }
    let mut wrong = reply("application/json");
    wrong.session = object("/session/other");
    assert!(
        wrong
            .into_value(&session)
            .unwrap_err()
            .to_string()
            .contains("different session")
    );
    let mut encrypted = reply("text/plain");
    encrypted.parameters = vec![1];
    assert!(
        encrypted
            .into_value(&session)
            .unwrap_err()
            .to_string()
            .contains("plain-session parameters")
    );
    let mut oversized = reply("text/plain");
    oversized.value = vec![0; LIMIT as usize + 1];
    assert!(
        oversized
            .into_value(&session)
            .unwrap_err()
            .to_string()
            .contains("storage limit")
    );
    let mut limit = reply("text/plain");
    limit.value = vec![0; LIMIT as usize];
    assert_eq!(limit.into_value(&session).unwrap().len(), LIMIT as usize);
}

#[tokio::test]
#[ignore = "Run only under a private dbus-run-session with TIDAL_TEST_SECRET_SERVICE=1"]
async fn native_secret_service_contract_on_isolated_bus() {
    assert_eq!(
        std::env::var("TIDAL_TEST_SECRET_SERVICE").as_deref(),
        Ok("1")
    );
    assert!(
        std::env::var("DBUS_SESSION_BUS_ADDRESS")
            .unwrap()
            .contains("/tmp/dbus-"),
        "Refusing to register a fake vault on a normal desktop bus"
    );
    let state = Arc::new(State {
        content_type: Mutex::new("application/json".into()),
        ..Default::default()
    });
    let connection = zbus::ConnectionBuilder::session()
        .unwrap()
        .name(SERVICE)
        .unwrap()
        .serve_at(ROOT, Service(state.clone()))
        .unwrap()
        .serve_at("/collection/test", Collection(state.clone()))
        .unwrap()
        .build()
        .await
        .unwrap();
    let key = "0123456789abcdef0123456789abcdef";
    let bytes = br#"{"synthetic":"not a real credential"}"#;
    NativeVault.write(key, bytes).await.unwrap();
    assert_eq!(NativeVault.read(key).await.unwrap().unwrap(), bytes);
    // GNOME Keyring 46.1 emits text/plain regardless of the supplied MIME type.
    for content_type in [
        "text/plain",
        "text/plain; charset=utf-8",
        "application/octet-stream",
    ] {
        *state.content_type.lock().unwrap() = content_type.into();
        assert_eq!(NativeVault.read(key).await.unwrap().unwrap(), bytes);
    }
    state.locked.store(true, Ordering::Relaxed);
    assert!(NativeVault.read(key).await.is_err());
    assert!(NativeVault.delete(key).await.is_err());
    state.locked.store(false, Ordering::Relaxed);
    NativeVault.delete(key).await.unwrap();
    assert!(NativeVault.read(key).await.unwrap().is_none());

    // Exercise the entire journal/verification/commit path, using only a private
    // temporary profile and this isolated fake service, never the desktop vault.
    *state.content_type.lock().unwrap() = "text/plain".into();
    let root = tempfile::tempdir().unwrap();
    let store = Store::at(root.path());
    let session = Session {
        access_token: "synthetic-access-token".into(),
        refresh_token: "synthetic-refresh-token".into(),
        expires_at: 123456,
        user_id: 7,
        country: "US".into(),
        pkce: true,
    };
    store.save(&session).await.unwrap();
    let legacy = fs::read(root.path().join("session.json")).unwrap();
    for (parameters, corrupt) in [(true, false), (false, true)] {
        state
            .invalid_parameters
            .store(parameters, Ordering::Relaxed);
        state.corrupt_read.store(corrupt, Ordering::Relaxed);
        let error = store.migrate(&session).await.unwrap_err().to_string();
        assert!(error.contains(if parameters {
            "plain-session parameters"
        } else {
            "read-back verification failed"
        }));
        let meta = Metadata::read(root.path()).unwrap();
        assert_eq!(meta.storage, Storage::Legacy);
        assert!(meta.entry.is_none());
        assert!(!meta.pending.is_empty());
        assert_eq!(fs::read(root.path().join("session.json")).unwrap(), legacy);
        assert!(store.status().unwrap().cleanup_pending);
    }
    state.corrupt_read.store(false, Ordering::Relaxed);
    store.migrate(&session).await.unwrap();
    let meta = Metadata::read(root.path()).unwrap();
    assert_eq!(meta.storage, Storage::Keyring);
    assert!(meta.pending.is_empty());
    assert!(!root.path().join("session.json").exists());
    assert!(!store.status().unwrap().cleanup_pending);
    assert_eq!(
        store.load().await.unwrap().unwrap().access_token,
        session.access_token
    );
    assert_eq!(
        state.values.lock().unwrap().len(),
        1,
        "Uncommitted copies must be cleaned only after verified replacement"
    );
    store::save_bytes(root.path(), "session.json", &legacy).unwrap();
    state.corrupt_read.store(true, Ordering::Relaxed);
    assert!(
        store
            .load()
            .await
            .err()
            .unwrap()
            .to_string()
            .contains("changed unexpectedly")
    );
    assert!(store.cleanup().await.is_err());
    assert_eq!(fs::read(root.path().join("session.json")).unwrap(), legacy);
    assert_eq!(
        state.values.lock().unwrap().len(),
        1,
        "Unverified data must not trigger deletion or plaintext fallback"
    );
    drop(connection);
}
