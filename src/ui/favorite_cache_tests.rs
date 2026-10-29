use super::*;
use crate::{
    api::Search,
    ui::tests::{fixture, track},
};

fn page(ids: &[u64], next: usize, total: u64) -> LibraryPage {
    LibraryPage {
        data: Search {
            tracks: ids.iter().copied().map(track).collect(),
            ..Default::default()
        },
        next_offset: next,
        more: (next as u64) < total,
        total: Some(total),
    }
}
fn request(
    app: &mut App,
    requests: &mut tokio::sync::mpsc::Receiver<Request>,
) -> (u64, FavoriteKind, usize) {
    app.favorite_cache_tick();
    match requests.try_recv().unwrap() {
        Request::FavoriteMembership {
            user: 7,
            serial,
            kind,
            offset,
        } => (serial, kind, offset),
        _ => panic!("expected membership read"),
    }
}
#[test]
fn prefetch_publishes_only_complete_unique_reads_without_touching_playback_or_browse() {
    let (_, mut app, _, mut requests) = fixture();
    app.tracks = vec![track(99)];
    app.queue_mut()
        .start(vec![track(88)], 0, "Current".into(), None)
        .unwrap();
    app.paused = false;
    let revision = app.queue.revision();
    app.start_favorite_cache();
    let (serial, kind, offset) = request(&mut app, &mut requests);
    app.favorite_cache_tick();
    assert!(requests.try_recv().is_err());
    app.favorite_cache_page(7, serial, kind, offset, Ok(page(&[1, 2], 2, 3)));
    assert_eq!(app.favorites.state(Favorite::new(kind, 1)), None);
    assert_eq!(app.favorites.state(Favorite::new(kind, 9)), None);
    let (serial, kind, offset) = request(&mut app, &mut requests);
    assert_eq!(offset, 2);
    app.favorite_cache_page(7, serial, kind, offset, Ok(page(&[3], 3, 3)));
    assert_eq!(app.favorites.state(Favorite::new(kind, 1)), Some(true));
    assert_eq!(app.favorites.state(Favorite::new(kind, 9)), Some(false));
    for expected in [FavoriteKind::Albums, FavoriteKind::Artists] {
        let (serial, kind, offset) = request(&mut app, &mut requests);
        assert_eq!(kind, expected);
        app.favorite_cache_page(7, serial, kind, offset, Ok(page(&[], 0, 0)));
        assert_eq!(app.favorites.state(Favorite::new(kind, 9)), Some(false));
    }
    assert!(app.favorite_cache.job.is_none());
    assert_eq!(app.tracks[0].id, 99);
    assert_eq!(app.queue.revision(), revision);
    assert!(!app.paused);
}
#[test]
fn invalid_or_incomplete_membership_pages_never_establish_absence() {
    for case in 0..8 {
        let (_, mut app, _, mut requests) = fixture();
        app.start_favorite_cache();
        let (serial, kind, offset) = request(&mut app, &mut requests);
        let mut p = page(&[1], 1, 1);
        match case {
            0 => p.total = None,
            1 => p.total = Some(50_001),
            2 => p.next_offset = 2, // unavailable raw row
            3 => p = page(&[1, 1], 2, 2),
            4 => p = page(&[0], 1, 1),
            5 => p.more = true,
            6 => p = page(&[], 0, 2),
            _ => p = page(&(1..=101).collect::<Vec<_>>(), 101, 101),
        }
        app.favorite_cache_page(7, serial, kind, offset, Ok(p));
        if matches!(case, 0 | 3 | 4) {
            assert!(
                app.favorite_cache.job.is_some(),
                "continue with other kinds"
            );
            assert_eq!(app.favorite_cache.partial.len(), 1);
        } else {
            assert!(app.favorite_cache.job.is_none(), "case {case}");
            let message = app.favorite_cache.status.as_deref().unwrap();
            let reason = match case {
                1 => "50,000-entry",
                6 => "empty unfinished",
                7 => "page range",
                _ => "page boundaries",
            };
            assert!(message.contains(reason), "case {case}: {message}");
            assert!(message.contains("tracks"));
        }
        assert_eq!(app.favorites.state(Favorite::new(kind, 9)), None);
    }
}
#[test]
fn unavailable_rows_keep_raw_progress_and_known_items_without_guessing_absence() {
    for first_ids in [vec![1], vec![]] {
        let (_, mut app, _, mut requests) = fixture();
        app.start_favorite_cache();
        let (serial, kind, offset) = request(&mut app, &mut requests);
        app.favorite_cache_page(7, serial, kind, offset, Ok(page(&first_ids, 2, 3)));
        assert_eq!(app.favorite_cache.job.as_ref().unwrap().offset, 2);
        let (serial, kind, offset) = request(&mut app, &mut requests);
        // A separate browse read must not leave implicit absence enabled when
        // the newer membership snapshot cannot identify every item.
        app.favorites.observe(Favorite::new(kind, 77));
        app.favorites.mark_complete(kind);
        app.favorite_cache_page(7, serial, kind, offset, Ok(page(&[2], 3, 3)));
        assert_eq!(app.favorites.state(Favorite::new(kind, 2)), Some(true));
        assert_eq!(app.favorites.state(Favorite::new(kind, 77)), Some(true));
        assert_eq!(app.favorites.state(Favorite::new(kind, 9)), None);
        for _ in 0..2 {
            let (serial, kind, offset) = request(&mut app, &mut requests);
            app.favorite_cache_page(7, serial, kind, offset, Ok(page(&[], 0, 0)));
            assert_eq!(app.favorites.state(Favorite::new(kind, 9)), Some(false));
        }
        let message = app.favorite_cache.status.as_deref().unwrap();
        assert!(message.contains("Saved tracks: some entries could not be identified"));
        assert!(message.contains("unchecked items remain unknown"));
    }
}

