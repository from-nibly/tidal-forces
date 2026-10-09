use super::super::tests::{fixture, track};
use super::*;
fn playlist() -> Playlist {
    Playlist {
        uuid: "test-list".into(),
        title: "Export".into(),
        ..Playlist::default()
    }
}
fn prepare_paste(app: &mut App, count: u64) {
    app.page = Page::Collection {
        kind: "playlists".into(),
        id: "test-list".into(),
        title: "Destination".into(),
    };
    app.playlist_page = Some(PlaylistPage {
        playlist: Playlist {
            number_of_tracks: count,
            ..playlist()
        },
        etag: "initial-revision".into(),
        editable: true,
        rows: Vec::new(),
        next_offset: 0,
        more: count > 0,
    });
}
fn paste(ctx: &egui::Context, app: &mut App, text: &str) {
    let _ = ctx.run(
        egui::RawInput {
            events: vec![egui::Event::Paste(text.into())],
            ..Default::default()
        },
        |ctx| app.handle_track_paste(ctx),
    );
}
fn checked(app: &mut App, requests: &mut tokio::sync::mpsc::Receiver<Request>, unique: Vec<u64>) {
    loop {
        app.export_tick();
        let Request::CheckPlaylistDuplicates {
            user,
            operation,
            id,
            count,
            offset,
            etag,
            ..
        } = requests.try_recv().unwrap()
        else {
            panic!("Only a duplicate check is allowed before confirmation");
        };
        assert_eq!(id, "test-list");
        assert_eq!(count, app.export.transfer.as_ref().unwrap().base_offset);
        assert_eq!(etag, "initial-revision");
        app.playlist_duplicates_checked(
            user,
            operation,
            offset,
            Ok(vec![1; (count - offset).min(100)]),
        );
        if app.export.transfer.as_ref().unwrap().unique_ids.is_some() {
            break;
        }
    }
    assert_eq!(app.export.transfer.as_ref().unwrap().ids, unique);
    app.export_tick();
    assert!(requests.try_recv().is_err());
}
#[test]
fn duplicate_check_deduplicates_copied_songs_for_an_empty_playlist() {
    let (ctx, mut app, _events, mut requests) = fixture();
    prepare_paste(&mut app, 0);
    paste(
        &ctx,
        &mut app,
        "tidal://track/9 tidal://track/10 tidal://track/9",
    );
    checked(&mut app, &mut requests, vec![9, 10]);
}
#[test]
fn duplicate_checks_cover_unloaded_pages_and_preserve_new_song_order() {
    let (ctx, mut app, _events, mut requests) = fixture();
    prepare_paste(&mut app, 3);
    paste(
        &ctx,
        &mut app,
        "tidal://track/12 tidal://track/9 tidal://track/12 tidal://track/11 tidal://track/13 tidal://track/13 tidal://track/10",
    );
    app.export_tick();
    let Request::CheckPlaylistDuplicates {
        operation,
        offset: 0,
        ..
    } = requests.try_recv().unwrap()
    else {
        panic!()
    };
    assert!(!app.export.transfer.as_ref().unwrap().started);
    app.playlist_duplicates_checked(7, operation, 0, Ok(vec![9, 10]));
    assert!(app.export.transfer.as_ref().unwrap().unique_ids.is_none());
    app.export_tick();
    assert!(matches!(
        requests.try_recv(),
        Ok(Request::CheckPlaylistDuplicates { offset: 2, .. })
    ));
    app.playlist_duplicates_checked(7, operation, 0, Ok(vec![99])); // Late page cannot advance a newer read.
    assert_eq!(app.export.transfer.as_ref().unwrap().checked_offset, 2);
    app.playlist_duplicates_checked(7, operation, 2, Ok(vec![11]));
    let transfer = app.export.transfer.as_ref().unwrap();
    assert_eq!(transfer.ids, [12, 13]);
    assert!(!transfer.include_duplicates);
    app.export_tick();
    assert!(requests.try_recv().is_err());
    app.export.transfer.as_mut().unwrap().started = true;
    app.export_tick();
    assert!(
        matches!(requests.try_recv(),Ok(Request::AppendQueueBatch {tracks,offset:3,..}) if tracks==vec![12,13])
    );
}
#[test]
fn duplicate_checks_stop_on_failure_cancel_account_changes_or_all_existing_songs() {
    for case in ["failure", "cancel", "account", "existing"] {
        let (ctx, mut app, _events, mut requests) = fixture();
        prepare_paste(&mut app, 1);
        paste(&ctx, &mut app, "tidal://track/9 tidal://track/9");
        app.export_tick();
        let Request::CheckPlaylistDuplicates { operation, .. } = requests.try_recv().unwrap()
        else {
            panic!()
        };
        if case == "cancel" {
            app.cancel_export(true);
        }
        if case == "account" {
            app.account = Some(8);
        }
        app.playlist_duplicates_checked(
            7,
            operation,
            0,
            if case == "failure" {
                Err("Playlist changed".into())
            } else {
                Ok(vec![9])
            },
        );
        app.export_tick();
        assert!(requests.try_recv().is_err());
        if case == "existing" {
            let transfer = app.export.transfer.as_ref().unwrap();
            assert_eq!(transfer.unique_ids, Some(vec![]));
            assert!(transfer.ids.is_empty());
            assert!(!transfer.started);
        }
        if case == "failure" {
            assert!(app.export.transfer.as_ref().unwrap().error.is_some());
        }
    }
}
#[test]
fn playlist_paste_requires_confirmation_and_appends_batches_after_existing_tracks() {
    let (ctx, mut app, _events, mut requests) = fixture();
    prepare_paste(&mut app, 7);
    let links = "tidal://track/9\n".repeat(205);
    paste(&ctx, &mut app, &links);
    checked(&mut app, &mut requests, vec![9]);
    let transfer = app.export.transfer.as_mut().unwrap();
    assert_eq!(transfer.ids, vec![9]);
    transfer.include_duplicates = true;
    transfer.ids = transfer.original_ids.clone();
    let transfer = app.export.transfer.as_mut().unwrap();
    assert!(transfer.paste);
    assert_eq!(transfer.base_offset, 7);
    assert_eq!(transfer.ids, vec![9; 205]);
    transfer.started = true;
    let operation = transfer.operation;
    for (confirmed, offset) in [(0, 7), (100, 107), (200, 207)] {
        app.export_tick();
        match requests.try_recv().unwrap() {
            Request::AppendQueueBatch {
                id,
                tracks,
                offset: actual,
                etag,
                ..
            } => {
                assert_eq!(id, "test-list");
                assert_eq!(actual, offset);
                assert_eq!(tracks, vec![9; (205 - confirmed).min(100)]);
                assert_eq!(
                    etag.as_deref(),
                    Some(if confirmed == 0 {
                        "initial-revision"
                    } else {
                        "next-revision"
                    })
                );
            }
            _ => panic!("Paste must append, never create a playlist"),
        }
        app.export_saved(7, operation, offset, Ok("next-revision".into()));
    }
    app.export_tick();
    let mut refreshed = false;
    while let Ok(request) = requests.try_recv() {
        match request {
            Request::Playlist { id, offset: 0, .. } => {
                assert_eq!(id, "test-list");
                refreshed = true;
            }
            Request::Folder { .. } => {}
            _ => panic!("Only read-only refresh is allowed after completion"),
        }
    }
    assert!(refreshed);
    assert_eq!(app.export.transfer.as_ref().unwrap().confirmed, 205);
    assert!(app.queue.current().is_none());
}
#[test]
fn stale_clipboard_requests_cannot_rebind_to_another_account_and_read_only_targets_refuse_paste() {
    let (ctx, mut app, _events, mut requests) = fixture();
    prepare_paste(&mut app, 7);
    app.request_track_paste(&ctx);
    app.cancel_export(true);
    app.account = Some(8);
    paste(&ctx, &mut app, "tidal://track/9");
    assert!(app.export.transfer.is_none());
    assert!(app.error.as_ref().unwrap().contains("Playlist changed"));
    app.playlist_page.as_mut().unwrap().editable = false;
    paste(&ctx, &mut app, "tidal://track/9");
    assert!(app.export.transfer.is_none());
    app.playlist_page.as_mut().unwrap().editable = true;
    app.playlist_page
        .as_mut()
        .unwrap()
        .playlist
        .number_of_videos = 1;
    paste(&ctx, &mut app, "tidal://track/9");
    assert!(app.export.transfer.is_none());
    assert!(requests.try_recv().is_err());
}

