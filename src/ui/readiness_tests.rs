use super::*;
use crate::api::TrackUnavailable::NotReady;

fn reject(app: &App, events: &std::sync::mpsc::Sender<Event>) {
    events
        .send(Event::TrackUnavailable {
            user: 7,
            generation: app.play_generation,
            id: app.queue.current().unwrap().id,
            reason: NotReady,
        })
        .unwrap();
}
#[test]
fn readiness_uses_shuffle_successor_and_ignores_a_reply_for_the_wrong_track() {
    let (_, mut app, events, mut requests) = tests::fixture();
    app.queue_mut()
        .start(
            (1..=8).map(tests::track).collect(),
            0,
            "Synthetic".into(),
            None,
        )
        .unwrap();
    app.queue_mut().toggle_shuffle();
    let expected = app.queue.next_natural().unwrap().occurrence;
    app.play_current();
    let _ = requests.try_recv();
    let current = app.queue.current_entry().unwrap().occurrence;
    events
        .send(Event::TrackUnavailable {
            user: 7,
            generation: app.play_generation,
            id: 999,
            reason: NotReady,
        })
        .unwrap();
    app.events();
    assert_eq!(app.queue.current_entry().unwrap().occurrence, current);
    assert_eq!(app.unavailable_skips, 0);
    assert!(requests.try_recv().is_err());
    reject(&app, &events);
    app.events();
    assert_eq!(app.queue.current_entry().unwrap().occurrence, expected);
    assert!(matches!(requests.try_recv(), Ok(Request::Play { .. })));
}
#[test]
fn readiness_skip_uses_manual_priority_then_duplicate_occurrences_despite_repeat_one() {
    let (_, mut app, events, mut requests) = tests::fixture();
    app.queue_mut()
        .start(
            vec![tests::track(1), tests::track(1), tests::track(2)],
            0,
            "Synthetic".into(),
            None,
        )
        .unwrap();
    app.queue_mut().set_repeat(crate::queue::Repeat::Track);
    app.queue_mut().add(tests::track(99), true).unwrap();
    app.play_current();
    let _ = requests.try_recv();
    let first = app.queue.current_entry().unwrap().occurrence;
    reject(&app, &events);
    app.events();
    assert!(matches!(
        requests.try_recv(),
        Ok(Request::Play { id: 99, .. })
    ));
    reject(&app, &events);
    app.events();
    assert!(matches!(
        requests.try_recv(),
        Ok(Request::Play { id: 1, .. })
    ));
    assert_ne!(app.queue.current_entry().unwrap().occurrence, first);
    assert_eq!(app.unavailable_skips, 2);
    assert!(app.notice.as_ref().unwrap().contains("4005"));
}
#[test]
fn readiness_limit_survives_repeat_context_and_resets_only_on_playing_or_explicit_retry() {
    let (_, mut app, events, mut requests) = tests::fixture();
    app.queue_mut()
        .start(vec![tests::track(1)], 0, "Synthetic".into(), None)
        .unwrap();
    app.queue_mut().set_repeat(crate::queue::Repeat::Context);
    app.play_current();
    let _ = requests.try_recv();
    for n in 1..=5 {
        reject(&app, &events);
        app.events();
        assert_eq!(app.unavailable_skips, n);
        if n < 5 {
            assert!(matches!(
                requests.try_recv(),
                Ok(Request::Play { id: 1, .. })
            ));
        }
    }
    assert!(app.paused && !app.buffering && requests.try_recv().is_err());
    assert!(
        app.error.as_ref().unwrap().contains("five")
            && app.error.as_ref().unwrap().contains("4005")
    );
    app.toggle();
    assert_eq!(app.unavailable_skips, 0);
    let _ = requests.try_recv();
    reject(&app, &events);
    app.events();
    let _ = requests.try_recv();
    assert_eq!(app.unavailable_skips, 1);
    events
        .send(Event::TrackAvailable {
            user: 7,
            generation: app.play_generation,
            id: 1,
        })
        .unwrap();
    app.events();
    assert_eq!(app.unavailable_skips, 1);
    events
        .send(Event::Playing {
            generation: app.play_generation,
            quality: "FLAC".into(),
        })
        .unwrap();
    app.events();
    assert_eq!(app.unavailable_skips, 0);
}
#[test]
fn paused_output_fault_and_last_track_readiness_errors_never_start_replacement_audio() {
    for mode in 0..3 {
        let (_, mut app, events, mut requests) = tests::fixture();
        app.queue_mut()
            .start(vec![tests::track(1)], 0, "Synthetic".into(), None)
            .unwrap();
        app.play_current();
        let _ = requests.try_recv();
        if mode == 0 {
            app.paused = true;
        }
        if mode == 1 {
            app.output_fault = Some(7);
        }
        reject(&app, &events);
        app.events();
        assert!(app.paused && !app.buffering && requests.try_recv().is_err());
        assert_eq!(app.queue.current().unwrap().id, 1);
        assert!(app.error.as_ref().unwrap().contains("4005"));
        assert_eq!(app.unavailable_skips, 1);
    }
}
