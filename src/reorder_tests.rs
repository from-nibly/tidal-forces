use super::playlist_tests::{mock, step};
use super::*;
use crate::reorder::{Check, Move, Target};
use serde_json::json;
use std::sync::Arc;

fn change() -> Move {
    Move {
        user: 7,
        id: "test-list".into(),
        etag: "\"revision-1\"".into(),
        ids: vec![9, 8, 9],
        from: 2,
        target: Target::Top,
    }
}
fn check(offset: usize, count: usize) -> Check {
    Check {
        user: 7,
        id: "test-list".into(),
        etag: "\"revision-1\"".into(),
        offset,
        count,
    }
}
fn metadata(count: usize) -> Value {
    json!({"uuid":"test-list","title":"Fixture","type":"USER","numberOfTracks":count,"numberOfVideos":0,"creator":{"id":7}})
}
fn items(ids: &[u64], count: usize) -> Value {
    json!({"totalNumberOfItems":count,"items":ids.iter().map(|id|json!({"type":"track","item":{"id":id,"title":"Fixture"}})).collect::<Vec<_>>()})
}

#[tokio::test]
async fn reorder_reads_bounded_raw_pages_at_pinned_revisions() {
    for offset in [0, 100, 200] {
        let ids: Vec<_> = (offset..(offset + 100).min(205))
            .map(|i| (i % 7 + 1) as u64)
            .collect();
        let mut rows = step("GET", "/v1/playlists/test-list/items", items(&ids, 205));
        rows.contains = vec![
            "limit=100",
            match offset {
                0 => "offset=0",
                100 => "offset=100",
                _ => "offset=200",
            },
        ];
        let (mut api, server) = mock(vec![
            step("GET", "/v1/playlists/test-list", metadata(205)),
            rows,
        ]);
        assert_eq!(
            api.check_reorder_page(&check(offset, 205), &AtomicBool::new(false))
                .await
                .unwrap(),
            ids
        );
        server.join().unwrap();
    }
}
#[tokio::test]
async fn reorder_page_rejects_holes_videos_count_and_revision_changes() {
    for case in 0..5 {
        let mut value = items(&[9, 8, 9], 3);
        match case {
            0 => value["items"][0]["item"] = Value::Null,
            1 => value["items"][0]["type"] = json!("video"),
            2 => value["totalNumberOfItems"] = json!(4),
            3 => {
                value["items"].as_array_mut().unwrap().pop();
            }
            _ => {}
        }
        let mut rows = step("GET", "/v1/playlists/test-list/items", value);
        if case == 4 {
            rows.etag = "\"revision-2\"";
        }
        let (mut api, server) = mock(vec![
            step("GET", "/v1/playlists/test-list", metadata(3)),
            rows,
        ]);
        assert!(
            api.check_reorder_page(&check(0, 3), &AtomicBool::new(false))
                .await
                .is_err()
        );
        server.join().unwrap();
    }
}
#[tokio::test]
async fn reorder_checks_account_owner_revision_bounds_and_cancellation_before_work() {
    for case in 0..3 {
        let mut meta = metadata(3);
        if case == 0 {
            meta["creator"]["id"] = json!(8);
        }
        if case == 1 {
            meta["numberOfVideos"] = json!(1);
        }
        let mut request = check(0, 3);
        if case == 2 {
            request.etag = "old".into();
        }
        let (mut api, server) = mock(vec![step("GET", "/v1/playlists/test-list", meta)]);
        assert!(
            api.check_reorder_page(&request, &AtomicBool::new(false))
                .await
                .is_err()
        );
        server.join().unwrap();
    }
    let (mut api, server) = mock(vec![]);
    let mut request = check(0, 3);
    request.user = 8;
    assert!(
        api.check_reorder_page(&request, &AtomicBool::new(false))
            .await
            .is_err()
    );
    assert!(
        api.check_reorder_page(&check(1, 3), &AtomicBool::new(false))
            .await
            .is_err()
    );
    assert!(
        api.check_reorder_page(&check(0, 3), &AtomicBool::new(true))
            .await
            .is_err()
    );
    assert!(
        api.write_reorder(&change(), &AtomicBool::new(true))
            .await
            .is_err()
    );
    server.join().unwrap();
}
#[tokio::test]
async fn single_reorder_write_uses_raw_occurrences_and_new_revision() {
    for (from, target, path, to) in [
        (
            2,
            Target::Top,
            "/v1/playlists/test-list/items/2",
            "toIndex=0",
        ),
        (
            0,
            Target::Bottom,
            "/v1/playlists/test-list/items/0",
            "toIndex=3",
        ),
        (
            2,
            Target::Up,
            "/v1/playlists/test-list/items/2",
            "toIndex=1",
        ),
        (
            1,
            Target::Down,
            "/v1/playlists/test-list/items/2",
            "toIndex=1",
        ),
    ] {
        let mut change = change();
        change.from = from;
        change.target = target;
        let mut write = step("POST", path, json!({}));
        write.etag = "\"revision-2\"";
        write.contains = vec![to, "if-none-match: \"revision-1\""];
        let (mut api, server) = mock(vec![
            step("GET", "/v1/playlists/test-list", metadata(3)),
            write,
        ]);
        assert_eq!(
            api.write_reorder(&change, &AtomicBool::new(false))
                .await
                .unwrap(),
            "\"revision-2\""
        );
        server.join().unwrap();
    }
}
#[tokio::test]
async fn reorder_write_rechecks_revision_and_cancellation_and_never_claims_ambiguous_success() {
    let cancelled = Arc::new(AtomicBool::new(false));
    let signal = cancelled.clone();
    let mut meta = step("GET", "/v1/playlists/test-list", metadata(3));
    meta.on_request = Some(Box::new(move || signal.store(true, Ordering::Release)));
    let (mut api, server) = mock(vec![meta]);
    assert!(api.write_reorder(&change(), &cancelled).await.is_err());
    server.join().unwrap();
    let mut meta = step("GET", "/v1/playlists/test-list", metadata(3));
    meta.etag = "\"revision-2\"";
    let (mut api, server) = mock(vec![meta]);
    assert!(
        api.write_reorder(&change(), &AtomicBool::new(false))
            .await
            .is_err()
    );
    server.join().unwrap();
    for case in 0..3 {
        let mut write = step("POST", "/v1/playlists/test-list/items/2", json!({}));
        match case {
            0 => write.status = 500,
            1 => write.etag = "",
            _ => {}
        }
        let (mut api, server) = mock(vec![
            step("GET", "/v1/playlists/test-list", metadata(3)),
            write,
        ]);
        assert!(
            api.write_reorder(&change(), &AtomicBool::new(false))
                .await
                .is_err()
        );
        server.join().unwrap();
    }
}
