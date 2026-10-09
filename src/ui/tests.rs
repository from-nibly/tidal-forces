use super::*;
use eframe::App as _;
use std::sync::mpsc;

pub(super) fn fixture() -> (
    egui::Context,
    App,
    mpsc::Sender<Event>,
    tokio::sync::mpsc::Receiver<Request>,
) {
    let ctx = egui::Context::default();
    theme::configure(&ctx);
    let (tx, requests) = tokio::sync::mpsc::channel(32);
    let (events, rx) = mpsc::channel();
    let backend = Backend {
        tx,
        rx,
        player: crate::audio::Player::inert(),
    };
    let mut app = App::with_backend(backend, crate::store::Appearance::default());
    app.connected = true;
    app.account = Some(7);
    app.country = "US".into();
    app.folders.insert("root".into(), Vec::new());
    (ctx, app, events, requests)
}

pub(super) fn track(id: u64) -> Track {
    Track {
        id,
        title: format!("Synthetic track {id} with a deliberately long title"),
        duration: 180,
        artist: Artist {
            id: 5,
            name: "Test artist".into(),
            ..Default::default()
        },
        ..Default::default()
    }
}

pub(super) fn frame(
    ctx: &egui::Context,
    app: &mut App,
    size: Vec2,
) -> (egui::FullOutput, egui::Rect) {
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
        ..Default::default()
    };
    let mut used = egui::Rect::NOTHING;
    let output = ctx.run(input, |ctx| {
        app.update(ctx, &mut eframe::Frame::_new_kittest());
        used = ctx.used_rect();
    });
    (output, used)
}

#[test]
fn native_pages_render_at_minimum_size_and_high_zoom_without_unbounded_track_widgets() {
    let (ctx, mut app, _events, _requests) = fixture();
    app.tracks = (1..=10000).map(track).collect();
    app.queue
        .start((1..=10000).map(track).collect(), 0, "Fixture".into(), None)
        .unwrap();
    for id in 1..=5 {
        app.queue.add(track(id), false).unwrap();
    }
    for size in [
        vec2(1920., 1080.),
        vec2(1240., 820.),
        vec2(1000., 660.),
        vec2(800., 528.),
        vec2(666., 440.),
    ] {
        // Floating areas retain their previous-frame bounds across viewport resizing.
        frame(&ctx, &mut app, size);
        for page in [
            Page::Home,
            Page::Search,
            Page::Library(FavoriteKind::Tracks),
            Page::Library(FavoriteKind::Albums),
            Page::Library(FavoriteKind::Artists),
            Page::Playlists,
            Page::Settings,
        ] {
            app.page = page;
            for queue_open in [false, true] {
                app.queue_open = queue_open;
                for compact in [false, true] {
                    app.appearance.compact_rows = compact;
                    let (output, used) = frame(&ctx, &mut app, size);
                    assert!(
                        output.shapes.len() < 1500,
                        "Unbounded rendering: {} shapes",
                        output.shapes.len()
                    );
                    assert!(
                        used.right() <= size.x + 1.,
                        "Horizontal overflow at {size:?}: {used:?} on {:?}",
                        app.page
                    );
                    assert!(app.paused);
                }
            }
        }
        app.page = Page::Settings;
        for section in [
            SettingsSection::General,
            SettingsSection::Appearance,
            SettingsSection::Playback,
            SettingsSection::Privacy,
            SettingsSection::Shortcuts,
            SettingsSection::About,
        ] {
            app.settings_section = section;
            frame(&ctx, &mut app, size);
        }
    }
}

