use super::*;
use crate::ui::tests::{fixture, track};

fn prepare(app: &mut App, count: usize) {
    app.page = Page::Collection {
        kind: "playlists".into(),
        id: "fixture-list".into(),
        title: "Fixture".into(),
    };
    app.tracks = (0..count).map(|i| track((i % 7 + 1) as u64)).collect();
    app.playlist_page = Some(PlaylistPage {
        playlist: Playlist {
            uuid: "fixture-list".into(),
            title: "Fixture".into(),
            number_of_tracks: count as u64,
            ..Default::default()
        },
        etag: "revision-1".into(),
        editable: true,
        rows: app.tracks.iter().cloned().enumerate().collect(),
        next_offset: count,
        more: false,
    });
    app.browse_loaded();
}
fn checked(app: &mut App, request: Request, ids: &[u64], offset: usize, etag: &str) {
    let Request::CheckReorder {
        operation,
        check,
        cancelled,
    } = request
    else {
        panic!("expected a verification page")
    };
    assert_eq!(check.offset, offset);
    assert_eq!(check.etag, etag);
    assert_eq!(check.count, ids.len());
    assert!(!cancelled.load(Ordering::Acquire));
    app.reorder_checked(
        operation,
        check,
        Ok(ids[offset..(offset + 100).min(ids.len())].to_vec()),
    );
}
#[test]
fn playlist_drag_boundaries_and_stale_snapshots_are_guarded() {
    let (_, mut app, _events, mut requests) = fixture();
    prepare(&mut app, 205);
    let drag = app.playlist_drag(0).unwrap();
    assert_eq!(app.playlist_drop_position(&drag, 0, false), None);
    assert_eq!(app.playlist_drop_position(&drag, 0, true), None);
    assert_eq!(app.playlist_drop_position(&drag, 103, true), Some(103));
    assert_eq!(app.playlist_drop_position(&drag, 204, true), Some(204));
    assert_eq!(app.playlist_drop_position(&drag, 205, true), None);
    app.tracks[1].id = 99;
    app.drop_playlist(&drag, 103);
    assert!(requests.try_recv().is_err());
    for case in 0..5 {
        let (_, mut app, _events, mut requests) = fixture();
        prepare(&mut app, 205);
        let drag = app.playlist_drag(204).unwrap();
        assert_eq!(app.playlist_drop_position(&drag, 0, false), Some(0));
        match case {
            0 => app.account = Some(8),
            1 => app.generation += 1,
            2 => app.track_view.reverse = true,
            3 => app.playlist_page.as_mut().unwrap().etag = "changed".into(),
            _ => app.invalidate_browse_cache(),
        }
        app.drop_playlist(&drag, 0);
        assert!(requests.try_recv().is_err());
    }
}

#[test]
fn dragging_a_playlist_grip_dispatches_only_on_drop_and_never_plays() {
    let (ctx, mut app, _events, mut requests) = fixture();
    prepare(&mut app, 3);
    let queue = app.queue.revision();
    let _ = draw(&ctx, &mut app, vec![]);
    let output = draw(&ctx, &mut app, vec![]);
    let source = label(&output, "2");
    let target = label(&output, "3") + vec2(200., 20.);
    let _ = draw(
        &ctx,
        &mut app,
        vec![
            egui::Event::PointerMoved(source),
            egui::Event::PointerButton {
                pos: source,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            },
        ],
    );
    let _ = draw(
        &ctx,
        &mut app,
        vec![egui::Event::PointerMoved(source + vec2(15., 0.))],
    );
    assert!(egui::DragAndDrop::has_payload_of_type::<playlist_drag::Drag>(&ctx));
    assert!(requests.try_recv().is_err());
    let _ = draw(&ctx, &mut app, vec![egui::Event::PointerMoved(target)]);
    let _ = draw(
        &ctx,
        &mut app,
        vec![egui::Event::PointerButton {
            pos: target,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        }],
    );
    assert!(matches!(
        requests.try_recv(),
        Ok(Request::CheckReorder { .. })
    ));
    assert!(requests.try_recv().is_err());
    assert_eq!(app.queue.revision(), queue);
    let dialog = app.reorder_dialog.as_ref().unwrap();
    assert_eq!(dialog.change.from, 1);
    assert_eq!(dialog.change.target, Target::Position(2));
}

