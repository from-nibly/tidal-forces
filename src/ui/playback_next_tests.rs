use super::super::tests::{fixture, track};
use super::*;

fn playing() -> (
    App,
    std::sync::mpsc::Sender<Event>,
    tokio::sync::mpsc::Receiver<Request>,
) {
    let (_, mut app, events, requests) = fixture();
    app.queue_mut()
        .start(
            vec![track(1), track(2), track(3)],
            0,
            "Fixture".into(),
            None,
        )
        .unwrap();
    app.backend.player.seed_buffered_fixture();
    app.paused = false;
    app.buffering = false;
    app.actual_quality = "Fixture FLAC".into();
    app.position = 151;
    (app, events, requests)
}
fn prepare(
    app: &mut App,
    requests: &mut tokio::sync::mpsc::Receiver<Request>,
    claimed: bool,
) -> crate::audio::NextRequest {
    app.prefetch_audio();
    let Request::PrepareNext(request) = requests.try_recv().unwrap() else {
        panic!("Expected one successor request");
    };
    app.backend.player.fixture_next(&request, claimed);
    request
}
#[test]
fn prefetch_is_near_end_single_attempt_and_paused_restore_is_inert() {
    let (mut app, _, mut requests) = playing();
    app.position = 149;
    app.prefetch_audio();
    assert!(requests.try_recv().is_err());
    app.position = 151;
    app.paused = true;
    app.prefetch_audio();
    assert!(requests.try_recv().is_err());
    app.paused = false;
    let request = prepare(&mut app, &mut requests, false);
    assert_eq!((request.key.user, request.key.track), (7, 2));
    app.prefetch_audio();
    assert!(requests.try_recv().is_err());
    app.backend.player.fail_next(&request);
    app.prefetch_audio();
    assert!(
        requests.try_recv().is_err(),
        "Do not retry failed speculation every frame"
    );
    assert!(app.error.is_none());
}
#[test]
fn queue_revision_churn_has_a_bounded_speculative_request_budget() {
    let (mut app, _, mut requests) = playing();
    for attempt in 0..8 {
        app.queue_mut()
            .append(vec![track(20 + attempt)], None)
            .unwrap();
        app.prefetch_audio();
        if attempt < 2 {
            let Request::PrepareNext(request) = requests.try_recv().unwrap() else {
                panic!("Expected bounded preparation");
            };
            app.backend.player.fail_next(&request);
        } else {
            assert!(requests.try_recv().is_err());
        }
    }
    assert!(app.error.is_none());
}

