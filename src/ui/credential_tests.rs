use super::super::tests::{fixture, frame};
use super::*;

#[test]
fn credential_status_from_unrelated_work_does_not_unlock_pending_controls() {
    let (_ctx, mut app, events, _requests) = fixture();
    app.credentials.busy = true;
    for finished in [false, true] {
        events
            .send(Event::Credentials {
                status: Ok(Status {
                    storage: Storage::Keyring,
                    signed_out: false,
                    cleanup_pending: false,
                }),
                warning: Some("Storage failed".into()),
                dirty: true,
                finished,
            })
            .unwrap();
        app.events();
        assert_eq!(app.credentials.busy, !finished);
        assert!(app.credentials.dirty);
    }
}
#[test]
fn unavailable_worker_does_not_pretend_to_sign_out_or_migrate() {
    let (_ctx, mut app, _events, requests) = fixture();
    drop(requests);
    app.logout();
    assert!(app.connected && app.account == Some(7));
    assert!(!app.credentials.busy);
    app.credential_request(Request::MigrateCredentials { user: 7 });
    assert!(!app.credentials.busy);
    assert!(app.error.is_some());
}
#[test]
fn close_requires_confirmation_for_unsaved_or_in_flight_credentials() {
    for (dirty, busy) in [(true, false), (false, true), (false, false)] {
        let (ctx, mut app, _events, _requests) = fixture();
        app.credentials.dirty = dirty;
        app.credentials.busy = busy;
        let mut input = egui::RawInput::default();
        input
            .viewports
            .entry(egui::ViewportId::ROOT)
            .or_default()
            .events
            .push(egui::ViewportEvent::Close);
        let output = ctx.run(input, |ctx| app.credential_close_guard(ctx));
        let cancelled = output.viewport_output[&egui::ViewportId::ROOT]
            .commands
            .iter()
            .any(|command| matches!(command, egui::ViewportCommand::CancelClose));
        assert_eq!(cancelled, dirty || busy);
        if dirty || busy {
            app.credentials.allow_exit = true;
            let mut input = egui::RawInput::default();
            input
                .viewports
                .entry(egui::ViewportId::ROOT)
                .or_default()
                .events
                .push(egui::ViewportEvent::Close);
            let output = ctx.run(input, |ctx| app.credential_close_guard(ctx));
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
fn credential_confirmation_and_failures_fit_narrow_privacy_settings() {
    let (ctx, mut app, _events, _requests) = fixture();
    app.page = Page::Settings;
    app.settings_section = SettingsSection::Privacy;
    app.credentials.status = Some(Ok(Status {
        storage: Storage::Legacy,
        signed_out: false,
        cleanup_pending: true,
    }));
    app.credentials.confirm = true;
    app.credentials.warning = Some("Credential storage is locked. The latest sign-in is in memory only; unlock and retry before exiting.".into());
    for size in [vec2(1240., 820.), vec2(1000., 660.), vec2(666., 440.)] {
        let (_, bounds) = frame(&ctx, &mut app, size);
        assert!(bounds.right() <= size.x + 1., "Overflow: {bounds:?}");
    }
}
