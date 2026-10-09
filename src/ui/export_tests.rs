use super::super::tests::{fixture, track};
use super::*;
fn playlist() -> Playlist {
    Playlist {
        uuid: "test-list".into(),
        title: "Export".into(),
        ..Playlist::default()
    }
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