#[test]
fn queue_edits_cancel_before_claim_and_reconcile_after_claim() {
    let edits: &[fn(&mut App)] = &[
        |a| {
            a.queue_mut().add(track(9), true).unwrap();
        },
        |a| {
            a.queue_mut().clear_manual();
        },
        |a| {
            a.queue_mut().toggle_shuffle();
        },
        |a| {
            a.queue_mut().set_repeat(Repeat::Track);
        },
        |a| {
            a.queue_mut().append(vec![track(4)], None).unwrap();
        },
        |a| {
            a.queue_mut().clear_context();
        },
    ];
    for edit in edits {
        let (mut app, _, mut requests) = playing();
        let request = prepare(&mut app, &mut requests, false);
        edit(&mut app);
        assert!(!app.backend.player.fixture_claim_next(&request));
        assert_eq!(app.queue.current().unwrap().id, 1);

        let (mut app, _, mut requests) = playing();
        prepare(&mut app, &mut requests, true);
        edit(&mut app);
        assert_eq!(app.queue.current().unwrap().id, 2);
        assert_eq!(app.play_generation, 1);
        assert_eq!(app.actual_quality, "Fixture next FLAC");
    }
}
#[test]
fn duplicate_occurrences_transition_once_and_stale_events_are_ignored() {
    let (mut app, events, mut requests) = playing();
    app.queue_mut().add(track(1), true).unwrap();
    let old_occurrence = app.queue.current_entry().unwrap().occurrence;
    let request = prepare(&mut app, &mut requests, true);
    app.events();
    assert_eq!(app.queue.current().unwrap().id, 1);
    assert_ne!(
        app.queue.current_entry().unwrap().occurrence,
        old_occurrence
    );
    assert_eq!(
        app.queue.current_entry().unwrap().occurrence,
        request.key.occurrence
    );
    events.send(Event::Transition(app.play_generation)).unwrap();
    events.send(Event::Ended(0)).unwrap();
    events
        .send(Event::PlaybackError {
            generation: 0,
            message: "stale".into(),
        })
        .unwrap();
    app.events();
    assert_eq!(
        app.queue.current_entry().unwrap().occurrence,
        request.key.occurrence
    );
    assert!(app.error.is_none());
    assert!(
        requests.try_recv().is_err(),
        "No stream reauthorization on accepted boundary"
    );
}
#[test]
fn seek_after_claim_uses_promoted_song_and_unique_generation() {
    let (mut app, _, mut requests) = playing();
    prepare(&mut app, &mut requests, true);
    app.seek(1);
    assert_eq!(app.queue.current().unwrap().id, 2);
    assert_eq!(app.play_generation, 2);
    assert!(matches!(
        requests.try_recv(),
        Ok(Request::SeekBuffered {
            generation: 2,
            position: 1
        })
    ));
    assert!(app.buffering);
}
#[test]
fn seek_before_claim_cancels_next_and_never_reuses_its_generation() {
    let (mut app, _, mut requests) = playing();
    let request = prepare(&mut app, &mut requests, false);
    app.seek(10);
    assert_eq!(app.queue.current().unwrap().id, 1);
    assert_eq!(app.play_generation, 2);
    assert!(!app.backend.player.fixture_claim_next(&request));
    assert!(matches!(
        requests.try_recv(),
        Ok(Request::SeekBuffered {
            generation: 2,
            position: 10
        })
    ));
}
#[test]
fn repeat_track_keeps_occurrence_but_advances_playback_generation() {
    let (mut app, _, mut requests) = playing();
    app.queue_mut().set_repeat(Repeat::Track);
    let occurrence = app.queue.current_entry().unwrap().occurrence;
    let request = prepare(&mut app, &mut requests, true);
    assert_eq!(request.key.occurrence, occurrence);
    app.events();
    assert_eq!(app.queue.current_entry().unwrap().occurrence, occurrence);
    assert_eq!(app.play_generation, 1);
    assert_eq!(app.queue.context_upcoming().next().unwrap().track.id, 2);
}
#[test]
fn account_mismatch_stops_claimed_audio_instead_of_advancing_wrong_queue() {
    let (mut app, _, mut requests) = playing();
    prepare(&mut app, &mut requests, true);
    app.account = Some(8);
    app.events();
    assert_eq!(app.queue.current().unwrap().id, 1);
    assert!(app.paused);
    assert!(app.actual_quality.is_empty());
    assert_eq!(app.play_generation, 2);
}
#[test]
fn output_loss_pauses_at_rendered_position_and_requires_an_explicit_retry() {
    let (mut app, events, mut requests) = playing();
    let request = prepare(&mut app, &mut requests, false);
    let occurrence = app.queue.current_entry().unwrap().occurrence;
    events
        .send(Event::OutputLost {
            epoch: 1,
            generation: 0,
            position: Some(157),
        })
        .unwrap();
    app.events();
    assert!(app.paused && !app.buffering && app.audio_available);
    assert_eq!(app.position, 157);
    assert_eq!(app.queue.current_entry().unwrap().occurrence, occurrence);
    assert!(app.actual_quality.is_empty());
    assert!(!app.backend.player.fixture_claim_next(&request));
    assert!(
        requests.try_recv().is_err(),
        "Never automatically authorize/restart playback"
    );
    app.toggle();
    assert!(matches!(
        requests.try_recv(),
        Ok(Request::Play {
            generation: 2,
            id: 1,
            position: 157,
            ..
        })
    ));
    assert!(!app.paused && app.buffering);
}
#[test]
fn stale_output_loss_cannot_cancel_a_valid_prepared_successor() {
    let (mut app, events, mut requests) = playing();
    let request = prepare(&mut app, &mut requests, false);
    events
        .send(Event::OutputLost {
            epoch: 1,
            generation: 99,
            position: Some(1),
        })
        .unwrap();
    app.events();
    assert!(!app.paused);
    assert_eq!(app.position, 151);
    assert!(app.error.is_none());
    assert!(app.backend.player.fixture_claim_next(&request));
}
#[test]
fn stale_unavailability_cannot_cancel_a_valid_prepared_successor() {
    let (mut app, events, mut requests) = playing();
    let request = prepare(&mut app, &mut requests, false);
    events
        .send(Event::TrackUnavailable {
            user: 7,
            generation: 99,
            id: 1,
        })
        .unwrap();
    app.events();
    assert!(!app.paused && app.unavailable.is_empty());
    assert!(app.backend.player.fixture_claim_next(&request));
}

#[test]
fn output_loss_after_claim_preserves_the_new_occurrence() {
    let (mut app, events, mut requests) = playing();
    prepare(&mut app, &mut requests, true);
    events
        .send(Event::OutputLost {
            epoch: 1,
            generation: 1,
            position: Some(2),
        })
        .unwrap();
    app.events();
    assert_eq!(app.queue.current().unwrap().id, 2);
    assert_eq!(app.play_generation, 1);
    assert_eq!(app.position, 2);
    assert!(app.paused && app.actual_quality.is_empty());
    assert!(requests.try_recv().is_err());
}

#[test]
fn pause_after_claim_preserves_next_occurrence_and_pause_intent() {
    let (mut app, _, mut requests) = playing();
    prepare(&mut app, &mut requests, true);
    app.toggle();
    assert_eq!(app.queue.current().unwrap().id, 2);
    assert!(app.paused);
    assert!(app.paused_since.is_some());
}
