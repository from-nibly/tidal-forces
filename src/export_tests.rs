use super::playlist_tests::{mock, step};
use super::*;
use serde_json::json;
use std::sync::atomic::AtomicBool;
fn metadata(count: usize) -> Value {
    json!({"uuid":"test-list","type":"USER","title":"Export","numberOfTracks":count,"numberOfVideos":0,"creator":{"id":7}})
}
#[tokio::test]
async fn queue_batch_preserves_duplicates_and_verifies_order_at_the_write_revision() {
    let mut post = step("POST", "/v1/playlists/test-list/items", json!({}));
    post.etag = "\"revision-2\"";
    post.contains = vec![
        "trackIds=9%2C9%2C10",
        "onDupes=ADD",
        "onArtifactNotFound=FAIL",
        "toIndex=0",
        "if-none-match: \"revision-1\"",
    ];
    let mut after = step("GET", "/v1/playlists/test-list", metadata(3));
    after.etag = post.etag;
    let mut page = step(
        "GET",
        "/v1/playlists/test-list/items",
        json!({"totalNumberOfItems":3,"items":[
        {"type":"track","item":{"id":9,"title":"Test"}}, {"type":"track","item":{"id":9,"title":"Test"}}, {"type":"track","item":{"id":10,"title":"Test"}}]}),
    );
    page.etag = post.etag;
    let (mut api, server) = mock(vec![
        step("GET", "/v1/playlists/test-list", metadata(0)),
        post,
        after,
        page,
    ]);
    let etag = api
        .append_queue_batch(
            7,
            "test-list",
            &[9, 9, 10],
            0,
            None,
            &AtomicBool::new(false),
        )
        .await
        .unwrap();
    assert_eq!(etag, "\"revision-2\"");
    server.join().unwrap();
}
#[tokio::test]
async fn queue_export_rejects_account_changes_cancellation_and_unsafe_batches_before_requests() {
    let (mut api, server) = mock(vec![]);
    assert!(
        api.create_queue_playlist(8, "Test", "", &AtomicBool::new(false))
            .await
            .is_err()
    );
    assert!(
        api.create_queue_playlist(7, "Test", "", &AtomicBool::new(true))
            .await
            .is_err()
    );
    assert!(
        api.append_queue_batch(8, "test-list", &[9], 0, None, &AtomicBool::new(false))
            .await
            .is_err()
    );
    assert!(
        api.append_queue_batch(7, "test-list", &[9], 0, None, &AtomicBool::new(true))
            .await
            .is_err()
    );
    assert!(
        api.append_queue_batch(7, "test-list", &[9], 100, None, &AtomicBool::new(false))
            .await
            .is_err()
    );
    assert!(
        api.append_queue_batch(7, "test-list", &[0], 0, None, &AtomicBool::new(false))
            .await
            .is_err()
    );
    server.join().unwrap();
}
#[tokio::test]
async fn queue_export_stops_on_external_edits_or_foreign_ownership_before_writing() {
    for (count, owner, etag) in [
        (101, 7, "\"revision-1\""),
        (100, 8, "\"revision-1\""),
        (100, 7, "old"),
    ] {
        let mut value = metadata(count);
        value["creator"]["id"] = json!(owner);
        let (mut api, server) = mock(vec![step("GET", "/v1/playlists/test-list", value)]);
        assert!(
            api.append_queue_batch(
                7,
                "test-list",
                &[9],
                100,
                Some(etag),
                &AtomicBool::new(false)
            )
            .await
            .is_err()
        );
        server.join().unwrap();
    }
}
#[tokio::test]
async fn cancellation_while_reading_ownership_prevents_the_next_write() {
    let cancelled = std::sync::Arc::new(AtomicBool::new(false));
    let flag = cancelled.clone();
    let mut read = step("GET", "/v1/playlists/test-list", metadata(0));
    read.on_request = Some(Box::new(move || {
        flag.store(true, std::sync::atomic::Ordering::Release)
    }));
    let (mut api, server) = mock(vec![read]);
    let error = api
        .append_queue_batch(7, "test-list", &[9], 0, None, &cancelled)
        .await
        .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("cancelled before the next write")
    );
    server.join().unwrap();
}

#[tokio::test]
async fn uncertain_or_unversioned_writes_are_never_retried() {
    for status in [0, 503, 200] {
        let mut post = step("POST", "/v1/playlists/test-list/items", json!({}));
        post.status = status;
        post.etag = "";
        let (mut api, server) = mock(vec![
            step("GET", "/v1/playlists/test-list", metadata(0)),
            post,
        ]);
        assert!(
            api.append_queue_batch(7, "test-list", &[9], 0, None, &AtomicBool::new(false))
                .await
                .is_err()
        );
        server.join().unwrap();
    }
}
#[tokio::test]
async fn readback_does_not_accept_a_different_revision_or_wrong_order() {
    let mut post = step("POST", "/v1/playlists/test-list/items", json!({}));
    post.etag = "\"revision-2\"";
    let (mut api, server) = mock(vec![
        step("GET", "/v1/playlists/test-list", metadata(0)),
        post,
        step("GET", "/v1/playlists/test-list", metadata(2)),
    ]);
    assert!(
        api.append_queue_batch(7, "test-list", &[9, 10], 0, None, &AtomicBool::new(false))
            .await
            .is_err()
    );
    server.join().unwrap();
    let (mut api, server) = mock(vec![
        step("GET", "/v1/playlists/test-list", metadata(0)),
        step("POST", "/v1/playlists/test-list/items", json!({})),
        step("GET", "/v1/playlists/test-list", metadata(2)),
        step(
            "GET",
            "/v1/playlists/test-list/items",
            json!({"totalNumberOfItems":2,"items":[{"type":"track","item":{"id":10,"title":"Test"}}, {"type":"track","item":{"id":9,"title":"Test"}}]}),
        ),
    ]);
    assert!(
        api.append_queue_batch(7, "test-list", &[9, 10], 0, None, &AtomicBool::new(false))
            .await
            .is_err()
    );
    server.join().unwrap();
}