#[test]
fn missing_totals_allow_observed_presence_but_never_absence() {
    let (_, mut app, _, mut requests) = fixture();
    app.start_favorite_cache();
    let (serial, kind, offset) = request(&mut app, &mut requests);
    let mut first = page(&(1..=100).collect::<Vec<_>>(), 100, 101);
    first.total = None;
    app.favorite_cache_page(7, serial, kind, offset, Ok(first));
    let (serial, kind, offset) = request(&mut app, &mut requests);
    let mut last = page(&[101], 101, 101);
    last.total = None;
    app.favorite_cache_page(7, serial, kind, offset, Ok(last));
    assert_eq!(app.favorites.state(Favorite::new(kind, 101)), Some(true));
    assert_eq!(app.favorites.state(Favorite::new(kind, 102)), None);
    assert!(app.favorite_cache.partial[0].contains("did not provide a total"));
}

#[test]
fn changing_counts_and_errors_abort_the_snapshot() {
    for case in 0..2 {
        let (_, mut app, _, mut requests) = fixture();
        app.start_favorite_cache();
        let (serial, kind, offset) = request(&mut app, &mut requests);
        app.favorite_cache_page(7, serial, kind, offset, Ok(page(&[1], 1, 2)));
        let (serial, kind, offset) = request(&mut app, &mut requests);
        let result = match case {
            0 => Ok(page(&[2], 2, 3)),
            _ => Err("Synthetic failure".into()),
        };
        app.favorite_cache_page(7, serial, kind, offset, result);
        assert!(app.favorite_cache.job.is_none());
        assert_eq!(app.favorites.state(Favorite::new(kind, 9)), None);
    }
}
#[test]
fn duplicate_entries_preserve_presence_not_absence_and_whole_repeats_stop_the_kind() {
    for case in 0..3 {
        let (_, mut app, _, mut requests) = fixture();
        app.start_favorite_cache();
        let (serial, kind, offset) = request(&mut app, &mut requests);
        if case == 0 {
            app.favorite_cache_page(7, serial, kind, offset, Ok(page(&[1, 1], 2, 2)));
        } else {
            let total = if case == 1 { 4 } else { 6 };
            app.favorite_cache_page(7, serial, kind, offset, Ok(page(&[1, 2], 2, total)));
            let (serial, kind, offset) = request(&mut app, &mut requests);
            let ids = if case == 1 { vec![2, 3] } else { vec![1, 2] };
            app.favorite_cache_page(7, serial, kind, offset, Ok(page(&ids, 4, total)));
        }
        assert_eq!(
            app.favorites.state(Favorite::new(FavoriteKind::Tracks, 1)),
            Some(true)
        );
        assert_eq!(
            app.favorites.state(Favorite::new(FavoriteKind::Tracks, 9)),
            None
        );
        if case == 1 {
            assert_eq!(
                app.favorites.state(Favorite::new(FavoriteKind::Tracks, 3)),
                Some(true)
            );
        }
        assert!(app.favorite_cache.partial[0].contains("repeated entries"));
        // Even a repeated page with `more=true` advances to Albums, not another
        // request for the same track data. Other kinds remain independently useful.
        for expected in [FavoriteKind::Albums, FavoriteKind::Artists] {
            let (serial, kind, offset) = request(&mut app, &mut requests);
            assert_eq!(kind, expected);
            app.favorite_cache_page(7, serial, kind, offset, Ok(page(&[], 0, 0)));
        }
        app.favorite_cache_tick();
        assert!(requests.try_recv().is_err());
        assert!(
            app.favorite_cache
                .status
                .as_deref()
                .unwrap()
                .contains("unchecked items remain unknown")
        );
    }
}

