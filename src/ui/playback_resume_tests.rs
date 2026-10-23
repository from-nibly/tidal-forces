use super::*;
use std::time::{Duration, SystemTime};

#[test]
fn long_pause_refreshes_same_occurrence_at_position_and_ignores_old_end() {
    let (_, mut app, events, mut requests) = tests::fixture();
    app.queue
        .start(
            vec![tests::track(1), tests::track(1), tests::track(2)],
            1,
            "Fixture".into(),
            None,
        )
        .unwrap();
    app.play_current();
    assert!(matches!(requests.try_recv(), Ok(Request::Play { .. })));
    let old = app.play_generation;
    events
        .send(Event::Playing {
            generation: old,
            quality: "FLAC".into(),
        })
        .unwrap();
    app.events();
    app.position = 42;
    let occurrence = app.queue.current_entry().unwrap().occurrence;
    let revision = app.queue.revision();
    app.toggle();
    assert!(app.paused && app.paused_since.is_some());
    app.paused_since = Some(SystemTime::now() - Duration::from_secs(3600));
    app.toggle();
    let new = app.play_generation;
    assert_ne!(old, new);
    assert!(
        matches!(requests.try_recv(), Ok(Request::Play { generation, id: 1, position: 42, .. }) if generation == new)
    );
    assert!(app.buffering && !app.paused && app.actual_quality.is_empty());
    assert_eq!(app.queue.current_entry().unwrap().occurrence, occurrence);
    assert_eq!(app.queue.revision(), revision);
    events.send(Event::Ended(old)).unwrap();
    app.events();
    assert!(requests.try_recv().is_err());
    assert_eq!(app.queue.current_entry().unwrap().occurrence, occurrence);
    app.toggle(); // A second click pauses the pending fresh stream, not another request.
    events
        .send(Event::Playing {
            generation: new,
            quality: "FLAC".into(),
        })
        .unwrap();
    app.events();
    assert!(app.paused && !app.buffering);
    assert!(requests.try_recv().is_err());
}

#[test]
fn short_pause_keeps_existing_stream() {
    let (_, mut app, _, mut requests) = tests::fixture();
    app.queue
        .start(vec![tests::track(1)], 0, "Fixture".into(), None)
        .unwrap();
    app.paused = false;
    app.actual_quality = "FLAC".into();
    let generation = app.play_generation;
    app.toggle();
    assert!(app.paused_since.is_some());
    app.toggle();
    assert!(!app.paused && !app.buffering && app.paused_since.is_none());
    assert_eq!(app.play_generation, generation);
    assert!(requests.try_recv().is_err());
}

#[test]
fn premature_end_retries_at_position_without_advancing_or_automatic_retry() {
    let (_, mut app, events, mut requests) = tests::fixture();
    app.queue
        .start(
            vec![tests::track(1), tests::track(2)],
            0,
            "Fixture".into(),
            None,
        )
        .unwrap();
    app.position = 42;
    app.paused = false;
    app.actual_quality = "FLAC".into();
    let revision = app.queue.revision();
    events.send(Event::Ended(app.play_generation)).unwrap();
    app.events();
    assert!(app.paused && !app.ended && app.actual_quality.is_empty());
    assert!(requests.try_recv().is_err());
    app.toggle();
    assert!(matches!(
        requests.try_recv(),
        Ok(Request::Play {
            id: 1,
            position: 42,
            ..
        })
    ));
    events
        .send(Event::PlaybackError {
            generation: app.play_generation,
            message: "Synthetic connection failure".into(),
        })
        .unwrap();
    app.events();
    assert!(app.paused && !app.buffering);
    assert_eq!(app.position, 42);
    assert_eq!(app.queue.revision(), revision);
    assert!(requests.try_recv().is_err());
}

#[test]
fn natural_end_still_advances_the_queue() {
    let (_, mut app, events, mut requests) = tests::fixture();
    app.queue
        .start(
            vec![tests::track(1), tests::track(2)],
            0,
            "Fixture".into(),
            None,
        )
        .unwrap();
    app.position = 180;
    events.send(Event::Ended(app.play_generation)).unwrap();
    app.events();
    assert!(matches!(
        requests.try_recv(),
        Ok(Request::Play {
            id: 2,
            position: 0,
            ..
        })
    ));
}