#[test]
fn paste_confirmation_button_is_the_first_write_and_fits_narrow_windows() {
    for include in [false, true] {
        let (ctx, mut app, _events, mut requests) = fixture();
        prepare_paste(&mut app, 7);
        paste(&ctx, &mut app, "tidal://track/9 tidal://track/9");
        checked(&mut app, &mut requests, vec![9]);
        let draw = |app: &mut App, events| {
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        vec2(666., 440.),
                    )),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    app.export_window(ctx);
                    app.export_tick();
                },
            )
        };
        draw(&mut app, Vec::new());
        let mut output = draw(&mut app, Vec::new());
        assert!(requests.try_recv().is_err());
        assert!(ctx.used_rect().right() <= 667.);
        if include {
            let pos = output
                .shapes
                .iter()
                .find_map(|shape| match &shape.shape {
                    egui::Shape::Text(text) if text.galley.job.text == "Add duplicates too" => {
                        Some(text.pos + vec2(4., 4.))
                    }
                    _ => None,
                })
                .unwrap();
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
            output = draw(&mut app, Vec::new());
        }
        assert_eq!(
            app.export.transfer.as_ref().unwrap().include_duplicates,
            include
        );
        assert!(!output.shapes.iter().any(|shape| matches!(&shape.shape,egui::Shape::Text(text) if text.galley.job.text.contains("Batches") || text.galley.job.text.contains("revision"))));
        let pos = output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Text(text) if text.galley.job.text == "Add songs" => {
                    Some(text.pos + vec2(4., 4.))
                }
                _ => None,
            })
            .unwrap();
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
        assert!(
            matches!(requests.try_recv(),Ok(Request::AppendQueueBatch {offset:7,tracks,..}) if tracks==if include {vec![9,9]} else {vec![9]})
        );
        assert!(requests.try_recv().is_err());
    }
}

