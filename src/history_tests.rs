use super::*;
fn track(duration: u64) -> Arc<Track> {
    Arc::new(Track {
        id: 9,
        title: "Fixture".into(),
        duration,
        ..Track::default()
    })
}
fn observe(
    listener: &mut Listener,
    history: &mut History,
    generation: u64,
    rendered: u64,
    elapsed: u64,
) {
    listener.observe(
        Some(history),
        generation,
        track(180),
        Duration::from_secs(rendered),
        Duration::from_secs(elapsed),
        1000 + elapsed,
    );
}
#[test]
fn opt_in_threshold_and_real_consumption_not_position_or_paused_wall_time() {
    let mut history = History::new(7);
    let mut listener = Listener::default();
    observe(&mut listener, &mut history, 1, 60, 60);
    assert!(history.entries.is_empty());
    history.set_enabled(true);
    listener.reset_collection();
    observe(&mut listener, &mut history, 1, 60, 61);
    observe(&mut listener, &mut history, 1, 89, 90);
    assert!(history.entries.is_empty());
    observe(&mut listener, &mut history, 1, 90, 91);
    assert_eq!(history.entries[0].listened_ms, 30_000);
    observe(&mut listener, &mut history, 1, 90, 5000); // paused, stalled, or only a seek
    assert_eq!(history.entries[0].listened_ms, 30_000);
    observe(&mut listener, &mut history, 1, 100, 5010);
    assert_eq!(history.entries[0].listened_ms, 40_000);
    observe(&mut listener, &mut history, 1, 1000, 5011); // faster-than-real-time output cannot invent listening
    assert_eq!(history.entries[0].listened_ms, 41_000);
}
#[test]
fn short_tracks_repeat_occurrences_and_clear_disable_do_not_resurrect_records() {
    let mut history = History::new(7);
    history.set_enabled(true);
    let mut listener = Listener::default();
    listener.observe(
        Some(&mut history),
        1,
        track(10),
        Duration::from_secs(5),
        Duration::from_secs(5),
        100,
    );
    assert_eq!(history.entries.len(), 1);
    listener.observe(
        Some(&mut history),
        2,
        track(10),
        Duration::from_secs(5),
        Duration::from_secs(5),
        105,
    );
    assert_eq!(history.entries.len(), 2);
    assert_ne!(history.entries[0].id, history.entries[1].id);
    history.clear();
    listener.reset_collection();
    observe(&mut listener, &mut history, 2, 90, 90);
    assert!(history.entries.is_empty());
    observe(&mut listener, &mut history, 2, 120, 120);
    assert_eq!(history.entries.len(), 1);
    assert_eq!(history.entries[0].listened_ms, 30_000);
    assert_eq!(history.entries[0].started_at, 1090);
    history.set_enabled(false);
    listener.reset_collection();
    observe(&mut listener, &mut history, 2, 200, 200);
    assert_eq!(history.entries[0].listened_ms, 30_000);
}
#[test]
fn retention_is_bounded_and_expired_records_are_removed_even_when_disabled() {
    let mut history = History::new(7);
    history.set_enabled(true);
    let mut listener = Listener::default();
    for generation in 1..=1100 {
        observe(&mut listener, &mut history, generation, 30, 30);
    }
    assert_eq!(history.entries.len(), MAX_ENTRIES);
    history.set_enabled(false);
    history.prune(RETENTION + 2000);
    assert!(history.entries.is_empty());
}
#[test]
fn account_scoped_atomic_private_history_is_separate_from_playback_and_credentials() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("session.json"), b"credentials untouched").unwrap();
    let mut history = History::new(7);
    history.set_enabled(true);
    observe(&mut Listener::default(), &mut history, 1, 30, 30);
    save(root.path(), &history).unwrap();
    assert_eq!(load(root.path(), 7, 1100).unwrap().entries.len(), 1);
    assert!(load(root.path(), 8, 1100).unwrap().entries.is_empty());
    assert!(!load(root.path(), 8, 1100).unwrap().enabled);
    assert_eq!(
        fs::read(root.path().join("session.json")).unwrap(),
        b"credentials untouched"
    );
    assert!(!root.path().join("accounts/7/player-state.json").exists());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(root.path().join("accounts/7/history.json"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        assert_eq!(
            fs::metadata(root.path().join("accounts/7"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
    }
    history.user = 8;
    fs::write(
        root.path().join("accounts/7/history.json"),
        serde_json::to_vec(&history).unwrap(),
    )
    .unwrap();
    assert!(load(root.path(), 7, 1100).is_err());
}
#[test]
fn corrupt_future_and_oversized_files_are_not_overwritten_on_load() {
    let root = tempfile::tempdir().unwrap();
    save(root.path(), &History::new(7)).unwrap();
    let path = root.path().join("accounts/7/history.json");
    for bytes in [
        b"broken".to_vec(),
        br#"{"version":999,"user":7,"enabled":true,"next_id":1,"entries":[]}"#.to_vec(),
    ] {
        fs::write(&path, &bytes).unwrap();
        assert!(load(root.path(), 7, 0).is_err());
        assert_eq!(fs::read(&path).unwrap(), bytes);
    }
    fs::File::create(&path)
        .unwrap()
        .set_len(MAX_BYTES + 1)
        .unwrap();
    assert!(load(root.path(), 7, 0).is_err());
}
#[test]
fn worker_flushes_clear_and_account_switch_in_order_without_recreating_deleted_entries() {
    let root = tempfile::tempdir().unwrap();
    let worker =
        crate::player_state::StateStore::new(root.path().into(), eframe::egui::Context::default())
            .unwrap();
    let mut history = History::new(7);
    history.set_enabled(true);
    observe(&mut Listener::default(), &mut history, 1, 30, 30);
    worker.history(history.clone());
    history.clear();
    worker.history(history);
    worker.history(History::new(8));
    worker.load_history(7, 1);
    loop {
        if let crate::player_state::Event::HistoryLoaded { result, .. } =
            worker.events.recv_timeout(Duration::from_secs(5)).unwrap()
        {
            assert!(result.unwrap().entries.is_empty());
            break;
        }
    }
    drop(worker);
    assert!(load(root.path(), 7, 0).unwrap().entries.is_empty());
    assert!(!load(root.path(), 8, 0).unwrap().enabled);
}
