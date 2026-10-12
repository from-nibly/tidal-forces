use super::*;
use crate::ui::tests::{fixture, track};

fn page(
    kind: FavoriteKind,
    start: usize,
    end: usize,
    total: Option<u64>,
    more: bool,
) -> LibraryPage {
    let mut data = crate::api::Search::default();
    for id in start + 1..=end {
        match kind {
            FavoriteKind::Tracks => data.tracks.push(track(id as u64)),
            FavoriteKind::Albums => data.albums.push(Album {
                id: id as u64,
                ..Default::default()
            }),
            FavoriteKind::Artists => data.artists.push(Artist {
                id: id as u64,
                ..Default::default()
            }),
        }
    }
    LibraryPage {
        data,
        next_offset: end,
        total,
        more,
    }
}
fn prepare(app: &mut App, kind: FavoriteKind, total: Option<u64>) {
    app.page = Page::Library(kind);
    let p = page(kind, 0, 100, total, true);
    app.tracks = p.data.tracks;
    app.albums = p.data.albums;
    app.artists = p.data.artists;
    app.library_offset = 100;
    app.library_total = total;
    app.more = true;
    app.loading = false;
    app.browse_loaded();
}

#[test]
fn all_library_kinds_load_one_page_at_a_time_without_playback_or_absence_inference() {
    for kind in FavoriteKind::ALL {
        let (_, mut app, events, mut requests) = fixture();
        prepare(&mut app, kind, Some(205));
        let revision = app.queue.revision();
        app.start_library_load();
        assert!(!app.library_read_valid);
        for (offset, end) in [(100, 200), (200, 205)] {
            app.library_load_tick();
            assert!(
                matches!(requests.try_recv(),Ok(Request::Library {kind:k,offset:o,..}) if k==kind && o==offset)
            );
            app.library_load_tick();
            assert!(requests.try_recv().is_err());
            events
                .send(Event::Library {
                    generation: app.generation,
                    kind,
                    append: true,
                    page: page(kind, offset, end, Some(205), end < 205),
                })
                .unwrap();
            app.events();
        }
        assert!(app.library_load.is_none());
        assert!(!app.more);
        assert_eq!(app.tracks.len() + app.albums.len() + app.artists.len(), 205);
        assert_eq!(app.queue.revision(), revision);
        assert_eq!(app.favorites.state(Favorite::new(kind, 999)), None);
        assert!(requests.try_recv().is_err());
    }
}

#[test]
fn overlap_is_deduplicated_and_a_repeated_page_stops_without_losing_loaded_data() {
    let (_, mut app, events, mut requests) = fixture();
    let kind = FavoriteKind::Tracks;
    prepare(&mut app, kind, Some(400));
    app.start_library_load();
    app.library_load_tick();
    requests.try_recv().unwrap();
    let mut overlapping = page(kind, 50, 150, Some(400), true);
    overlapping.next_offset = 200;
    events
        .send(Event::Library {
            generation: app.generation,
            kind,
            append: true,
            page: overlapping,
        })
        .unwrap();
    app.events();
    assert_eq!(app.tracks.len(), 150);
    assert_eq!(app.library_offset, 200);
    app.library_load_tick();
    requests.try_recv().unwrap();
    let mut repeated = page(kind, 50, 150, Some(400), true);
    repeated.next_offset = 300;
    events
        .send(Event::Library {
            generation: app.generation,
            kind,
            append: true,
            page: repeated,
        })
        .unwrap();
    app.events();
    assert!(app.library_load.is_none());
    assert_eq!(app.tracks.len(), 150);
    assert!(app.notice.as_ref().unwrap().contains("repeated"));
    app.library_load_tick();
    assert!(requests.try_recv().is_err());
    assert_eq!(app.favorites.state(Favorite::new(kind, 151)), None);
}

