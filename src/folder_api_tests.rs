use super::*;
use crate::api::playlist_tests::{mock, step};
use serde_json::json;

fn folder(name: &str) -> Value {
    json!({"itemType":"FOLDER","name":name,"data":{"id":"folder-a","totalNumberOfItems":0}})
}
fn playlist() -> Value {
    json!({"itemType":"PLAYLIST","data":{"uuid":"test-list","title":"Fixture"}})
}
fn listing(items: Vec<Value>) -> Value {
    json!({"items":items,"cursor":null})
}
fn selected() -> Folder {
    Folder {
        id: "folder-a".into(),
        parent: Some("root".into()),
        name: "A".into(),
    }
}
#[tokio::test]
async fn creation_requires_exact_readback_and_returns_created_identity_even_if_unverified() {
    for verified in [true, false] {
        let mut create = step(
            "PUT",
            "/v2/my-collection/playlists/folders/create-folder",
            json!({"name":"New folder","data":{"id":"new-folder"}}),
        );
        create.contains = vec!["folderId=root", "name=New+folder"];
        let body = if verified {
            listing(vec![
                json!({"itemType":"FOLDER","name":"New folder","data":{"id":"new-folder"}}),
            ])
        } else {
            json!({"items":[]})
        };
        let (mut api, server) = mock(vec![
            step(
                "GET",
                "/v2/my-collection/playlists/folders",
                listing(vec![]),
            ),
            create,
            step("GET", "/v2/my-collection/playlists/folders", body),
        ]);
        let result = api
            .manage_folder(
                7,
                &Change::Create {
                    parent: Folder::root(),
                    name: " New folder ".into(),
                },
                &AtomicBool::new(false),
            )
            .await
            .unwrap();
        assert_eq!(result.verified, verified);
        assert_eq!(result.created.unwrap().id, "new-folder");
        server.join().unwrap();
    }
}
#[tokio::test]
async fn rename_checks_old_name_and_verifies_new_name() {
    let mut write = step(
        "PUT",
        "/v2/my-collection/playlists/folders/rename",
        json!({}),
    );
    write.contains = vec!["trn=trn%3Afolder%3Afolder-a", "name=B"];
    let (mut api, server) = mock(vec![
        step(
            "GET",
            "/v2/my-collection/playlists/folders",
            listing(vec![folder("A")]),
        ),
        write,
        step(
            "GET",
            "/v2/my-collection/playlists/folders",
            listing(vec![folder("B")]),
        ),
    ]);
    assert!(
        api.manage_folder(
            7,
            &Change::Rename {
                folder: selected(),
                name: "B".into()
            },
            &AtomicBool::new(false)
        )
        .await
        .unwrap()
        .verified
    );
    server.join().unwrap();
}
#[tokio::test]
async fn playlist_move_uses_only_playlist_trn_and_proves_both_locations() {
    let mut write = step("PUT", "/v2/my-collection/playlists/folders/move", json!({}));
    write.contains = vec!["trns=trn%3Aplaylist%3Atest-list", "folderId=folder-a"];
    let (mut api, server) = mock(vec![
        step(
            "GET",
            "/v2/my-collection/playlists/folders",
            listing(vec![playlist(), folder("A")]),
        ),
        step(
            "GET",
            "/v2/my-collection/playlists/folders",
            listing(vec![playlist(), folder("A")]),
        ),
        step(
            "GET",
            "/v2/my-collection/playlists/folders",
            listing(vec![]),
        ),
        write,
        step(
            "GET",
            "/v2/my-collection/playlists/folders",
            listing(vec![folder("A")]),
        ),
        step(
            "GET",
            "/v2/my-collection/playlists/folders",
            listing(vec![playlist()]),
        ),
    ]);
    assert!(
        api.manage_folder(
            7,
            &Change::MovePlaylist {
                id: "test-list".into(),
                title: "Fixture".into(),
                source: "root".into(),
                destination: selected()
            },
            &AtomicBool::new(false)
        )
        .await
        .unwrap()
        .verified
    );
    server.join().unwrap();
}
#[tokio::test]
async fn incomplete_repeated_changed_and_cancelled_folders_cannot_write() {
    for body in [
        json!({"items":[]}),
        listing(vec![folder("different")]),
        listing(vec![folder("A"), folder("A")]),
        json!({"items":[folder("A")],"cursor":null,"totalNumberOfItems":2}),
    ] {
        let (mut api, server) = mock(vec![step(
            "GET",
            "/v2/my-collection/playlists/folders",
            body,
        )]);
        assert!(
            api.manage_folder(
                7,
                &Change::Rename {
                    folder: selected(),
                    name: "B".into()
                },
                &AtomicBool::new(false)
            )
            .await
            .is_err()
        );
        server.join().unwrap();
    }
    let (mut api, server) = mock(vec![]);
    let change = Change::Create {
        parent: Folder::root(),
        name: "A".into(),
    };
    assert!(
        api.manage_folder(8, &change, &AtomicBool::new(false))
            .await
            .is_err()
    );
    assert!(
        api.manage_folder(7, &change, &AtomicBool::new(true))
            .await
            .is_err()
    );
    server.join().unwrap();
}

