use super::*;
use crate::ui::tests::{fixture, track};

#[test]
fn disk_snapshot_survives_restart_without_restoring_write_authority_or_pagination() {
    use crate::metadata_cache::{Record, Snapshot as DiskSnapshot};
    let root = tempfile::tempdir().unwrap();
    let location = Location {
        page: Page::Collection {
            kind: "playlists".into(),
            id: "test-list".into(),
            title: "Fixture".into(),
        },
        query: String::new(),
    };
    crate::metadata_cache::save(
        root.path(),
        Record::new(
            7,
            vec![DiskSnapshot {
                location: location.clone(),
                tracks: vec![track(1), track(1)],
                albums: vec![],
                artists: vec![],
            }],
        ),
    )
    .unwrap();
    let (ctx, mut app, _events, mut requests) = fixture();
    app.appearance.metadata_cache = true;
    app.browse_loaded();
    app.state_store = Some(StateStore::new(root.path().into(), ctx).unwrap());
    // Startup's fresh Home read must not overwrite the disk cache before loading it.
    app.fresh_browse_read();
    app.load_metadata_cache();
    let crate::player_state::Event::CacheLoaded {
        user,
        request,
        result,
    } = app
        .state_store
        .as_ref()
        .unwrap()
        .events
        .recv_timeout(std::time::Duration::from_secs(2))
        .unwrap()
    else {
        panic!()
    };
    app.metadata_cache_loaded(user, request, result);
    let queue = app.queue.revision();
    app.navigate(location.page);
    assert!(app.browse_cache.persisted);
    assert!(app.browse_cache.restored);
    assert_eq!(app.tracks.len(), 2);
    assert!(app.playlist_page.is_none());
    assert!(!app.more);
    assert!(!app.can_reorder_playlist());
    assert!(!app.library_read_valid);
    assert_eq!(app.queue.revision(), queue);
    assert!(requests.try_recv().is_err());
    app.load_page(0);
    assert!(matches!(
        requests.try_recv(),
        Ok(Request::Playlist {
            offset: 0,
            etag: None,
            ..
        })
    ));
    drop(app.state_store.take());
    assert!(
        crate::metadata_cache::load(root.path(), 7)
            .unwrap()
            .entries
            .is_empty()
    );
}
#[test]
fn late_disk_loads_cannot_cross_accounts_or_restore_invalidated_snapshots() {
    use crate::metadata_cache::{Record, Snapshot as DiskSnapshot};
    let (_, mut app, _events, _requests) = fixture();
    app.appearance.metadata_cache = true;
    app.browse_loaded();
    let request = app.browse_cache.disk_serial;
    let snapshot = || DiskSnapshot {
        location: Location {
            page: Page::Track(1),
            query: String::new(),
        },
        tracks: vec![track(1)],
        albums: vec![],
        artists: vec![],
    };
    app.invalidate_browse_cache();
    app.metadata_cache_loaded(7, request, Ok(Record::new(7, vec![snapshot()])));
    assert!(app.browse_cache.entries.is_empty());
    app.account = Some(8);
    let request = app.browse_cache.disk_serial;
    app.metadata_cache_loaded(7, request, Ok(Record::new(7, vec![snapshot()])));
    assert!(app.browse_cache.entries.is_empty());
}

#[test]
fn back_forward_restore_occurrences_options_selection_and_scroll_without_playing() {
    let (_, mut app, events, mut requests) = fixture();
    app.query = "first search".into();
    app.navigate(Page::Search);
    events
        .send(Event::Search {
            generation: app.generation,
            data: crate::api::Search {
                tracks: vec![track(3), track(1), track(1)],
                ..Default::default()
            },
        })
        .unwrap();
    app.events();
    app.track_view.query = "synthetic".into();
    app.track_view.sort = track_view::Sort::Title;
    app.sync_track_view();
    app.track_selection
        .sync(app.account, app.generation, app.tracks.len());
    app.track_selection.select(2, false, false);
    app.track_selection.pending_focus = Some(2);
    app.browse_cache.scroll = 321.;
    app.queue
        .start(vec![track(9)], 0, "Playing".into(), None)
        .unwrap();
    app.queue.add(track(8), false).unwrap();
    app.paused = false;
    let revision = app.queue.revision();
    app.query = "second search".into();
    app.navigate(Page::Search);
    let second_generation = app.generation;
    events
        .send(Event::Search {
            generation: app.generation,
            data: crate::api::Search {
                tracks: vec![track(4)],
                ..Default::default()
            },
        })
        .unwrap();
    app.events();
    while requests.try_recv().is_ok() {}
    // A newer membership check is independent of the older browse data.
    app.favorites.invalidate(FavoriteKind::Tracks);
    app.favorites.mark_complete(FavoriteKind::Tracks);
    app.history(false);
    assert!(requests.try_recv().is_err());
    assert_eq!(app.search_query, "first search");
    assert_eq!(
        app.tracks.iter().map(|t| t.id).collect::<Vec<_>>(),
        vec![3, 1, 1]
    );
    app.sync_track_view();
    assert_eq!(app.track_view.query, "synthetic");
    assert_eq!(app.track_view.rows, vec![1, 2, 0]);
    assert!(app.track_selection.rows.contains(&2));
    assert_eq!(app.track_selection.pending_focus, None);
    assert_eq!(app.browse_cache.pending_scroll, Some(321.));
    assert!(app.browse_cache.restored);
    assert_eq!(
        app.favorites.state(Favorite::new(FavoriteKind::Tracks, 1)),
        Some(false)
    );
    assert_eq!(app.queue.revision(), revision);
    assert!(!app.paused);
    events
        .send(Event::Search {
            generation: second_generation,
            data: crate::api::Search {
                tracks: vec![track(99)],
                ..Default::default()
            },
        })
        .unwrap();
    app.events();
    assert_eq!(app.tracks[0].id, 3);
    app.history(true);
    assert_eq!(app.search_query, "second search");
    assert_eq!(app.tracks[0].id, 4);
    assert!(requests.try_recv().is_err());
    assert_eq!(app.queue.revision(), revision);
}

