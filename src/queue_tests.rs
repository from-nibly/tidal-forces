use super::*;
fn track(id: u64) -> Track {
    Track {
        id,
        title: format!("Track {id}"),
        duration: 120,
        ..Default::default()
    }
}
fn queue() -> Queue {
    let mut queue = Queue::default();
    queue
        .start(vec![track(1), track(2), track(3)], 0, "Source".into(), None)
        .unwrap();
    queue
}
fn upcoming(queue: &Queue) -> Vec<u64> {
    queue
        .context_upcoming()
        .map(|entry| entry.track.id)
        .collect()
}

#[test]
fn bulk_queue_addition_is_ordered_duplicate_safe_and_atomic() {
    for next in [false, true] {
        let mut q = queue();
        q.add(track(8), false).unwrap();
        let current = q.current_entry().unwrap().occurrence;
        let source = upcoming(&q);
        q.add_many(vec![track(9), track(9), track(10)], next)
            .unwrap();
        assert_eq!(
            q.manual().iter().map(|e| e.track.id).collect::<Vec<_>>(),
            if next {
                vec![9, 9, 10, 8]
            } else {
                vec![8, 9, 9, 10]
            }
        );
        assert_eq!(
            q.manual()
                .iter()
                .map(|e| e.occurrence)
                .collect::<std::collections::HashSet<_>>()
                .len(),
            4
        );
        assert_eq!(q.current_entry().unwrap().occurrence, current);
        assert_eq!(upcoming(&q), source);
        let before = serde_json::to_value(&q).unwrap();
        assert!(
            q.add_many(vec![track(2), track(0), track(3)], next)
                .is_err()
        );
        assert_eq!(serde_json::to_value(&q).unwrap(), before);
        assert!(q.add_many(vec![track(2); MAX_ENTRIES], next).is_err());
        assert_eq!(serde_json::to_value(&q).unwrap(), before);
        q.add_many(Vec::new(), next).unwrap();
        assert_eq!(serde_json::to_value(&q).unwrap(), before);
        q.next_id = u64::MAX - 1;
        let before = serde_json::to_value(&q).unwrap();
        assert!(q.add_many(vec![track(2), track(3)], next).is_err());
        assert_eq!(serde_json::to_value(&q).unwrap(), before);
        assert!(q.validate().is_ok());
    }
}

#[test]
fn explicit_collection_start_keeps_every_occurrence_and_preserves_manual_order() {
    for shuffle in [false, true] {
        let mut q = queue();
        q.add(track(8), false).unwrap();
        q.add(track(8), false).unwrap();
        let manual: Vec<_> = q.manual().iter().map(|entry| entry.occurrence).collect();
        q.start_collection(
            vec![track(4), track(5), track(4), track(6)],
            "Collection".into(),
            Some(Continuation::Album { id: 42, offset: 4 }),
            shuffle,
        )
        .unwrap();
        assert_eq!(q.shuffled(), shuffle);
        assert_eq!(
            q.manual()
                .iter()
                .map(|entry| entry.occurrence)
                .collect::<Vec<_>>(),
            manual
        );
        let mut ids = vec![q.current().unwrap().id];
        ids.extend(upcoming(&q));
        ids.sort();
        assert_eq!(ids, [4, 4, 5, 6]);
        let mut occurrences = vec![q.current_entry().unwrap().occurrence];
        occurrences.extend(q.context_upcoming().map(|entry| entry.occurrence));
        assert_eq!(occurrences.into_iter().collect::<HashSet<_>>().len(), 4);
        q.append(vec![track(7), track(8)], None).unwrap();
        if shuffle {
            q.toggle_shuffle();
        }
        let current = q.current_entry().unwrap().occurrence;
        assert_eq!(
            q.context_upcoming()
                .map(|entry| entry.occurrence)
                .collect::<Vec<_>>(),
            q.context
                .iter()
                .filter(|entry| entry.occurrence != current)
                .map(|entry| entry.occurrence)
                .collect::<Vec<_>>()
        );
        assert_eq!(
            q.manual()
                .iter()
                .map(|entry| entry.occurrence)
                .collect::<Vec<_>>(),
            manual
        );
        assert!(q.validate().is_ok());
        let revision = q.revision();
        assert!(
            q.start_collection(vec![], "Invalid".into(), None, true)
                .is_err()
        );
        assert_eq!(q.revision(), revision);
        assert_eq!(q.current_entry().unwrap().occurrence, current);
    }
}

