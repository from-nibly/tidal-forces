use super::super::tests::{fixture, frame, track};
use super::*;

fn ready(app: &mut App, ctx: &egui::Context) -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    app.state_store = Some(StateStore::new(root.path().into(), ctx.clone()).unwrap());
    app.listening.state = Some(History::new(7));
    app.set_history_enabled(true);
    app.queue
        .start(vec![track(9)], 0, "Test".into(), None)
        .unwrap();
    app.play_generation = 4;
    root
}
#[test]
fn restore_position_and_stale_events_cannot_fabricate_history_or_cross_accounts() {
    let (ctx, mut app, events, _requests) = fixture();
    let _root = ready(&mut app, &ctx);
    app.position = 120;
    app.paused = true;
    events
        .send(Event::Position {
            generation: 4,
            seconds: 179,
        })
        .unwrap();
    events
        .send(Event::Listening {
            generation: 3,
            rendered: Duration::from_secs(100),
            elapsed: Duration::from_secs(100),
        })
        .unwrap();
    app.events();
    assert!(app.listening.state.as_ref().unwrap().entries.is_empty());
    app.observe_listening(4, Duration::ZERO, Duration::ZERO); // opt-in baseline
    app.observe_listening(4, Duration::from_secs(30), Duration::from_secs(30));
    assert_eq!(app.listening.state.as_ref().unwrap().entries.len(), 1);
    let old_request = app.listening.request;
    let old = app.listening.state.clone().unwrap();
    app.history_account_changed();
    app.account = Some(8);
    app.history_loaded(7, old_request, Ok(old));
    app.observe_listening(3, Duration::from_secs(100), Duration::from_secs(100));
    assert!(app.listening.state.is_none());
}
#[test]
fn clear_and_disable_flush_on_exit_without_changing_the_queue() {
    let (ctx, mut app, _events, _requests) = fixture();
    let root = ready(&mut app, &ctx);
    app.observe_listening(4, Duration::ZERO, Duration::ZERO);
    app.observe_listening(4, Duration::from_secs(30), Duration::from_secs(30));
    assert_eq!(app.listening.state.as_ref().unwrap().entries.len(), 1);
    app.clear_history();
    app.set_history_enabled(false);
    app.checkpoint_history(true);
    app.state_store.take();
    let history = crate::history::load(root.path(), 7, crate::history::now()).unwrap();
    assert!(history.entries.is_empty());
    assert!(!history.enabled);
    assert_eq!(app.queue.current().unwrap().id, 9);
}
#[test]
fn unreadable_history_remains_disabled_until_explicit_reset() {
    let (ctx, mut app, _events, _requests) = fixture();
    let root = ready(&mut app, &ctx);
    app.history_loaded(7, app.listening.request, Err("Invalid history".into()));
    app.observe_listening(4, Duration::from_secs(120), Duration::from_secs(120));
    assert!(app.listening.state.is_none());
    app.clear_history();
    app.state_store.take();
    assert!(!crate::history::load(root.path(), 7, 0).unwrap().enabled);
}
#[test]
fn save_failures_remain_visible_when_new_playback_progress_arrives() {
    let (ctx, mut app, _events, _requests) = fixture();
    let _root = ready(&mut app, &ctx);
    let submitted = app.listening.saved_revision.unwrap();
    app.observe_listening(4, Duration::ZERO, Duration::ZERO);
    app.observe_listening(4, Duration::from_secs(30), Duration::from_secs(30));
    assert_ne!(app.listening.state.as_ref().unwrap().revision(), submitted);
    app.history_saved(7, submitted, Err("Disk unavailable".into()));
    assert!(
        app.error
            .as_ref()
            .unwrap()
            .contains("previous history and recording preference may return")
    );
    app.history_saved(8, submitted, Ok(()));
    assert!(app.listening.error.is_some());
    app.history_saved(7, submitted, Ok(()));
    assert!(app.listening.error.is_none() && app.error.is_none());
}

fn panel(ctx: &egui::Context, app: &mut App, events: Vec<egui::Event>) -> egui::FullOutput {
    ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                vec2(320., 660.),
            )),
            events,
            ..Default::default()
        },
        |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| app.history_panel(ui));
        },
    )
}

