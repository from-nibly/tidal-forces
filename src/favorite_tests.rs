use super::playlist_tests::{mock, step};
use super::*;
use serde_json::json;

#[tokio::test]
async fn favorite_writes_use_kind_specific_forms_and_accept_empty_success() {
    for (kind, path, field, removal) in [
        (
            FavoriteKind::Tracks,
            "/v1/users/7/favorites/tracks",
            "trackId=91",
            "/v1/users/7/favorites/tracks/91",
        ),
        (
            FavoriteKind::Albums,
            "/v1/users/7/favorites/albums",
            "albumId=91",
            "/v1/users/7/favorites/albums/91",
        ),
        (
            FavoriteKind::Artists,
            "/v1/users/7/favorites/artists",
            "artistId=91",
            "/v1/users/7/favorites/artists/91",
        ),
    ] {
        let mut add = step("POST", path, Value::Null);
        add.status = 204;
        add.contains = vec![field];
        let mut remove = step("DELETE", removal, Value::Null);
        remove.status = 204;
        let (mut api, server) = mock(vec![add, remove]);
        let item = Favorite::new(kind, 91);
        api.set_favorite(7, item, true).await.unwrap();
        api.set_favorite(7, item, false).await.unwrap();
        server.join().unwrap();
    }
}

#[tokio::test]
async fn favorites_refuse_cross_account_commands_and_invalid_ids_before_network() {
    let (mut api, server) = mock(vec![]);
    assert!(
        api.set_favorite(8, Favorite::new(FavoriteKind::Tracks, 91), true)
            .await
            .unwrap_err()
            .to_string()
            .contains("account changed")
    );
    assert!(
        api.set_favorite(7, Favorite::new(FavoriteKind::Tracks, 0), true)
            .await
            .is_err()
    );
    server.join().unwrap();
}

#[tokio::test]
async fn failed_and_uncertain_favorite_writes_are_not_retried() {
    for status in [403, 429, 500, 503, 0] {
        let mut request = step("POST", "/v1/users/7/favorites/tracks", Value::Null);
        request.status = status;
        let (mut api, server) = mock(vec![request]);
        let message = api
            .set_favorite(7, Favorite::new(FavoriteKind::Tracks, 91), true)
            .await
            .unwrap_err()
            .to_string();
        if status >= 500 || status == 0 {
            assert!(message.contains("may have completed"), "{message}");
        }
        server.join().unwrap();
    }
}

#[tokio::test]
async fn library_pages_preserve_raw_offsets_and_unwrap_all_kinds() {
    let mut tracks = step(
        "GET",
        "/v1/users/7/favorites/tracks",
        json!({"totalNumberOfItems":104,"items":[{"item":{"id":1,"title":"Test"}},{"item":null},{"item":{"id":2,"title":"Test 2"}}]}),
    );
    tracks.contains = vec!["offset=100", "limit=100"];
    let (mut api, server) = mock(vec![
        tracks,
        step(
            "GET",
            "/v1/users/7/favorites/albums",
            json!({"totalNumberOfItems":1,"items":[{"item":{"id":3,"title":"Album"}}]}),
        ),
        step(
            "GET",
            "/v1/users/7/favorites/artists",
            json!({"totalNumberOfItems":1,"items":[{"item":{"id":4,"name":"Artist"}}]}),
        ),
    ]);
    let page = api.library(FavoriteKind::Tracks, 100).await.unwrap();
    assert_eq!(page.next_offset, 103);
    assert!(page.more);
    assert_eq!(page.data.tracks.len(), 2);
    let page = api.library(FavoriteKind::Albums, 0).await.unwrap();
    assert_eq!(page.data.albums[0].id, 3);
    assert!(!page.more);
    let page = api.library(FavoriteKind::Artists, 0).await.unwrap();
    assert_eq!(page.data.artists[0].id, 4);
    assert!(!page.more);
    server.join().unwrap();
}

#[test]
fn invalid_pagination_fails_instead_of_looping_or_dropping_positions() {
    assert!(
        parse_library_page(
            json!({"totalNumberOfItems":1,"items":[]}),
            FavoriteKind::Tracks,
            0
        )
        .is_err()
    );
    assert!(parse_library_page(json!({"items":[null]}), FavoriteKind::Tracks, usize::MAX).is_err());
    let page = parse_library_page(
        json!({"totalNumberOfItems":1,"items":[null]}),
        FavoriteKind::Tracks,
        0,
    )
    .unwrap();
    assert_eq!(page.next_offset, 1);
    assert!(!page.more);
    assert!(page.data.tracks.is_empty());
}
