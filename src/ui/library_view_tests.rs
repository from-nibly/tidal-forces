use super::*;
use crate::ui::tests::fixture;

fn albums() -> Vec<Album> {
    [
        (9, "Zulu", "Beta"),
        (8, "alpha", "Zed"),
        (9, "alpha", "Alpha"),
    ]
    .into_iter()
    .map(|(id, title, artist)| Album {
        id,
        title: title.into(),
        artist: Artist {
            name: artist.into(),
            ..Default::default()
        },
        ..Default::default()
    })
    .collect()
}
#[test]
fn projections_preserve_source_occurrences_and_use_stable_ties() {
    let source = albums();
    let mut view = View {
        sort: Sort::Name,
        ..Default::default()
    };
    view.sync(Some(7), 1, FavoriteKind::Albums, &source, &[]);
    assert_eq!(view.rows, vec![1, 2, 0]);
    view.sort = Sort::Artist;
    view.sync(Some(7), 1, FavoriteKind::Albums, &source, &[]);
    assert_eq!(view.rows, vec![2, 0, 1]);
    view.query = "ALPHA".into();
    view.sync(Some(7), 1, FavoriteKind::Albums, &source, &[]);
    assert_eq!(view.rows, vec![2, 1]);
    view.reverse = true;
    view.sync(Some(7), 1, FavoriteKind::Albums, &source, &[]);
    assert_eq!(view.rows, vec![1, 2]);
    assert_eq!(
        source.iter().map(|a| a.id).collect::<Vec<_>>(),
        vec![9, 8, 9]
    );
}
#[test]
fn append_reprojects_without_idle_rebuilds_and_replacement_invalidates_keys() {
    let mut source = albums();
    let mut view = View {
        sort: Sort::Name,
        ..Default::default()
    };
    assert!(view.sync(Some(7), 1, FavoriteKind::Albums, &source, &[]));
    assert!(!view.sync(Some(7), 1, FavoriteKind::Albums, &source, &[]));
    source.push(Album {
        title: "aardvark".into(),
        ..Default::default()
    });
    view.sync(Some(7), 1, FavoriteKind::Albums, &source, &[]);
    assert_eq!(view.rows[0], 3);
    source[3].title = "zzzz".into();
    view.invalidate();
    view.sync(Some(7), 1, FavoriteKind::Albums, &source, &[]);
    assert_eq!(*view.rows.last().unwrap(), 3);
}
#[test]
fn artist_filters_and_scope_changes_do_not_retain_foreign_options() {
    let artists = vec![
        Artist {
            id: 1,
            name: "Echo".into(),
            ..Default::default()
        },
        Artist {
            id: 2,
            name: "Alpha".into(),
            ..Default::default()
        },
    ];
    let mut view = View {
        query: "ECHO".into(),
        ..Default::default()
    };
    view.sync(Some(7), 1, FavoriteKind::Artists, &[], &artists);
    assert_eq!(view.rows, vec![0]);
    view.sync(Some(8), 1, FavoriteKind::Artists, &[], &artists);
    assert!(view.query.is_empty());
    assert_eq!(view.rows, vec![0, 1]);
    view.query = "no match".into();
    view.sync(Some(8), 1, FavoriteKind::Artists, &[], &artists);
    assert!(view.rows.is_empty());
    view.sync(Some(8), 2, FavoriteKind::Artists, &[], &artists);
    assert!(view.query.is_empty());
}
#[test]
fn back_forward_keeps_card_options_without_changing_raw_pagination_or_queue() {
    let (ctx, mut app, _events, mut requests) = fixture();
    app.page = Page::Library(FavoriteKind::Albums);
    app.albums = albums();
    app.artists.clear();
    app.library_offset = 205;
    app.library_total = Some(500);
    app.more = true;
    app.browse_loaded();
    app.library_view.query = "alpha".into();
    app.library_view.sort = Sort::Name;
    app.library_view.reverse = true;
    let queue = app.queue.revision();
    let _ = ctx.run(egui::RawInput::default(), |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| app.library_view_controls(ui));
    });
    assert_eq!(app.library_view.rows.len(), 2);
    app.navigate(Page::Settings);
    app.history(false);
    assert_eq!(app.library_view.query, "alpha");
    assert!(app.library_view.reverse);
    assert!(app.library_view.sort == Sort::Name);
    assert_eq!(app.library_offset, 205);
    assert_eq!(app.library_total, Some(500));
    assert!(app.more);
    assert_eq!(app.queue.revision(), queue);
    assert!(requests.try_recv().is_err());
}

fn draw(
    ctx: &egui::Context,
    order: &[usize],
    events: Vec<egui::Event>,
    selected: &mut Option<u64>,
) -> egui::FullOutput {
    let artists = [
        Artist {
            id: 11,
            name: "Zulu".into(),
            ..Default::default()
        },
        Artist {
            id: 22,
            name: "Alpha".into(),
            ..Default::default()
        },
    ];
    ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                vec2(900., 650.),
            )),
            events,
            ..Default::default()
        },
        |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                surfaces::media_shelf_ordered(
                    ui,
                    "identity-test",
                    &artists,
                    Some(order),
                    true,
                    100.,
                    |ui, a, _| {
                        if ui.button(&a.name).clicked() {
                            *selected = Some(a.id);
                        }
                    },
                )
            });
        },
    )
}
fn label(output: &egui::FullOutput, name: &str) -> egui::Pos2 {
    output
        .shapes
        .iter()
        .find_map(|s| match &s.shape {
            egui::Shape::Text(t) if t.galley.job.text == name => {
                Some(t.pos + t.galley.size() * 0.5)
            }
            _ => None,
        })
        .unwrap()
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
fn projected_card_click_uses_source_identity_and_reordering_cannot_retarget_a_press() {
    let ctx = egui::Context::default();
    let mut selected = None;
    let _ = draw(&ctx, &[1, 0], vec![], &mut selected);
    let output = draw(&ctx, &[1, 0], vec![], &mut selected);
    let pos = label(&output, "Alpha");
    for pressed in [true, false] {
        let _ = draw(&ctx, &[1, 0], pointer(pos, pressed), &mut selected);
    }
    assert_eq!(selected, Some(22));
    selected = None;
    let _ = draw(&ctx, &[1, 0], pointer(pos, true), &mut selected);
    let _ = draw(&ctx, &[0, 1], pointer(pos, false), &mut selected);
    assert_eq!(selected, None);
}
