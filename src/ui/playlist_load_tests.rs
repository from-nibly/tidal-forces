use super::*;
use crate::ui::tests::{fixture, track};

fn page(start: usize, end: usize, total: usize) -> PlaylistPage {
    PlaylistPage {
        playlist: Playlist {
            uuid: "test-list".into(),
            title: "Fixture".into(),
            number_of_tracks: total as u64,
            ..Default::default()
        },
        etag: "r1".into(),
        editable: false,
        rows: (start..end)
            .map(|i| (i, track((i % 3 + 1) as u64)))
            .collect(),
        next_offset: end,
        more: end < total,
    }
}
fn prepare(app: &mut App, total: usize) {
    let p = page(0, 100, total);
    app.page = Page::Collection {
        kind: "playlists".into(),
        id: "test-list".into(),
        title: "Fixture".into(),
    };
    app.tracks = p.rows.iter().map(|(_, t)| t.clone()).collect();
    app.playlist_page = Some(p);
    app.more = true;
}
#[test]
fn load_all_and_stop_buttons_only_schedule_requested_pages() {
    let (ctx, mut app, _events, mut requests) = fixture();
    prepare(&mut app, 305);
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
                egui::CentralPanel::default().show(ctx, |ui| app.playlist_load_controls(ui));
                app.playlist_load_tick();
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
            .find_map(|s| {
                if let egui::Shape::Text(t) = &s.shape {
                    if t.galley.job.text == label {
                        Some(t.pos + vec2(4., 4.))
                    } else {
                        None
                    }
                } else {
                    None
                }
            })
            .unwrap()
    };
    let pos = point(&output, "Load all playlist songs");
    for pressed in [true, false] {
        draw(
            &mut app,
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
    }
    assert!(matches!(
        requests.try_recv(),
        Ok(Request::Playlist { offset: 100, .. })
    ));
    let output = draw(&mut app, vec![]);
    let pos = point(&output, "Stop loading");
    for pressed in [true, false] {
        draw(
            &mut app,
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
    }
    assert!(app.playlist_load.is_none());
    assert!(requests.try_recv().is_err());
}
#[test]
fn full_playlist_reads_use_raw_offsets_and_keep_playback_selection_and_revision() {
    let (_ctx, mut app, events, mut requests) = fixture();
    prepare(&mut app, 305);
    app.queue
        .start(vec![track(9)], 0, "Current source".into(), None)
        .unwrap();
    app.queue.add(track(8), false).unwrap();
    app.paused = false;
    let revision = app.queue.revision();
    app.track_selection
        .sync(app.account, app.generation, app.tracks.len());
    app.track_selection.select(3, false, false);
    app.start_playlist_load();
    for (offset, end) in [(100, 200), (200, 300), (300, 305)] {
        app.playlist_load_tick();
        let Request::Playlist {
            generation,
            id,
            offset: actual,
            etag,
        } = requests.try_recv().unwrap()
        else {
            panic!("Only playlist reads allowed");
        };
        assert_eq!(actual, offset);
        assert_eq!(id, "test-list");
        assert_eq!(etag.as_deref(), Some("r1"));
        app.playlist_load_tick();
        assert!(requests.try_recv().is_err());
        let mut p = page(offset, end, 305);
        // Unavailable rows must not change the next raw offset.
        p.rows.retain(|(i, _)| *i != 101);
        events
            .send(Event::Playlist {
                generation,
                page: p,
                append: true,
            })
            .unwrap();
        app.events();
    }
    app.playlist_load_tick();
    assert!(requests.try_recv().is_err());
    assert!(app.playlist_load.is_none());
    assert!(!app.more);
    assert_eq!(app.tracks.len(), 304);
    assert_eq!(app.playlist_page.as_ref().unwrap().next_offset, 305);
    assert_eq!(app.queue.revision(), revision);
    assert_eq!(app.queue.manual()[0].track.id, 8);
    assert_eq!(app.queue.current().unwrap().id, 9);
    assert!(!app.paused);
    assert_eq!(
        app.track_selection.rows.iter().copied().collect::<Vec<_>>(),
        [3]
    );
}
#[test]
fn full_playlist_loading_stops_on_cancel_errors_and_scope_changes() {
    for mode in ["cancel", "error", "navigate", "account", "refresh"] {
        let (_ctx, mut app, events, mut requests) = fixture();
        prepare(&mut app, 305);
        app.start_playlist_load();
        app.playlist_load_tick();
        let Request::Playlist { generation, .. } = requests.try_recv().unwrap() else {
            panic!()
        };
        match mode {
            "cancel" => app.playlist_load = None,
            "error" => events
                .send(Event::RequestError {
                    generation,
                    message: "Synthetic read error".into(),
                })
                .unwrap(),
            "navigate" => app.navigate(Page::Settings),
            "account" => {
                app.account = Some(8);
                app.generation += 1;
            }
            _ => {
                app.generation += 1;
                app.load_page(0);
                assert!(matches!(
                    requests.try_recv(),
                    Ok(Request::Playlist { offset: 0, .. })
                ));
            }
        }
        app.events();
        app.playlist_load_tick();
        assert!(app.playlist_load.is_none(), "{mode}");
        assert!(requests.try_recv().is_err());
        if mode == "cancel" {
            events
                .send(Event::Playlist {
                    generation,
                    page: page(100, 200, 305),
                    append: true,
                })
                .unwrap();
            app.events();
            app.playlist_load_tick();
            assert_eq!(app.tracks.len(), 200);
            assert!(requests.try_recv().is_err());
        }
    }
}
#[test]
fn changed_or_incomplete_pages_are_rejected_before_merging() {
    for mode in ["revision", "total", "end", "stall", "identity", "position"] {
        let (_ctx, mut app, events, mut requests) = fixture();
        prepare(&mut app, 305);
        app.start_playlist_load();
        app.playlist_load_tick();
        let Request::Playlist { generation, .. } = requests.try_recv().unwrap() else {
            panic!()
        };
        let mut p = page(100, 200, 305);
        match mode {
            "revision" => p.etag = "r2".into(),
            "total" => p.playlist.number_of_tracks = 306,
            "end" => p.more = false,
            "stall" => p.next_offset = 100,
            "identity" => p.playlist.uuid = "other-list".into(),
            _ => p.rows[0].0 = 99,
        }
        events
            .send(Event::Playlist {
                generation,
                page: p,
                append: true,
            })
            .unwrap();
        app.events();
        app.playlist_load_tick();
        assert!(app.playlist_load.is_none(), "{mode}");
        assert_eq!(app.tracks.len(), 100);
        assert!(app.error.is_some());
        assert!(requests.try_recv().is_err());
    }
}
#[test]
fn full_load_refuses_unknown_revisions_oversize_sources_and_backpressure() {
    for mode in ["revision", "limit", "busy", "route"] {
        let (_ctx, mut app, _events, mut requests) = fixture();
        prepare(&mut app, 305);
        match mode {
            "revision" => app.playlist_page.as_mut().unwrap().etag.clear(),
            "limit" => {
                app.playlist_page
                    .as_mut()
                    .unwrap()
                    .playlist
                    .number_of_tracks = 50_001
            }
            "busy" => app.loading = true,
            _ => app.page = Page::Home,
        }
        app.start_playlist_load();
        app.playlist_load_tick();
        assert!(app.playlist_load.is_none());
        assert!(requests.try_recv().is_err());
    }
    let (_ctx, mut app, _events, mut requests) = fixture();
    prepare(&mut app, 305);
    requests.close();
    app.start_playlist_load();
    app.playlist_load_tick();
    assert!(app.playlist_load.is_none());
    assert!(!app.loading);
    assert!(app.error.is_some());
}