#[test]
fn playlist_drag_scrolls_at_viewport_edges_without_dispatching() {
    let (ctx, mut app, _events, mut requests) = fixture();
    prepare(&mut app, 205);
    egui::DragAndDrop::set_payload(&ctx, app.playlist_drag(0).unwrap());
    let mut offset = 0.;
    for _ in 0..24 {
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    vec2(300., 250.),
                )),
                events: vec![egui::Event::PointerMoved(pos2(150., 235.))],
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    offset = egui::ScrollArea::vertical()
                        .show(ui, |ui| {
                            app.playlist_drag_scroll(ui);
                            ui.allocate_space(vec2(200., 3000.));
                        })
                        .state
                        .offset
                        .y;
                });
            },
        );
    }
    assert!(offset > 0.);
    assert!(requests.try_recv().is_err());
}

#[test]
fn position_picker_is_inert_until_a_valid_destination_and_explicit_submit() {
    let (_, mut app, _events, mut requests) = fixture();
    prepare(&mut app, 205);
    app.open_position_reorder(204);
    app.reorder_tick();
    assert!(requests.try_recv().is_err());
    for position in [
        "0",
        "205",
        "206",
        "-1",
        "1.5",
        "+2",
        "9999999999999999999999999999999999",
        "00000000000000000000000000000004",
        "",
    ] {
        app.reorder_dialog.as_mut().unwrap().position = position.into();
        app.submit_position_reorder();
        assert!(requests.try_recv().is_err());
        assert!(app.reorder_dialog.as_ref().unwrap().choosing_position);
    }
    app.reorder_dialog.as_mut().unwrap().position = "104".into();
    app.submit_position_reorder();
    app.submit_position_reorder();
    assert!(matches!(requests.try_recv(),Ok(Request::CheckReorder{check,..}) if check.offset==0));
    assert!(requests.try_recv().is_err());
    let dialog = app.reorder_dialog.as_ref().unwrap();
    assert_eq!(dialog.change.target, Target::Position(103));
    assert!(!dialog.choosing_position);
}
#[test]
fn position_picker_rejects_changed_scope_or_source_before_dispatch() {
    for case in 0..5 {
        let (_, mut app, _events, mut requests) = fixture();
        prepare(&mut app, 205);
        app.open_position_reorder(204);
        app.reorder_dialog.as_mut().unwrap().position = "1".into();
        match case {
            0 => app.account = Some(8),
            1 => app.generation += 1,
            2 => app.track_view.reverse = true,
            3 => app.playlist_page.as_mut().unwrap().etag = "changed".into(),
            _ => app.tracks[0].id = 99,
        }
        app.submit_position_reorder();
        assert!(requests.try_recv().is_err());
    }
}
#[test]
fn positions_complete_paged_verification_in_both_directions_without_changing_queue() {
    for (from, to) in [(204, 103), (0, 103), (103, 204), (204, 0)] {
        let (_, mut app, _events, mut requests) = fixture();
        prepare(&mut app, 205);
        let original: Vec<_> = app.tracks.iter().map(|t| t.id).collect();
        let queue = app.queue.revision();
        app.open_position_reorder(from);
        app.reorder_dialog.as_mut().unwrap().position = (to + 1).to_string();
        app.submit_position_reorder();
        let expected = app.reorder_dialog.as_ref().unwrap().expected.clone();
        for offset in [0, 100, 200] {
            if offset > 0 {
                app.reorder_tick();
            }
            checked(
                &mut app,
                requests.try_recv().unwrap(),
                &original,
                offset,
                "revision-1",
            );
        }
        app.reorder_tick();
        let Request::ReorderPlaylist {
            operation, change, ..
        } = requests.try_recv().unwrap()
        else {
            panic!()
        };
        assert_eq!(change.indices().unwrap(), (from, to));
        app.reorder_result(7, operation, "fixture-list".into(), Ok("revision-2".into()));
        for offset in [0, 100, 200] {
            app.reorder_tick();
            checked(
                &mut app,
                requests.try_recv().unwrap(),
                &expected,
                offset,
                "revision-2",
            );
        }
        assert!(app.reorder_dialog.is_none());
        assert_eq!(app.queue.revision(), queue);
        assert_eq!(
            app.tracks.iter().map(|t| t.id).collect::<Vec<_>>(),
            expected
        );
    }
}

