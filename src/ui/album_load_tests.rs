use super::*;
use crate::ui::tests::{fixture, track};

fn page(ids: &[u64], offset: usize, total: Option<u64>, more: bool) -> LibraryPage {
    LibraryPage {
        data: crate::api::Search {
            tracks: ids.iter().map(|id| track(*id)).collect(),
            ..Default::default()
        },
        next_offset: offset,
        total,
        more,
    }
}
fn prepare(app: &mut App) {
    app.page = Page::Collection {
        kind: "albums".into(),
        id: "12".into(),
        title: "Fixture album".into(),
    };
    app.album_page(
        app.generation,
        "albums",
        "12",
        0,
        page(&[1, 1], 100, Some(202), true),
    );
}

#[test]
fn raw_album_pages_preserve_holes_duplicates_and_queue_state() {
    let (_, mut app, events, mut requests) = fixture();
    prepare(&mut app);
    assert_eq!(app.tracks.len(), 2);
    assert_eq!(app.album_position.as_ref().unwrap().offset, 100);
    let revision = app.queue.revision();
    app.start_album_load();
    for (offset, end, ids) in [(100, 200, vec![2, 2]), (200, 202, vec![3])] {
        app.album_load_tick();
        assert!(matches!(requests.try_recv(),Ok(Request::Collection{offset:o,..}) if o==offset));
        app.album_load_tick();
        assert!(requests.try_recv().is_err());
        events
            .send(Event::CollectionPage {
                generation: app.generation,
                kind: "albums".into(),
                id: "12".into(),
                offset,
                page: page(&ids, end, Some(202), end < 202),
            })
            .unwrap();
        app.events();
    }
    assert!(app.album_load.is_none());
    assert!(!app.more);
    assert_eq!(
        app.tracks.iter().map(|t| t.id).collect::<Vec<_>>(),
        [1, 1, 2, 2, 3]
    );
    assert_eq!(app.queue.revision(), revision);
    assert!(requests.try_recv().is_err());
}

#[test]
fn album_playback_continues_from_raw_offset_not_playable_length() {
    let (_, mut app, _events, mut requests) = fixture();
    prepare(&mut app);
    app.play_collection(false);
    assert!(matches!(
        app.queue.continuation(),
        Some(crate::queue::Continuation::Album {
            offset: 100,
            id: 12
        })
    ));
    assert!(matches!(requests.try_recv(), Ok(Request::Play { .. })));
}

#[test]
fn malformed_album_pages_are_rejected_before_merge() {
    for case in 0..6 {
        let (_, mut app, _events, _requests) = fixture();
        prepare(&mut app);
        app.start_album_load();
        app.album_load_tick();
        let mut next = page(&[2], 200, Some(202), true);
        match case {
            0 => next.total = Some(203),
            1 => next.next_offset = 100,
            2 => next.next_offset = 201,
            3 => next.more = false,
            4 => next.data.artists.push(Artist::default()),
            _ => next.next_offset = 100,
        }
        app.album_page(
            app.generation,
            "albums",
            "12",
            if case == 5 { 0 } else { 100 },
            next,
        );
        assert!(app.album_load.is_none());
        assert!(app.error.is_some());
        assert_eq!(app.tracks.len(), 2);
        assert!(!app.browse_cache.ready);
    }
}

#[test]
fn stopped_album_read_can_finish_but_cannot_schedule_more_and_history_retains_metadata() {
    let (_, mut app, _events, mut requests) = fixture();
    prepare(&mut app);
    app.start_album_load();
    app.album_load_tick();
    requests.try_recv().unwrap();
    app.album_load = None;
    app.album_page(
        app.generation,
        "albums",
        "12",
        100,
        page(&[2], 200, Some(202), true),
    );
    app.album_load_tick();
    assert!(requests.try_recv().is_err());
    let generation = app.generation;
    app.navigate(Page::Settings);
    app.history(false);
    assert_eq!(app.album_position.as_ref().unwrap().offset, 200);
    assert!(app.browse_cache.restored);
    app.album_page(
        generation,
        "albums",
        "12",
        200,
        page(&[3], 202, Some(202), false),
    );
    assert_eq!(app.tracks.len(), 3);
    app.start_album_load();
    app.album_load_tick();
    assert!(matches!(
        requests.try_recv(),
        Ok(Request::Collection { offset: 200, .. })
    ));
}

#[test]
fn album_controller_yields_cancels_and_handles_closed_worker() {
    let (_, mut app, _events, requests) = fixture();
    prepare(&mut app);
    app.start_album_load();
    app.buffering = true;
    app.album_load_tick();
    assert!(!app.loading);
    app.buffering = false;
    app.account = Some(8);
    app.album_load_tick();
    assert!(app.album_load.is_none());
    app.account = Some(7);
    app.start_album_load();
    app.load_page(0);
    assert!(app.album_load.is_none());
    app.loading = false;
    app.browse_loaded();
    app.start_album_load();
    app.invalidate_browse_cache();
    assert!(app.album_load.is_none());
    app.browse_loaded();
    app.start_album_load();
    drop(requests);
    app.album_load_tick();
    assert!(app.album_load.is_none());
    assert!(!app.loading);
}

#[test]
fn unknown_total_terminal_page_and_size_limit_are_bounded() {
    let (_, mut app, _events, _requests) = fixture();
    prepare(&mut app);
    app.album_position.as_mut().unwrap().total = Some(50_001);
    app.start_album_load();
    assert!(app.album_load.is_none());
    app.album_position.as_mut().unwrap().total = None;
    app.start_album_load();
    app.album_load_tick();
    app.album_page(
        app.generation,
        "albums",
        "12",
        100,
        page(&[], 100, None, false),
    );
    assert!(!app.more);
    assert!(app.album_load.is_none());
    prepare(&mut app);
    let position = app.album_position.as_mut().unwrap();
    position.total = None;
    position.offset = 49_999;
    app.start_album_load();
    app.album_load_tick();
    app.album_page(
        app.generation,
        "albums",
        "12",
        49_999,
        page(&[9], 50_000, None, true),
    );
    assert!(app.album_load.is_none());
    assert!(app.notice.as_ref().unwrap().contains("50,000"));
    app.start_album_load();
    assert!(app.album_load.is_none());
}