#[test]
fn restored_playlist_preserves_raw_indices_revision_and_guarded_pagination() {
    let (_, mut app, events, mut requests) = fixture();
    app.page = Page::Collection {
        kind: "playlists".into(),
        id: "test-list".into(),
        title: "Old title".into(),
    };
    events
        .send(Event::Playlist {
            generation: app.generation,
            append: false,
            page: PlaylistPage {
                playlist: Playlist {
                    uuid: "test-list".into(),
                    title: "Fixture title".into(),
                    number_of_tracks: 200,
                    ..Default::default()
                },
                editable: true,
                etag: "r1".into(),
                rows: vec![(0, track(1)), (101, track(2))],
                next_offset: 102,
                more: true,
            },
        })
        .unwrap();
    app.events();
    app.track_selection
        .sync(app.account, app.generation, app.tracks.len());
    app.track_selection.select(1, false, false);
    app.navigate(Page::Settings);
    app.history(false);
    assert!(app.browse_cache.restored);
    assert!(app.track_selection.rows.contains(&1));
    let p = app.playlist_page.as_ref().unwrap();
    assert_eq!(p.rows[1].0, 101);
    assert_eq!(p.etag, "r1");
    assert_eq!(p.next_offset, 102);
    app.load_page(102);
    match requests.try_recv().unwrap() {
        Request::Playlist {
            generation,
            id,
            offset,
            etag,
        } => {
            assert_eq!(generation, app.generation);
            assert_eq!(id, "test-list");
            assert_eq!(offset, 102);
            assert_eq!(etag.as_deref(), Some("r1"));
        }
        _ => panic!("expected revision-pinned continuation"),
    }
}

#[test]
fn appending_to_restored_favorites_cannot_infer_absence_from_cached_prefix_rows() {
    let (_, mut app, events, _) = fixture();
    app.page = Page::Library(FavoriteKind::Tracks);
    events
        .send(Event::Library {
            generation: app.generation,
            kind: FavoriteKind::Tracks,
            append: false,
            page: crate::api::LibraryPage {
                data: crate::api::Search {
                    tracks: vec![track(1)],
                    ..Default::default()
                },
                next_offset: 1,
                more: true,
                total: Some(3),
            },
        })
        .unwrap();
    app.events();
    app.navigate(Page::Settings);
    app.favorites = Favorites::default(); // an independent membership refresh
    app.history(false);
    assert!(!app.library_read_valid);
    events
        .send(Event::Library {
            generation: app.generation,
            kind: FavoriteKind::Tracks,
            append: true,
            page: crate::api::LibraryPage {
                data: crate::api::Search {
                    tracks: vec![track(2), track(3)],
                    ..Default::default()
                },
                next_offset: 3,
                more: false,
                total: Some(3),
            },
        })
        .unwrap();
    app.events();
    assert_eq!(app.tracks.len(), 3);
    assert_eq!(
        app.favorites.state(Favorite::new(FavoriteKind::Tracks, 1)),
        None
    );
    assert_eq!(
        app.favorites.state(Favorite::new(FavoriteKind::Tracks, 2)),
        Some(true)
    );
    assert!(app.browse_cache.restored); // the prefix still came from a snapshot
}

#[test]
fn fresh_navigation_and_refresh_do_not_reuse_cached_data() {
    let (_, mut app, events, mut requests) = fixture();
    app.page = Page::Track(1);
    app.tracks = vec![track(1)];
    app.browse_loaded();
    app.navigate(Page::Settings);
    app.navigate(Page::Track(1));
    assert!(!app.browse_cache.restored);
    assert!(app.tracks.is_empty());
    assert!(matches!(
        requests.try_recv().unwrap(),
        Request::Track { id: 1, .. }
    ));
    app.tracks = vec![track(1)];
    app.loading = false;
    app.browse_loaded();
    app.navigate(Page::Settings);
    app.history(false);
    assert!(app.browse_cache.restored);
    app.load_page(0);
    assert!(app.browse_cache.restored); // old rows remain visibly cached until success
    assert!(!app.browse_cache.ready);
    assert!(matches!(
        requests.try_recv().unwrap(),
        Request::Track { id: 1, .. }
    ));
    events
        .send(Event::Tracks {
            generation: app.generation,
            tracks: vec![track(1)],
            append: false,
        })
        .unwrap();
    app.events();
    assert!(!app.browse_cache.restored);
}

