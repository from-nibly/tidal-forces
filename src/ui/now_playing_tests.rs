use super::*;
use std::time::Duration;

fn fixture() -> (egui::Context, App, tokio::sync::mpsc::Receiver<Request>) {
    let (ctx, mut app, _, requests) = super::super::tests::fixture();
    app.queue_mut()
        .start(
            vec![super::super::tests::track(1)],
            0,
            "Fixture".into(),
            None,
        )
        .unwrap();
    (ctx, app, requests)
}

#[test]
fn focused_view_preserves_browsing_selection_scroll_panel_and_playback() {
    let (ctx, mut app, mut requests) = fixture();
    app.page = Page::Search;
    app.query = "Edited query".into();
    app.search_query = "Loaded query".into();
    app.browse_cache.scroll = 137.5;
    app.queue_open = true;
    app.track_selection.rows.insert(2);
    app.track_selection.focus = Some(2);
    ctx.memory_mut(|memory| memory.request_focus(egui::Id::new("prior-browse-focus")));
    let focus = ctx.memory(|memory| memory.focused());
    let location = app.location();
    let revision = app.queue.revision();
    let generation = app.play_generation;
    app.open_now_playing(&ctx);
    assert!(app.now_playing.open);
    app.close_now_playing(&ctx);
    assert!(!app.now_playing.open);
    assert_eq!(app.location(), location);
    assert_eq!(app.query, "Edited query");
    assert_eq!(app.browse_cache.scroll, 137.5);
    assert!(app.queue_open && app.track_selection.rows.contains(&2));
    assert_eq!(app.track_selection.focus, Some(2));
    assert_eq!(ctx.memory(|memory| memory.focused()), focus);
    assert_eq!(app.queue.revision(), revision);
    assert_eq!(app.play_generation, generation);
    assert!(requests.try_recv().is_err());
}

#[test]
fn lyrics_cancel_on_close_and_ignore_stale_account_track_and_request_results() {
    let (ctx, mut app, mut requests) = fixture();
    app.open_now_playing(&ctx);
    app.lyrics_tick();
    let Request::Lyrics {
        user,
        serial,
        id,
        mut cancel,
    } = requests.try_recv().unwrap()
    else {
        panic!("Expected lyrics");
    };
    app.close_now_playing(&ctx);
    assert!(matches!(
        cancel.try_recv(),
        Err(tokio::sync::oneshot::error::TryRecvError::Closed)
    ));
    app.lyrics_result(user, serial, id, Err("Late failure".into()));
    assert!(matches!(app.now_playing.state, State::Idle));
    app.open_now_playing(&ctx);
    app.lyrics_tick();
    let Request::Lyrics {
        serial: current, ..
    } = requests.try_recv().unwrap()
    else {
        panic!("Expected retry");
    };
    for (user, serial, id) in [
        (user + 1, current, id),
        (user, serial, id),
        (user, current, id + 1),
    ] {
        app.lyrics_result(user, serial, id, Ok(None));
        assert!(matches!(app.now_playing.state, State::Loading));
    }
    app.error = Some("Unrelated playback message".into());
    let paused = app.paused;
    app.lyrics_result(user, current, id, Err("Lyrics denied".into()));
    assert!(matches!(app.now_playing.state, State::Failed(_)));
    assert_eq!(app.paused, paused);
    assert_eq!(app.error.as_deref(), Some("Unrelated playback message"));
}

#[test]
fn occurrence_changes_reset_follow_without_refetching_identical_track_metadata() {
    let (ctx, mut app, mut requests) = fixture();
    app.fixture_now_playing(&ctx, "now-playing");
    app.now_playing.follow = false;
    app.queue_mut()
        .start(vec![super::super::tests::track(1)], 0, "Again".into(), None)
        .unwrap();
    app.lyrics_tick();
    assert!(app.now_playing.follow);
    assert!(matches!(app.now_playing.state, State::Ready(_)));
    assert!(requests.try_recv().is_err());
    app.account = Some(8);
    app.lyrics_tick();
    assert!(matches!(app.now_playing.state, State::Loading));
    assert!(matches!(
        requests.try_recv(),
        Ok(Request::Lyrics { user: 8, .. })
    ));
}

#[test]
fn scrolling_the_lyrics_suspends_follow_without_a_playback_request() {
    let (ctx, mut app, mut requests) = fixture();
    app.fixture_now_playing(&ctx, "now-playing");
    for scrolling in [false, false, true] {
        let events = if scrolling {
            vec![
                egui::Event::PointerMoved(egui::pos2(750., 300.)),
                egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: egui::vec2(0., -120.),
                    modifiers: egui::Modifiers::NONE,
                },
            ]
        } else {
            Vec::new()
        };
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    vec2(1000., 660.),
                )),
                events,
                ..Default::default()
            },
            |ctx| {
                app.player_bar(ctx);
                app.now_playing_view(ctx);
            },
        );
    }
    assert!(!app.now_playing.follow);
    assert!(requests.try_recv().is_err());
}

#[test]
fn lyrics_surfaces_render_at_minimum_and_zoom_sizes_without_live_services() {
    for kind in [
        "now-playing",
        "lyrics-plain",
        "lyrics-missing",
        "lyrics-rtl",
    ] {
        for size in [vec2(1000., 660.), vec2(667., 440.), vec2(1280., 800.)] {
            let (ctx, mut app, _) = fixture();
            app.fixture_now_playing(&ctx, kind);
            let output = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
                    ..Default::default()
                },
                |ctx| {
                    app.player_bar(ctx);
                    app.now_playing_view(ctx);
                },
            );
            assert!(!output.shapes.is_empty());
            assert_eq!(app.position_time(), Duration::from_secs(84));
            assert!(app.media.is_none() && app.instance.is_none() && app.state_store.is_none());
        }
    }
}