#[test]
fn wrong_forward_readback_never_reports_success_or_retries() {
    let (_, mut app, _events, mut requests) = fixture();
    prepare(&mut app, 205);
    let original: Vec<_> = app.tracks.iter().map(|t| t.id).collect();
    app.open_position_reorder(0);
    app.reorder_dialog.as_mut().unwrap().position = "104".into();
    app.submit_position_reorder();
    for offset in [0, 100, 200] {
        if offset > 0 {
            app.reorder_tick();
        }
        checked(
            &mut app,
            requests.try_recv().unwrap(),
            &original,
            offset,
            "revision-1",
        );
    }
    app.reorder_tick();
    let Request::ReorderPlaylist { operation, .. } = requests.try_recv().unwrap() else {
        panic!()
    };
    app.reorder_result(7, operation, "fixture-list".into(), Ok("revision-2".into()));
    app.reorder_tick();
    checked(
        &mut app,
        requests.try_recv().unwrap(),
        &original,
        0,
        "revision-2",
    );
    assert!(app.reorder_dialog.as_ref().unwrap().error.is_some());
    assert!(!app.browse_cache.ready);
    assert_eq!(
        app.tracks.iter().map(|t| t.id).collect::<Vec<_>>(),
        original
    );
    app.reorder_tick();
    assert!(requests.try_recv().is_err());
}

