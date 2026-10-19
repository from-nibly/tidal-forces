use super::*;
use crate::api::UnfollowVerification;
use crate::ui::tests::fixture;

fn prepare(app: &mut App) {
    let playlist = Playlist {
        uuid: "fixture-list".into(),
        title: "Other playlist".into(),
        kind: "USER".into(),
        creator: Some(crate::model::PlaylistCreator { id: 8 }),
        ..Default::default()
    };
    app.page = Page::Collection {
        kind: "playlists".into(),
        id: playlist.uuid.clone(),
        title: playlist.title.clone(),
    };
    app.folders.insert(
        "root".into(),
        vec![LibraryEntry::Playlist(playlist.clone())],
    );
    app.playlist_page = Some(PlaylistPage {
        playlist,
        etag: "revision-1".into(),
        editable: false,
        rows: vec![],
        next_offset: 0,
        more: false,
    });
    app.browse_loaded();
}

#[test]
fn unfollow_requires_positive_saved_membership_and_known_non_ownership() {
    for case in 0..5 {
        let (_, mut app, _events, mut requests) = fixture();
        prepare(&mut app);
        match case {
            0 => app.folders.clear(),
            1 => app.playlist_page.as_mut().unwrap().playlist.creator = None,
            2 => {
                app.playlist_page
                    .as_mut()
                    .unwrap()
                    .playlist
                    .creator
                    .as_mut()
                    .unwrap()
                    .id = 7
            }
            3 => app.browse_cache.owner = Some(8),
            _ => app.playlist_page.as_mut().unwrap().editable = true,
        }
        app.open_unfollow_playlist();
        assert!(app.delete_dialog.is_none());
        assert!(requests.try_recv().is_err());
    }
}

#[test]
fn unfollow_confirmation_is_distinct_and_single_dispatch() {
    let (_, mut app, _events, mut requests) = fixture();
    prepare(&mut app);
    app.open_delete_playlist();
    assert!(app.delete_dialog.is_none());
    app.open_unfollow_playlist();
    assert!(requests.try_recv().is_err());
    app.confirm_delete_playlist();
    app.confirm_delete_playlist();
    assert!(
        matches!(requests.try_recv(),Ok(Request::UnfollowPlaylist{user:7,id,etag,..}) if id=="fixture-list" && etag=="revision-1")
    );
    assert!(requests.try_recv().is_err());
}

#[test]
fn unfollow_button_click_never_dispatches_delete() {
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
    let (ctx, mut app, _events, mut requests) = fixture();
    prepare(&mut app);
    app.open_unfollow_playlist();
    let _ = draw(&ctx, &mut app, vec![]);
    let output = draw(&ctx, &mut app, vec![]);
    let pos = output
        .shapes
        .iter()
        .find_map(|s| match &s.shape {
            egui::Shape::Text(t) if t.galley.job.text == "Unfollow playlist" => {
                Some(t.pos + t.galley.size() * 0.5)
            }
            _ => None,
        })
        .unwrap();
    assert!(requests.try_recv().is_err());
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
    assert!(matches!(
        requests.try_recv(),
        Ok(Request::UnfollowPlaylist { .. })
    ));
    assert!(requests.try_recv().is_err());
}

#[test]
fn unknown_verification_never_claims_absence_or_retries() {
    let (_, mut app, _events, mut requests) = fixture();
    prepare(&mut app);
    app.open_unfollow_playlist();
    app.confirm_delete_playlist();
    let Request::UnfollowPlaylist { operation, .. } = requests.try_recv().unwrap() else {
        panic!()
    };
    app.playlist_unfollowed(
        7,
        operation,
        "fixture-list".into(),
        Ok(UnfollowVerification::Unknown),
    );
    assert!(
        app.delete_dialog
            .as_ref()
            .unwrap()
            .error
            .as_ref()
            .unwrap()
            .contains("could not be verified")
    );
    assert_eq!(app.folders["root"].len(), 1);
    app.confirm_delete_playlist();
    assert!(requests.try_recv().is_err());
}

#[test]
fn verified_unfollow_preserves_current_playlist_and_queue() {
    let (_, mut app, _events, mut requests) = fixture();
    prepare(&mut app);
    let page = app.page.clone();
    let queue = app.queue.revision();
    app.open_unfollow_playlist();
    app.confirm_delete_playlist();
    let Request::UnfollowPlaylist { operation, .. } = requests.try_recv().unwrap() else {
        panic!()
    };
    app.playlist_deleted(7, operation, "fixture-list".into(), Ok(()));
    assert!(app.delete_dialog.as_ref().unwrap().busy);
    app.playlist_unfollowed(
        7,
        operation,
        "fixture-list".into(),
        Ok(UnfollowVerification::Absent),
    );
    assert!(app.delete_dialog.is_none());
    assert_eq!(app.page, page);
    assert_eq!(app.queue.revision(), queue);
    assert!(app.folders["root"].is_empty());
}

#[test]
fn navigation_cancels_unfollow_and_late_account_results_do_not_redirect() {
    let (_, mut app, _events, mut requests) = fixture();
    prepare(&mut app);
    app.open_unfollow_playlist();
    app.confirm_delete_playlist();
    let Request::UnfollowPlaylist {
        operation,
        cancelled,
        ..
    } = requests.try_recv().unwrap()
    else {
        panic!()
    };
    app.navigate(Page::Settings);
    assert!(cancelled.load(Ordering::Acquire));
    app.playlist_unfollowed(
        8,
        operation,
        "fixture-list".into(),
        Ok(UnfollowVerification::Absent),
    );
    assert_eq!(app.folders["root"].len(), 1);
    app.playlist_unfollowed(
        7,
        operation,
        "fixture-list".into(),
        Ok(UnfollowVerification::Absent),
    );
    assert_eq!(app.page, Page::Settings);
}
