use super::playlist_tests::{mock, step};
use super::*;
use serde_json::json;

#[tokio::test]
async fn album_continuation_loads_beyond_the_initial_hundred() {
    let mut page = step(
        "GET",
        "/v1/albums/12/tracks",
        json!({"totalNumberOfItems":102,
        "items":[{"id":101,"title":"101"},{"id":102,"title":"102"}]}),
    );
    page.contains = vec!["offset=100", "limit=100"];
    let (mut api, server) = mock(vec![page]);
    let page = api
        .context_page(
            7,
            &Continuation::Album {
                id: 12,
                offset: 100,
            },
        )
        .await
        .unwrap();
    assert_eq!(
        page.tracks.iter().map(|track| track.id).collect::<Vec<_>>(),
        [101, 102]
    );
    assert!(page.continuation.is_none());
    server.join().unwrap();
}

#[tokio::test]
async fn playlist_continuation_keeps_revision_raw_positions_and_duplicate_songs() {
    let mut page = step(
        "GET",
        "/v1/playlists/test-list/items",
        json!({"totalNumberOfItems":105,
        "items":[{"type":"track","item":{"id":9,"title":"Duplicate"}},
        {"type":"video","item":{"id":10}}, {"type":"track","item":{"id":9,"title":"Duplicate"}}]}),
    );
    page.contains = vec!["offset=100"];
    let (mut api, server) = mock(vec![
        step(
            "GET",
            "/v1/playlists/test-list",
            json!({"uuid":"test-list","title":"Test","type":"USER","creator":{"id":7}}),
        ),
        page,
    ]);
    let page = api
        .context_page(
            7,
            &Continuation::Playlist {
                id: "test-list".into(),
                etag: "\"revision-1\"".into(),
                offset: 100,
            },
        )
        .await
        .unwrap();
    assert_eq!(page.tracks.len(), 2);
    assert_eq!(page.tracks[0].id, page.tracks[1].id);
    assert!(matches!(
        page.continuation,
        Some(Continuation::Playlist { offset: 103, .. })
    ));
    server.join().unwrap();
}

#[tokio::test]
async fn changed_playlist_or_favorite_snapshot_is_not_silently_combined() {
    let (mut api, server) = mock(vec![step(
        "GET",
        "/v1/playlists/test-list",
        json!({"uuid":"test-list","title":"Test"}),
    )]);
    assert!(
        api.context_page(
            7,
            &Continuation::Playlist {
                id: "test-list".into(),
                etag: "old".into(),
                offset: 100
            }
        )
        .await
        .unwrap_err()
        .to_string()
        .contains("changed elsewhere")
    );
    server.join().unwrap();
    let (mut api, server) = mock(vec![step(
        "GET",
        "/v1/users/7/favorites/tracks",
        json!({"totalNumberOfItems":101,"items":[{"item":{"id":101,"title":"Changed"}}]}),
    )]);
    assert!(
        api.context_page(
            7,
            &Continuation::Favorites {
                offset: 100,
                total: Some(102)
            }
        )
        .await
        .is_err()
    );
    server.join().unwrap();
}

#[tokio::test]
async fn old_account_continuation_cannot_query_the_new_account() {
    let (mut api, server) = mock(vec![]);
    assert!(
        api.context_page(
            8,
            &Continuation::Favorites {
                offset: 100,
                total: Some(101)
            }
        )
        .await
        .is_err()
    );
    server.join().unwrap();
}
