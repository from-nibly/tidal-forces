use super::*;

#[test]
fn streaming_underflow_is_not_a_pending_play_request_and_pause_still_works() {
    let (_, mut app, events, mut requests) = tests::fixture();
    app.queue
        .start(vec![tests::track(1)], 0, "Fixture".into(), None)
        .unwrap();
    app.paused = false;
    app.actual_quality = "FLAC".into();
    app.position = 42;
    events
        .send(Event::StreamBuffering {
            generation: app.play_generation,
            buffering: true,
        })
        .unwrap();
    app.events();
    assert!(app.audio_buffering() && !app.buffering);
    app.toggle();
    assert!(app.paused);
    assert_eq!(app.position, 42);
    assert!(requests.try_recv().is_err());
    events
        .send(Event::StreamBuffering {
            generation: app.play_generation,
            buffering: false,
        })
        .unwrap();
    app.events();
    assert!(!app.audio_buffering());
    assert!(app.paused);
}

#[test]
fn seek_prepares_new_stream_without_changing_queue_or_pause_intent() {
    for paused in [true, false] {
        let (_, mut app, events, mut requests) = tests::fixture();
        app.queue
            .start(
                vec![tests::track(1), tests::track(1)],
                1,
                "Fixture".into(),
                None,
            )
            .unwrap();
        app.paused = paused;
        app.actual_quality = "FLAC".into();
        let occurrence = app.queue.current_entry().unwrap().occurrence;
        let revision = app.queue.revision();
        let old = app.play_generation;
        app.seek(45);
        assert!(app.buffering);
        assert_eq!(app.paused, paused);
        assert_eq!(app.queue.current_entry().unwrap().occurrence, occurrence);
        assert_eq!(app.queue.revision(), revision);
        assert!(
            matches!(requests.try_recv(),Ok(Request::Play{id:1,position:45,generation,..}) if generation==app.play_generation && generation!=old)
        );
        events
            .send(Event::StreamBuffering {
                generation: old,
                buffering: true,
            })
            .unwrap();
        events.send(Event::Ended(old)).unwrap();
        events
            .send(Event::Playing {
                generation: app.play_generation,
                quality: "FLAC".into(),
            })
            .unwrap();
        app.events();
        assert_eq!(app.paused, paused);
        assert!(!app.audio_buffering());
        assert_eq!(app.position, 45);
        assert!(requests.try_recv().is_err());
    }
}

#[test]
fn buffered_seek_keeps_occurrence_pause_and_idle_refresh_guards() {
    for paused in [true, false] {
        let (_, mut app, events, mut requests) = tests::fixture();
        app.queue
            .start(
                vec![tests::track(1), tests::track(1)],
                1,
                "Fixture".into(),
                None,
            )
            .unwrap();
        app.backend.player.seed_buffered_fixture();
        app.paused = paused;
        app.paused_since =
            paused.then(|| std::time::SystemTime::now() - std::time::Duration::from_secs(120));
        let paused_since = app.paused_since;
        app.actual_quality = "FLAC".into();
        let occurrence = app.queue.current_entry().unwrap().occurrence;
        let revision = app.queue.revision();
        let old = app.play_generation;
        app.seek(45);
        assert!(
            matches!(requests.try_recv(), Ok(Request::SeekBuffered { generation, position:45 }) if generation == app.play_generation && generation != old)
        );
        assert_eq!(app.paused, paused);
        assert_eq!(app.paused_since, paused_since);
        assert_eq!(app.queue.current_entry().unwrap().occurrence, occurrence);
        assert_eq!(app.queue.revision(), revision);
        events.send(Event::Ended(old)).unwrap();
        events
            .send(Event::Position {
                generation: old,
                seconds: 10,
            })
            .unwrap();
        events
            .send(Event::Playing {
                generation: app.play_generation,
                quality: "FLAC".into(),
            })
            .unwrap();
        app.events();
        assert!(!app.buffering);
        assert_eq!(app.position, 45);
        assert_eq!(app.paused, paused);
        assert!(requests.try_recv().is_err());
    }
}

#[test]
fn stopped_or_failed_stream_cannot_leave_a_buffering_indicator() {
    let (_, mut app, events, _) = tests::fixture();
    app.actual_quality = "FLAC".into();
    app.queue
        .start(vec![tests::track(1)], 0, "Fixture".into(), None)
        .unwrap();
    events
        .send(Event::StreamBuffering {
            generation: app.play_generation,
            buffering: true,
        })
        .unwrap();
    app.events();
    assert!(app.audio_buffering());
    events
        .send(Event::PlaybackError {
            generation: app.play_generation,
            message: "Synthetic decoder failure".into(),
        })
        .unwrap();
    app.events();
    assert!(!app.audio_buffering() && app.paused);
    events
        .send(Event::StreamBuffering {
            generation: app.play_generation,
            buffering: true,
        })
        .unwrap();
    app.events();
    assert!(
        !app.audio_buffering(),
        "Late status after failure is ignored"
    );
    app.actual_quality = "FLAC".into();
    app.starved_generation = Some(app.play_generation);
    app.stop_and_clear();
    assert!(!app.audio_buffering());
}