#[test]
fn playlist_paste_respects_text_edits_and_rejects_changed_clipboard_revisions() {
    let (ctx, mut app, _events, mut requests) = fixture();
    prepare_paste(&mut app, 7);
    let mut text = String::new();
    let draw = |app: &mut App, text: &mut String, events| {
        ctx.run(
            egui::RawInput {
                events,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let response = ui.text_edit_singleline(text);
                    response.request_focus();
                });
                app.handle_track_paste(ctx);
            },
        )
    };
    draw(&mut app, &mut text, Vec::new());
    draw(
        &mut app,
        &mut text,
        vec![egui::Event::Paste("tidal://track/9".into())],
    );
    assert_eq!(text, "tidal://track/9");
    assert!(app.export.transfer.is_none());
    app.request_track_paste(&ctx); // Explicit button intent releases the old text focus.
    app.playlist_page.as_mut().unwrap().etag = "changed-revision".into();
    paste(&ctx, &mut app, "tidal://track/9");
    assert!(app.export.transfer.is_none());
    assert!(app.error.as_ref().unwrap().contains("Playlist changed"));
    app.request_track_paste(&ctx);
    app.generation += 1;
    paste(&ctx, &mut app, "tidal://track/9");
    assert!(app.export.transfer.is_none());
    assert!(requests.try_recv().is_err());
}

