use super::*;

#[test]
fn transport_icons_share_a_vertical_center_at_all_supported_widths() {
    for width in [666., 1000., 1240., 1920.] {
        for paused in [true, false] {
            let (ctx, mut app, _events, _requests) = tests::fixture();
            app.queue
                .start(vec![tests::track(1)], 0, "Fixture".into(), None)
                .unwrap();
            app.paused = paused;
            let output = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        vec2(width, 820.),
                    )),
                    ..Default::default()
                },
                |ctx| app.player_bar(ctx),
            );
            let center = output
                .shapes
                .iter()
                .find_map(|shape| match &shape.shape {
                    egui::Shape::Circle(circle) if (circle.radius - 21.).abs() < 0.01 => {
                        Some(circle.center.y)
                    }
                    _ => None,
                })
                .expect("Transport button");
            let mut triangles = 0;
            let mut shuffle = 0;
            let mut repeat = Vec::new();
            let mut pause = 0;
            for shape in &output.shapes {
                let y = match &shape.shape {
                    egui::Shape::Path(path) if path.points.len() == 3 && path.closed => {
                        triangles += 1;
                        Some(
                            (path
                                .points
                                .iter()
                                .map(|p| p.y)
                                .fold(f32::INFINITY, f32::min)
                                + path
                                    .points
                                    .iter()
                                    .map(|p| p.y)
                                    .fold(f32::NEG_INFINITY, f32::max))
                                / 2.,
                        )
                    }
                    egui::Shape::LineSegment { points, .. } => {
                        let delta = points[1] - points[0];
                        if (delta.x.abs() - 13.26).abs() < 0.01
                            && (delta.y.abs() - 9.36).abs() < 0.01
                        {
                            shuffle += 1;
                            Some((points[0].y + points[1].y) / 2.)
                        } else {
                            if (delta.x.abs() - 12.48).abs() < 0.01 && delta.y.abs() < 0.01 {
                                repeat.push(points[0].y);
                            }
                            None
                        }
                    }
                    egui::Shape::Rect(rect)
                        if (rect.rect.width() - 3.276).abs() < 0.01
                            && (rect.rect.height() - 17.472).abs() < 0.01 =>
                    {
                        pause += 1;
                        Some(rect.rect.center().y)
                    }
                    _ => None,
                };
                if let Some(y) = y {
                    assert!(
                        (y - center).abs() < 0.1,
                        "Transport center mismatch at {width}, paused={paused}: {y} vs {center}"
                    );
                }
            }
            assert_eq!(triangles, if paused { 3 } else { 2 });
            assert_eq!(pause, if paused { 0 } else { 2 });
            assert_eq!(shuffle, 2);
            assert_eq!(repeat.len(), 2);
            assert!(((repeat[0] + repeat[1]) / 2. - center).abs() < 0.1);
            assert!(app.queue.validate().is_ok());
        }
    }
}
