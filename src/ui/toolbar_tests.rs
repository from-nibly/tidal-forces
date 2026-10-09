use super::*;

fn draw(
    ctx: &egui::Context,
    app: &mut App,
    width: f32,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                vec2(width, 400.),
            )),
            events,
            ..Default::default()
        },
        |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| app.toolbar(ui));
        },
    )
}

fn field(output: &egui::FullOutput) -> egui::Rect {
    output
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::Shape::Rect(rect) if rect.fill == Color32::from_rgb(24, 27, 31) => {
                Some(rect.rect)
            }
            _ => None,
        })
        .unwrap()
}

#[test]
fn search_is_compact_right_aligned_and_contains_its_icon_and_shortcut() {
    for width in [444., 666., 1000., 1240., 1920.] {
        let (ctx, mut app, _events, _requests) = tests::fixture();
        draw(&ctx, &mut app, width, vec![]);
        let output = draw(&ctx, &mut app, width, vec![]);
        let rect = field(&output);
        assert!(
            (rect.right() - (width - 8.)).abs() < 1.,
            "{width}: {rect:?}"
        );
        assert_eq!(rect.height(), 32.);
        assert_eq!(rect.width(), if width <= 1050. { 200. } else { 210. });
        for label in ["Search TIDAL", "Ctrl K"] {
            let text = output
                .shapes
                .iter()
                .find_map(|shape| match &shape.shape {
                    egui::Shape::Text(text) if text.galley.job.text == label => Some(text),
                    _ => None,
                })
                .unwrap();
            assert!(rect.contains_rect(egui::Rect::from_min_size(text.pos, text.galley.size())));
        }
        assert!(output.shapes.iter().any(|shape| matches!(&shape.shape,egui::Shape::Circle(circle) if circle.radius>3. && circle.radius<8. && rect.contains(circle.center))),"Search icon must be inside the field");
        assert_eq!(output.shapes.iter().any(|shape|matches!(&shape.shape,egui::Shape::Text(text) if text.galley.job.text=="Home")),width>1050.);
    }
}

#[test]
fn requested_search_focus_dismisses_only_the_narrow_queue_overlay() {
    for width in [444., 1000., 1240.] {
        let (ctx, mut app, _events, mut requests) = tests::fixture();
        app.queue_open = true;
        app.focus_search = true;
        draw(&ctx, &mut app, width, vec![]);
        assert_eq!(app.queue_open, width >= 1180.);
        assert_eq!(
            ctx.memory(|m| m.focused()),
            Some(egui::Id::new("global-search"))
        );
        assert!(requests.try_recv().is_err());
    }
}

#[test]
fn toolbar_breadcrumb_uses_the_current_route() {
    let (ctx, mut app, _events, _requests) = tests::fixture();
    for (page, label) in [
        (
            Page::Collection {
                kind: "playlists".into(),
                id: "fixture".into(),
                title: "Fixture".into(),
            },
            "Library / Playlists",
        ),
        (
            Page::Collection {
                kind: "albums".into(),
                id: "1".into(),
                title: "Fixture".into(),
            },
            "Albums",
        ),
        (Page::Settings, "Preferences"),
        (Page::Search, "Search"),
    ] {
        app.page = page;
        let output = draw(&ctx, &mut app, 1240., vec![]);
        assert!(output.shapes.iter().any(
            |shape| matches!(&shape.shape,egui::Shape::Text(text) if text.galley.job.text==label)
        ));
    }
}

#[test]
fn search_preserves_focus_enter_link_and_click_submission_without_writes() {
    for (query, enter) in [
        ("Afterimage", true),
        ("tidal://track/123", true),
        ("Afterimage", false),
    ] {
        let (ctx, mut app, _events, mut requests) = tests::fixture();
        app.focus_search = true;
        draw(&ctx, &mut app, 1240., vec![]);
        assert_eq!(
            ctx.memory(|m| m.focused()),
            Some(egui::Id::new("global-search"))
        );
        let output = draw(&ctx, &mut app, 1240., vec![egui::Event::Text(query.into())]);
        if enter {
            draw(
                &ctx,
                &mut app,
                1240.,
                vec![egui::Event::Key {
                    key: egui::Key::Enter,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                }],
            );
        } else {
            let rect = field(&output);
            let pos = pos2(rect.left() + 16., rect.center().y);
            for pressed in [true, false] {
                draw(
                    &ctx,
                    &mut app,
                    1240.,
                    vec![
                        egui::Event::PointerMoved(pos),
                        egui::Event::PointerButton {
                            pos,
                            button: egui::PointerButton::Primary,
                            pressed,
                            modifiers: egui::Modifiers::NONE,
                        },
                    ],
                );
            }
        }
        let request = requests.try_recv().unwrap();
        if query.starts_with("tidal:") {
            assert!(matches!(request, Request::Track { id: 123, .. }));
        } else {
            assert!(matches!(request,Request::Search {query,..} if query=="Afterimage"));
        }
        assert!(requests.try_recv().is_err());
        assert!(app.queue.current().is_none());
    }
    let (ctx, mut app, _events, mut requests) = tests::fixture();
    app.connected = false;
    app.focus_search = true;
    draw(&ctx, &mut app, 666., vec![]);
    assert_ne!(
        ctx.memory(|m| m.focused()),
        Some(egui::Id::new("global-search"))
    );
    assert!(requests.try_recv().is_err());
}