#[test]
fn manual_duplicates_are_distinct_and_context_replacement_keeps_manual_intent() {
    let mut q = queue();
    q.add(track(8), false).unwrap();
    q.add(track(8), true).unwrap();
    let first = q.manual()[0].occurrence;
    let second = q.manual()[1].occurrence;
    assert_ne!(first, second);
    q.start(vec![track(4), track(5)], 0, "New source".into(), None)
        .unwrap();
    assert_eq!(q.manual().len(), 2);
    assert!(q.move_manual(second, 0));
    assert_eq!(q.manual()[0].occurrence, second);
    assert!(q.remove_manual(first));
    assert!(q.advance(false));
    assert_eq!(q.current_entry().unwrap().occurrence, second);
    assert!(q.advance(false));
    assert_eq!(q.current().unwrap().id, 5);
    q.validate().unwrap();
}

#[test]
fn repeat_one_only_affects_natural_end_and_repeat_source_uses_new_occurrences() {
    let mut q = queue();
    let first = q.current_entry().unwrap().occurrence;
    q.set_repeat(Repeat::Track);
    assert!(q.advance(true));
    assert_eq!(q.current_entry().unwrap().occurrence, first);
    assert!(q.advance(false));
    assert_eq!(q.current().unwrap().id, 2);
    q.set_repeat(Repeat::Context);
    q.advance(true);
    q.advance(true);
    assert_eq!(q.current().unwrap().id, 1);
    assert_ne!(q.current_entry().unwrap().occurrence, first);
    assert!(q.previous());
    assert_eq!(q.current().unwrap().id, 3);
    q.validate().unwrap();
    assert!(q.advance(false));
    assert_eq!(q.current().unwrap().id, 1);
    q.validate().unwrap();
}

#[test]
fn previous_retraces_manual_and_context_without_consuming_upcoming_twice() {
    let mut q = queue();
    q.add(track(8), false).unwrap();
    q.advance(false);
    q.advance(false);
    assert_eq!(q.current().unwrap().id, 2);
    q.previous();
    assert_eq!(q.current().unwrap().id, 8);
    q.previous();
    assert_eq!(q.current().unwrap().id, 1);
    assert_eq!(upcoming(&q), [8, 2, 3]);
    q.advance(false);
    q.advance(false);
    q.advance(false);
    assert_eq!(q.current().unwrap().id, 3);
    assert!(!q.advance(false));
    q.validate().unwrap();
}

#[test]
fn shuffle_is_reversible_and_never_reorders_manual_entries_or_reshuffles_existing_pages() {
    let mut q = queue();
    q.add(track(9), false).unwrap();
    q.add(track(10), false).unwrap();
    q.toggle_shuffle();
    let order = upcoming(&q);
    q.append(vec![track(4), track(5)], None).unwrap();
    let retained: Vec<_> = upcoming(&q).into_iter().filter(|id| *id <= 3).collect();
    assert_eq!(retained, order);
    q.toggle_shuffle();
    assert_eq!(upcoming(&q), [2, 3, 4, 5]);
    assert_eq!(
        q.manual()
            .iter()
            .map(|entry| entry.track.id)
            .collect::<Vec<_>>(),
        [9, 10]
    );
    q.validate().unwrap();
}

