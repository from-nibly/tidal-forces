use super::*;
use crate::cast_control::Control;
fn fixture() -> (
    App,
    Control,
    mpsc::Sender<crate::backend::Event>,
    tokio::sync::mpsc::Receiver<Request>,
) {
    let (_, mut app, events, requests) = super::super::tests::fixture();
    app.queue_mut()
        .start(
            vec![
                super::super::tests::track(1),
                super::super::tests::track(1),
                super::super::tests::track(2),
            ],
            0,
            "Synthetic".into(),
            None,
        )
        .unwrap();
    let scope = Scope {
        user: 7,
        generation: app.play_generation,
        occurrence: app.queue.current_entry().unwrap().occurrence,
    };
    let (handle, control) = crate::cast_control::channel();
    app.cast.enabled = true;
    app.cast.worker = Some(Running { scope, handle });
    app.paused = false;
    (app, control, events, requests)
}
#[test]
fn icon_states_require_confirmed_session_and_keep_uncertainty_visible() {
    let (mut app, control, _, _) = fixture();
    assert_eq!(app.cast_state(), ConnectionState::Connecting);
    control
        .emit(CastEvent::Phase(crate::cast_control::Phase::Monitoring))
        .unwrap();
    app.events();
    assert_eq!(app.cast_state(), ConnectionState::Connected);
    app.cast.returning = true;
    assert_eq!(app.cast_state(), ConnectionState::Releasing);
    app.cast.unknown = true;
    assert_eq!(app.cast_state(), ConnectionState::Unconfirmed);
    app.cast.unknown = false;
    app.cast.returning = false;
    control.close(Ok(End::Stopped), true);
    app.events();
    assert_eq!(app.cast_state(), ConnectionState::Selected);
    app.cast_return_local();
    assert_eq!(app.cast_state(), ConnectionState::Local);
    assert!(app.paused);
}
fn picker_frame(
    ctx: &egui::Context,
    app: &mut App,
    events: Vec<egui::Event>,
) -> (egui::FullOutput, egui::Id) {
    let mut id = egui::Id::NULL;
    let output = ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(380., 300.),
            )),
            events,
            ..Default::default()
        },
        |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                id = app.cast_button(ui, false).id;
            });
            app.cast_picker(ctx);
        },
    );
    (output, id)
}
#[test]
fn picker_opens_from_player_without_network_io_or_technical_fields_and_escape_closes() {
    let (ctx, mut app, _, mut requests) = super::super::tests::fixture();
    assert!(app.cast.discovery_disabled);
    app.cast.devices = vec![crate::cast_discovery::Device {
        id: "synthetic".into(),
        name: "Living room".into(),
        model: "Chromecast".into(),
        endpoint: "192.168.99.1:8009".parse().unwrap(),
    }];
    let (_, id) = picker_frame(&ctx, &mut app, vec![]);
    let pos = ctx.read_response(id).unwrap().rect.center();
    for pressed in [true, false] {
        picker_frame(
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
    assert_eq!(app.cast.picker, Some(egui::ViewportId::ROOT));
    let (output, _) = picker_frame(&ctx, &mut app, vec![]);
    let text = output
        .shapes
        .iter()
        .filter_map(|s| {
            if let egui::Shape::Text(t) = &s.shape {
                Some(t.galley.job.text.as_str())
            } else {
                None
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    for label in ["Playback devices", "This computer", "Living room"] {
        assert!(text.contains(label), "Missing picker label: {label}");
    }
    for technical in ["192.168.", "certificate", "IPv4", "legacy"] {
        assert!(!text.contains(technical));
    }
    assert!(app.cast.discovery.is_none() && requests.try_recv().is_err());
    picker_frame(
        &ctx,
        &mut app,
        vec![egui::Event::Key {
            key: egui::Key::Escape,
            physical_key: Some(egui::Key::Escape),
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }],
    );
    assert!(app.cast.picker.is_none());
}
#[test]
fn picker_owner_is_viewport_scoped_and_closing_mini_does_not_close_main_picker() {
    let (ctx, mut app, _, _) = super::super::tests::fixture();
    app.cast.picker = Some(egui::ViewportId::ROOT);
    app.cast.picker_mini = false;
    app.cast_mini_closed();
    assert!(app.cast_picker_open(&ctx));
    app.cast.picker = Some(super::super::mini::viewport_id());
    app.cast.picker_mini = true;
    assert!(!app.cast_picker_open(&ctx));
    app.cast_mini_closed();
    assert!(app.cast.picker.is_none());
}
#[test]
fn return_to_this_computer_preserves_position_and_waits_for_owned_stop() {
    let (mut app, control, _, mut requests) = fixture();
    let position = Duration::from_millis(12_345);
    app.position_clock
        .target(app.play_generation, position, Instant::now());
    app.position = 12;
    app.paused = true;
    app.cast_return_local();
    assert!(control.stopped() && app.cast.enabled && app.paused);
    assert_eq!(app.position_time(), position);
    control.close(Ok(End::Stopped), true);
    app.events();
    assert!(!app.cast.enabled && app.paused);
    assert_eq!(app.position_time(), position);
    assert!(requests.try_recv().is_err());
}
#[test]
fn metadata_tracks_current_queue_occurrence_and_omits_missing_artwork() {
    let (_, mut app, _, _) = super::super::tests::fixture();
    let mut first = super::super::tests::track(1);
    first.title = "First title".into();
    first.artist.name = "First artist".into();
    first.album.title = "First album".into();
    first.album.cover = Some("12345678-1234-1234-1234-123456789abc".into());
    let mut second = first.clone();
    second.title = "Second title".into();
    second.album.cover = None;
    app.queue_mut()
        .start(vec![first, second], 0, "Synthetic".into(), None)
        .unwrap();
    let metadata = app.cast_track_metadata().receiver_json();
    assert_eq!(metadata["title"], "First title");
    assert_eq!(metadata["artist"], "First artist");
    assert_eq!(metadata["albumName"], "First album");
    assert_eq!(
        metadata["images"][0]["url"],
        "https://resources.tidal.com/images/12345678/1234/1234/1234/123456789abc/640x640.jpg"
    );
    app.queue_mut().advance(false);
    let metadata = app.cast_track_metadata().receiver_json();
    assert_eq!(metadata["title"], "Second title");
    assert!(metadata.get("images").is_none());
}
fn prepare_after_release(
    app: &mut App,
    requests: &mut tokio::sync::mpsc::Receiver<Request>,
) -> (u64, Control) {
    let quiet = app
        .cast
        .quiet
        .as_mut()
        .expect("Next LOAD must wait for local release acknowledgement");
    let (tx, rx) = mpsc::channel();
    tx.send(true).unwrap();
    quiet.reply = rx;
    app.cast_tick();
    let scope = app.cast.preparing.take().unwrap();
    assert!(scope.matches(app));
    let id = match requests.try_recv().unwrap() {
        Request::CastPrepare {
            user,
            generation,
            id,
            position,
            autoplay,
            quality,
        } => {
            assert_eq!(user, 7);
            assert_eq!(generation, app.play_generation);
            assert_eq!(position, Duration::ZERO);
            assert!(autoplay);
            assert_eq!(quality, app.quality);
            id
        }
        _ => panic!("Cast continuation must not request local decoding"),
    };
    let (handle, control) = crate::cast_control::channel();
    app.cast.worker = Some(Running { scope, handle });
    (id, control)
}
#[test]
fn repeat_one_reauthorizes_same_occurrence_once_with_a_fresh_generation() {
    let (mut app, control, _, mut requests) = fixture();
    app.queue_mut().set_repeat(crate::queue::Repeat::Track);
    let occurrence = app.queue.current_entry().unwrap().occurrence;
    let generation = app.play_generation;
    control.close(Ok(End::Finished), true);
    app.events();
    assert_eq!(app.queue.current_entry().unwrap().occurrence, occurrence);
    assert_ne!(app.play_generation, generation);
    let (id, next) = prepare_after_release(&mut app, &mut requests);
    assert_eq!(id, 1);
    control.close(Ok(End::Finished), true);
    app.events();
    assert_eq!(app.queue.current_entry().unwrap().occurrence, occurrence);
    assert!(!next.stopped());
}
#[test]
fn shuffle_and_manual_entries_use_the_shared_queue_successor() {
    let (mut app, control, _, mut requests) = fixture();
    app.queue_mut().toggle_shuffle();
    let next = app.queue.next_natural().unwrap().occurrence;
    control.close(Ok(End::Finished), true);
    app.events();
    assert_eq!(app.queue.current_entry().unwrap().occurrence, next);
    let (_, control) = prepare_after_release(&mut app, &mut requests);
    app.queue_mut()
        .add(super::super::tests::track(99), true)
        .unwrap();
    control.close(Ok(End::Finished), true);
    app.events();
    assert_eq!(prepare_after_release(&mut app, &mut requests).0, 99);
}
#[test]
fn repeat_context_cycles_with_fresh_occurrences() {
    let (mut app, mut control, _, mut requests) = fixture();
    app.queue_mut().set_repeat(crate::queue::Repeat::Context);
    let mut seen = std::collections::HashSet::new();
    for _ in 0..4 {
        assert!(seen.insert(app.queue.current_entry().unwrap().occurrence));
        control.close(Ok(End::Finished), true);
        drop(control);
        app.events();
        let (_, next) = prepare_after_release(&mut app, &mut requests);
        control = next;
    }
    assert!(!app.paused && app.cast.enabled);
}
#[test]
fn next_then_previous_cancel_only_owned_load_and_keep_occurrence_history() {
    let (mut app, control, _, mut requests) = fixture();
    let first = app.queue.current_entry().unwrap().occurrence;
    app.next();
    assert!(control.stopped());
    assert!(requests.try_recv().is_err());
    assert!(app.cast.pending.is_some());
    control.close(Ok(End::Stopped), true);
    app.events();
    let (_, control) = prepare_after_release(&mut app, &mut requests);
    app.previous();
    assert!(control.stopped());
    assert_eq!(app.queue.current_entry().unwrap().occurrence, first);
    control.close(Ok(End::Stopped), true);
    app.events();
    assert_eq!(prepare_after_release(&mut app, &mut requests).0, 1);
}
#[test]
fn rapid_next_past_queue_end_cannot_restart_a_pending_track() {
    let (mut app, control, _, mut requests) = fixture();
    app.next();
    app.next();
    assert!(app.cast.pending.is_some());
    app.next();
    assert!(app.cast.pending.is_none());
    assert!(app.paused && app.ended);
    control.close(Ok(End::Stopped), true);
    app.events();
    assert!(app.cast.worker.is_none() && app.cast.quiet.is_none());
    assert!(requests.try_recv().is_err());
}
#[test]
fn pause_racing_finish_selects_next_without_retaining_old_position() {
    let (mut app, control, _, mut requests) = fixture();
    app.queue_mut()
        .append(
            vec![],
            Some(crate::queue::Continuation::Playlist {
                id: "synthetic".into(),
                etag: "revision".into(),
                offset: 3,
            }),
        )
        .unwrap();
    app.paused = true;
    app.position = 170;
    app.position_clock.target(
        app.play_generation,
        Duration::from_secs(170),
        Instant::now(),
    );
    control.close(Ok(End::Finished), true);
    app.events();
    assert!(app.paused && app.cast.quiet.is_none());
    assert!(
        !app.waiting_context,
        "A loaded next occurrence must not be mistaken for missing pagination"
    );
    assert_eq!(app.position_time(), Duration::ZERO);
    let occurrence = app.queue.current_entry().unwrap().occurrence;
    app.cast_toggle();
    assert_eq!(app.queue.current_entry().unwrap().occurrence, occurrence);
    assert_eq!(prepare_after_release(&mut app, &mut requests).0, 1);
}
#[test]
fn stop_errors_and_stale_account_completion_cannot_advance_or_play_locally() {
    for result in [
        Ok(End::Stopped),
        Err("Synthetic cancellation"),
        Err("Synthetic disconnect"),
    ] {
        let (mut app, control, _, mut requests) = fixture();
        let occurrence = app.queue.current_entry().unwrap().occurrence;
        control.close(result, true);
        app.events();
        assert_eq!(app.queue.current_entry().unwrap().occurrence, occurrence);
        assert!(app.paused);
        assert!(requests.try_recv().is_err());
    }
    let (mut app, control, _, mut requests) = fixture();
    let occurrence = app.queue.current_entry().unwrap().occurrence;
    app.account = Some(8);
    control.close(Ok(End::Finished), true);
    app.events();
    assert_eq!(app.queue.current_entry().unwrap().occurrence, occurrence);
    assert!(app.paused);
    assert!(requests.try_recv().is_err());
}
#[test]
fn remote_queue_runs_through_all_occurrences_and_stops_at_end() {
    let (mut app, mut control, _, _) = fixture();
    let mut occurrences = std::collections::HashSet::new();
    for expected in [1, 1, 2] {
        let entry = app.queue.current_entry().unwrap();
        assert_eq!(entry.track.id, expected);
        assert!(occurrences.insert(entry.occurrence));
        control.close(Ok(End::Finished), true);
        drop(control);
        app.events();
        if expected == 2 {
            break;
        }
        assert!(app.cast.enabled && !app.paused && app.cast.quiet.is_some());
        app.cast.quiet = None;
        let (handle, next) = crate::cast_control::channel();
        app.cast.worker = Some(Running {
            scope: Scope {
                user: 7,
                generation: app.play_generation,
                occurrence: app.queue.current_entry().unwrap().occurrence,
            },
            handle,
        });
        control = next;
    }
    assert!(app.cast.enabled && app.paused && app.ended);
    assert!(app.cast.worker.is_none() && app.cast.quiet.is_none());
}
#[test]
fn changing_receiver_waits_for_confirmed_release() {
    let (mut app, control, _, _) = fixture();
    app.cast.endpoint = Some("192.168.1.2:8009".parse().unwrap());
    app.cast_select(crate::cast_discovery::Device {
        id: "synthetic".into(),
        name: "Other room".into(),
        model: "Chromecast".into(),
        endpoint: "192.168.1.3:8009".parse().unwrap(),
    });
    assert!(control.stopped());
    assert_eq!(app.cast.endpoint, Some("192.168.1.2:8009".parse().unwrap()));
    assert!(app.cast.switching.is_some());
    control.close(Err("Synthetic uncertain stop"), false);
    app.events();
    assert!(app.cast.switching.is_none() && app.cast.unknown);
    assert_eq!(app.cast.endpoint, Some("192.168.1.2:8009".parse().unwrap()));
}
#[test]
fn successful_worker_exit_does_not_invent_unknown_cleanup() {
    let (mut app, control, _, _) = fixture();
    control.close(Ok(End::Stopped), true);
    drop(control);
    app.events();
    assert!(!app.cast.unknown);
    assert!(app.cast.worker.is_none());
    app.events();
    assert!(!app.cast.unknown);
}
#[test]
fn pending_volume_steps_accumulate_without_claiming_confirmation() {
    let (mut app, control, _, _) = fixture();
    app.volume = 0.5;
    app.playback_volume(app.volume_target() + 0.1);
    app.playback_volume(app.volume_target() + 0.1);
    assert!((app.volume_target() - 0.7).abs() < 0.001);
    assert_eq!(app.volume, 0.5);
    control.emit(CastEvent::Volume(0.6)).unwrap();
    app.events();
    assert!((app.volume_target() - 0.7).abs() < 0.001);
    assert!((app.volume - 0.6).abs() < 0.001);
    control.close(Err("Synthetic volume rejection"), true);
    drop(control);
    app.events();
    assert!(app.cast.volume_intent.is_none());
    assert!((app.volume_target() - 0.6).abs() < 0.001);
    assert!(!app.cast.unknown);
}
#[test]
fn local_audio_events_cannot_move_or_stop_remote_playback() {
    let (mut app, _control, events, _) = fixture();
    let generation = app.play_generation;
    let occurrence = app.queue.current_entry().unwrap().occurrence;
    events
        .send(crate::backend::Event::Position {
            generation,
            position: Duration::from_secs(99),
        })
        .unwrap();
    events
        .send(crate::backend::Event::Ended(generation))
        .unwrap();
    events
        .send(crate::backend::Event::PlaybackError {
            generation,
            message: "Synthetic stale error".into(),
        })
        .unwrap();
    app.events();
    assert_eq!(app.position, 0);
    assert_eq!(app.queue.current_entry().unwrap().occurrence, occurrence);
    assert!(!app.paused);
    assert!(app.error.is_none());
}
#[test]
fn confirmed_remote_finish_advances_duplicate_occurrence_once() {
    let (mut app, control, _, mut requests) = fixture();
    let occurrence = app.queue.current_entry().unwrap().occurrence;
    control.close(Ok(End::Finished), true);
    app.events();
    assert_eq!(app.queue.current().unwrap().id, 1);
    assert_ne!(app.queue.current_entry().unwrap().occurrence, occurrence);
    let next = app.queue.current_entry().unwrap().occurrence;
    control.close(Ok(End::Finished), true);
    app.events();
    assert_eq!(app.queue.current_entry().unwrap().occurrence, next);
    assert!(
        requests.try_recv().is_err(),
        "Missing local release ACK must not start a new source"
    );
}
#[test]
fn unconfirmed_release_blocks_replacement_and_never_advances() {
    let (mut app, control, _, mut requests) = fixture();
    let occurrence = app.queue.current_entry().unwrap().occurrence;
    control.close(Err("Synthetic disconnect"), false);
    app.events();
    assert!(app.cast.unknown && app.cast.enabled && app.paused);
    app.cast_start(Duration::ZERO, true);
    assert_eq!(app.queue.current_entry().unwrap().occurrence, occurrence);
    assert!(requests.try_recv().is_err());
}
#[test]
fn seek_keeps_pause_and_occurrence_and_rejects_old_position_snapshots() {
    let (mut app, control, _, _) = fixture();
    app.paused = true;
    let occurrence = app.queue.current_entry().unwrap().occurrence;
    let position = Duration::from_millis(42_125);
    app.cast_seek(position);
    assert!(matches!(control.next(),Some(Command::Seek {serial:1,position:p}) if p==position));
    control
        .emit(CastEvent::Snapshot(Snapshot {
            state: State::Playing,
            position: Duration::from_secs(99),
            seek_serial: 0,
            observed: Instant::now(),
        }))
        .unwrap();
    app.events();
    assert!(app.paused);
    assert_eq!(app.position_time(), position);
    control
        .emit(CastEvent::Snapshot(Snapshot {
            state: State::Paused,
            position,
            seek_serial: 1,
            observed: Instant::now(),
        }))
        .unwrap();
    app.events();
    assert!(app.paused && !app.buffering);
    assert_eq!(app.position_time(), position);
    assert_eq!(app.queue.current_entry().unwrap().occurrence, occurrence);
    assert_eq!(app.cast.credit, Duration::ZERO);
}
#[test]
fn remote_pause_and_volume_do_not_route_to_local_controls() {
    let (mut app, control, _, _) = fixture();
    let ctx = egui::Context::default();
    app.dispatch(crate::actions::Action::Pause, &ctx);
    assert!(matches!(control.next(), Some(Command::Pause(true))));
    let before = app.volume;
    app.dispatch(crate::actions::Action::Volume(0.2), &ctx);
    assert!(matches!(control.next(),Some(Command::Volume(v)) if (v-0.2).abs()<1e-6));
    assert_eq!(
        app.volume, before,
        "Do not claim volume confirmation before the reply"
    );
}
#[test]
fn paused_completion_does_not_start_the_next_track() {
    let (mut app, control, _, mut requests) = fixture();
    app.paused = true;
    control.close(Ok(End::Finished), true);
    app.events();
    assert!(app.paused);
    assert!(app.cast.quiet.is_none() && app.cast.worker.is_none());
    assert!(requests.try_recv().is_err());
}
