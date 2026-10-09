//! Freedesktop Secret Service, using the existing native zbus transport.
//! No automatic Unlock/Prompt calls: the user unlocks/configures their desktop vault.
use super::*;
use std::{collections::HashMap, future::Future, time::Duration};
use zbus::{
    Connection, Proxy,
    zvariant::{OwnedObjectPath, OwnedValue, Type, Value},
};
const SERVICE: &str = "org.freedesktop.secrets";
const ROOT: &str = "/org/freedesktop/secrets";
const INTERFACE: &str = "org.freedesktop.Secret.Service";
const ITEM: &str = "org.freedesktop.Secret.Item";
const COLLECTION: &str = "org.freedesktop.Secret.Collection";
fn unavailable() -> anyhow::Error {
    anyhow::anyhow!(
        "OS Secret Service is unavailable, locked, or requires a prompt. Unlock/configure the desktop keyring and retry; no plaintext fallback was used"
    )
}
fn attributes(key: &str) -> HashMap<&str, &str> {
    HashMap::from([("application", "rocks.tidalforces.Player"), ("entry", key)])
}

#[derive(Serialize, Deserialize, Type)]
struct Secret {
    session: OwnedObjectPath,
    parameters: Vec<u8>,
    value: Vec<u8>,
    content_type: String,
}
impl Secret {
    fn into_value(self, session: &OwnedObjectPath) -> Result<Vec<u8>> {
        ensure!(
            self.session == *session,
            "Keyring secret belongs to a different session"
        );
        ensure!(
            self.parameters.is_empty(),
            "Keyring returned unexpected plain-session parameters"
        );
        ensure!(
            self.value.len() as u64 <= LIMIT,
            "Keyring secret exceeds the storage limit"
        );
        // MIME is advisory: GNOME Keyring returns text/plain even for JSON writes.
        // The credential store verifies the exact bytes/digest and credential schema.
        Ok(self.value)
    }
}
struct Client {
    connection: Connection,
    session: OwnedObjectPath,
}
impl Client {
    async fn new() -> Result<Self> {
        let connection = Connection::session().await.map_err(|_| unavailable())?;
        let service = Proxy::new(&connection, SERVICE, ROOT, INTERFACE)
            .await
            .map_err(|_| unavailable())?;
        // Plain is a standard, strongly recommended local session algorithm. Protection at rest
        // belongs to the desktop provider; do not claim encryption merely from this API.
        let (output, session): (OwnedValue, OwnedObjectPath) = service
            .call("OpenSession", &("plain", Value::from("")))
            .await
            .map_err(|_| unavailable())?;
        ensure!(
            String::try_from(output).ok().as_deref() == Some(""),
            "Secret Service returned an unsupported session algorithm"
        );
        ensure!(
            session.as_str() != "/",
            "Secret Service did not open a session"
        );
        drop(service);
        Ok(Self {
            connection,
            session,
        })
    }
    async fn proxy(&self, path: &str, interface: &str) -> Result<Proxy<'static>> {
        Proxy::new_owned(
            self.connection.clone(),
            SERVICE.to_owned(),
            path.to_owned(),
            interface.to_owned(),
        )
        .await
        .map_err(|_| unavailable())
    }
    async fn find(&self, key: &str) -> Result<Option<OwnedObjectPath>> {
        let service = self.proxy(ROOT, INTERFACE).await?;
        let (mut unlocked, locked): (Vec<OwnedObjectPath>, Vec<OwnedObjectPath>) = service
            .call("SearchItems", &(attributes(key),))
            .await
            .map_err(|_| unavailable())?;
        ensure!(
            locked.is_empty(),
            "Saved credentials are locked. Unlock the desktop keyring and retry"
        );
        ensure!(
            unlocked.len() <= 1,
            "Ambiguous keyring entries; no credential was changed"
        );
        Ok(unlocked.pop())
    }
    async fn read(&self, key: &str) -> Result<Option<Vec<u8>>> {
        let Some(path) = self.find(key).await? else {
            return Ok(None);
        };
        let item = self.proxy(path.as_str(), ITEM).await?;
        let secret: Secret = item
            .call("GetSecret", &(&self.session,))
            .await
            .map_err(|_| unavailable())?;
        secret.into_value(&self.session).map(Some)
    }
    async fn write(&self, key: &str, bytes: &[u8]) -> Result<()> {
        let service = self.proxy(ROOT, INTERFACE).await?;
        let collection: OwnedObjectPath = service
            .call("ReadAlias", &("default",))
            .await
            .map_err(|_| unavailable())?;
        ensure!(
            collection.as_str() != "/",
            "Configure an unlocked default desktop keyring before migrating"
        );
        let collection = self.proxy(collection.as_str(), COLLECTION).await?;
        let locked: bool = collection
            .get_property("Locked")
            .await
            .map_err(|_| unavailable())?;
        ensure!(!locked, "Default keyring is locked; unlock it and retry");
        let properties = HashMap::from([
            (
                "org.freedesktop.Secret.Item.Label",
                Value::from("Tidal Forces sign-in"),
            ),
            (
                "org.freedesktop.Secret.Item.Attributes",
                Value::from(attributes(key)),
            ),
        ]);
        let secret = Secret {
            session: self.session.clone(),
            parameters: vec![],
            value: bytes.to_vec(),
            content_type: "application/json".into(),
        };
        let (item, prompt): (OwnedObjectPath, OwnedObjectPath) = collection
            .call("CreateItem", &(properties, secret, false))
            .await
            .map_err(|_| unavailable())?;
        ensure!(
            item.as_str() != "/" && prompt.as_str() == "/",
            "Keyring creation requires a prompt; unlock/configure it and retry"
        );
        Ok(())
    }
    async fn delete(&self, key: &str) -> Result<()> {
        let Some(path) = self.find(key).await? else {
            return Ok(());
        };
        let item = self.proxy(path.as_str(), ITEM).await?;
        let prompt: OwnedObjectPath = item.call("Delete", &()).await.map_err(|_| unavailable())?;
        ensure!(
            prompt.as_str() == "/",
            "Keyring deletion requires a prompt; unlock it and retry removal"
        );
        ensure!(
            self.find(key).await?.is_none(),
            "Could not confirm removal of the saved keyring item"
        );
        Ok(())
    }
}
async fn bounded<T>(future: impl Future<Output = Result<T>>) -> Result<T> {
    tokio::time::timeout(Duration::from_secs(5), future)
        .await
        .map_err(|_| unavailable())?
}
pub(super) struct NativeVault;
impl Vault for NativeVault {
    async fn read(&self, key: &str) -> Result<Option<Vec<u8>>> {
        bounded(async { Client::new().await?.read(key).await }).await
    }
    async fn write(&self, key: &str, bytes: &[u8]) -> Result<()> {
        bounded(async { Client::new().await?.write(key, bytes).await }).await
    }
    async fn delete(&self, key: &str) -> Result<()> {
        bounded(async { Client::new().await?.delete(key).await }).await
    }
}

#[cfg(test)]
#[path = "secret_service_tests.rs"]
mod tests;