fn click_label(ctx: &egui::Context, app: &mut App, label: &str) {
    let output = panel(ctx, app, vec![]);
    let pos = output
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::Shape::Text(text) if text.galley.job.text == label => {
                Some(text.pos + text.galley.size() / 2.)
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("Missing {label}"));
    for pressed in [true, false] {
        panel(
            ctx,
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
}

#[test]
fn history_switch_is_direct_keyboard_accessible_and_retains_records_when_disabled() {
    let (ctx, mut app, _events, mut requests) = fixture();
    let root = ready(&mut app, &ctx);
    app.observe_listening(4, Duration::ZERO, Duration::ZERO);
    app.observe_listening(4, Duration::from_secs(30), Duration::from_secs(30));
    let revision = app.queue.revision();
    let output = panel(&ctx, &mut app, vec![]);
    for removed in [
        "Recording & privacy",
        "Local only · Recording on",
        "Local only · Recording off",
        "Local to this account. Nothing is uploaded to TIDAL.",
    ] {
        assert!(!output.shapes.iter().any(
            |shape| matches!(&shape.shape,egui::Shape::Text(text) if text.galley.job.text==removed)
        ));
    }
    click_label(&ctx, &mut app, "Record listening history");
    assert!(!app.listening.state.as_ref().unwrap().enabled);
    assert_eq!(app.listening.state.as_ref().unwrap().entries.len(), 1);
    panel(
        &ctx,
        &mut app,
        vec![egui::Event::Key {
            key: egui::Key::Space,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }],
    );
    assert!(app.listening.state.as_ref().unwrap().enabled);
    panel(
        &ctx,
        &mut app,
        vec![egui::Event::Key {
            key: egui::Key::Space,
            physical_key: None,
            pressed: false,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }],
    );
    click_label(&ctx, &mut app, "Record listening history");
    assert_eq!(app.queue.revision(), revision);
    assert!(requests.try_recv().is_err());
    app.state_store.take();
    let history = crate::history::load(root.path(), 7, crate::history::now()).unwrap();
    assert!(!history.enabled);
    assert_eq!(history.entries.len(), 1);
}

#[test]
fn history_switch_respects_unavailable_states_and_errors_and_clear_stay_visible() {
    for state in [
        "loading",
        "storage",
        "unreadable",
        "signed-out",
        "other-account",
    ] {
        let (ctx, mut app, _events, mut requests) = fixture();
        let _root = ready(&mut app, &ctx);
        match state {
            "loading" => app.listening.loading = true,
            "storage" => {
                app.state_store.take();
            }
            "unreadable" => app.listening.state = None,
            "signed-out" => app.account = None,
            _ => app.account = Some(8),
        }
        let before = app.listening.state.as_ref().map(|history| history.enabled);
        click_label(&ctx, &mut app, "Record listening history");
        assert_eq!(
            app.listening.state.as_ref().map(|history| history.enabled),
            before,
            "{state}"
        );
        assert!(requests.try_recv().is_err());
    }
    let (ctx, mut app, _events, mut requests) = fixture();
    let _root = ready(&mut app, &ctx);
    app.observe_listening(4, Duration::ZERO, Duration::ZERO);
    app.observe_listening(4, Duration::from_secs(30), Duration::from_secs(30));
    app.listening.error = Some("Disk unavailable".into());
    let output = panel(&ctx, &mut app, vec![]);
    for visible in ["Disk unavailable", "Retry saving history"] {
        assert!(output.shapes.iter().any(
            |shape| matches!(&shape.shape,egui::Shape::Text(text) if text.galley.job.text==visible)
        ));
    }
    app.listening.error = None;
    click_label(&ctx, &mut app, "Clear local history…");
    assert!(app.listening.confirm_clear);
    assert_eq!(app.listening.state.as_ref().unwrap().entries.len(), 1);
    click_label(&ctx, &mut app, "Cancel");
    assert!(!app.listening.confirm_clear);
    assert_eq!(app.listening.state.as_ref().unwrap().entries.len(), 1);
    click_label(&ctx, &mut app, "Clear local history…");
    click_label(&ctx, &mut app, "Confirm clear history");
    assert!(app.listening.state.as_ref().unwrap().entries.is_empty());
    assert!(app.listening.state.as_ref().unwrap().enabled);
    assert_eq!(app.queue.current().unwrap().id, 9);
    assert!(requests.try_recv().is_err());
}

#[test]
fn history_panel_is_virtualized_and_bounded_at_minimum_and_high_zoom_sizes() {
    let (ctx, mut app, _events, _requests) = fixture();
    let _root = ready(&mut app, &ctx);
    // Enable directly here; the recording control deliberately discards its first interval.
    app.listening.listener = Listener::default();
    for generation in 1..=1000 {
        app.play_generation = generation;
        app.observe_listening(generation, Duration::from_secs(30), Duration::from_secs(30));
    }
    assert_eq!(app.listening.state.as_ref().unwrap().entries.len(), 1000);
    app.queue_open = true;
    app.listening.show_panel = true;
    for size in [vec2(1240., 820.), vec2(1000., 660.), vec2(666., 440.)] {
        for page in [Page::Home, Page::Settings] {
            app.page = page;
            app.settings_section = SettingsSection::Privacy;
            let (output, bounds) = frame(&ctx, &mut app, size);
            assert!(
                bounds.right() <= size.x + 1.,
                "Horizontal overflow: {bounds:?}"
            );
            assert!(
                output.shapes.len() < 2000,
                "History rendered unbounded widgets"
            );
        }
    }
}