#[test]
fn paged_preflight_single_write_and_full_readback_preserve_loaded_view_and_queue() {
    let (_, mut app, _events, mut requests) = fixture();
    prepare(&mut app, 205);
    let original: Vec<_> = app.tracks.iter().map(|t| t.id).collect();
    let queue_revision = app.queue.revision();
    let generation = app.generation;
    app.track_selection.rows.insert(204);
    app.open_reorder(204, Target::Top);
    let expected = app.reorder_dialog.as_ref().unwrap().expected.clone();
    for offset in [0, 100, 200] {
        if offset > 0 {
            app.reorder_tick();
        }
        let request = requests.try_recv().unwrap();
        app.reorder_tick();
        assert!(requests.try_recv().is_err());
        checked(&mut app, request, &original, offset, "revision-1");
    }
    assert!(requests.try_recv().is_err());
    app.reorder_tick();
    let Request::ReorderPlaylist {
        operation, change, ..
    } = requests.try_recv().unwrap()
    else {
        panic!("expected one write")
    };
    assert_eq!(change.from, 204);
    assert_eq!(change.ids, original);
    assert!(!app.browse_cache.ready);
    app.reorder_tick();
    assert!(requests.try_recv().is_err());
    app.reorder_result(7, operation, "fixture-list".into(), Ok("revision-2".into()));
    app.track_view.query = "fixture".into();
    for offset in [0, 100, 200] {
        app.reorder_tick();
        checked(
            &mut app,
            requests.try_recv().unwrap(),
            &expected,
            offset,
            "revision-2",
        );
    }
    assert!(app.reorder_dialog.is_none());
    assert!(requests.try_recv().is_err());
    assert_eq!(
        app.tracks.iter().map(|t| t.id).collect::<Vec<_>>(),
        expected
    );
    assert_eq!(app.tracks.len(), 205);
    assert_eq!(app.playlist_page.as_ref().unwrap().etag, "revision-2");
    assert_eq!(app.track_view.query, "fixture");
    assert!(app.browse_cache.ready);
    assert!(app.generation > generation);
    assert!(app.track_selection.rows.is_empty());
    assert_eq!(app.queue.revision(), queue_revision);
}
#[test]
fn verification_yields_to_buffering_and_cancellation_reaches_queued_work() {
    let (_, mut app, _events, mut requests) = fixture();
    prepare(&mut app, 205);
    app.open_reorder(204, Target::Top);
    let original = app.reorder_dialog.as_ref().unwrap().change.ids.clone();
    checked(
        &mut app,
        requests.try_recv().unwrap(),
        &original,
        0,
        "revision-1",
    );
    app.buffering = true;
    app.reorder_tick();
    assert!(requests.try_recv().is_err());
    app.buffering = false;
    app.reorder_tick();
    let Request::CheckReorder {
        operation,
        check,
        cancelled,
    } = requests.try_recv().unwrap()
    else {
        panic!()
    };
    app.navigate(Page::Settings);
    assert!(cancelled.load(Ordering::Acquire));
    assert!(app.reorder_dialog.is_none());
    app.reorder_checked(operation, check, Ok(original[100..200].to_vec()));
    app.reorder_tick();
    assert!(requests.try_recv().is_err());
}
#[test]
fn preflight_mismatch_or_scope_change_never_dispatches_a_write() {
    for case in 0..4 {
        let (_, mut app, _events, mut requests) = fixture();
        prepare(&mut app, 3);
        app.open_reorder(2, Target::Top);
        let Request::CheckReorder {
            operation,
            mut check,
            cancelled,
        } = requests.try_recv().unwrap()
        else {
            panic!()
        };
        match case {
            0 => check.offset = 100,
            1 => check.etag = "other".into(),
            2 => app.account = Some(8),
            _ => app.generation += 1,
        }
        app.reorder_checked(operation, check, Ok(vec![1, 2, 3]));
        app.reorder_tick();
        assert!(requests.try_recv().is_err());
        assert!(cancelled.load(Ordering::Acquire));
    }
    let (_, mut app, _events, mut requests) = fixture();
    prepare(&mut app, 3);
    app.open_reorder(2, Target::Top);
    let Request::CheckReorder {
        operation, check, ..
    } = requests.try_recv().unwrap()
    else {
        panic!()
    };
    app.reorder_checked(operation, check, Ok(vec![3, 2, 1]));
    app.reorder_tick();
    assert!(requests.try_recv().is_err());
    assert!(
        app.reorder_dialog
            .as_ref()
            .unwrap()
            .error
            .as_ref()
            .unwrap()
            .contains("not sent")
    );
}
#[test]
fn readback_failure_never_reports_success_or_retries_write() {
    let (_, mut app, _events, mut requests) = fixture();
    prepare(&mut app, 3);
    app.open_reorder(2, Target::Top);
    checked(
        &mut app,
        requests.try_recv().unwrap(),
        &[1, 2, 3],
        0,
        "revision-1",
    );
    app.reorder_tick();
    let Request::ReorderPlaylist { operation, .. } = requests.try_recv().unwrap() else {
        panic!()
    };
    app.reorder_result(7, operation, "fixture-list".into(), Ok("revision-2".into()));
    app.reorder_tick();
    let Request::CheckReorder {
        operation, check, ..
    } = requests.try_recv().unwrap()
    else {
        panic!()
    };
    app.reorder_checked(operation, check, Ok(vec![1, 2, 3]));
    app.reorder_tick();
    assert!(requests.try_recv().is_err());
    assert!(!app.browse_cache.ready);
    assert_eq!(
        app.tracks.iter().map(|t| t.id).collect::<Vec<_>>(),
        [1, 2, 3]
    );
    assert!(
        app.reorder_dialog
            .as_ref()
            .unwrap()
            .error
            .as_ref()
            .unwrap()
            .contains("may have completed")
    );
}
#[test]
fn mutation_or_refresh_stops_preflight_and_closed_worker_releases_busy_state() {
    let (_, mut app, _events, mut requests) = fixture();
    prepare(&mut app, 3);
    app.open_reorder(2, Target::Top);
    let Request::CheckReorder {
        operation,
        check,
        cancelled,
    } = requests.try_recv().unwrap()
    else {
        panic!()
    };
    app.invalidate_browse_cache();
    app.reorder_checked(operation, check, Ok(vec![1, 2, 3]));
    app.reorder_tick();
    assert!(cancelled.load(Ordering::Acquire));
    assert!(app.reorder_dialog.is_none());
    assert!(requests.try_recv().is_err());
    app.browse_loaded();
    drop(requests);
    app.open_reorder(2, Target::Top);
    assert!(!app.reorder_dialog.as_ref().unwrap().busy);
    assert!(app.reorder_dialog.as_ref().unwrap().error.is_some());
}
#[test]
fn cancellation_before_commit_or_after_queued_write_never_retries_or_reports_verified() {
    let (_, mut app, _events, mut requests) = fixture();
    prepare(&mut app, 3);
    app.open_reorder(2, Target::Top);
    checked(
        &mut app,
        requests.try_recv().unwrap(),
        &[1, 2, 3],
        0,
        "revision-1",
    );
    app.invalidate_browse_cache();
    app.reorder_tick();
    assert!(app.reorder_dialog.is_none());
    assert!(requests.try_recv().is_err());
    app.browse_loaded();
    app.open_reorder(2, Target::Top);
    checked(
        &mut app,
        requests.try_recv().unwrap(),
        &[1, 2, 3],
        0,
        "revision-1",
    );
    app.reorder_tick();
    let Request::ReorderPlaylist {
        operation,
        cancelled,
        ..
    } = requests.try_recv().unwrap()
    else {
        panic!()
    };
    app.navigate(Page::Settings);
    assert!(cancelled.load(Ordering::Acquire));
    app.reorder_result(7, operation, "fixture-list".into(), Ok("revision-2".into()));
    app.reorder_tick();
    assert!(requests.try_recv().is_err());
    assert!(app.reorder_dialog.is_none());
    assert!(app.notice.as_ref().unwrap().contains("not fully verified"));
}
#[test]
fn maximum_loaded_snapshot_can_begin_without_lifting_the_bound() {
    let (_, mut app, _events, mut requests) = fixture();
    prepare(&mut app, MAX_TRACKS);
    assert!(app.can_reorder_playlist());
    app.open_reorder(MAX_TRACKS - 1, Target::Top);
    assert!(
        matches!(requests.try_recv(),Ok(Request::CheckReorder{check,..}) if check.count==MAX_TRACKS && check.offset==0)
    );
    app.cancel_reorder();
    prepare(&mut app, MAX_TRACKS + 1);
    assert!(!app.can_reorder_playlist());
}