#[test]
fn media_grids_and_shelves_remain_bounded_with_ten_thousand_long_titles() {
    let (ctx, mut app, _events, mut requests) = fixture();
    app.albums = (1..=10000)
        .map(|id| Album {
            id,
            title: "An album with a deliberately long title that must not stretch its card".into(),
            artist: Artist {
                name: "A very long artist name".into(),
                ..Default::default()
            },
            ..Default::default()
        })
        .collect();
    app.artists = (1..=10000)
        .map(|id| Artist {
            id,
            name: "A deliberately long artist name that must stay within its card".into(),
            ..Default::default()
        })
        .collect();
    for size in [
        vec2(1920., 1080.),
        vec2(1240., 820.),
        vec2(1000., 660.),
        vec2(666., 440.),
    ] {
        for page in [
            Page::Search,
            Page::Library(FavoriteKind::Albums),
            Page::Library(FavoriteKind::Artists),
        ] {
            app.page = page;
            let (output, used) = frame(&ctx, &mut app, size);
            assert!(
                output.shapes.len() < 1500,
                "Unbounded media widgets: {}",
                output.shapes.len()
            );
            assert!(
                used.right() <= size.x + 1.,
                "Media cards overflow at {size:?}: {used:?}"
            );
        }
    }
    assert!(requests.try_recv().is_err());
}

#[test]
fn sidebar_create_action_is_inline_aligned_and_only_opens_confirmation() {
    for (connected, busy) in [(true, false), (false, false), (true, true)] {
        let (ctx, mut app, _events, mut requests) = fixture();
        app.connected = connected;
        app.playlist_busy = busy;
        let draw = |app: &mut App, events| {
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        vec2(1240., 820.),
                    )),
                    events,
                    ..Default::default()
                },
                |ctx| app.sidebar(ctx),
            )
        };
        draw(&mut app, vec![]);
        let output = draw(&mut app, vec![]);
        let label = output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Text(text) if text.galley.job.text == "YOUR PLAYLISTS" => {
                    Some(egui::Rect::from_min_size(text.pos, text.galley.size()))
                }
                _ => None,
            })
            .unwrap();
        assert!(!output.shapes.iter().any(|shape| matches!(&shape.shape,egui::Shape::Text(text) if text.galley.job.text == "+ New playlist")));
        let plus = output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::LineSegment { points, .. }
                    if points[0].y == points[1].y
                        && ((points[1].x - points[0].x) - 9.464).abs() < 0.02 =>
                {
                    Some(points[0].lerp(points[1], 0.5))
                }
                _ => None,
            })
            .unwrap();
        assert!(plus.x > label.right() + 8.);
        assert!((plus.y - label.center().y).abs() <= 1.);
        for pressed in [true, false] {
            draw(
                &mut app,
                vec![
                    egui::Event::PointerMoved(plus),
                    egui::Event::PointerButton {
                        pos: plus,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            );
        }
        assert_eq!(app.playlist_dialog, connected && !busy);
        assert!(
            requests.try_recv().is_err(),
            "Opening the form must not create a playlist"
        );
    }
}

#[test]
fn collection_header_actions_start_once_and_preserve_manual_queue_and_raw_continuation() {
    for shuffle in [false, true] {
        let (ctx, mut app, _events, mut requests) = fixture();
        app.page = Page::Collection {
            kind: "playlists".into(),
            id: "fixture-list".into(),
            title: "Playlist fixture".into(),
        };
        app.tracks = vec![track(1), track(2), track(1)];
        app.more = true;
        app.playlist_page = Some(PlaylistPage {
            playlist: Playlist {
                uuid: "fixture-list".into(),
                title: "Playlist fixture".into(),
                ..Default::default()
            },
            etag: "revision-1".into(),
            editable: true,
            rows: app.tracks.iter().cloned().enumerate().collect(),
            next_offset: 100,
            more: true,
        });
        app.queue.add(track(9), false).unwrap();
        if !shuffle {
            app.queue.toggle_shuffle();
        }
        let manual = app.queue.manual()[0].occurrence;
        let draw = |app: &mut App, events| {
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        vec2(800., 700.),
                    )),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| app.content(ui));
                },
            )
        };
        draw(&mut app, vec![]);
        let output = draw(&mut app, vec![]);
        let label = if shuffle { "Shuffle" } else { "Play" };
        let position = output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Text(text) if text.galley.job.text == label => {
                    Some(text.pos + text.galley.size() * 0.5)
                }
                _ => None,
            })
            .unwrap();
        let table_heading = output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Text(text) if text.galley.job.text == "Tracks" => Some(text.pos.y),
                _ => None,
            })
            .unwrap();
        assert!(position.y < table_heading);
        assert!(!output.shapes.iter().any(|shape|matches!(&shape.shape,egui::Shape::Text(text) if text.galley.job.text == "Play all")));
        for pressed in [true, false] {
            draw(
                &mut app,
                vec![
                    egui::Event::PointerMoved(position),
                    egui::Event::PointerButton {
                        pos: position,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            );
        }
        assert_eq!(app.queue.shuffled(), shuffle);
        assert_eq!(app.queue.manual()[0].occurrence, manual);
        assert!(
            matches!(app.queue.continuation(),Some(Continuation::Playlist { id,etag,offset:100 }) if id=="fixture-list" && etag=="revision-1")
        );
        let current = app.queue.current().unwrap().id;
        assert!(matches!(requests.try_recv(),Ok(Request::Play { id, .. }) if id==current));
        assert!(
            requests.try_recv().is_err(),
            "Do not play an unintended first track before shuffling"
        );
        let mut ids = vec![current];
        ids.extend(app.queue.context_upcoming().map(|entry| entry.track.id));
        ids.sort();
        assert_eq!(ids, [1, 1, 2]);
        assert!(app.queue.validate().is_ok());
    }
}

