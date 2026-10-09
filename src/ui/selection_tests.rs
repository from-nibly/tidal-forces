use super::super::tests::{fixture, track};
use super::*;

fn draw(
    ctx: &egui::Context,
    app: &mut App,
    modifiers: egui::Modifiers,
    events: Vec<egui::Event>,
    search: bool,
) -> egui::FullOutput {
    ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                vec2(620., 450.),
            )),
            modifiers,
            events,
            ..Default::default()
        },
        |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                if search {
                    app.toolbar(ui);
                }
                egui::ScrollArea::vertical().show(ui, |ui| app.track_list(ui));
            });
        },
    )
}
fn point(output: &egui::FullOutput, label: &str) -> egui::Pos2 {
    output
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::Shape::Text(text) if text.galley.job.text == label => {
                Some(text.pos + vec2(4., 4.))
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("Missing {label}"))
}
fn click(ctx: &egui::Context, app: &mut App, pos: egui::Pos2, modifiers: egui::Modifiers) {
    for pressed in [true, false] {
        draw(
            ctx,
            app,
            modifiers,
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers,
                },
            ],
            false,
        );
    }
}
fn key(key: egui::Key, modifiers: egui::Modifiers) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers,
    }
}
fn copied(output: &egui::FullOutput) -> Vec<&str> {
    output
        .platform_output
        .commands
        .iter()
        .filter_map(|command| match command {
            egui::OutputCommand::CopyText(text) => Some(text.as_str()),
            _ => None,
        })
        .collect()
}
fn rows(app: &App) -> Vec<usize> {
    app.track_selection.rows.iter().copied().collect()
}
fn tracks() -> Vec<Track> {
    [1, 1, 2, 3]
        .into_iter()
        .enumerate()
        .map(|(i, id)| Track {
            title: format!("Occurrence {i}"),
            ..track(id)
        })
        .collect()
}

#[test]
fn modifier_selection_copies_duplicates_without_playing_and_bulk_queue_keeps_order() {
    let (ctx, mut app, _events, mut requests) = fixture();
    app.tracks = tracks();
    draw(&ctx, &mut app, egui::Modifiers::NONE, vec![], false);
    let output = draw(&ctx, &mut app, egui::Modifiers::NONE, vec![], false);
    let positions: Vec<_> = (0..4)
        .map(|i| point(&output, &format!("Occurrence {i}")))
        .collect();
    click(&ctx, &mut app, positions[0], egui::Modifiers::NONE);
    click(&ctx, &mut app, positions[1], egui::Modifiers::CTRL);
    assert_eq!(rows(&app), [0, 1]);
    click(&ctx, &mut app, positions[3], egui::Modifiers::SHIFT);
    assert_eq!(rows(&app), [1, 2, 3]);
    click(&ctx, &mut app, positions[0], egui::Modifiers::CTRL);
    click(&ctx, &mut app, positions[2], egui::Modifiers::CTRL);
    assert_eq!(rows(&app), [0, 1, 3]);
    // Fast repeated modified clicks must toggle selection, never double-click-play.
    click(&ctx, &mut app, positions[2], egui::Modifiers::CTRL);
    click(&ctx, &mut app, positions[2], egui::Modifiers::CTRL);
    assert_eq!(rows(&app), [0, 1, 3]);
    let output = draw(
        &ctx,
        &mut app,
        egui::Modifiers::CTRL,
        vec![egui::Event::Copy],
        false,
    );
    assert_eq!(
        copied(&output),
        [
            "https://tidal.com/browse/track/1\nhttps://tidal.com/browse/track/1\nhttps://tidal.com/browse/track/3"
        ]
    );
    app.queue.add(track(8), false).unwrap();
    app.apply_selection_action(&ctx, SelectionAction::Next);
    assert_eq!(
        app.queue
            .manual()
            .iter()
            .map(|entry| entry.track.id)
            .collect::<Vec<_>>(),
        [1, 1, 3, 8]
    );
    assert_eq!(
        app.queue
            .manual()
            .iter()
            .map(|entry| entry.occurrence)
            .collect::<std::collections::HashSet<_>>()
            .len(),
        4
    );
    assert!(app.queue.current().is_none());
    assert!(requests.try_recv().is_err());
}

#[test]
fn table_keyboard_ranges_and_clipboard_do_not_steal_text_edit_shortcuts() {
    let (ctx, mut app, _events, mut requests) = fixture();
    app.tracks = tracks();
    draw(&ctx, &mut app, egui::Modifiers::NONE, vec![], false);
    let output = draw(&ctx, &mut app, egui::Modifiers::NONE, vec![], false);
    click(
        &ctx,
        &mut app,
        point(&output, "Occurrence 1"),
        egui::Modifiers::NONE,
    );
    draw(
        &ctx,
        &mut app,
        egui::Modifiers::SHIFT,
        vec![key(egui::Key::ArrowDown, egui::Modifiers::SHIFT)],
        false,
    );
    assert_eq!(rows(&app), [1, 2]);
    draw(&ctx, &mut app, egui::Modifiers::NONE, vec![], false);
    draw(
        &ctx,
        &mut app,
        egui::Modifiers::CTRL,
        vec![key(egui::Key::A, egui::Modifiers::CTRL)],
        false,
    );
    assert_eq!(rows(&app), [0, 1, 2, 3]);
    app.track_selection.select(0, false, false);
    app.query = "An editable search".into();
    app.track_selection.pending_focus = Some(0);
    app.focus_search = true;
    draw(&ctx, &mut app, egui::Modifiers::NONE, vec![], true);
    assert_eq!(
        ctx.memory(|memory| memory.focused()),
        Some(egui::Id::new("global-search"))
    );
    let command = egui::Modifiers {
        ctrl: true,
        command: true,
        ..egui::Modifiers::NONE
    };
    draw(
        &ctx,
        &mut app,
        command,
        vec![key(egui::Key::A, command)],
        true,
    );
    let output = draw(
        &ctx,
        &mut app,
        egui::Modifiers::CTRL,
        vec![egui::Event::Copy],
        true,
    );
    assert_eq!(copied(&output), ["An editable search"]);
    assert_eq!(rows(&app), [0]);
    assert!(requests.try_recv().is_err());
}

