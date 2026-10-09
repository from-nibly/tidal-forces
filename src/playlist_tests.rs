use super::*;
use serde_json::json;
use std::{
    io::{Read, Write},
    net::TcpListener,
    thread,
    time::Instant,
};

pub(super) struct Step {
    method: &'static str,
    path: &'static str,
    pub(super) contains: Vec<&'static str>,
    pub(super) status: u16,
    pub(super) etag: &'static str,
    body: Value,
    pub(super) on_request: Option<Box<dyn FnOnce() + Send>>,
}
pub(super) fn step(method: &'static str, path: &'static str, body: Value) -> Step {
    Step {
        method,
        path,
        contains: vec![],
        status: 200,
        etag: "\"revision-1\"",
        body,
        on_request: None,
    }
}
fn metadata() -> Value {
    json!({"uuid":"test-list", "title":"Test", "type":"USER", "numberOfTracks":3,"numberOfVideos":1,"creator":{"id":7}})
}
fn track(id: u64) -> Value {
    json!({"type":"track","item":{"id":id,"title":"Synthetic metadata"}})
}
pub(super) fn mock(steps: Vec<Step>) -> (Api, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let mut api = Api::new().unwrap();
    api.base = format!("http://{}", listener.local_addr().unwrap());
    api.session = Some(Session {
        access_token: "test-only".into(),
        user_id: 7,
        country: "US".into(),
        expires_at: store::now() + 3600,
        ..Default::default()
    });
    let server = thread::spawn(move || {
        for step in steps {
            let deadline = Instant::now() + Duration::from_secs(5);
            let mut socket = loop {
                match listener.accept() {
                    Ok((socket, _)) => break socket,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(
                            Instant::now() < deadline,
                            "Missing request {} {}",
                            step.method,
                            step.path
                        );
                        thread::sleep(Duration::from_millis(2));
                    }
                    Err(e) => panic!("{e}"),
                }
            };
            socket
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut data = Vec::new();
            loop {
                let mut buffer = [0; 4096];
                let count = socket.read(&mut buffer).unwrap();
                assert!(count > 0);
                data.extend_from_slice(&buffer[..count]);
                let text = String::from_utf8_lossy(&data);
                if let Some(end) = text.find("\r\n\r\n") {
                    let length = text[..end]
                        .lines()
                        .find_map(|l| {
                            l.to_lowercase()
                                .strip_prefix("content-length:")
                                .map(|n| n.trim().parse::<usize>().unwrap())
                        })
                        .unwrap_or(0);
                    if data.len() >= end + 4 + length {
                        break;
                    }
                }
            }
            let request = String::from_utf8(data).unwrap();
            assert!(
                request.starts_with(&format!("{} {}?", step.method, step.path)),
                "{request}"
            );
            for needle in step.contains {
                assert!(request.contains(needle), "Missing {needle}: {request}");
            }
            if let Some(on_request) = step.on_request {
                on_request();
            }
            if step.status == 0 {
                continue;
            }
            let body = if step.status == 204 {
                String::new()
            } else {
                step.body.to_string()
            };
            let etag = if step.etag.is_empty() {
                String::new()
            } else {
                format!("ETag: {}\r\n", step.etag)
            };
            write!(socket, "HTTP/1.1 {} Test\r\nContent-Type: application/json\r\n{}Content-Length: {}\r\nConnection: close\r\n\r\n{}", step.status, etag, body.len(), body).unwrap();
        }
    });
    (api, server)
}

#[tokio::test]
async fn playlist_pages_preserve_positions_across_videos_and_duplicates() {
    let mut page = step(
        "GET",
        "/v1/playlists/test-list/items",
        json!({"totalNumberOfItems":9,"items":[track(9),{"type":"video","item":{"id":2}},{"type":"track","item":null},track(9)]}),
    );
    page.contains = vec!["offset=4"];
    let (mut api, server) = mock(vec![
        step("GET", "/v1/playlists/test-list", metadata()),
        page,
    ]);
    let page = api
        .playlist_page("test-list", 4, Some("\"revision-1\""))
        .await
        .unwrap();
    assert!(page.editable && page.more);
    assert_eq!(
        page.rows
            .iter()
            .map(|(i, t)| (*i, t.id))
            .collect::<Vec<_>>(),
        [(4, 9), (7, 9)]
    );
    assert_eq!(page.next_offset, 8);
    server.join().unwrap();
}

