use super::super::tests::{fixture, track};
use super::*;

#[test]
fn manual_drop_uses_occurrences_and_rejects_stale_or_cross_account_payloads() {
    let (_ctx, mut app, _events, _requests) = fixture();
    for _ in 0..3 {
        app.queue.add(track(9), false).unwrap();
    }
    let ids: Vec<_> = app
        .queue
        .manual()
        .iter()
        .map(|entry| entry.occurrence)
        .collect();
    let drag = QueueDrag {
        user: 7,
        context: app.context_generation,
        revision: app.queue.revision(),
        occurrence: ids[0],
    };
    assert!(app.drop_manual(&drag, ids[2], true));
    assert_eq!(
        app.queue
            .manual()
            .iter()
            .map(|entry| entry.occurrence)
            .collect::<Vec<_>>(),
        [ids[1], ids[2], ids[0]]
    );
    assert!(!app.drop_manual(&drag, ids[1], false));
    let drag = QueueDrag {
        revision: app.queue.revision(),
        ..drag
    };
    app.account = Some(8);
    assert!(!app.drop_manual(&drag, ids[1], false));
    app.account = Some(7);
    app.cancel_context();
    assert!(!app.drop_manual(&drag, ids[1], false));
}

fn draw(
    ctx: &egui::Context,
    app: &mut App,
    size: Vec2,
    full_app: bool,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
            events,
            ..Default::default()
        },
        |ctx| {
            if full_app {
                use eframe::App as _;
                app.update(ctx, &mut eframe::Frame::_new_kittest());
            } else {
                egui::CentralPanel::default().show(ctx, |ui| app.queue_contents(ui));
                app.queue_drag_cursor(ctx);
            }
        },
    )
}
#[test]
fn dragging_the_handle_reorders_without_clicking_play() {
    for (size, full_app) in [
        (vec2(500., 1000.), false),
        (vec2(1240., 820.), true),
        (vec2(1000., 660.), true),
    ] {
        let (ctx, mut app, _events, mut requests) = fixture();
        app.queue_open = true;
        for _ in 0..3 {
            app.queue.add(track(9), false).unwrap();
        }
        let first = app.queue.manual()[0].occurrence;
        draw(&ctx, &mut app, size, full_app, vec![]);
        let output = draw(&ctx, &mut app, size, full_app, vec![]);
        let dots: Vec<_> = output
            .shapes
            .iter()
            .filter_map(|shape| match &shape.shape {
                egui::Shape::Circle(circle) if circle.radius == 1.5 => {
                    Some((circle.center, shape.clip_rect))
                }
                _ => None,
            })
            .collect();
        assert_eq!(
            dots.len(),
            18,
            "Expected six drawn dots per grip, not font glyphs"
        );
        let handles: Vec<_> = dots
            .as_chunks::<6>()
            .0
            .iter()
            .map(|dots| dots[0].0 + vec2(3.5, 5.))
            .collect();
        let start = handles[0];
        let destination = (1..handles.len())
            .rev()
            .find(|&index| dots[index * 6].1.contains(handles[index] + vec2(40., 24.)))
            .expect("At least two manual entries must be visible");
        let target = handles[destination] + vec2(40., 24.);
        let hover = draw(
            &ctx,
            &mut app,
            size,
            full_app,
            vec![egui::Event::PointerMoved(start)],
        );
        assert_eq!(
            hover.platform_output.cursor_icon,
            egui::CursorIcon::Grab,
            "Grip should never be a text-selection target"
        );
        draw(
            &ctx,
            &mut app,
            size,
            full_app,
            vec![
                egui::Event::PointerMoved(start),
                egui::Event::PointerButton {
                    pos: start,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        let dragging = draw(
            &ctx,
            &mut app,
            size,
            full_app,
            vec![egui::Event::PointerMoved(start + vec2(0., 20.))],
        );
        assert!(egui::DragAndDrop::has_payload_of_type::<QueueDrag>(&ctx));
        assert_eq!(
            dragging.platform_output.cursor_icon,
            egui::CursorIcon::Grabbing
        );
        let moving = draw(
            &ctx,
            &mut app,
            size,
            full_app,
            vec![egui::Event::PointerMoved(target)],
        );
        assert_eq!(
            moving.platform_output.cursor_icon,
            egui::CursorIcon::Grabbing
        );
        draw(
            &ctx,
            &mut app,
            size,
            full_app,
            vec![
                egui::Event::PointerMoved(target),
                egui::Event::PointerButton {
                    pos: target,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        assert_eq!(
            app.queue.manual()[destination].occurrence,
            first,
            "Drag failed at {size:?}, full_app={full_app}"
        );
        assert!(requests.try_recv().is_err());
        assert!(app.queue.current().is_none());
    }
}