#[test]
fn pasted_batches_stop_after_failure_or_cancellation_without_losing_the_destination() {
    for cancel in [false, true] {
        let (ctx, mut app, _events, mut requests) = fixture();
        prepare_paste(&mut app, 7);
        paste(&ctx, &mut app, &"tidal://track/9\n".repeat(205));
        checked(&mut app, &mut requests, vec![9]);
        let transfer = app.export.transfer.as_mut().unwrap();
        transfer.include_duplicates = true;
        transfer.ids = transfer.original_ids.clone();
        transfer.started = true;
        let operation = transfer.operation;
        app.export_tick();
        assert!(matches!(
            requests.try_recv(),
            Ok(Request::AppendQueueBatch { offset: 7, .. })
        ));
        if cancel {
            app.cancel_export(false);
            app.export_saved(7, operation, 7, Ok("next-revision".into()));
        } else {
            app.export_saved(7, operation, 7, Err("Uncertain write".into()));
        }
        app.export_tick();
        assert!(requests.try_recv().is_err());
        let transfer = app.export.transfer.as_ref().unwrap();
        assert_eq!(transfer.playlist.as_ref().unwrap().uuid, "test-list");
        assert_eq!(transfer.confirmed, if cancel { 100 } else { 0 });
        assert!(!transfer.busy);
    }
}

#[test]
fn paste_ignores_other_dialogs_loading_and_missing_revisions_and_bounds_the_destination() {
    for state in ["dialog", "loading", "revision", "page"] {
        let (ctx, mut app, _events, mut requests) = fixture();
        prepare_paste(&mut app, 7);
        match state {
            "dialog" => app.playlist_dialog = true,
            "loading" => app.loading = true,
            "revision" => app.playlist_page.as_mut().unwrap().etag.clear(),
            _ => app.page = Page::Home,
        }
        paste(&ctx, &mut app, "tidal://track/9");
        assert!(app.export.transfer.is_none(), "{state}");
        assert!(requests.try_recv().is_err());
    }
    let (ctx, mut app, _events, mut requests) = fixture();
    prepare_paste(&mut app, 49_999);
    paste(&ctx, &mut app, "tidal://track/9 tidal://track/9");
    checked(&mut app, &mut requests, vec![9]);
    let transfer = app.export.transfer.as_mut().unwrap();
    assert_eq!(transfer.ids, vec![9]);
    transfer.include_duplicates = true;
    transfer.ids = transfer.original_ids.clone();
    transfer.started = true;
    app.export_tick();
    assert!(requests.try_recv().is_err());
    let (ctx, mut app, _events, mut requests) = fixture();
    prepare_paste(&mut app, 7);
    app.credentials.dirty = true;
    let mut input = egui::RawInput::default();
    input
        .viewports
        .get_mut(&egui::ViewportId::ROOT)
        .unwrap()
        .events
        .push(egui::ViewportEvent::Close);
    let _ = ctx.run(input, |ctx| app.credential_close_guard(ctx));
    assert!(app.credential_close_pending());
    paste(&ctx, &mut app, "tidal://track/9");
    assert!(app.export.transfer.is_none());
    assert!(requests.try_recv().is_err());
}