#[tokio::test]
async fn create_and_add_use_real_endpoint_shapes_and_revision_guards() {
    let mut create = step(
        "PUT",
        "/v2/my-collection/playlists/folders/create-playlist",
        json!({"data":metadata()}),
    );
    create.contains = vec!["name=Rock+%26+roll", "folderId=root", "description=Test"];
    let mut add = step(
        "POST",
        "/v1/playlists/test-list/items",
        json!({"addedItemIds":[9]}),
    );
    add.contains = vec![
        "if-none-match: \"revision-1\"",
        "trackIds=9",
        "toIndex=4",
        "onDupes=SKIP",
        "onArtifactNotFound=FAIL",
    ];
    let (mut api, server) = mock(vec![
        create,
        step("GET", "/v1/playlists/test-list", metadata()),
        add,
    ]);
    assert_eq!(
        api.create_playlist(" Rock & roll ", "Test")
            .await
            .unwrap()
            .uuid,
        "test-list"
    );
    assert!(api.add_to_playlist("test-list", 9).await.unwrap());
    server.join().unwrap();
}

#[tokio::test]
async fn removes_only_the_confirmed_occurrence() {
    let mut delete = step("DELETE", "/v1/playlists/test-list/items/6", Value::Null);
    delete.status = 204;
    delete.contains = vec!["if-none-match: \"revision-1\""];
    let (mut api, server) = mock(vec![
        step("GET", "/v1/playlists/test-list", metadata()),
        step(
            "GET",
            "/v1/playlists/test-list/items",
            json!({"totalNumberOfItems":7,"items":[track(9)]}),
        ),
        delete,
    ]);
    api.remove_from_playlist("test-list", 6, 9, "\"revision-1\"")
        .await
        .unwrap();
    server.join().unwrap();
}

#[tokio::test]
async fn refuses_stale_snapshots_wrong_tracks_and_foreign_playlists() {
    let (mut api, server) = mock(vec![step("GET", "/v1/playlists/test-list", metadata())]);
    assert!(
        api.remove_from_playlist("test-list", 0, 9, "old-revision")
            .await
            .unwrap_err()
            .to_string()
            .contains("changed elsewhere")
    );
    server.join().unwrap();
    let (mut api, server) = mock(vec![
        step("GET", "/v1/playlists/test-list", metadata()),
        step(
            "GET",
            "/v1/playlists/test-list/items",
            json!({"totalNumberOfItems":1,"items":[track(10)]}),
        ),
    ]);
    assert!(
        api.remove_from_playlist("test-list", 0, 9, "\"revision-1\"")
            .await
            .is_err()
    );
    server.join().unwrap();
    let mut foreign = metadata();
    foreign["creator"]["id"] = json!(8);
    let (mut api, server) = mock(vec![step("GET", "/v1/playlists/test-list", foreign)]);
    assert!(
        api.add_to_playlist("test-list", 9)
            .await
            .unwrap_err()
            .to_string()
            .contains("own playlists")
    );
    server.join().unwrap();
}

#[tokio::test]
async fn ownerless_and_artist_playlists_are_browsable_but_read_only() {
    for (kind, creator) in [("EDITORIAL", Value::Null), ("ARTIST", json!({"id":7}))] {
        let mut info = metadata();
        info["type"] = json!(kind);
        info["creator"] = creator;
        let (mut api, server) = mock(vec![
            step("GET", "/v1/playlists/test-list", info.clone()),
            step(
                "GET",
                "/v1/playlists/test-list/items",
                json!({"totalNumberOfItems":1,"items":[track(9)]}),
            ),
            step("GET", "/v1/playlists/test-list", info),
        ]);
        let page = api.playlist_page("test-list", 0, None).await.unwrap();
        assert!(!page.editable);
        assert_eq!(page.rows.len(), 1);
        assert!(api.add_to_playlist("test-list", 9).await.is_err());
        server.join().unwrap();
    }
}