#[test]
fn incomplete_context_does_not_repeat_and_clear_actions_have_separate_scopes() {
    let mut q = Queue::default();
    q.start(
        vec![track(1)],
        0,
        "Album".into(),
        Some(Continuation::Album { id: 1, offset: 1 }),
    )
    .unwrap();
    q.set_repeat(Repeat::Context);
    assert!(!q.advance(true));
    q.append(vec![track(2)], None).unwrap();
    assert!(q.advance(true));
    q.add(track(9), false).unwrap();
    q.clear_context();
    assert_eq!(q.current().unwrap().id, 2);
    assert_eq!(q.manual().len(), 1);
    q.clear_manual();
    assert!(!q.advance(false));
    q.clear();
    assert!(q.current().is_none());
    q.validate().unwrap();
}

#[test]
fn jump_and_remove_use_occurrences_not_tidal_ids() {
    let mut q = Queue::default();
    q.start(
        vec![track(1), track(1), track(1)],
        0,
        "Duplicates".into(),
        None,
    )
    .unwrap();
    let last = q.context_upcoming().last().unwrap().occurrence;
    assert!(q.jump_context(last));
    assert_eq!(q.current_entry().unwrap().occurrence, last);
    assert_eq!(q.upcoming_len(), 0);
    q.add(track(1), false).unwrap();
    q.add(track(1), false).unwrap();
    let id = q.manual()[1].occurrence;
    assert!(q.jump_manual(id));
    assert_eq!(q.current_entry().unwrap().occurrence, id);
    assert_eq!(q.manual().len(), 1);
    q.validate().unwrap();
}

#[test]
fn round_trip_rejects_corrupt_indices_colliding_occurrences_and_invalid_continuations() {
    let q = queue();
    let saved = serde_json::to_value(&q).unwrap();
    let restored: Queue = serde_json::from_value(saved.clone()).unwrap();
    restored.validate().unwrap();
    for (field, value) in [
        ("upcoming", serde_json::json!([9000])),
        ("next_id", serde_json::json!(1)),
        (
            "continuation",
            serde_json::json!({"Playlist":{"id":"../sessions","etag":"x","offset":1}}),
        ),
    ] {
        let mut invalid = saved.clone();
        invalid[field] = value;
        let invalid: Queue = serde_json::from_value(invalid).unwrap();
        assert!(invalid.validate().is_err());
    }
}

#[test]
fn mixed_queue_operations_preserve_identity_invariants_across_repeat_and_backtracking() {
    use rand::SeedableRng;
    for seed in 0..20 {
        let mut rng = rand::rngs::StdRng::seed_from_u64(seed);
        let mut q = queue();
        for step in 0..500 {
            match rng.random_range(0..12) {
                0 => {
                    q.start(
                        vec![track(1), track(1), track(2)],
                        rng.random_range(0..3),
                        "New source".into(),
                        None,
                    )
                    .unwrap();
                }
                1 => {
                    q.advance(rng.random());
                }
                2 => {
                    q.previous();
                }
                3 => {
                    q.add(track(8), rng.random()).unwrap();
                }
                4 => q.toggle_shuffle(),
                5 => q.set_repeat(q.repeat().next()),
                6 => {
                    if let Some(entry) = q.manual().front() {
                        q.remove_manual(entry.occurrence);
                    }
                }
                7 => {
                    if let Some(entry) = q.manual().front() {
                        q.move_manual(entry.occurrence, q.manual().len() - 1);
                    }
                }
                8 => {
                    let id = q.context_upcoming().last().map(|entry| entry.occurrence);
                    if let Some(id) = id {
                        q.jump_context(id);
                    }
                }
                9 => {
                    if let Some(entry) = q.manual().back() {
                        q.jump_manual(entry.occurrence);
                    }
                }
                10 => q.clear_context(),
                _ => q.clear_manual(),
            }
            q.validate()
                .unwrap_or_else(|error| panic!("seed {seed}, step {step}: {error}; {q:?}"));
        }
    }
}

#[test]
fn favorites_continuation_refuses_duplicate_boundary_items_without_partial_append() {
    let mut q = queue();
    q.continuation = Some(Continuation::Favorites {
        offset: 3,
        total: Some(5),
    });
    assert!(q.append(vec![track(4), track(3)], None).is_err());
    assert_eq!(upcoming(&q), [2, 3]);
}
