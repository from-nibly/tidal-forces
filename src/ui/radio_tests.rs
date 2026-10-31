use super::super::tests::{fixture, track};
use super::*;

fn playing() -> (App, tokio::sync::mpsc::Receiver<Request>) {
    let (_, mut app, _, requests) = fixture();
    app.queue_mut()
        .start(vec![track(1), track(2)], 0, "Source".into(), None)
        .unwrap();
    app.paused = false;
    (app, requests)
}
fn request(
    app: &mut App,
    requests: &mut tokio::sync::mpsc::Receiver<Request>,
) -> (u64, tokio::sync::oneshot::Receiver<()>) {
    app.appearance.radio_continuation = true;
    app.radio_tick();
    let Request::Recommendations {
        user,
        serial,
        id,
        cancel,
    } = requests.try_recv().unwrap()
    else {
        panic!("Expected recommendations");
    };
    assert_eq!(user, 7);
    assert_eq!(id, app.queue.current().unwrap().id);
    (serial, cancel)
}

#[test]
fn continuation_is_opt_in_preserves_manual_priority_and_deduplicates() {
    let (mut app, mut requests) = playing();
    app.radio_tick();
    assert!(requests.try_recv().is_err());
    app.queue_mut().add(track(3), true).unwrap();
    let generation = app.play_generation;
    let current = app.queue.current_entry().unwrap().occurrence;
    let (serial, _) = request(&mut app, &mut requests);
    app.radio_result(
        7,
        serial,
        Ok(vec![
            track(1),
            track(2),
            track(3),
            track(4),
            track(4),
            track(5),
        ]),
    );
    assert_eq!(app.queue.export_ids().unwrap(), vec![1, 3, 2, 4, 5]);
    assert_eq!(app.queue.current_entry().unwrap().occurrence, current);
    assert_eq!(app.play_generation, generation);
    assert!(!app.paused);
    assert_eq!(app.radio.added, 2);
    assert!(app.queue.title().ends_with("track recommendations"));
    assert!(requests.try_recv().is_err());
}

#[test]
fn stale_account_revision_occurrence_generation_and_paused_results_never_append_or_resume() {
    for change in 0..6 {
        let (mut app, mut requests) = playing();
        let (serial, mut cancel) = request(&mut app, &mut requests);
        match change {
            0 => app.account = Some(8),
            1 => app.queue_mut().add(track(8), true).unwrap(),
            2 => {
                app.queue_mut().advance(false);
            }
            3 => app.play_generation = app.backend.player.reserve(),
            4 => app.paused = true,
            _ => {
                app.paused = true;
                app.ended = true;
            }
        }
        let before = app.queue.export_ids().unwrap();
        let paused = app.paused;
        app.radio_result(7, serial, Ok(vec![track(99)]));
        assert_eq!(app.queue.export_ids().unwrap(), before);
        assert_eq!(app.paused, paused);
        assert!(matches!(
            cancel.try_recv(),
            Err(tokio::sync::oneshot::error::TryRecvError::Closed)
        ));
    }
}

#[test]
fn errors_empty_pages_repeat_and_clear_source_stop_automatic_work() {
    for result in [
        Err("Synthetic failure".into()),
        Ok(vec![]),
        Ok(vec![track(1), track(2)]),
    ] {
        let (mut app, mut requests) = playing();
        let (serial, _) = request(&mut app, &mut requests);
        let revision = app.queue.revision();
        app.radio_result(7, serial, result);
        assert!(app.radio.blocked);
        assert_eq!(app.queue.revision(), revision);
        assert!(!app.paused && app.error.is_none());
        app.radio_tick();
        assert!(requests.try_recv().is_err());
    }
    let (mut app, mut requests) = playing();
    app.appearance.radio_continuation = true;
    app.queue_mut().set_repeat(crate::queue::Repeat::Context);
    app.radio_tick();
    assert!(requests.try_recv().is_err());
    app.queue_mut().set_repeat(crate::queue::Repeat::Off);
    let (_, mut cancel) = request(&mut app, &mut requests);
    app.clear_source();
    assert!(cancel.try_recv().is_err());
    app.radio_tick();
    assert!(requests.try_recv().is_err());
}

#[test]
fn request_and_track_budgets_are_finite_without_replacing_prior_queue_entries() {
    let (mut app, mut requests) = playing();
    for attempt in 0..3 {
        let (serial, _) = request(&mut app, &mut requests);
        app.radio_result(
            7,
            serial,
            Ok((0..100).map(|i| track(100 + attempt * 100 + i)).collect()),
        );
        assert_eq!(app.radio.added, (attempt as usize + 1) * 50);
        while app.queue.upcoming_len() > 2 {
            assert!(app.queue_mut().advance(false));
        }
    }
    app.radio_tick();
    assert!(requests.try_recv().is_err());
    assert_eq!(app.radio.attempts, 3);
    assert_eq!(app.radio.added, 150);
}

#[test]
fn browsing_radio_does_not_replace_or_restart_playback() {
    let (_, mut app, events, mut requests) = fixture();
    app.queue_mut()
        .start(vec![track(1)], 0, "Original".into(), None)
        .unwrap();
    app.paused = false;
    let revision = app.queue.revision();
    let generation = app.play_generation;
    app.apply_track_action(TrackAction::BrowseRadio(RadioSeed::Track {
        id: 9,
        title: "Seed".into(),
    }));
    let Request::Radio {
        generation: view, ..
    } = requests.try_recv().unwrap()
    else {
        panic!("Expected browse read");
    };
    events
        .send(Event::Radio {
            generation: view,
            tracks: vec![track(10), track(11)],
        })
        .unwrap();
    app.events();
    assert!(!app.autoplay_page && !app.paused);
    assert_eq!(app.queue.revision(), revision);
    assert_eq!(app.play_generation, generation);
    assert_eq!(app.tracks[0].id, 10);
    assert!(requests.try_recv().is_err());
}

#[test]
fn refreshed_session_cancels_pending_recommendation_scope() {
    let (_, mut app, events, mut requests) = fixture();
    app.queue_mut()
        .start(vec![track(1)], 0, "Source".into(), None)
        .unwrap();
    app.paused = false;
    let (_, mut cancel) = request(&mut app, &mut requests);
    events.send(Event::Session(Some((7, "US".into())))).unwrap();
    app.events();
    assert!(app.radio.pending.is_none());
    assert!(matches!(
        cancel.try_recv(),
        Err(tokio::sync::oneshot::error::TryRecvError::Closed)
    ));
}

#[test]
fn shuffled_recommendations_follow_existing_source_and_manual_entries() {
    let (mut app, mut requests) = playing();
    app.queue_mut().toggle_shuffle();
    app.queue_mut().add(track(3), true).unwrap();
    let (serial, _) = request(&mut app, &mut requests);
    app.radio_result(7, serial, Ok(vec![track(4), track(5)]));
    let ids = app.queue.export_ids().unwrap();
    assert_eq!(&ids[..3], &[1, 3, 2]);
    assert!(ids[3..].contains(&4) && ids[3..].contains(&5));
}