#[test]
fn changing_counts_or_invalid_progress_is_rejected_before_merge() {
    for case in 0..7 {
        let (_, mut app, events, _requests) = fixture();
        let kind = FavoriteKind::Tracks;
        prepare(&mut app, kind, Some(300));
        app.start_library_load();
        app.library_load_tick();
        let mut p = page(kind, 100, 200, Some(300), true);
        match case {
            0 => p.total = Some(301),
            1 => p.next_offset = 100,
            2 => p.next_offset = 201,
            3 => p.more = false,
            4 => p.next_offset = 301,
            5 => p.total = None,
            _ => p.data.albums.push(Album {
                id: 8,
                ..Default::default()
            }),
        }
        events
            .send(Event::Library {
                generation: app.generation,
                kind,
                append: true,
                page: p,
            })
            .unwrap();
        app.events();
        assert!(app.library_load.is_none());
        assert!(!app.loading);
        assert!(app.error.is_some());
        assert_eq!(app.tracks.len(), 100);
    }
}

#[test]
fn unknown_totals_and_unavailable_rows_remain_positive_only() {
    let (_, mut app, events, _requests) = fixture();
    let kind = FavoriteKind::Tracks;
    prepare(&mut app, kind, None);
    app.start_library_load();
    app.library_load_tick();
    let mut p = page(kind, 100, 200, None, true);
    p.data.tracks.retain(|t| t.id % 2 == 0);
    events
        .send(Event::Library {
            generation: app.generation,
            kind,
            append: true,
            page: p,
        })
        .unwrap();
    app.events();
    app.library_load_tick();
    events
        .send(Event::Library {
            generation: app.generation,
            kind,
            append: true,
            page: page(kind, 200, 200, None, false),
        })
        .unwrap();
    app.events();
    assert!(app.library_load.is_none());
    assert_eq!(app.tracks.len(), 150);
    assert_eq!(app.favorites.state(Favorite::new(kind, 102)), Some(true));
    assert_eq!(app.favorites.state(Favorite::new(kind, 101)), None);
}

#[test]
fn stop_allows_inflight_page_but_navigation_and_account_changes_cancel_followups() {
    let (_, mut app, events, mut requests) = fixture();
    let kind = FavoriteKind::Tracks;
    prepare(&mut app, kind, Some(300));
    app.start_library_load();
    app.library_load_tick();
    requests.try_recv().unwrap();
    app.library_load = None;
    events
        .send(Event::Library {
            generation: app.generation,
            kind,
            append: true,
            page: page(kind, 100, 200, Some(300), true),
        })
        .unwrap();
    app.events();
    app.library_load_tick();
    assert!(requests.try_recv().is_err());
    assert_eq!(app.tracks.len(), 200);
    app.start_library_load();
    let old = app.generation;
    app.navigate(Page::Settings);
    assert!(app.library_load.is_none());
    events
        .send(Event::Library {
            generation: old,
            kind,
            append: true,
            page: page(kind, 200, 300, Some(300), false),
        })
        .unwrap();
    app.events();
    assert!(app.tracks.is_empty());
    prepare(&mut app, kind, Some(300));
    app.start_library_load();
    app.account = Some(99);
    app.library_load_tick();
    assert!(app.library_load.is_none());
}

#[test]
fn refresh_mutations_errors_and_buffering_guard_the_controller() {
    let (_, mut app, events, mut requests) = fixture();
    let kind = FavoriteKind::Tracks;
    prepare(&mut app, kind, Some(300));
    app.start_library_load();
    app.buffering = true;
    app.library_load_tick();
    assert!(requests.try_recv().is_err());
    app.buffering = false;
    app.library_load_tick();
    requests.try_recv().unwrap();
    events
        .send(Event::RequestError {
            generation: app.generation,
            message: "Fixture failure".into(),
        })
        .unwrap();
    app.events();
    assert!(app.library_load.is_none());
    assert!(!app.browse_cache.ready);
    app.start_library_load();
    assert!(app.library_load.is_none());
    prepare(&mut app, kind, Some(300));
    app.start_library_load();
    assert!(app.library_load.is_some());
    app.set_favorite(Favorite::new(kind, 1), false);
    assert!(app.library_load.is_none());
    app.favorites.finish(Favorite::new(kind, 1), None);
    prepare(&mut app, kind, Some(300));
    app.start_library_load();
    assert!(app.library_load.is_some());
    app.load_page(0);
    assert!(app.library_load.is_none());
}

