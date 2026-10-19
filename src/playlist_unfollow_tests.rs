use super::playlist_tests::{mock, step};
use super::*;
use serde_json::json;
use std::sync::Arc;

fn metadata() -> Value {
    json!({"uuid":"test-list","title":"Other playlist","type":"USER","creator":{"id":8}})
}
fn collection(ids: &[&str], total: u64) -> Value {
    json!({"totalNumberOfItems":total,"items":ids.iter().map(|id|json!({"playlist":{"uuid":id}})).collect::<Vec<_>>()})
}

#[test]
fn absence_requires_complete_unique_single_response_without_pagination() {
    assert!(complete_playlist_absence(&collection(&[], 0), "test-list"));
    assert!(complete_playlist_absence(
        &collection(&["other"], 1),
        "test-list"
    ));
    for value in [
        collection(&["test-list"], 1),
        collection(&[], 51),
        collection(&["other", "other"], 2),
        collection(&[""], 1),
        json!({"totalNumberOfItems":0}),
        json!({"items":[]}),
        collection(&["bad/id"], 1),
    ] {
        assert!(!complete_playlist_absence(&value, "test-list"));
    }
    for (key, value) in [
        ("offset", json!(1)),
        ("offset", json!("0")),
        ("hasMore", json!(true)),
        ("hasMore", json!("false")),
        ("cursor", json!("next")),
        ("cursor", json!(1)),
    ] {
        let mut data = collection(&[], 0);
        data[key] = value;
        assert!(!complete_playlist_absence(&data, "test-list"));
    }
}

#[tokio::test]
async fn unfollow_uses_only_playlist_trn_and_keeps_partial_verification_unknown() {
    for (value, expected) in [
        (collection(&[], 0), UnfollowVerification::Absent),
        (collection(&[], 51), UnfollowVerification::Unknown),
        (collection(&["test-list"], 1), UnfollowVerification::Unknown),
    ] {
        let mut write = step(
            "PUT",
            "/v2/my-collection/playlists/folders/remove",
            json!({}),
        );
        write.contains = vec!["trns=trn%3Aplaylist%3Atest-list"];
        let mut verify = step("GET", "/v1/users/7/playlistsAndFavoritePlaylists", value);
        verify.contains = vec!["limit=50", "offset=0"];
        let (mut api, server) = mock(vec![
            step("GET", "/v1/playlists/test-list", metadata()),
            write,
            verify,
        ]);
        assert_eq!(
            api.unfollow_playlist(7, "test-list", "\"revision-1\"", &AtomicBool::new(false))
                .await
                .unwrap(),
            expected
        );
        server.join().unwrap();
    }
}

#[tokio::test]
async fn unfollow_rejects_owned_unknown_stale_wrong_account_and_cancelled_scope() {
    for case in 0..5 {
        let mut value = metadata();
        match case {
            0 => value["creator"]["id"] = json!(7),
            1 => value["creator"] = Value::Null,
            2 => value["type"] = json!("UNKNOWN"),
            _ => {}
        }
        let token = Arc::new(AtomicBool::new(false));
        let cancel = token.clone();
        let mut before = step("GET", "/v1/playlists/test-list", value);
        if case == 3 {
            before.etag = "changed";
        }
        if case == 4 {
            before.on_request = Some(Box::new(move || cancel.store(true, Ordering::Release)));
        }
        let (mut api, server) = mock(vec![before]);
        assert!(
            api.unfollow_playlist(7, "test-list", "\"revision-1\"", &token)
                .await
                .is_err()
        );
        server.join().unwrap();
    }
    let (mut api, server) = mock(vec![]);
    assert!(
        api.unfollow_playlist(8, "test-list", "x", &AtomicBool::new(false))
            .await
            .is_err()
    );
    assert!(
        api.unfollow_playlist(7, "test-list", "x", &AtomicBool::new(true))
            .await
            .is_err()
    );
    server.join().unwrap();
}

#[tokio::test]
async fn unfollow_does_not_retry_or_treat_read_errors_as_absence() {
    for status in [403, 404, 429, 500] {
        let mut verify = step(
            "GET",
            "/v1/users/7/playlistsAndFavoritePlaylists",
            json!({}),
        );
        verify.status = status;
        let (mut api, server) = mock(vec![
            step("GET", "/v1/playlists/test-list", metadata()),
            step(
                "PUT",
                "/v2/my-collection/playlists/folders/remove",
                json!({}),
            ),
            verify,
        ]);
        assert_eq!(
            api.unfollow_playlist(7, "test-list", "\"revision-1\"", &AtomicBool::new(false))
                .await
                .unwrap(),
            UnfollowVerification::Unknown
        );
        server.join().unwrap();
    }
    let mut write = step(
        "PUT",
        "/v2/my-collection/playlists/folders/remove",
        json!({}),
    );
    write.status = 500;
    let (mut api, server) = mock(vec![
        step("GET", "/v1/playlists/test-list", metadata()),
        write,
    ]);
    assert!(
        api.unfollow_playlist(7, "test-list", "\"revision-1\"", &AtomicBool::new(false))
            .await
            .is_err()
    );
    server.join().unwrap();
}

#[tokio::test]
async fn unfollow_cancellation_after_write_is_uncertain() {
    let token = Arc::new(AtomicBool::new(false));
    let cancel = token.clone();
    let mut write = step(
        "PUT",
        "/v2/my-collection/playlists/folders/remove",
        json!({}),
    );
    write.on_request = Some(Box::new(move || cancel.store(true, Ordering::Release)));
    let (mut api, server) = mock(vec![
        step("GET", "/v1/playlists/test-list", metadata()),
        write,
    ]);
    assert!(
        api.unfollow_playlist(7, "test-list", "\"revision-1\"", &token)
            .await
            .unwrap_err()
            .to_string()
            .contains("may have completed")
    );
    server.join().unwrap();
}

#[test]
fn only_explicit_non_ownership_is_eligible() {
    let mut playlist: Playlist = serde_json::from_value(metadata()).unwrap();
    assert!(playlist.is_known_non_owned(7));
    assert!(!playlist.is_known_non_owned(8));
    assert!(!playlist.is_known_non_owned(0));
    playlist.creator = None;
    assert!(!playlist.is_known_non_owned(7));
    playlist.kind = "EDITORIAL".into();
    assert!(playlist.is_known_non_owned(7));
}
