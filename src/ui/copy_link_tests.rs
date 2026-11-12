use super::tests::{fixture, track};
use super::*;

fn copied(output: &egui::FullOutput) -> Vec<&str> {
    output
        .platform_output
        .commands
        .iter()
        .filter_map(|c| match c {
            egui::OutputCommand::CopyText(s) => Some(s.as_str()),
            _ => None,
        })
        .collect()
}
fn text_position(output: &egui::FullOutput, label: &str) -> egui::Pos2 {
    output
        .shapes
        .iter()
        .find_map(|s| {
            if let egui::Shape::Text(t) = &s.shape {
                (t.galley.job.text == label).then(|| t.pos + t.galley.size() * 0.5)
            } else {
                None
            }
        })
        .expect("Copy action must be visible")
}
fn pointer(pos: egui::Pos2, pressed: bool) -> Vec<egui::Event> {
    vec![
        egui::Event::PointerMoved(pos),
        egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        },
    ]
}
#[test]
fn direct_track_menu_copy_does_not_enqueue_navigate_or_start_playback() {
    for (label, expected) in [
        ("Copy TIDAL link", "https://tidal.com/browse/track/42"),
        ("Copy album link", "https://tidal.com/browse/album/12"),
        ("Copy artist link", "https://tidal.com/browse/artist/34"),
    ] {
        let (ctx, mut app, _, mut requests) = fixture();
        let mut song = track(42);
        song.album.id = 12;
        song.artist.id = 34;
        app.queue_mut()
            .start(vec![song.clone()], 0, "Synthetic".into(), None)
            .unwrap();
        let before = (
            app.queue.revision(),
            app.play_generation,
            app.paused,
            app.page.clone(),
        );
        let mut action = None;
        let mut draw = |events| {
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        vec2(600., 660.),
                    )),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    let mut open = true;
                    egui::Window::new("Track actions")
                        .open(&mut open)
                        .show(ctx, |ui| track_menu(ui, &song, &mut action));
                },
            )
        };
        draw(vec![]);
        let output = draw(vec![]);
        let pos = text_position(&output, label);
        assert!(copied(&draw(pointer(pos, true))).is_empty());
        let output = draw(pointer(pos, false));
        assert_eq!(copied(&output), [expected]);
        assert!(action.is_none() && requests.try_recv().is_err());
        assert_eq!(
            (
                app.queue.revision(),
                app.play_generation,
                app.paused,
                app.page.clone()
            ),
            before
        );
    }
}
#[test]
fn now_playing_has_a_one_click_current_track_link() {
    let (ctx, mut app, _, mut requests) = fixture();
    app.queue_mut()
        .start(vec![track(42)], 0, "Synthetic".into(), None)
        .unwrap();
    let before = (
        app.queue.revision(),
        app.play_generation,
        app.paused,
        app.page.clone(),
    );
    let mut draw = |events| {
        ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    vec2(1000., 660.),
                )),
                events,
                ..Default::default()
            },
            |ctx| app.now_playing_view(ctx),
        )
    };
    draw(vec![]);
    let output = draw(vec![]);
    let pos = text_position(&output, "Copy TIDAL link");
    draw(pointer(pos, true));
    let output = draw(pointer(pos, false));
    assert_eq!(copied(&output), ["https://tidal.com/browse/track/42"]);
    assert!(requests.try_recv().is_err());
    assert_eq!(
        (
            app.queue.revision(),
            app.play_generation,
            app.paused,
            app.page.clone()
        ),
        before
    );
}
#[test]
fn collection_types_map_to_their_own_catalog_link_without_network_or_mutation() {
    for (page, expected) in [
        (Page::Artist(34), Link::Artist(34)),
        (
            Page::Collection {
                kind: "albums".into(),
                id: "12".into(),
                title: "Album".into(),
            },
            Link::Album(12),
        ),
        (
            Page::Collection {
                kind: "playlists".into(),
                id: "12345678-1234-1234-1234-123456789abc".into(),
                title: "Playlist".into(),
            },
            Link::Playlist("12345678-1234-1234-1234-123456789abc".into()),
        ),
        (
            Page::Mix {
                id: "mix_ABC".into(),
                title: "Mix".into(),
            },
            Link::Mix("mix_ABC".into()),
        ),
    ] {
        assert_eq!(page.public_link(), Some(expected));
    }
    for page in [
        Page::Home,
        Page::Search,
        Page::Settings,
        Page::Collection {
            kind: "unsupported".into(),
            id: "12".into(),
            title: "".into(),
        },
    ] {
        assert!(page.public_link().is_none());
    }
}
#[test]
fn invalid_link_buttons_do_not_overwrite_the_clipboard() {
    let ctx = egui::Context::default();
    let mut response = None;
    let mut draw = |events| {
        ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    vec2(600., 440.),
                )),
                events,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    response = Some(copy_link_button(ui, Link::Track(0), "Copy TIDAL link"));
                });
            },
        )
    };
    draw(vec![]);
    let output = draw(vec![]);
    let pos = text_position(&output, "Copy TIDAL link");
    draw(pointer(pos, true));
    let output = draw(pointer(pos, false));
    assert!(copied(&output).is_empty());
    assert!(!response.unwrap().enabled());
}
