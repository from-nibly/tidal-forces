use super::*;
use crate::ui::tests::{fixture, track};

fn prepare(app: &mut App) {
    app.page = Page::Collection {
        kind: "playlists".into(),
        id: "fixture-list".into(),
        title: "Fixture".into(),
    };
    app.tracks = vec![track(9), track(8), track(9)];
    app.playlist_page = Some(PlaylistPage {
        playlist: Playlist {
            uuid: "fixture-list".into(),
            title: "Fixture".into(),
            number_of_tracks: 3,
            ..Default::default()
        },
        etag: "revision-1".into(),
        editable: true,
        rows: app.tracks.iter().cloned().enumerate().collect(),
        next_offset: 3,
        more: false,
    });
    app.browse_loaded();
}
#[test]
fn reorder_action_freezes_raw_occurrence_and_never_changes_queue() {
    let (_, mut app, _events, mut requests) = fixture();
    prepare(&mut app);
    let revision = app.queue.revision();
    app.open_reorder(2, Target::Top);
    app.open_reorder(2, Target::Top);
    assert!(
        matches!(requests.try_recv(),Ok(Request::ReorderPlaylist{change,..}) if change.from==2 && change.ids==[9,8,9] && change.target==Target::Top)
    );
    assert!(requests.try_recv().is_err());
    assert_eq!(app.queue.revision(), revision);
    assert!(!app.browse_cache.ready);
}
#[test]
fn reorder_is_unavailable_for_partial_transformed_video_or_foreign_views() {
    for case in 0..5 {
        let (_, mut app, _events, mut requests) = fixture();
        prepare(&mut app);
        match case {
            0 => app.playlist_page.as_mut().unwrap().more = true,
            1 => app.track_view.reverse = true,
            2 => {
                app.playlist_page
                    .as_mut()
                    .unwrap()
                    .playlist
                    .number_of_videos = 1
            }
            3 => app.playlist_page.as_mut().unwrap().editable = false,
            _ => {
                app.playlist_page.as_mut().unwrap().rows.remove(0);
            }
        }
        app.open_reorder(2, Target::Top);
        assert!(app.reorder_dialog.is_none());
        assert!(requests.try_recv().is_err());
    }
}
#[test]
fn stale_account_or_unready_view_blocks_dispatch_and_failures_require_refresh() {
    for account in [true, false] {
        let (_, mut app, _events, mut requests) = fixture();
        prepare(&mut app);
        if account {
            app.account = Some(8)
        } else {
            app.browse_cache.ready = false;
        }
        app.open_reorder(2, Target::Top);
        assert!(requests.try_recv().is_err());
    }
    let (_, mut app, _events, mut requests) = fixture();
    prepare(&mut app);
    app.open_reorder(2, Target::Top);
    requests.try_recv().unwrap();
    let operation = app.reorder_serial;
    app.reorder_result(8, operation, "fixture-list".into(), Err("foreign".into()));
    assert!(app.reorder_dialog.as_ref().unwrap().error.is_none());
    app.reorder_result(
        7,
        operation,
        "fixture-list".into(),
        Err("Refresh before retrying".into()),
    );
    app.open_reorder(2, Target::Top);
    assert!(requests.try_recv().is_err());
}
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
        |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| app.track_list(ui));
            app.reorder_window(ctx);
        },
    )
}
fn click(ctx: &egui::Context, app: &mut App, pos: egui::Pos2) {
    for pressed in [true, false] {
        let _ = draw(
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
fn label(output: &egui::FullOutput, value: &str) -> egui::Pos2 {
    output
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::Shape::Text(text) if text.galley.job.text == value => {
                Some(text.pos + text.galley.size() * 0.5)
            }
            _ => None,
        })
        .unwrap()
}
#[test]
fn row_menu_dispatches_once_without_a_confirmation_click() {
    for (action, target, from) in [
        ("Move to playlist bottom", Target::Bottom, 0),
        ("Move down in playlist", Target::Down, 0),
        ("Move up in playlist", Target::Up, 1),
        ("Move to playlist top", Target::Top, 1),
    ] {
        let (ctx, mut app, _events, mut requests) = fixture();
        prepare(&mut app);
        let revision = app.queue.revision();
        let _ = draw(&ctx, &mut app, vec![]);
        let output = draw(&ctx, &mut app, vec![]);
        let more = output
            .shapes
            .iter()
            .filter_map(|shape| match &shape.shape {
                egui::Shape::Circle(circle) if circle.radius == 1.25 => Some(circle.center),
                _ => None,
            })
            .nth(from * 3)
            .unwrap();
        click(&ctx, &mut app, more);
        let output = draw(&ctx, &mut app, vec![]);
        click(&ctx, &mut app, label(&output, action));
        let output = draw(&ctx, &mut app, vec![]);
        assert!(!output.shapes.iter().any(
        |shape| matches!(&shape.shape,egui::Shape::Text(text) if text.galley.job.text=="Move track")
    ));
        assert!(app.reorder_dialog.as_ref().unwrap().busy);
        assert!(
            matches!(requests.try_recv(),Ok(Request::ReorderPlaylist{change,..}) if change.from==from && change.target==target)
        );
        assert_eq!(app.queue.revision(), revision);
        assert!(requests.try_recv().is_err());
    }
}

#[test]
fn closed_worker_releases_reorder_busy_state_without_retry() {
    let (_, mut app, _events, requests) = fixture();
    prepare(&mut app);
    drop(requests);
    app.open_reorder(2, Target::Top);
    let dialog = app.reorder_dialog.as_ref().unwrap();
    assert!(!dialog.busy);
    assert!(dialog.error.is_some());
}