#[test]
fn sidebar_rows_align_labels_and_preserve_folder_expansion_and_navigation() {
    let (ctx, mut app, _events, mut requests) = fixture();
    app.folders.insert(
        "root".into(),
        vec![
            LibraryEntry::Folder {
                id: "folder".into(),
                name: "Folder".into(),
                count: 1,
            },
            LibraryEntry::Playlist(Playlist {
                uuid: "root-list".into(),
                title: "Root playlist".into(),
                ..Default::default()
            }),
        ],
    );
    app.folders.insert(
        "folder".into(),
        vec![LibraryEntry::Playlist(Playlist {
            uuid: "nested-list".into(),
            title: "Nested playlist".into(),
            ..Default::default()
        })],
    );
    let draw = |app: &mut App, events| {
        ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    vec2(216., 500.),
                )),
                events,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| app.folder_tree(ui, "root", 0));
            },
        )
    };
    let text_rect = |output: &egui::FullOutput, name: &str| {
        output.shapes.iter().find_map(|shape| match &shape.shape {
            egui::Shape::Text(text) if text.galley.job.text == name => {
                Some(egui::Rect::from_min_size(text.pos, text.galley.size()))
            }
            _ => None,
        })
    };
    draw(&mut app, vec![]);
    let output = draw(&mut app, vec![]);
    let folder = text_rect(&output, "Folder").unwrap();
    let root = text_rect(&output, "Root playlist").unwrap();
    assert_eq!(folder.left(), root.left());
    assert!((root.center().y - folder.center().y - 48.).abs() < 1.);
    assert!(output.shapes.iter().any(|shape|matches!(&shape.shape,egui::Shape::Path(path) if path.closed && path.points.len()==6)),"Folder must have a vector folder icon");
    let click = |app: &mut App, pos| {
        for pressed in [true, false] {
            draw(
                app,
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
    };
    click(&mut app, folder.center());
    assert!(app.expanded.contains("folder"));
    let expanded = draw(&mut app, vec![]);
    assert!(text_rect(&expanded, "Nested playlist").unwrap().left() > folder.left());
    assert!(
        requests.try_recv().is_err(),
        "Cached expansion must not fetch or mutate"
    );
    click(&mut app, folder.center());
    assert!(!app.expanded.contains("folder"));
    let collapsed = draw(&mut app, vec![]);
    assert!(text_rect(&collapsed, "Nested playlist").is_none());
    click(
        &mut app,
        text_rect(&collapsed, "Root playlist").unwrap().center(),
    );
    assert!(matches!(requests.try_recv(),Ok(Request::Playlist {id,..}) if id=="root-list"));
    assert!(requests.try_recv().is_err());
    assert!(app.queue.current().is_none());
}

#[test]
fn deeply_nested_sidebar_rows_keep_long_names_inside_the_rail() {
    let (ctx, mut app, _events, mut requests) = fixture();
    let name = "A very long folder name that must remain within the narrow sidebar";
    for index in 0..=20 {
        let parent = if index == 0 {
            "root".into()
        } else {
            format!("folder-{}", index - 1)
        };
        let id = format!("folder-{index}");
        app.folders.insert(
            parent,
            vec![LibraryEntry::Folder {
                id: id.clone(),
                name: name.into(),
                count: 1,
            }],
        );
        app.expanded.insert(id);
    }
    app.folders.insert("folder-20".into(), Vec::new());
    let mut used = egui::Rect::NOTHING;
    let output = ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                vec2(216., 660.),
            )),
            ..Default::default()
        },
        |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| app.folder_tree(ui, "root", 0));
            });
            used = ctx.used_rect();
        },
    );
    assert!(used.right() <= 217.);
    assert!(output.shapes.len() < 500);
    assert!(requests.try_recv().is_err());
}