#[test]
fn snapshots_are_bounded_and_titles_do_not_split_the_same_source() {
    let (_, mut app, _, _) = fixture();
    for id in 1..=10 {
        app.page = Page::Track(id);
        app.tracks = vec![track(id)];
        app.browse_loaded();
        app.save_browse_view();
    }
    assert_eq!(app.browse_cache.entries.len(), MAX_VIEWS);
    assert_eq!(
        app.browse_cache.entries.front().unwrap().location.page,
        Page::Track(3)
    );
    app.invalidate_browse_cache();
    for id in 1..=3 {
        app.page = Page::Track(id);
        app.tracks = vec![
            Track {
                id,
                ..Default::default()
            };
            20_000
        ];
        app.browse_loaded();
        app.save_browse_view();
    }
    assert_eq!(app.browse_cache.entries.len(), 2);
    assert!(
        app.browse_cache
            .entries
            .iter()
            .map(Snapshot::rows)
            .sum::<usize>()
            <= MAX_ROWS
    );
    app.page = Page::Track(4);
    app.tracks = vec![Track::default(); MAX_ROWS + 1];
    app.browse_loaded();
    app.save_browse_view();
    assert_eq!(app.browse_cache.entries.len(), 2);
    let location = |title: &str| Location {
        page: Page::Collection {
            kind: "playlists".into(),
            id: "a".into(),
            title: title.into(),
        },
        query: String::new(),
    };
    assert!(same_location(
        &location("Placeholder"),
        &location("Loaded title")
    ));
}

#[test]
fn incomplete_failed_and_cross_account_views_cannot_be_restored() {
    let (_, mut app, events, _) = fixture();
    app.page = Page::Track(1);
    app.tracks = vec![track(1)];
    app.browse_loaded();
    app.loading = true;
    app.save_browse_view();
    assert!(app.browse_cache.entries.is_empty());
    events
        .send(Event::RequestError {
            generation: app.generation,
            message: "Synthetic failure".into(),
        })
        .unwrap();
    app.events();
    app.save_browse_view();
    assert!(app.browse_cache.entries.is_empty());
    app.browse_loaded();
    let location = app.location();
    app.save_browse_view();
    app.account = Some(8);
    assert!(!app.restore_browse_view(&location));
    assert!(app.browse_cache.entries.is_empty());
}

#[test]
fn restored_scroll_is_applied_by_the_native_scroll_area() {
    let (ctx, mut app, _, _) = fixture();
    app.page = Page::Search;
    app.query = "fixture".into();
    app.search_query = "fixture".into();
    app.tracks = (1..=200).map(track).collect();
    app.browse_loaded();
    app.browse_cache.pending_scroll = Some(1200.);
    crate::ui::tests::frame(&ctx, &mut app, vec2(1240., 820.));
    let offset = app.browse_cache.scroll;
    assert!(offset > 1000.);
    app.navigate(Page::Settings);
    crate::ui::tests::frame(&ctx, &mut app, vec2(1240., 820.));
    app.history(false);
    crate::ui::tests::frame(&ctx, &mut app, vec2(1240., 820.));
    assert!(app.browse_cache.restored);
    assert!((app.browse_cache.scroll - offset).abs() < 1.);
    assert!(app.browse_cache.pending_scroll.is_none());
}

#[test]
fn session_events_clear_snapshots_before_loading_another_accounts_page() {
    let (_, mut app, events, _) = fixture();
    app.page = Page::Track(1);
    app.tracks = vec![track(1)];
    app.browse_loaded();
    app.save_browse_view();
    events.send(Event::Session(Some((8, "US".into())))).unwrap();
    app.events();
    assert!(app.browse_cache.entries.is_empty());
    assert!(!app.browse_cache.ready);
    assert!(app.tracks.is_empty());
    assert!(!app.navigation.can_back());
}

#[test]
fn writes_invalidate_snapshots_both_before_sending_and_after_completion() {
    let (_, mut app, events, _) = fixture();
    app.page = Page::Track(1);
    app.tracks = vec![track(1)];
    app.browse_loaded();
    app.save_browse_view();
    app.try_send(Request::SetFavorite {
        user: 7,
        item: Favorite::new(FavoriteKind::Tracks, 1),
        saved: true,
    });
    assert!(app.browse_cache.entries.is_empty());
    app.tracks = vec![track(1)];
    app.browse_loaded();
    app.save_browse_view();
    events
        .send(Event::Favorite {
            user: 7,
            item: Favorite::new(FavoriteKind::Tracks, 1),
            result: Ok(true),
        })
        .unwrap();
    app.events();
    assert!(app.browse_cache.entries.is_empty());
    assert!(!app.browse_cache.ready);
}
