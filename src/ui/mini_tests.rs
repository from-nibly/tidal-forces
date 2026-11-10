use super::super::tests::{fixture, track};
use super::*;

#[test]
fn mini_and_focused_view_share_queue_clock_and_volume_without_restarting_playback() {
    let (ctx, mut app, _, mut requests) = fixture();
    app.queue_mut()
        .start(vec![track(1)], 0, "Fixture".into(), None)
        .unwrap();
    let revision = app.queue.revision();
    let generation = app.play_generation;
    app.dispatch(Action::MiniPlayer, &ctx);
    assert!(app.mini_open);
    let volume = app.volume;
    app.dispatch(Action::VolumeBy(-0.1), &ctx);
    assert!((app.volume - (volume - 0.1)).abs() < 0.001);
    app.dispatch(Action::NowPlaying, &ctx);
    assert!(app.now_playing.open);
    app.mini_open = false;
    assert_eq!(app.queue.revision(), revision);
    assert_eq!(app.play_generation, generation);
    assert!(requests.try_recv().is_err());
    app.dispatch(Action::VolumeBy(f32::NAN), &ctx);
    assert!(app.volume.is_finite());
}

#[test]
fn close_to_tray_requires_opt_in_host_safe_credentials_and_supported_window_backend() {
    for (enabled, ready, dirty, busy, quit) in [
        (false, true, false, false, false),
        (true, false, false, false, false),
        (true, true, true, false, false),
        (true, true, false, true, false),
        (true, true, false, false, true),
        (true, true, false, false, false),
    ] {
        let (ctx, mut app, _, _) = fixture();
        app.appearance.tray_enabled = enabled;
        app.tray = Some(crate::tray::Tray::fixture(ctx.clone(), ready));
        app.credentials.dirty = dirty;
        app.credentials.busy = busy;
        if quit {
            app.dispatch(Action::Quit, &ctx);
        }
        let mut input = egui::RawInput::default();
        input
            .viewports
            .entry(egui::ViewportId::ROOT)
            .or_default()
            .events
            .push(egui::ViewportEvent::Close);
        let output = ctx.run(input, |ctx| {
            assert_eq!(
                app.background_close(ctx),
                enabled && ready && !dirty && !busy && !quit && !wayland()
            );
            app.credential_close_guard(ctx);
        });
        assert_eq!(app.credential_close_pending(), dirty || busy);
        if quit && !dirty && !busy {
            assert!(
                !output.viewport_output[&egui::ViewportId::ROOT]
                    .commands
                    .iter()
                    .any(|command| matches!(command, egui::ViewportCommand::CancelClose))
            );
        }
    }
}

#[test]
fn mini_renders_at_minimum_and_normal_sizes_without_mutating_queue() {
    let (ctx, mut app, _, mut requests) = fixture();
    app.queue_mut()
        .start(vec![track(1)], 0, "Fixture".into(), None)
        .unwrap();
    let revision = app.queue.revision();
    for size in [vec2(380., 250.), vec2(460., 260.)] {
        let output = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default()
                    .frame(egui::Frame::new().inner_margin(16))
                    .show(ctx, |ui| {
                        app.mini_contents(ui);
                        assert!(ui.min_rect().bottom() <= ui.max_rect().bottom() + 1.);
                        assert!(ui.min_rect().right() <= ui.max_rect().right() + 1.);
                    });
            },
        );
        assert!(!output.shapes.is_empty());
        assert!(
            output
                .shapes
                .iter()
                .any(|s| matches!(&s.shape,egui::Shape::Rect(r) if r.rect.size()==vec2(21.,15.))),
            "Mini player must expose the Cast control"
        );
    }
    assert_eq!(app.queue.revision(), revision);
    assert!(requests.try_recv().is_err());
}

#[test]
fn legacy_preferences_do_not_enable_tray_or_window_pinning() {
    let appearance: crate::store::Appearance = serde_json::from_str("{}").unwrap();
    assert!(
        !appearance.tray_enabled
            && !appearance.mini_on_top
            && !appearance.radio_continuation
            && !appearance.classic_skin
    );
}