#[test]
fn selection_does_not_change_raw_playlist_removal_identity_or_revision() {
    let (ctx, mut app, _events, mut requests) = fixture();
    app.tracks = tracks();
    app.playlist_page = Some(PlaylistPage {
        playlist: Playlist {
            uuid: "fixture-list".into(),
            title: "Fixture".into(),
            ..Default::default()
        },
        etag: "revision-1".into(),
        editable: true,
        rows: [100, 102, 104, 107]
            .into_iter()
            .zip(app.tracks.iter().cloned())
            .collect(),
        next_offset: 108,
        more: true,
    });
    draw(&ctx, &mut app, egui::Modifiers::NONE, vec![], false);
    app.track_selection.select(0, false, false);
    let output = draw(&ctx, &mut app, egui::Modifiers::NONE, vec![], false);
    let pos = point(&output, "Occurrence 1");
    for pressed in [true, false] {
        draw(
            &ctx,
            &mut app,
            egui::Modifiers::NONE,
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Secondary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
            false,
        );
    }
    let output = draw(&ctx, &mut app, egui::Modifiers::NONE, vec![], false);
    click(
        &ctx,
        &mut app,
        point(&output, "Remove from this playlist…"),
        egui::Modifiers::NONE,
    );
    let removal = app.removal.as_ref().unwrap();
    assert_eq!(removal.index, 102);
    assert_eq!(removal.track.id, 1);
    assert_eq!(removal.etag, "revision-1");
    assert_eq!(removal.playlist, "fixture-list");
    assert!(
        requests.try_recv().is_err(),
        "Removal must still wait for confirmation"
    );
}

#[test]
fn loaded_select_all_and_checkboxes_never_start_playback_or_fetch_more() {
    let (ctx, mut app, _events, mut requests) = fixture();
    app.tracks = tracks();
    app.more = true;
    draw(&ctx, &mut app, egui::Modifiers::NONE, vec![], false);
    let output = draw(&ctx, &mut app, egui::Modifiers::NONE, vec![], false);
    let header = point(&output, "#");
    click(&ctx, &mut app, pos2(16., header.y), egui::Modifiers::NONE);
    assert_eq!(rows(&app), [0, 1, 2, 3]);
    let row = point(&output, "Occurrence 1");
    let check = pos2(16., row.y + 15.);
    click(&ctx, &mut app, check, egui::Modifiers::NONE);
    assert_eq!(rows(&app), [0, 2, 3]);
    click(&ctx, &mut app, check, egui::Modifiers::NONE);
    assert_eq!(rows(&app), [0, 1, 2, 3]);
    assert!(app.queue.current().is_none());
    assert!(requests.try_recv().is_err());
}

#[test]
fn keyboard_range_navigation_reaches_unrendered_rows_without_losing_focus() {
    let (ctx, mut app, _events, mut requests) = fixture();
    app.tracks = (1..=10000).map(track).collect();
    draw(&ctx, &mut app, egui::Modifiers::NONE, vec![], false);
    let output = draw(&ctx, &mut app, egui::Modifiers::NONE, vec![], false);
    let first = app.tracks[0].title.clone();
    click(
        &ctx,
        &mut app,
        point(&output, &first),
        egui::Modifiers::NONE,
    );
    draw(
        &ctx,
        &mut app,
        egui::Modifiers::SHIFT,
        vec![key(egui::Key::End, egui::Modifiers::SHIFT)],
        false,
    );
    assert_eq!(app.track_selection.rows.len(), 10000);
    let mut output = draw(&ctx, &mut app, egui::Modifiers::NONE, vec![], false);
    for _ in 0..30 {
        output = draw(&ctx, &mut app, egui::Modifiers::NONE, vec![], false);
    }
    assert!(app.track_selection.pending_focus.is_none());
    assert!(ctx.memory(|memory| memory.focused().is_some()));
    assert!(output.shapes.len() < 1000);
    point(&output, &app.tracks[9999].title);
    assert!(requests.try_recv().is_err());
}

#[test]
fn selection_survives_append_but_not_replacement_refresh_or_account_change() {
    let (ctx, mut app, events, _requests) = fixture();
    app.tracks = tracks();
    draw(&ctx, &mut app, egui::Modifiers::NONE, vec![], false);
    app.track_selection.select(1, false, false);
    events
        .send(Event::Tracks {
            generation: app.generation,
            tracks: vec![track(9)],
            append: true,
        })
        .unwrap();
    app.events();
    draw(&ctx, &mut app, egui::Modifiers::NONE, vec![], false);
    assert_eq!(rows(&app), [1]);
    events
        .send(Event::Tracks {
            generation: app.generation,
            tracks: tracks(),
            append: false,
        })
        .unwrap();
    app.events();
    assert!(rows(&app).is_empty());
    app.track_selection.select(1, false, false);
    app.generation += 1;
    draw(&ctx, &mut app, egui::Modifiers::NONE, vec![], false);
    assert!(rows(&app).is_empty());
    app.track_selection.select(1, false, false);
    app.account = Some(8);
    draw(&ctx, &mut app, egui::Modifiers::NONE, vec![], false);
    assert!(rows(&app).is_empty());
}
