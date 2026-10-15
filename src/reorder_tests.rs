use super::playlist_tests::{mock, step};
use super::*;
use serde_json::json;

fn change() -> crate::reorder::Move {
    crate::reorder::Move {
        user: 7,
        id: "test-list".into(),
        etag: "\"revision-1\"".into(),
        ids: vec![9, 8, 9],
        from: 2,
        to_end: false,
    }
}
fn metadata() -> Value {
    json!({"uuid":"test-list","title":"Fixture","type":"USER","numberOfTracks":3,"numberOfVideos":0,"creator":{"id":7}})
}
fn items(ids: &[u64]) -> Value {
    json!({"totalNumberOfItems":3,"items":ids.iter().map(|id|json!({"type":"track","item":{"id":id,"title":"Fixture"}})).collect::<Vec<_>>()})
}

#[tokio::test]
async fn reorder_rejects_stale_preflight_and_intervening_readback_revision() {
    let mut stale = change();
    stale.etag = "old".into();
    let (mut api, server) = mock(vec![step("GET", "/v1/playlists/test-list", metadata())]);
    assert!(api.reorder_playlist(&stale).await.is_err());
    server.join().unwrap();
    let mut write = step("POST", "/v1/playlists/test-list/items/2", json!({}));
    write.etag = "\"revision-2\"";
    let mut intervening = step("GET", "/v1/playlists/test-list", metadata());
    intervening.etag = "\"revision-3\"";
    let (mut api, server) = mock(vec![
        step("GET", "/v1/playlists/test-list", metadata()),
        step("GET", "/v1/playlists/test-list/items", items(&[9, 8, 9])),
        write,
        intervening,
    ]);
    assert!(api.reorder_playlist(&change()).await.is_err());
    server.join().unwrap();
}

#[tokio::test]
async fn reorder_posts_raw_occurrence_and_verifies_both_endpoints() {
    for bottom in [false, true] {
        let mut change = change();
        if bottom {
            change.from = 0;
            change.to_end = true;
        }
        let mut write = step(
            "POST",
            if bottom {
                "/v1/playlists/test-list/items/0"
            } else {
                "/v1/playlists/test-list/items/2"
            },
            json!({}),
        );
        write.contains = vec![
            "if-none-match: \"revision-1\"",
            if bottom { "toIndex=3" } else { "toIndex=0" },
        ];
        write.etag = "\"revision-2\"";
        let mut read_meta = step("GET", "/v1/playlists/test-list", metadata());
        read_meta.etag = "\"revision-2\"";
        let mut read_items = step(
            "GET",
            "/v1/playlists/test-list/items",
            items(&change.after().unwrap()),
        );
        read_items.etag = "\"revision-2\"";
        let (mut api, server) = mock(vec![
            step("GET", "/v1/playlists/test-list", metadata()),
            step("GET", "/v1/playlists/test-list/items", items(&change.ids)),
            write,
            read_meta,
            read_items,
        ]);
        api.reorder_playlist(&change).await.unwrap();
        server.join().unwrap();
    }
}
#[tokio::test]
async fn reorder_rejects_changed_order_foreign_ownership_videos_and_holes_before_write() {
    for case in 0..4 {
        let mut meta = metadata();
        let mut rows = items(&[9, 8, 9]);
        match case {
            0 => rows = items(&[8, 9, 9]),
            1 => meta["creator"]["id"] = json!(8),
            2 => meta["numberOfVideos"] = json!(1),
            _ => rows["items"][0]["item"] = Value::Null,
        }
        let (mut api, server) = mock(vec![
            step("GET", "/v1/playlists/test-list", meta),
            step("GET", "/v1/playlists/test-list/items", rows),
        ]);
        assert!(api.reorder_playlist(&change()).await.is_err());
        server.join().unwrap();
    }
    let (mut api, server) = mock(vec![]);
    let mut change = change();
    change.user = 8;
    assert!(api.reorder_playlist(&change).await.is_err());
    server.join().unwrap();
}
#[tokio::test]
async fn reorder_does_not_claim_success_for_ambiguous_or_mismatched_readback() {
    for case in 0..4 {
        let mut write = step("POST", "/v1/playlists/test-list/items/2", json!({}));
        write.etag = "\"revision-2\"";
        match case {
            0 => write.status = 500,
            1 => write.etag = "",
            2 => write.etag = "\"revision-1\"",
            _ => {}
        }
        let mut steps = vec![
            step("GET", "/v1/playlists/test-list", metadata()),
            step("GET", "/v1/playlists/test-list/items", items(&[9, 8, 9])),
            write,
        ];
        if case == 3 {
            let mut meta = step("GET", "/v1/playlists/test-list", metadata());
            meta.etag = "\"revision-2\"";
            let mut rows = step("GET", "/v1/playlists/test-list/items", items(&[9, 8, 9]));
            rows.etag = "\"revision-2\"";
            steps.extend([meta, rows]);
        }
        let (mut api, server) = mock(steps);
        assert!(api.reorder_playlist(&change()).await.is_err());
        server.join().unwrap();
    }
}
