use super::*;
use crate::{model::Track, queue::Repeat};
use std::time::Duration;

fn state(user: u64) -> PlaybackState {
    let mut queue = Queue::default();
    queue
        .start(
            vec![Track {
                id: 9,
                title: "Synthetic".into(),
                duration: 120,
                ..Default::default()
            }],
            0,
            "Test".into(),
            None,
        )
        .unwrap();
    queue.set_repeat(Repeat::Track);
    let occurrence = queue.current_entry().map(|entry| entry.occurrence);
    PlaybackState::new(
        user,
        queue,
        Progress {
            occurrence,
            position: 45,
            volume: 0.2,
            quality: "HIGH".into(),
            location: Location {
                page: Page::Search,
                query: "synthetic query".into(),
            },
        },
    )
}

#[test]
fn state_is_account_scoped_atomic_private_and_separate_from_credentials() {
    let root = tempfile::tempdir().unwrap();
    fs::write(
        root.path().join("session.json"),
        b"do not change credentials",
    )
    .unwrap();
    save(root.path(), &state(7)).unwrap();
    save(root.path(), &state(7)).unwrap();
    let restored = load(root.path(), 7).unwrap().unwrap();
    assert_eq!(restored.progress.position, 45);
    assert_eq!(restored.progress.volume, 0.2);
    assert_eq!(restored.queue.repeat(), Repeat::Track);
    assert!(load(root.path(), 8).unwrap().is_none());
    assert_eq!(
        fs::read(root.path().join("session.json")).unwrap(),
        b"do not change credentials"
    );
    let bytes = fs::read(state_dir(root.path(), 7).join("player-state.json")).unwrap();
    let text = String::from_utf8(bytes).unwrap();
    assert!(
        !text.contains("access_token") && !text.contains("refresh_token") && !text.contains("http")
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        for dir in [
            root.path().to_path_buf(),
            root.path().join("accounts"),
            state_dir(root.path(), 7),
        ] {
            assert_eq!(
                fs::metadata(dir).unwrap().permissions().mode() & 0o777,
                0o700
            );
        }
        assert_eq!(
            fs::metadata(state_dir(root.path(), 7).join("player-state.json"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
}

#[test]
fn invalid_or_future_state_is_rejected_without_overwriting_it() {
    let root = tempfile::tempdir().unwrap();
    save(root.path(), &state(7)).unwrap();
    let path = state_dir(root.path(), 7).join("player-state.json");
    for invalid in [
        serde_json::json!("broken"),
        {
            let mut value = serde_json::to_value(state(7)).unwrap();
            value["version"] = serde_json::json!(99);
            value
        },
        {
            let mut value = serde_json::to_value(state(7)).unwrap();
            value["user"] = serde_json::json!(8);
            value
        },
        {
            let mut value = serde_json::to_value(state(7)).unwrap();
            value["progress"]["volume"] = serde_json::json!(12);
            value
        },
    ] {
        let bytes = serde_json::to_vec(&invalid).unwrap();
        fs::write(&path, &bytes).unwrap();
        assert!(load(root.path(), 7).is_err());
        assert_eq!(fs::read(&path).unwrap(), bytes);
    }
}

#[test]
fn worker_coalesces_edits_flushes_on_drop_and_does_not_mix_occurrences() {
    let root = tempfile::tempdir().unwrap();
    let store = StateStore::new(root.path().to_owned(), eframe::egui::Context::default()).unwrap();
    store.load(7, 1);
    assert!(matches!(
        store.events.recv_timeout(Duration::from_secs(5)).unwrap(),
        Event::Loaded {
            result: Ok(None),
            request: 1,
            ..
        }
    ));
    let mut snapshot = state(7);
    for position in 0..100 {
        snapshot.progress.position = position;
        store.snapshot(snapshot.clone());
    }
    let mut stale = snapshot.progress.clone();
    stale.occurrence = Some(900);
    stale.position = 2;
    store.progress(7, stale);
    drop(store);
    assert_eq!(load(root.path(), 7).unwrap().unwrap().progress.position, 99);
}

#[test]
fn loads_follow_pending_old_account_saves_and_progress_is_account_scoped() {
    let root = tempfile::tempdir().unwrap();
    let store = StateStore::new(root.path().to_owned(), eframe::egui::Context::default()).unwrap();
    store.snapshot(state(7));
    store.load(8, 2);
    assert!(matches!(
        store.events.recv_timeout(Duration::from_secs(5)).unwrap(),
        Event::Loaded {
            user: 8,
            request: 2,
            result: Ok(None)
        }
    ));
    store.snapshot(state(8));
    let mut progress = state(8).progress;
    progress.position = 80;
    store.progress(8, progress);
    drop(store);
    assert_eq!(load(root.path(), 7).unwrap().unwrap().progress.position, 45);
    assert_eq!(load(root.path(), 8).unwrap().unwrap().progress.position, 80);
}

#[test]
fn oversized_files_and_untrusted_navigation_are_refused() {
    let root = tempfile::tempdir().unwrap();
    save(root.path(), &state(7)).unwrap();
    let file = fs::OpenOptions::new()
        .write(true)
        .open(state_dir(root.path(), 7).join("player-state.json"))
        .unwrap();
    file.set_len(MAX_BYTES + 1).unwrap();
    assert!(
        load(root.path(), 7)
            .unwrap_err()
            .to_string()
            .contains("size limit")
    );
    let mut invalid = state(7);
    invalid.progress.location.page = Page::Collection {
        kind: "sessions".into(),
        id: "../tokens".into(),
        title: String::new(),
    };
    assert!(invalid.validate(7).is_err());
}