#[test]
fn history_and_refresh_do_not_restart_a_radio_or_track() {
    let (_ctx, mut app, events, mut requests) = fixture();
    let radio = RadioSeed::Track {
        id: 1,
        title: "Synthetic".into(),
    };
    app.navigate(Page::Radio(radio.clone()));
    assert!(matches!(requests.try_recv(), Ok(Request::Radio { .. })));
    events
        .send(Event::Radio {
            generation: app.generation,
            tracks: vec![track(1)],
        })
        .unwrap();
    app.events();
    assert!(matches!(
        requests.try_recv(),
        Ok(Request::Play { id: 1, .. })
    ));
    app.navigate(Page::Home);
    assert!(matches!(requests.try_recv(), Ok(Request::Home { .. })));
    app.history(false);
    assert_eq!(app.page, Page::Radio(radio));
    assert!(matches!(requests.try_recv(), Ok(Request::Radio { .. })));
    events
        .send(Event::Radio {
            generation: app.generation,
            tracks: vec![track(1)],
        })
        .unwrap();
    app.events();
    assert!(requests.try_recv().is_err());
    app.navigate(Page::Track(2));
    assert!(matches!(requests.try_recv(), Ok(Request::Track { .. })));
    app.autoplay_page = false;
    events
        .send(Event::Tracks {
            generation: app.generation,
            tracks: vec![track(2)],
            append: false,
        })
        .unwrap();
    app.events();
    assert!(requests.try_recv().is_err());
}

#[test]
fn favorites_confirm_writes_and_ignore_other_account_events() {
    let (_ctx, mut app, events, mut requests) = fixture();
    let item = Favorite::new(FavoriteKind::Tracks, 9);
    app.set_favorite(item, true);
    app.set_favorite(item, true);
    assert!(matches!(
        requests.try_recv(),
        Ok(Request::SetFavorite {
            user: 7,
            saved: true,
            ..
        })
    ));
    assert!(requests.try_recv().is_err());
    assert_eq!(app.favorites.state(item), None);
    events
        .send(Event::Favorite {
            user: 8,
            item,
            result: Ok(true),
        })
        .unwrap();
    app.events();
    assert!(app.favorites.pending(item));
    events
        .send(Event::Favorite {
            user: 7,
            item,
            result: Ok(true),
        })
        .unwrap();
    app.events();
    assert_eq!(app.favorites.state(item), Some(true));
    events.send(Event::Session(Some((8, "GB".into())))).unwrap();
    app.events();
    assert_eq!(app.favorites.state(item), None);
    assert!(app.queue.current().is_none());
    assert!(app.paused);
}

