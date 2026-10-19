use super::playlist_tests::{mock, step};
use super::*;
use serde_json::json;
use std::sync::Arc;

fn metadata() -> Value {
    json!({"uuid":"test-list","title":"Fixture","type":"USER","creator":{"id":7},"numberOfTracks":205,"numberOfVideos":1})
}

#[tokio::test]
async fn deletion_checks_ownership_revision_then_verifies_exact_absence() {
    let mut write = step("DELETE", "/v1/playlists/test-list", json!({}));
    write.contains = vec!["if-none-match: \"revision-1\""];
    let mut missing = step("GET", "/v1/playlists/test-list", json!({}));
    missing.status = 404;
    let (mut api, server) = mock(vec![
        step("GET", "/v1/playlists/test-list", metadata()),
        write,
        missing,
    ]);
    api.delete_playlist(7, "test-list", "\"revision-1\"", &AtomicBool::new(false))
        .await
        .unwrap();
    server.join().unwrap();
}

#[tokio::test]
async fn deletion_rejects_foreign_stale_missing_and_cancelled_preflights() {
    for case in 0..5 {
        let cancelled = Arc::new(AtomicBool::new(false));
        let mut value = metadata();
        if case == 0 {
            value["creator"]["id"] = json!(8);
        }
        if case == 1 {
            value["uuid"] = json!("other-list");
        }
        let mut before = step("GET", "/v1/playlists/test-list", value);
        if case == 2 {
            before.etag = "changed";
        }
        if case == 3 {
            before.status = 404;
        }
        if case == 4 {
            let token = cancelled.clone();
            before.on_request = Some(Box::new(move || token.store(true, Ordering::Release)));
        }
        let (mut api, server) = mock(vec![before]);
        assert!(
            api.delete_playlist(7, "test-list", "\"revision-1\"", &cancelled)
                .await
                .is_err()
        );
        server.join().unwrap();
    }
    let (mut api, server) = mock(vec![]);
    assert!(
        api.delete_playlist(8, "test-list", "x", &AtomicBool::new(false))
            .await
            .is_err()
    );
    assert!(
        api.delete_playlist(7, "test-list", "x", &AtomicBool::new(true))
            .await
            .is_err()
    );
    server.join().unwrap();
}

#[tokio::test]
async fn deletion_does_not_retry_writes_or_confuse_denial_with_absence() {
    for status in [200, 403, 429, 500] {
        let mut verify = step("GET", "/v1/playlists/test-list", json!({}));
        verify.status = status;
        let (mut api, server) = mock(vec![
            step("GET", "/v1/playlists/test-list", metadata()),
            step("DELETE", "/v1/playlists/test-list", json!({})),
            verify,
        ]);
        assert!(
            api.delete_playlist(7, "test-list", "\"revision-1\"", &AtomicBool::new(false))
                .await
                .unwrap_err()
                .to_string()
                .contains("could not be verified")
        );
        server.join().unwrap();
    }
    for status in [404, 409, 412, 500] {
        let mut write = step("DELETE", "/v1/playlists/test-list", json!({}));
        write.status = status;
        let (mut api, server) = mock(vec![
            step("GET", "/v1/playlists/test-list", metadata()),
            write,
        ]);
        assert!(
            api.delete_playlist(7, "test-list", "\"revision-1\"", &AtomicBool::new(false))
                .await
                .is_err()
        );
        server.join().unwrap();
    }
}

#[tokio::test]
async fn cancellation_after_delete_never_claims_verified_success() {
    let token = Arc::new(AtomicBool::new(false));
    let cancel = token.clone();
    let mut write = step("DELETE", "/v1/playlists/test-list", json!({}));
    write.on_request = Some(Box::new(move || cancel.store(true, Ordering::Release)));
    let (mut api, server) = mock(vec![
        step("GET", "/v1/playlists/test-list", metadata()),
        write,
    ]);
    assert!(
        api.delete_playlist(7, "test-list", "\"revision-1\"", &token)
            .await
            .unwrap_err()
            .to_string()
            .contains("may have completed")
    );
    server.join().unwrap();
}

#[test]
fn a_request_cannot_follow_token_renewal_into_another_known_account() {
    assert!(ensure_request_user(7, 7).is_ok());
    assert!(ensure_request_user(7, 8).is_err());
    assert!(ensure_request_user(7, 0).is_err());
    assert!(ensure_request_user(0, 7).is_ok()); // Initial sign-in identification.
}

#[test]
fn missing_resource_errors_do_not_display_resource_identity() {
    let error = RequestNotFound {
        path: "private-playlist-identity".into(),
    };
    assert!(!error.to_string().contains("private-playlist"));
    assert!(!format!("{error:?}").contains("private-playlist"));
}
