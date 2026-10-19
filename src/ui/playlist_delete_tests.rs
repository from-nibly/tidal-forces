use super::*;
use crate::ui::tests::fixture;

fn prepare(app: &mut App) {
    app.page = Page::Collection {
        kind: "playlists".into(),
        id: "fixture-list".into(),
        title: "Fixture".into(),
    };
    app.playlist_page = Some(PlaylistPage {
        playlist: Playlist {
            uuid: "fixture-list".into(),
            title: "Fixture".into(),
            ..Default::default()
        },
        etag: "revision-1".into(),
        editable: true,
        rows: vec![],
        next_offset: 0,
        more: true,
    });
    app.browse_loaded();
    app.open_delete_playlist();
}

#[test]
fn deletion_requires_explicit_confirmation_and_never_double_dispatches() {
    let (_, mut app, _events, mut requests) = fixture();
    prepare(&mut app);
    app.confirm_delete_playlist();
    assert!(requests.try_recv().is_err());
    app.delete_dialog.as_mut().unwrap().confirmation = "delete".into();
    app.confirm_delete_playlist();
    assert!(requests.try_recv().is_err());
    let queue = app.queue.revision();
    app.delete_dialog.as_mut().unwrap().confirmation = "DELETE".into();
    app.confirm_delete_playlist();
    app.confirm_delete_playlist();
    assert!(
        matches!(requests.try_recv(),Ok(Request::DeletePlaylist{user:7,id,etag,..}) if id=="fixture-list" && etag=="revision-1")
    );
    assert!(requests.try_recv().is_err());
    assert_eq!(queue, app.queue.revision());
    assert!(!app.browse_cache.ready);
}

#[test]
fn deletion_rejects_stale_account_revision_generation_or_ownership_at_confirmation() {
    for case in 0..7 {
        let (_, mut app, _events, mut requests) = fixture();
        prepare(&mut app);
        app.delete_dialog.as_mut().unwrap().confirmation = "DELETE".into();
        match case {
            0 => app.account = Some(8),
            1 => app.generation += 1,
            2 => app.playlist_page.as_mut().unwrap().editable = false,
            3 => app.playlist_page.as_mut().unwrap().etag = "new".into(),
            4 => app.connected = false,
            5 => app.browse_cache.owner = Some(8),
            _ => app.playlist_page.as_mut().unwrap().playlist.title.clear(),
        }
        app.confirm_delete_playlist();
        assert!(requests.try_recv().is_err());
    }
}

#[test]
fn deletion_cancel_navigation_and_late_reply_do_not_change_current_page() {
    let (_, mut app, _events, mut requests) = fixture();
    prepare(&mut app);
    let token = app.delete_dialog.as_ref().unwrap().cancelled.clone();
    app.cancel_delete_playlist();
    assert!(token.load(Ordering::Acquire));
    assert!(requests.try_recv().is_err());
    app.open_delete_playlist();
    app.delete_dialog.as_mut().unwrap().confirmation = "DELETE".into();
    app.confirm_delete_playlist();
    let Request::DeletePlaylist {
        operation,
        cancelled,
        ..
    } = requests.try_recv().unwrap()
    else {
        panic!()
    };
    app.navigate(Page::Settings);
    assert!(cancelled.load(Ordering::Acquire));
    app.playlist_deleted(7, operation, "fixture-list".into(), Ok(()));
    assert_eq!(app.page, Page::Settings);
    assert!(app.delete_dialog.is_none());
    assert!(requests.try_recv().is_err());
}

#[test]
fn deletion_failure_requires_refresh_and_foreign_replies_are_ignored() {
    let (_, mut app, _events, mut requests) = fixture();
    prepare(&mut app);
    app.delete_dialog.as_mut().unwrap().confirmation = "DELETE".into();
    app.confirm_delete_playlist();
    let Request::DeletePlaylist { operation, .. } = requests.try_recv().unwrap() else {
        panic!()
    };
    app.playlist_deleted(8, operation, "fixture-list".into(), Ok(()));
    assert!(app.delete_dialog.as_ref().unwrap().busy);
    app.playlist_deleted(
        7,
        operation,
        "fixture-list".into(),
        Err("May have completed".into()),
    );
    app.confirm_delete_playlist();
    assert!(requests.try_recv().is_err());
    assert!(app.delete_dialog.as_ref().unwrap().error.is_some());
}

#[test]
fn verified_deletion_returns_to_library_without_touching_queue() {
    let (_, mut app, _events, mut requests) = fixture();
    prepare(&mut app);
    let queue = app.queue.revision();
    app.delete_dialog.as_mut().unwrap().confirmation = "DELETE".into();
    app.confirm_delete_playlist();
    let Request::DeletePlaylist { operation, .. } = requests.try_recv().unwrap() else {
        panic!()
    };
    app.playlist_deleted(7, operation, "fixture-list".into(), Ok(()));
    assert_eq!(app.page, Page::Playlists);
    assert_eq!(app.queue.revision(), queue);
    assert!(app.delete_dialog.is_none());
    assert!(app.notice.as_ref().unwrap().contains("deletion verified"));
}

#[test]
fn destructive_button_click_requires_typed_confirmation() {
    fn draw(ctx: &egui::Context, app: &mut App, events: Vec<egui::Event>) -> egui::FullOutput {
        ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    vec2(900., 650.),
                )),
                events,
                ..Default::default()
            },
            |ctx| app.delete_playlist_window(ctx),
        )
    }
    fn button(output: &egui::FullOutput) -> egui::Pos2 {
        output
            .shapes
            .iter()
            .find_map(|s| match &s.shape {
                egui::Shape::Text(t) if t.galley.job.text == "Delete playlist permanently" => {
                    Some(t.pos + t.galley.size() * 0.5)
                }
                _ => None,
            })
            .unwrap()
    }
    let (ctx, mut app, _events, mut requests) = fixture();
    prepare(&mut app);
    for confirmed in [false, true] {
        if confirmed {
            app.delete_dialog.as_mut().unwrap().confirmation = "DELETE".into();
        }
        let _ = draw(&ctx, &mut app, vec![]);
        let output = draw(&ctx, &mut app, vec![]);
        let pos = button(&output);
        for pressed in [true, false] {
            let _ = draw(
                &ctx,
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
        if confirmed {
            assert!(matches!(
                requests.try_recv(),
                Ok(Request::DeletePlaylist { .. })
            ));
        }
        assert!(requests.try_recv().is_err());
    }
}

#[test]
fn deletion_backpressure_and_editor_overlap_are_safe() {
    let (_, mut app, _events, requests) = fixture();
    prepare(&mut app);
    app.playlist_page.as_mut().unwrap().playlist.description = Some(String::new());
    app.open_metadata_editor();
    assert!(app.metadata_editor.is_none());
    drop(requests);
    app.delete_dialog.as_mut().unwrap().confirmation = "DELETE".into();
    app.confirm_delete_playlist();
    assert!(!app.delete_dialog.as_ref().unwrap().busy);
    assert!(app.delete_dialog.as_ref().unwrap().error.is_some());
}