#[test]
fn cancellation_mutation_and_new_accounts_reject_late_results() {
    let (_, mut app, _, mut requests) = fixture();
    app.start_favorite_cache();
    let (serial, kind, offset) = request(&mut app, &mut requests);
    app.set_favorite(Favorite::new(kind, 1), false);
    assert!(app.favorite_cache.job.is_none());
    app.favorite_cache_page(7, serial, kind, offset, Ok(page(&[1], 1, 1)));
    assert_eq!(app.favorites.state(Favorite::new(kind, 1)), None);
    app.start_favorite_cache(); // pending write forbids a new read
    assert!(app.favorite_cache.job.is_none());
    app.favorites.finish(Favorite::new(kind, 1), Some(false));
    app.favorite_cache_page(7, serial, kind, offset, Ok(page(&[1], 1, 1)));
    assert_eq!(app.favorites.state(Favorite::new(kind, 1)), Some(false));
    app.account = Some(8);
    app.start_favorite_cache();
    app.favorite_cache_page(7, serial, kind, offset, Ok(page(&[1], 1, 1)));
    assert_eq!(app.favorites.state(Favorite::new(kind, 1)), None);
    app.account = Some(7);
    app.start_favorite_cache();
    app.favorite_cache_page(7, serial, kind, offset, Ok(page(&[1], 1, 1)));
    assert_eq!(app.favorites.state(Favorite::new(kind, 1)), None);
    app.stop_favorite_cache("Stopped");
    assert!(app.favorite_cache.job.is_none());
}
#[test]
fn a_write_finishing_after_a_new_check_started_cancels_that_check() {
    let (_, mut app, events, mut requests) = fixture();
    app.start_favorite_cache();
    let (serial, kind, offset) = request(&mut app, &mut requests);
    let item = Favorite::new(kind, 1);
    events
        .send(Event::Favorite {
            user: 7,
            item,
            result: Ok(false),
        })
        .unwrap();
    app.events();
    assert!(app.favorite_cache.job.is_none());
    app.favorite_cache_page(7, serial, kind, offset, Ok(page(&[1], 1, 1)));
    assert_eq!(app.favorites.state(item), Some(false));
}

#[test]
fn manual_library_holes_unknown_totals_and_duplicates_do_not_guess_absence() {
    for case in 0..4 {
        let (_, mut app, events, _) = fixture();
        let mut p = page(&[1, 2], 2, 2);
        match case {
            0 => {
                p.data.tracks.pop();
            }
            1 => p.total = None,
            2 => p.data.tracks[1].id = 1,
            _ => {}
        }
        events
            .send(Event::Library {
                generation: app.generation,
                kind: FavoriteKind::Tracks,
                page: p,
                append: false,
            })
            .unwrap();
        app.events();
        assert_eq!(
            app.favorites.state(Favorite::new(FavoriteKind::Tracks, 9)),
            if case == 3 { Some(false) } else { None }
        );
    }
    let (_, mut app, events, _) = fixture();
    for (p, append) in [
        (page(&[1], 1, 2), false),
        (page(&[2], 2, 3), true),
        (page(&[3], 3, 3), true),
    ] {
        events
            .send(Event::Library {
                generation: app.generation,
                kind: FavoriteKind::Tracks,
                page: p,
                append,
            })
            .unwrap();
        app.events();
    }
    assert_eq!(
        app.favorites.state(Favorite::new(FavoriteKind::Tracks, 9)),
        None
    );
}

#[test]
fn background_status_change_cannot_turn_a_menu_click_into_a_write() {
    let (ctx, mut app, _, _) = fixture();
    let item = Favorite::new(FavoriteKind::Tracks, 1);
    let mut action = None;
    let mut point = egui::Pos2::ZERO;
    let draw = |favorites: &Favorites,
                events,
                action: &mut Option<(Favorite, bool)>,
                point: &mut egui::Pos2| {
        ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    vec2(400., 220.),
                )),
                events,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    *point = ui.cursor().min + vec2(14., 14.);
                    crate::ui::library::favorite_button(ui, favorites, item, action);
                });
            },
        )
    };
    let _ = draw(&app.favorites, vec![], &mut action, &mut point);
    let pos = point;
    let button = |pressed| egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    let _ = draw(
        &app.favorites,
        vec![egui::Event::PointerMoved(pos), button(true)],
        &mut action,
        &mut point,
    );
    app.favorites.observe(item);
    let _ = draw(&app.favorites, vec![button(false)], &mut action, &mut point);
    assert!(action.is_none());
    assert!(egui::Popup::is_any_open(&ctx));
    let output = draw(&app.favorites, vec![], &mut action, &mut point);
    assert!(output.shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Text(text) if text.galley.text() == "Favorite actions")));
    assert!(action.is_none());
}

#[test]
fn background_reads_yield_to_loading_buffering_and_backpressure() {
    let (_, mut app, _, mut requests) = fixture();
    app.start_favorite_cache();
    app.loading = true;
    app.favorite_cache_tick();
    assert!(requests.try_recv().is_err());
    app.loading = false;
    app.buffering = true;
    app.favorite_cache_tick();
    assert!(requests.try_recv().is_err());
    app.buffering = false;
    for _ in 0..32 {
        app.backend.tx.try_send(Request::CancelPkce).unwrap();
    }
    app.favorite_cache_tick();
    assert!(!app.favorite_cache.job.as_ref().unwrap().in_flight);
    assert!(app.error.is_none());
    drop(requests);
    app.favorite_cache_tick();
    assert!(app.favorite_cache.job.is_none());
}