#[test]
fn incomplete_transformed_foreign_and_video_views_are_not_eligible() {
    for case in 0..5 {
        let (_, mut app, _events, mut requests) = fixture();
        prepare(&mut app, 205);
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
            3 => app.account = Some(8),
            _ => {
                app.playlist_page.as_mut().unwrap().rows.pop();
            }
        }
        app.open_reorder(2, Target::Top);
        assert!(app.reorder_dialog.is_none());
        assert!(requests.try_recv().is_err());
    }
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
fn position_menu_opens_for_each_row_then_one_click_starts_checks() {
    for from in 0..3 {
        let (ctx, mut app, _events, mut requests) = fixture();
        prepare(&mut app, 3);
        let _ = draw(&ctx, &mut app, vec![]);
        let output = draw(&ctx, &mut app, vec![]);
        let more = output
            .shapes
            .iter()
            .filter_map(|s| match &s.shape {
                egui::Shape::Circle(c) if c.radius == 1.25 => Some(c.center),
                _ => None,
            })
            .nth(from * 3)
            .unwrap();
        click(&ctx, &mut app, more);
        let output = draw(&ctx, &mut app, vec![]);
        click(&ctx, &mut app, label(&output, "Move to position…"));
        assert!(app.reorder_dialog.as_ref().unwrap().choosing_position);
        assert!(requests.try_recv().is_err());
        app.reorder_dialog.as_mut().unwrap().position = if from == 0 { "3" } else { "1" }.into();
        let _ = draw(&ctx, &mut app, vec![]);
        let output = draw(&ctx, &mut app, vec![]);
        click(&ctx, &mut app, label(&output, "Move to position"));
        assert!(matches!(
            requests.try_recv(),
            Ok(Request::CheckReorder { .. })
        ));
        assert!(requests.try_recv().is_err());
    }
}

#[test]
fn menu_actions_begin_checks_immediately_without_confirmation_or_double_dispatch() {
    for (action, target, from) in [
        ("Move to playlist bottom", Target::Bottom, 0),
        ("Move down in playlist", Target::Down, 0),
        ("Move up in playlist", Target::Up, 1),
        ("Move to playlist top", Target::Top, 1),
    ] {
        let (ctx, mut app, _events, mut requests) = fixture();
        prepare(&mut app, 3);
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
            |shape| matches!(&shape.shape,egui::Shape::Text(t) if t.galley.job.text=="Move track")
        ));
        assert!(
            matches!(requests.try_recv(),Ok(Request::CheckReorder{check,..}) if check.offset==0)
        );
        let dialog = app.reorder_dialog.as_ref().unwrap();
        assert!(dialog.busy);
        assert_eq!(dialog.change.from, from);
        assert_eq!(dialog.change.target, target);
        app.reorder_tick();
        assert!(requests.try_recv().is_err());
        assert_eq!(app.queue.revision(), revision);
    }
}