#[test]
fn favorite_buttons_require_explicit_unknown_state_actions_and_disable_pending_writes() {
    let item = Favorite::new(FavoriteKind::Tracks, 9);
    for (state, pending) in [
        (None, false),
        (Some(true), false),
        (Some(false), false),
        (Some(true), true),
        (None, true),
    ] {
        let ctx = egui::Context::default();
        let mut favorites = Favorites::default();
        if state.is_some() {
            favorites.finish(item, state);
        }
        if pending {
            favorites.begin(item);
        }
        let mut action = None;
        for pressed in [None, Some(true), Some(false)] {
            let pos = pos2(22., 22.);
            let mut events = vec![egui::Event::PointerMoved(pos)];
            if let Some(pressed) = pressed {
                events.push(egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                });
            }
            let output = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        vec2(400., 300.),
                    )),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default()
                        .show(ctx, |ui| favorite_button(ui, &favorites, item, &mut action));
                },
            );
            if pressed.is_none() {
                assert!(!output.shapes.iter().any(|shape|matches!(&shape.shape,egui::Shape::Text(text) if text.galley.job.text == "?")));
                let menu_indicator = output.shapes.iter().any(|shape|matches!(&shape.shape,egui::Shape::Path(path) if path.points.len()==3 && !path.closed));
                assert_eq!(menu_indicator, state.is_none());
            }
        }
        assert_eq!(
            action,
            state.filter(|_| !pending).map(|saved| (item, !saved))
        );
    }
}

fn attach_saved_state(ctx: &egui::Context, app: &mut App) -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    let mut queue = Queue::default();
    queue
        .start(vec![track(9), track(10)], 0, "Restore source".into(), None)
        .unwrap();
    let progress = Progress {
        occurrence: queue.current_entry().map(|entry| entry.occurrence),
        position: 45,
        volume: 0.25,
        quality: "HIGH".into(),
        location: Location {
            page: Page::Radio(RadioSeed::Track {
                id: 9,
                title: "Saved radio".into(),
            }),
            query: String::new(),
        },
    };
    let writer = StateStore::new(root.path().into(), ctx.clone()).unwrap();
    writer.snapshot(PlaybackState::new(7, queue, progress));
    drop(writer);
    app.state_store = Some(StateStore::new(root.path().into(), ctx.clone()).unwrap());
    app.request_restore();
    root
}

fn finish_restore(app: &mut App) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while !app.restore_ready {
        assert!(
            std::time::Instant::now() < deadline,
            "Restore did not complete"
        );
        app.state_events();
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}

fn read_saved(root: &std::path::Path) -> PlaybackState {
    let reader = StateStore::new(root.into(), egui::Context::default()).unwrap();
    reader.load(7, 1);
    match reader
        .events
        .recv_timeout(std::time::Duration::from_secs(5))
        .unwrap()
    {
        crate::player_state::Event::Loaded {
            result: Ok(Some(state)),
            ..
        } => *state,
        _ => panic!("Missing saved playback state"),
    }
}

#[test]
fn restored_radio_stays_paused_and_play_resolves_the_saved_occurrence_and_position() {
    let (ctx, mut app, events, mut requests) = fixture();
    let _root = attach_saved_state(&ctx, &mut app);
    finish_restore(&mut app);
    assert!(app.notice.is_none(), "Normal restoration must be silent");
    assert!(app.paused && !app.buffering && app.actual_quality.is_empty());
    assert_eq!(app.position, 45);
    assert_eq!(app.volume, 0.25);
    assert_eq!(app.queue.current().unwrap().id, 9);
    assert!(matches!(requests.try_recv(), Ok(Request::Radio { .. })));
    events
        .send(Event::Radio {
            generation: app.generation,
            tracks: vec![track(22)],
        })
        .unwrap();
    app.events();
    assert!(requests.try_recv().is_err());
    app.toggle();
    assert!(matches!(
        requests.try_recv(),
        Ok(Request::Play {
            id: 9,
            position: 45,
            ..
        })
    ));
}

#[test]
fn late_restore_does_not_replace_new_playback_or_user_volume_and_navigation() {
    let (ctx, mut app, _events, _requests) = fixture();
    let root = attach_saved_state(&ctx, &mut app);
    app.tracks = vec![track(99)];
    app.play_track(0);
    finish_restore(&mut app);
    assert_eq!(app.queue.current().unwrap().id, 99);
    app.checkpoint(true);
    app.state_store.take();
    assert_eq!(read_saved(root.path()).queue.current().unwrap().id, 99);

    let (ctx, mut app, _events, _requests) = fixture();
    let root = attach_saved_state(&ctx, &mut app);
    app.volume = 0.1;
    app.navigate(Page::Settings);
    finish_restore(&mut app);
    assert_eq!(app.queue.current().unwrap().id, 9);
    assert_eq!(app.volume, 0.1);
    assert_eq!(app.page, Page::Settings);
    app.checkpoint(true);
    app.state_store.take();
    let saved = read_saved(root.path());
    assert_eq!(saved.progress.volume, 0.1);
    assert_eq!(saved.progress.location.page, Page::Settings);
}