#[tokio::test]
async fn cancellation_before_write_and_uncertain_server_failures_never_retry() {
    let cancelled = std::sync::Arc::new(AtomicBool::new(false));
    let flag = cancelled.clone();
    let mut read = step(
        "GET",
        "/v2/my-collection/playlists/folders",
        listing(vec![]),
    );
    read.on_request = Some(Box::new(move || flag.store(true, Ordering::Release)));
    let (mut api, server) = mock(vec![read]);
    let change = Change::Create {
        parent: Folder::root(),
        name: "Fixture".into(),
    };
    assert!(api.manage_folder(7, &change, &cancelled).await.is_err());
    server.join().unwrap();
    let mut write = step(
        "PUT",
        "/v2/my-collection/playlists/folders/create-folder",
        json!({}),
    );
    write.status = 500;
    let (mut api, server) = mock(vec![
        step(
            "GET",
            "/v2/my-collection/playlists/folders",
            listing(vec![]),
        ),
        write,
    ]);
    assert!(
        api.manage_folder(7, &change, &AtomicBool::new(false))
            .await
            .is_err()
    );
    server.join().unwrap();
    let (mut api, server) = mock(vec![]);
    assert!(
        api.manage_folder(
            7,
            &Change::Create {
                parent: selected(),
                name: "Nested".into()
            },
            &AtomicBool::new(false)
        )
        .await
        .is_err()
    );
    assert!(api.folder_for(8, "root").await.is_err());
    server.join().unwrap();
}

#[tokio::test]
#[ignore = "Read-only live folder contract check; requires explicit operator opt-in and a fresh signed-in session"]
async fn live_folder_read_contract() {
    assert_eq!(
        std::env::var("TIDAL_TEST_FOLDERS").as_deref(),
        Ok("1"),
        "Explicit opt-in required"
    );
    let mut api = Api::new().expect("Could not initialize API client");
    api.prohibit_refresh = true;
    api.session = api.credentials.load().await.ok().flatten();
    let session = api
        .session
        .as_ref()
        .expect("A signed-in session is required");
    assert!(
        session.user_id != 0 && session.expires_at > store::now() + 180,
        "A fresh session is required; no credential renewal was attempted"
    );
    let user = session.user_id;
    let identity = api
        .get("sessions", &[])
        .await
        .unwrap_or_else(|_| panic!("Read-only identity check failed"));
    assert!(
        identity["userId"].as_u64() == Some(user),
        "Account identity changed"
    );
    assert!(
        api.folder_snapshot(user, "root", &AtomicBool::new(false))
            .await
            .is_ok(),
        "Folder listing completeness contract could not be established"
    );
}
