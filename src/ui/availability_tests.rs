use super::*;
#[test]
fn confirmed_unavailability_is_account_and_generation_scoped_and_bounded() {
    let (_, mut app, events, mut requests) = tests::fixture();
    app.account = Some(7);
    app.queue_mut()
        .start(
            (1..=10).map(tests::track).collect(),
            0,
            "Fixture".into(),
            None,
        )
        .unwrap();
    app.play_current();
    assert!(matches!(requests.try_recv(), Ok(Request::Play { .. })));
    let first = app.play_generation;
    for id in 1..=5 {
        events
            .send(Event::TrackUnavailable {
                user: 8,
                generation: app.play_generation,
                id,
            })
            .unwrap();
        app.events();
        assert!(!app.unavailable.contains(&id));
        events
            .send(Event::TrackUnavailable {
                user: 7,
                generation: app.play_generation,
                id,
            })
            .unwrap();
        app.events();
        assert!(app.unavailable.contains(&id));
        if id < 5 {
            assert!(matches!(requests.try_recv(),Ok(Request::Play{id:next,..}) if next==id+1));
        }
    }
    assert!(app.paused && !app.buffering);
    assert_eq!(app.queue.current().unwrap().id, 5);
    assert!(requests.try_recv().is_err());
    events
        .send(Event::TrackUnavailable {
            user: 7,
            generation: first,
            id: 1,
        })
        .unwrap();
    app.events();
    assert_eq!(app.queue.current().unwrap().id, 5);
    app.next();
    assert_eq!(app.unavailable_skips, 0);
    assert!(matches!(
        requests.try_recv(),
        Ok(Request::Play { id: 6, .. })
    ));
    events.send(Event::Session(Some((8, "US".into())))).unwrap();
    app.events();
    assert!(app.unavailable.is_empty());
}
#[test]
fn duplicate_unavailable_tracks_keep_distinct_queue_occurrences() {
    let (_, mut app, events, mut requests) = tests::fixture();
    app.account = Some(7);
    app.queue_mut()
        .start(
            vec![tests::track(1), tests::track(1), tests::track(2)],
            0,
            "Fixture".into(),
            None,
        )
        .unwrap();
    app.play_current();
    let _ = requests.try_recv();
    let first = app.queue.current_entry().unwrap().occurrence;
    let generation = app.play_generation;
    events
        .send(Event::TrackUnavailable {
            user: 7,
            generation,
            id: 1,
        })
        .unwrap();
    app.events();
    assert_ne!(app.queue.current_entry().unwrap().occurrence, first);
    assert!(matches!(
        requests.try_recv(),
        Ok(Request::Play { id: 1, .. })
    ));
    let second = app.queue.current_entry().unwrap().occurrence;
    events
        .send(Event::TrackUnavailable {
            user: 7,
            generation,
            id: 1,
        })
        .unwrap();
    app.events();
    assert_eq!(app.queue.current_entry().unwrap().occurrence, second);
    assert!(requests.try_recv().is_err());
    events
        .send(Event::TrackUnavailable {
            user: 7,
            generation: app.play_generation,
            id: 1,
        })
        .unwrap();
    app.events();
    assert!(matches!(
        requests.try_recv(),
        Ok(Request::Play { id: 2, .. })
    ));
}

#[test]
fn availability_markers_expire_without_restarting_playback() {
    let (_, mut app, _, mut requests) = tests::fixture();
    app.unavailable.insert(1);
    app.unavailable_expiry = Some(std::time::Instant::now());
    app.events();
    assert!(app.unavailable.is_empty() && app.unavailable_expiry.is_none());
    assert!(requests.try_recv().is_err());
}

#[test]
fn paused_and_generic_failures_never_advance_and_explicit_retry_can_clear_a_marker() {
    let (_, mut app, events, mut requests) = tests::fixture();
    app.account = Some(7);
    app.queue_mut()
        .start(
            vec![tests::track(1), tests::track(2)],
            0,
            "Fixture".into(),
            None,
        )
        .unwrap();
    app.play_current();
    let _ = requests.try_recv();
    events
        .send(Event::PlaybackError {
            generation: app.play_generation,
            message: "Temporary failure".into(),
        })
        .unwrap();
    app.events();
    assert!(app.paused);
    assert_eq!(app.queue.current().unwrap().id, 1);
    assert!(app.unavailable.is_empty());
    assert!(requests.try_recv().is_err());
    events
        .send(Event::TrackUnavailable {
            user: 7,
            generation: app.play_generation,
            id: 1,
        })
        .unwrap();
    app.events();
    assert!(app.paused && app.unavailable.contains(&1));
    assert!(requests.try_recv().is_err());
    app.toggle();
    assert_eq!(app.unavailable_skips, 0);
    assert!(matches!(
        requests.try_recv(),
        Ok(Request::Play { id: 1, .. })
    ));
    events
        .send(Event::Playing {
            generation: app.play_generation,
            quality: "FLAC".into(),
        })
        .unwrap();
    app.events();
    assert!(!app.unavailable.contains(&1));
}