#[test]
fn external_album_link_keeps_its_view_while_restoring_a_paused_queue() {
    let (ctx, mut app, _events, _requests) = fixture();
    let page = Page::Collection {
        kind: "albums".into(),
        id: "123".into(),
        title: "Linked album".into(),
    };
    app.navigate(page.clone());
    let _root = attach_saved_state(&ctx, &mut app);
    finish_restore(&mut app);
    assert_eq!(app.page, page);
    assert!(app.paused);
    assert_eq!(app.queue.current().unwrap().id, 9);
}

#[test]
fn queue_clear_during_restore_is_flushed_even_if_the_window_closes_immediately() {
    let (ctx, mut app, _events, _requests) = fixture();
    let root = attach_saved_state(&ctx, &mut app);
    app.stop_and_clear();
    app.checkpoint(true);
    app.state_store.take();
    assert!(read_saved(root.path()).queue.current().is_none());
}

#[test]
fn pagination_continues_past_hundred_and_respects_pause_while_waiting() {
    for pause in [false, true] {
        let (_ctx, mut app, events, mut requests) = fixture();
        app.page = Page::Collection {
            kind: "albums".into(),
            id: "12".into(),
            title: "Long album".into(),
        };
        app.tracks = (1..=100).map(track).collect();
        app.more = true;
        app.play_track(99);
        assert!(matches!(
            requests.try_recv(),
            Ok(Request::Play { id: 100, .. })
        ));
        events
            .send(Event::Playing {
                generation: app.play_generation,
                quality: "FLAC".into(),
            })
            .unwrap();
        app.events();
        app.prefetch_context();
        app.prefetch_context();
        assert!(matches!(
            requests.try_recv(),
            Ok(Request::ContextPage {
                source: Continuation::Album { offset: 100, .. },
                ..
            })
        ));
        assert!(requests.try_recv().is_err());
        app.advance(true);
        assert!(app.waiting_context && app.buffering);
        if pause {
            app.toggle();
        }
        events
            .send(Event::ContextPage {
                generation: app.context_generation,
                result: Ok(crate::api::ContextPage {
                    tracks: vec![track(101), track(102)],
                    continuation: None,
                }),
            })
            .unwrap();
        app.events();
        assert_eq!(app.queue.current().unwrap().id, 101);
        if pause {
            assert!(app.paused && !app.buffering);
            assert!(requests.try_recv().is_err());
            app.toggle();
        }
        assert!(matches!(
            requests.try_recv(),
            Ok(Request::Play { id: 101, .. })
        ));
    }
}

#[test]
fn replaced_context_ignores_stale_continuations_and_errors() {
    let (_ctx, mut app, events, _requests) = fixture();
    app.tracks = vec![track(1)];
    app.play_track(0);
    let old = app.context_generation;
    app.tracks = vec![track(2)];
    app.play_track(0);
    events
        .send(Event::ContextPage {
            generation: old,
            result: Ok(crate::api::ContextPage {
                tracks: vec![track(99)],
                continuation: None,
            }),
        })
        .unwrap();
    events
        .send(Event::ContextPage {
            generation: old,
            result: Err("Old error".into()),
        })
        .unwrap();
    app.events();
    assert_eq!(app.queue.current().unwrap().id, 2);
    assert_eq!(app.queue.upcoming_len(), 0);
    assert!(app.context_error.is_none());
}

#[test]
fn unavailable_worker_does_not_leave_a_favorite_stuck_pending() {
    let (_ctx, mut app, _events, requests) = fixture();
    drop(requests);
    app.buffering = true;
    app.loading = true;
    let item = Favorite::new(FavoriteKind::Tracks, 9);
    app.set_favorite(item, true);
    assert!(!app.favorites.pending(item));
    assert_eq!(app.favorites.state(item), None);
    assert!(app.buffering && app.loading);
    assert!(app.error.is_some());
}
