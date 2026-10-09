use super::playlist_tests::{mock, step};
use super::*;
use serde_json::json;
use std::sync::atomic::AtomicBool;
fn metadata(count: usize) -> Value {
    json!({"uuid":"test-list","type":"USER","title":"Export","numberOfTracks":count,"numberOfVideos":0,"creator":{"id":7}})
}
#[tokio::test]
async fn duplicate_check_reads_revision_pinned_pages_without_writing() {
    let items = |ids: &[u64]| json!({"totalNumberOfItems":3,"items":ids.iter().map(|id|json!({"type":"track","item":{"id":id,"title":"Synthetic"}})).collect::<Vec<_>>()});
    let mut last = step("GET", "/v1/playlists/test-list/items", items(&[11]));
    last.contains = vec!["offset=2"];
    let (mut api, server) = mock(vec![
        step("GET", "/v1/playlists/test-list", metadata(3)),
        step("GET", "/v1/playlists/test-list/items", items(&[9, 10])),
        step("GET", "/v1/playlists/test-list", metadata(3)),
        last,
    ]);
    let ids = api
        .playlist_duplicate_page(
            7,
            "test-list",
            0,
            3,
            "\"revision-1\"",
            &AtomicBool::new(false),
        )
        .await
        .unwrap();
    assert_eq!(ids, [9, 10]);
    let ids = api
        .playlist_duplicate_page(
            7,
            "test-list",
            2,
            3,
            "\"revision-1\"",
            &AtomicBool::new(false),
        )
        .await
        .unwrap();
    assert_eq!(ids, [11]);
    server.join().unwrap();
}
#[tokio::test]
async fn duplicate_check_accepts_an_empty_playlist() {
    let (mut api, server) = mock(vec![
        step("GET", "/v1/playlists/test-list", metadata(0)),
        step(
            "GET",
            "/v1/playlists/test-list/items",
            json!({"totalNumberOfItems":0,"items":[]}),
        ),
    ]);
    assert!(
        api.playlist_duplicate_page(
            7,
            "test-list",
            0,
            0,
            "\"revision-1\"",
            &AtomicBool::new(false)
        )
        .await
        .unwrap()
        .is_empty()
    );
    server.join().unwrap();
}
#[tokio::test]
async fn duplicate_check_refuses_unavailable_rows_changes_and_cancellation() {
    for case in ["unavailable", "revision", "count", "owner", "cancel"] {
        let cancelled = std::sync::Arc::new(AtomicBool::new(false));
        let mut first = metadata(2);
        if case == "owner" {
            first["creator"]["id"] = json!(8);
        }
        let mut page = step(
            "GET",
            "/v1/playlists/test-list/items",
            json!({"totalNumberOfItems":2,"items":[if case=="unavailable" {json!({"type":"track","item":null})} else {json!({"type":"track","item":{"id":9,"title":"Synthetic"}})}]}),
        );
        if case == "cancel" {
            let flag = cancelled.clone();
            page.on_request = Some(Box::new(move || {
                flag.store(true, std::sync::atomic::Ordering::Release)
            }));
        }
        let mut steps = vec![step("GET", "/v1/playlists/test-list", first), page];
        if case == "revision" {
            let mut changed = step("GET", "/v1/playlists/test-list", metadata(2));
            changed.etag = "\"revision-2\"";
            steps.push(changed);
        }
        if case == "count" {
            steps.push(step("GET", "/v1/playlists/test-list", metadata(3)));
            steps.push(step(
                "GET",
                "/v1/playlists/test-list/items",
                json!({"totalNumberOfItems":2,"items":[{"type":"track","item":{"id":10,"title":"Synthetic"}}]}),
            ));
        }
        let (mut api, server) = mock(steps);
        let result = api
            .playlist_duplicate_page(7, "test-list", 0, 2, "\"revision-1\"", &cancelled)
            .await;
        if case == "revision" || case == "count" {
            assert_eq!(result.unwrap(), [9]);
            assert!(
                api.playlist_duplicate_page(7, "test-list", 1, 2, "\"revision-1\"", &cancelled)
                    .await
                    .is_err(),
                "{case}"
            );
        } else {
            assert!(result.is_err(), "{case}");
        }
        server.join().unwrap();
    }
    for user in [0, 8] {
        let (mut api, server) = mock(vec![]);
        assert!(
            api.playlist_duplicate_page(
                user,
                "test-list",
                0,
                0,
                "\"revision-1\"",
                &AtomicBool::new(false)
            )
            .await
            .is_err()
        );
        server.join().unwrap();
    }
}

#[tokio::test]
async fn queue_batch_preserves_duplicates_and_verifies_order_at_the_write_revision() {
    for offset in [0, 7] {
        let mut post = step("POST", "/v1/playlists/test-list/items", json!({}));
        post.etag = "\"revision-2\"";
        post.contains = vec![
            "trackIds=9%2C9%2C10",
            "onDupes=ADD",
            "onArtifactNotFound=FAIL",
            if offset == 0 {
                "toIndex=0"
            } else {
                "toIndex=7"
            },
            "if-none-match: \"revision-1\"",
        ];
        let mut after = step("GET", "/v1/playlists/test-list", metadata(offset + 3));
        after.etag = post.etag;
        let mut page = step(
            "GET",
            "/v1/playlists/test-list/items",
            json!({"totalNumberOfItems":offset+3,"items":[
        {"type":"track","item":{"id":9,"title":"Test"}}, {"type":"track","item":{"id":9,"title":"Test"}}, {"type":"track","item":{"id":10,"title":"Test"}}]}),
        );
        page.etag = post.etag;
        page.contains = vec![if offset == 0 { "offset=0" } else { "offset=7" }];
        let (mut api, server) = mock(vec![
            step("GET", "/v1/playlists/test-list", metadata(offset)),
            post,
            after,
            page,
        ]);
        let etag = api
            .append_queue_batch(
                7,
                "test-list",
                &[9, 9, 10],
                offset,
                (offset != 0).then_some("\"revision-1\""),
                &AtomicBool::new(false),
            )
            .await
            .unwrap();
        assert_eq!(etag, "\"revision-2\"");
        server.join().unwrap();
    }
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