#[test]
fn export_requires_confirmation_and_sends_bounded_snapshot_batches_beyond_hundred() {
    let (_ctx, mut app, _events, mut requests) = fixture();
    app.queue
        .start((1..=205).map(track).collect(), 0, "Test".into(), None)
        .unwrap();
    app.open_queue_export();
    app.export_tick();
    assert!(requests.try_recv().is_err());
    app.export.transfer.as_mut().unwrap().started = true;
    app.export_tick();
    let operation = match requests.try_recv().unwrap() {
        Request::CreateQueuePlaylist {
            user: 7, operation, ..
        } => operation,
        _ => panic!("Expected create"),
    };
    app.export_created(7, operation, Ok(playlist()));
    while let Ok(request) = requests.try_recv() {
        assert!(matches!(request, Request::Folder { .. }));
    }
    app.queue.clear(); // The confirmed snapshot, not the subsequently edited queue, is exported.
    for offset in [0, 100, 200] {
        app.export_tick();
        app.export_tick();
        match requests.try_recv().unwrap() {
            Request::AppendQueueBatch {
                tracks,
                offset: actual,
                etag,
                ..
            } => {
                assert_eq!(actual, offset);
                assert_eq!(
                    tracks,
                    (offset as u64 + 1..=(offset + 100).min(205) as u64).collect::<Vec<_>>()
                );
                assert_eq!(etag.is_some(), offset != 0);
            }
            _ => panic!("Expected batch"),
        }
        assert!(requests.try_recv().is_err());
        app.export_saved(7, operation, offset, Ok(format!("revision-{offset}")));
    }
    assert_eq!(app.export.transfer.as_ref().unwrap().confirmed, 205);
    app.export_tick();
    assert!(requests.try_recv().is_err());
}
#[test]
fn cancelled_failed_or_stale_exports_cannot_send_more_batches_or_change_new_account() {
    let (_ctx, mut app, _events, mut requests) = fixture();
    app.queue.add(track(9), false).unwrap();
    app.open_queue_export();
    app.export.transfer.as_mut().unwrap().started = true;
    app.export_tick();
    let (operation, cancelled) = match requests.try_recv().unwrap() {
        Request::CreateQueuePlaylist {
            operation,
            cancelled,
            ..
        } => (operation, cancelled),
        _ => panic!(),
    };
    app.cancel_export(false);
    assert!(cancelled.load(Ordering::Acquire));
    app.export_created(7, operation, Ok(playlist()));
    while requests.try_recv().is_ok() {}
    app.export_tick();
    assert!(requests.try_recv().is_err());
    app.cancel_export(true);
    app.account = Some(8);
    app.export_created(7, operation, Ok(playlist()));
    assert!(app.export.transfer.is_none());
    assert!(requests.try_recv().is_err());
}
#[test]
fn failed_batch_has_no_automatic_retry_and_retains_destination_for_inspection() {
    let (_ctx, mut app, _events, mut requests) = fixture();
    app.queue.add(track(9), false).unwrap();
    app.open_queue_export();
    let transfer = app.export.transfer.as_mut().unwrap();
    transfer.started = true;
    transfer.playlist = Some(playlist());
    let operation = transfer.operation;
    app.export_tick();
    assert!(matches!(
        requests.try_recv(),
        Ok(Request::AppendQueueBatch { .. })
    ));
    app.export_saved(7, operation, 0, Err("Uncertain write".into()));
    app.export_tick();
    assert!(requests.try_recv().is_err());
    let transfer = app.export.transfer.as_ref().unwrap();
    assert!(transfer.error.is_some() && transfer.playlist.is_some() && !transfer.busy);
    assert_eq!(transfer.confirmed, 0);
}
#[test]
fn export_confirmation_fits_minimum_and_high_zoom_viewports_without_sending_writes() {
    let (ctx, mut app, _events, mut requests) = fixture();
    app.queue.add(track(9), false).unwrap();
    app.open_queue_export();
    for size in [vec2(1240., 820.), vec2(1000., 660.), vec2(666., 440.)] {
        let (output, bounds) = super::super::tests::frame(&ctx, &mut app, size);
        assert!(bounds.right() <= size.x + 1., "Overflow: {bounds:?}");
        assert!(output.shapes.len() < 1500);
        assert!(requests.try_recv().is_err());
    }
}

#[test]
fn snapshot_preserves_duplicates_and_refuses_unloaded_context_or_missing_worker() {
    let (_ctx, mut app, _events, requests) = fixture();
    app.queue
        .start(
            vec![track(9), track(10)],
            0,
            "Test".into(),
            Some(Continuation::Album { id: 3, offset: 2 }),
        )
        .unwrap();
    app.open_queue_export();
    assert!(app.export.transfer.is_none());
    app.queue
        .start(vec![track(9), track(10)], 0, "Test".into(), None)
        .unwrap();
    app.queue.add(track(9), false).unwrap();
    app.open_queue_export();
    assert_eq!(app.export.transfer.as_ref().unwrap().ids, [9, 9, 10]);
    drop(requests);
    app.export.transfer.as_mut().unwrap().started = true;
    app.export_tick();
    assert!(app.export.transfer.as_ref().unwrap().error.is_some());
    assert!(!app.export.transfer.as_ref().unwrap().busy);
}