#[tokio::test]
async fn reports_duplicates_and_conflicts_without_retrying_writes() {
    let (mut api, server) = mock(vec![
        step("GET", "/v1/playlists/test-list", metadata()),
        step(
            "POST",
            "/v1/playlists/test-list/items",
            json!({"addedItemIds":[]}),
        ),
    ]);
    assert!(!api.add_to_playlist("test-list", 9).await.unwrap());
    server.join().unwrap();
    let mut conflict = step("POST", "/v1/playlists/test-list/items", json!({}));
    conflict.status = 412;
    let (mut api, server) = mock(vec![
        step("GET", "/v1/playlists/test-list", metadata()),
        conflict,
    ]);
    assert!(
        api.add_to_playlist("test-list", 9)
            .await
            .unwrap_err()
            .to_string()
            .contains("changed elsewhere")
    );
    server.join().unwrap();
}

#[tokio::test]
async fn owned_picker_paginates_and_rejects_cross_page_revision_changes() {
    let page: Vec<_> = (0..50)
        .map(|i| json!({"uuid":format!("list-{i}"),"title":format!("List {i}"),"type":"USER","creator":{"id":7}}))
        .collect();
    let mut second = step(
        "GET",
        "/v1/users/7/playlists",
        json!({"totalNumberOfItems":51,"items":[{"uuid":"foreign","title":"Not editable","creator":{"id":8}}]}),
    );
    second.contains = vec!["offset=50"];
    let (mut api, server) = mock(vec![
        step(
            "GET",
            "/v1/users/7/playlists",
            json!({"totalNumberOfItems":51,"items":page}),
        ),
        second,
    ]);
    assert_eq!(api.owned_playlists().await.unwrap().len(), 50);
    server.join().unwrap();
    let mut page = step("GET", "/v1/playlists/test-list/items", json!({"items":[]}));
    page.etag = "\"changed\"";
    let (mut api, server) = mock(vec![
        step("GET", "/v1/playlists/test-list", metadata()),
        page,
    ]);
    assert!(api.playlist_page("test-list", 0, None).await.is_err());
    server.join().unwrap();
}

#[tokio::test]
#[ignore = "Mutates a temporary playlist on the locally signed-in account; run explicitly"]
async fn live_playlist_round_trip() {
    let mut api = Api::new().unwrap();
    api.session = api.credentials.load().await.unwrap();
    api.identify().await.unwrap();
    let title = format!("Tidal Forces verification {} (temporary)", store::now());
    let playlist = api
        .create_playlist(
            &title,
            "Temporary create/add/remove verification; deleted after test.",
        )
        .await
        .unwrap();
    // Persist the cleanup ID locally before doing anything else, never in source control.
    let cleanup_path = std::env::temp_dir().join("tidal-forces-test-playlist-id");
    std::fs::write(&cleanup_path, &playlist.uuid).unwrap();
    let result: Result<()> = async {
        anyhow::ensure!(
            api.owned_playlists()
                .await?
                .iter()
                .any(|p| p.uuid == playlist.uuid),
            "Created playlist missing from picker"
        );
        let id = 257836968;
        anyhow::ensure!(
            api.add_to_playlist(&playlist.uuid, id).await?,
            "Track not added"
        );
        anyhow::ensure!(
            !api.add_to_playlist(&playlist.uuid, id).await?,
            "Duplicate should be skipped"
        );
        let page = api.playlist_page(&playlist.uuid, 0, None).await?;
        anyhow::ensure!(
            page.rows.len() == 1 && page.rows[0].1.id == id,
            "Unexpected playlist contents"
        );
        api.remove_from_playlist(&playlist.uuid, page.rows[0].0, id, &page.etag)
            .await?;
        anyhow::ensure!(
            api.playlist_page(&playlist.uuid, 0, None)
                .await?
                .rows
                .is_empty(),
            "Track was not removed"
        );
        Ok(())
    }
    .await;
    api.delete_test_playlist(&playlist.uuid)
        .await
        .expect("Cleanup failed; the temporary playlist ID is recorded under /tmp");
    std::fs::remove_file(cleanup_path).unwrap();
    result.unwrap();
}