#[test]
fn closed_or_full_worker_queue_stops_without_losing_rows() {
    for closed in [false, true] {
        let (_, mut app, _events, requests) = fixture();
        prepare(&mut app, FavoriteKind::Tracks, Some(300));
        let _requests = if closed {
            drop(requests);
            None
        } else {
            while app
                .backend
                .tx
                .try_send(Request::Home {
                    generation: app.generation,
                })
                .is_ok()
            {}
            Some(requests)
        };
        app.start_library_load();
        app.library_load_tick();
        assert!(app.library_load.is_none());
        assert!(!app.loading);
        assert_eq!(app.tracks.len(), 100);
    }
}

#[test]
fn appending_keeps_filter_selection_and_source_occurrences() {
    let (_, mut app, events, _requests) = fixture();
    prepare(&mut app, FavoriteKind::Tracks, Some(200));
    app.sync_track_view();
    app.track_selection
        .sync(app.account, app.generation, app.tracks.len());
    app.track_selection.select(2, false, false);
    assert!(app.track_selection.rows.contains(&2));
    app.track_view.sort = track_view::Sort::Title;
    app.start_library_load();
    app.library_load_tick();
    events
        .send(Event::Library {
            generation: app.generation,
            kind: FavoriteKind::Tracks,
            append: true,
            page: page(FavoriteKind::Tracks, 100, 200, Some(200), false),
        })
        .unwrap();
    app.events();
    app.sync_track_view();
    assert!(app.track_view.sort == track_view::Sort::Title);
    assert!(app.track_selection.rows.contains(&2));
    assert_eq!(app.tracks[2].id, 3);
}

#[test]
fn controls_start_and_stop_only_explicitly() {
    let (ctx, mut app, _events, mut requests) = fixture();
    prepare(&mut app, FavoriteKind::Tracks, Some(300));
    let draw = |app: &mut App, events| {
        ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    vec2(620., 440.),
                )),
                events,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| app.library_load_controls(ui));
                app.library_load_tick();
            },
        )
    };
    draw(&mut app, vec![]);
    let output = draw(&mut app, vec![]);
    assert!(requests.try_recv().is_err());
    let point = |output: &egui::FullOutput, label: &str| {
        output
            .shapes
            .iter()
            .find_map(|s| match &s.shape {
                egui::Shape::Text(t) if t.galley.job.text == label => Some(t.pos + vec2(4., 4.)),
                _ => None,
            })
            .unwrap()
    };
    let click = |pos| {
        vec![
            egui::Event::PointerMoved(pos),
            egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            },
            egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            },
        ]
    };
    draw(&mut app, click(point(&output, "Load all saved tracks")));
    assert!(matches!(
        requests.try_recv(),
        Ok(Request::Library { offset: 100, .. })
    ));
    let output = draw(&mut app, vec![]);
    draw(&mut app, click(point(&output, "Stop loading")));
    assert!(app.library_load.is_none());
    assert!(requests.try_recv().is_err());
}

#[test]
fn library_loading_is_bounded_including_unknown_total_reads() {
    let (_, mut app, events, _requests) = fixture();
    let kind = FavoriteKind::Tracks;
    prepare(&mut app, kind, Some(50_001));
    app.start_library_load();
    assert!(app.library_load.is_none());
    app.library_total = None;
    app.library_offset = 49_999;
    app.start_library_load();
    app.library_load_tick();
    events
        .send(Event::Library {
            generation: app.generation,
            kind,
            append: true,
            page: page(kind, 49_999, 50_000, None, true),
        })
        .unwrap();
    app.events();
    assert!(app.library_load.is_none());
    assert!(app.notice.as_ref().unwrap().contains("50,000"));
    app.start_library_load();
    assert!(app.library_load.is_none());
}
