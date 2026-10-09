use super::*;

fn visible_rows(
    bounds: egui::Rect,
    clip: egui::Rect,
    stride: f32,
    count: usize,
) -> std::ops::Range<usize> {
    if !bounds.intersects(clip) || count == 0 {
        return 0..0;
    }
    let start = (((clip.top() - bounds.top()) / stride).floor().max(0.) as usize).min(count);
    let end = (((clip.bottom() - bounds.top()) / stride).ceil().max(0.) as usize).min(count);
    start..end
}

impl App {
    pub(super) fn track_list(&mut self, ui: &mut egui::Ui) {
        if self.tracks.is_empty() {
            return;
        }
        let mut play = None;
        let mut add = None;
        let mut radio = None;
        let mut remove = None;
        let mut favorite = None;
        let removable = !self.loading
            && !self.playlist_busy
            && self.playlist_page.as_ref().is_some_and(|p| p.editable);
        let height = if self.appearance.compact_rows {
            52.
        } else {
            64.
        };
        let cover_size = height - 20.;
        let width = ui.available_width();
        let title_offset = 40. + cover_size + 12.;
        let actions_width = 152.;
        let artist_width = if width > 640. {
            (width * 0.23).min(220.)
        } else {
            0.
        };
        let (header, _) = ui.allocate_exact_size(vec2(width, 28.), egui::Sense::hover());
        for (x, label) in [
            (header.left() + 18., "#"),
            (header.left() + title_offset, "TITLE"),
        ] {
            ui.painter().text(
                pos2(x, header.center().y),
                egui::Align2::LEFT_CENTER,
                label,
                egui::FontId::proportional(10.),
                MUTED,
            );
        }
        ui.painter().text(
            pos2(header.right() - 104., header.center().y),
            egui::Align2::RIGHT_CENTER,
            "TIME",
            egui::FontId::proportional(10.),
            MUTED,
        );
        if artist_width > 0. {
            ui.painter().text(
                pos2(
                    header.right() - actions_width - artist_width - 12.,
                    header.center().y,
                ),
                egui::Align2::LEFT_CENTER,
                "ARTIST",
                egui::FontId::proportional(10.),
                MUTED,
            );
        }
        ui.painter().hline(
            header.x_range(),
            header.bottom(),
            Stroke::new(1.0_f32, BORDER),
        );
        let (id, bounds) = ui.allocate_space(vec2(width, height * self.tracks.len() as f32));
        for i in visible_rows(bounds, ui.clip_rect(), height, self.tracks.len()) {
            let t = &self.tracks[i];
            let playing = self
                .queue
                .current()
                .is_some_and(|current| current.id == t.id);
            let rect = egui::Rect::from_min_size(
                bounds.min + vec2(0., height * i as f32),
                vec2(width, height),
            );
            let response = ui.interact(rect, id.with(i), egui::Sense::click());
            if response.clicked() {
                response.request_focus();
            }
            if response.hovered() || playing {
                ui.painter()
                    .rect_filled(rect, 6., if playing { PLAYING } else { CARD });
            }
            if playing {
                ui.painter().rect_filled(
                    egui::Rect::from_min_size(
                        rect.left_top() + vec2(0., 10.),
                        vec2(2., height - 20.),
                    ),
                    1.,
                    ACCENT,
                );
                paint_icon(
                    ui.painter(),
                    Icon::Play,
                    pos2(rect.left() + 20., rect.center().y),
                    10.,
                    ACCENT,
                );
            } else {
                ui.painter().text(
                    pos2(rect.left() + 20., rect.center().y),
                    egui::Align2::CENTER_CENTER,
                    format!("{}", i + 1),
                    egui::FontId::proportional(11.),
                    MUTED,
                );
            }
            ui.painter()
                .hline(rect.x_range(), rect.bottom(), Stroke::new(0.5_f32, BORDER));
            if response.has_focus() {
                ui.painter().rect_stroke(
                    rect,
                    6.,
                    Stroke::new(1.0_f32, ACCENT),
                    egui::StrokeKind::Inside,
                );
            }
            response.widget_info(|| {
                egui::WidgetInfo::labeled(
                    egui::WidgetType::Button,
                    ui.is_enabled(),
                    format!("Play {} by {}", t.title, t.artist.name),
                )
            });
            let mut cover_ui =
                ui.new_child(egui::UiBuilder::new().id_salt(("track_cover", i)).max_rect(
                    egui::Rect::from_min_size(
                        pos2(rect.left() + 40., rect.center().y - cover_size / 2.),
                        Vec2::splat(cover_size),
                    ),
                ));
            let cover = artwork(&mut cover_ui, t.cover_url(80), cover_size);
            if cover.double_clicked() && self.audio_available {
                play = Some(i);
            }
            let title_right = rect.right() - actions_width - artist_width - 24.;
            let mut title = ui.new_child(
                egui::UiBuilder::new()
                    .id_salt(("track_title", i))
                    .max_rect(egui::Rect::from_min_max(
                        pos2(rect.left() + title_offset, rect.center().y - 19.),
                        pos2(title_right, rect.bottom()),
                    ))
                    .layout(egui::Layout::top_down(egui::Align::Min)),
            );
            title.spacing_mut().item_spacing.y = 2.;
            title.add(
                egui::Label::new(RichText::new(&t.title).size(14.).color(if playing {
                    ACCENT
                } else {
                    TEXT
                }))
                .truncate()
                .selectable(false),
            );
            title.add(
                egui::Label::new(
                    RichText::new(format!(
                        "{}{}",
                        if artist_width > 0. && !t.album.title.is_empty() {
                            &t.album.title
                        } else {
                            &t.artist.name
                        },
                        if t.explicit { " · Explicit" } else { "" }
                    ))
                    .size(12.)
                    .color(MUTED),
                )
                .truncate()
                .selectable(false),
            );
            if artist_width > 0. {
                let mut artist = ui.new_child(
                    egui::UiBuilder::new()
                        .id_salt(("track_artist", i))
                        .max_rect(egui::Rect::from_min_size(
                            pos2(title_right + 12., rect.center().y - 8.),
                            vec2(artist_width, 20.),
                        )),
                );
                artist.add(
                    egui::Label::new(RichText::new(&t.artist.name).size(12.).color(MUTED))
                        .truncate()
                        .selectable(false),
                );
            }
            let mut menu = |ui: &mut egui::Ui| {
                track_menu(ui, t, &mut radio);
                if removable && ui.button("Remove from this playlist…").clicked() {
                    remove = Some(i);
                    ui.close();
                }
            };
            let mut actions = ui.new_child(
                egui::UiBuilder::new()
                    .id_salt(("track_actions", i))
                    .max_rect(egui::Rect::from_min_max(
                        pos2(rect.right() - actions_width, rect.top()),
                        rect.right_bottom() - vec2(8., 0.),
                    ))
                    .layout(egui::Layout::right_to_left(egui::Align::Center)),
            );
            actions.spacing_mut().item_spacing.x = 4.;
            let more = icon_button(&mut actions, Icon::More, "Track actions", false, 28.);
            egui::Popup::menu(&more).show(&mut menu);
            if icon_button(&mut actions, Icon::Add, "Add to Up next", false, 28.).clicked() {
                add = Some(t.clone());
            }
            favorite_button(
                &mut actions,
                &self.favorites,
                Favorite::new(FavoriteKind::Tracks, t.id),
                &mut favorite,
            );
            actions.label(RichText::new(time(t.duration)).size(12.).color(MUTED));
            if self.audio_available
                && (response.double_clicked()
                    || (response.has_focus()
                        && ui.input(|input| input.key_pressed(egui::Key::Enter))))
            {
                play = Some(i);
            }
            cover.context_menu(&mut menu);
            response.context_menu(menu);
            response.on_hover_text("Double-click or press Enter to play · Right-click for playlists and radio · + to queue");
        }
        if let Some(i) = play {
            self.play_track(i);
        }
        if let Some(t) = add {
            self.queue_track(t, false);
        }
        if let Some(action) = radio {
            self.apply_track_action(action);
        }
        if let Some((item, saved)) = favorite {
            self.set_favorite(item, saved);
        }
        if let Some(i) = remove
            && let Some(page) = &self.playlist_page
            && let Some((index, track)) = page.rows.get(i)
        {
            self.playlist_error = None;
            self.removal = Some(Removal {
                playlist: page.playlist.uuid.clone(),
                title: page.playlist.title.clone(),
                index: *index,
                track: track.clone(),
                etag: page.etag.clone(),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn draw(ctx: &egui::Context, app: &mut App, events: Vec<egui::Event>) -> egui::FullOutput {
        ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    vec2(620., 320.),
                )),
                events,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| app.track_list(ui));
            },
        )
    }
    fn click(ctx: &egui::Context, app: &mut App, point: egui::Pos2) {
        for pressed in [true, false] {
            draw(
                ctx,
                app,
                vec![
                    egui::Event::PointerMoved(point),
                    egui::Event::PointerButton {
                        pos: point,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            );
        }
    }
    #[test]
    fn table_actions_do_not_play_and_title_keeps_keyboard_playback() {
        let (ctx, mut app, _events, mut requests) = super::super::tests::fixture();
        let mut track = super::super::tests::track(1);
        track.title = "A fixture song".into();
        app.tracks = vec![track];
        app.favorites
            .observe(Favorite::new(FavoriteKind::Tracks, 1));
        draw(&ctx, &mut app, vec![]);
        let output = draw(&ctx, &mut app, vec![]);
        let plus = output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::LineSegment { points, .. }
                    if points[0].y == points[1].y
                        && (10. ..10.5).contains(&(points[1].x - points[0].x)) =>
                {
                    Some(points[0].lerp(points[1], 0.5))
                }
                _ => None,
            })
            .unwrap();
        click(&ctx, &mut app, plus);
        click(&ctx, &mut app, plus);
        assert_eq!(app.queue.manual().len(), 2);
        assert!(app.queue.current().is_none());
        assert!(requests.try_recv().is_err());
        let heart = output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Path(path) if path.points.len() == 9 => {
                    Some(path.points[0] - vec2(0., 6.))
                }
                _ => None,
            })
            .unwrap();
        click(&ctx, &mut app, heart);
        assert!(matches!(
            requests.try_recv(),
            Ok(Request::SetFavorite { saved: false, .. })
        ));
        assert!(app.queue.current().is_none());
        let title = output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Text(text) if text.galley.job.text == "A fixture song" => {
                    Some(text.pos + vec2(8., 5.))
                }
                _ => None,
            })
            .unwrap();
        click(&ctx, &mut app, title);
        assert!(app.queue.current().is_none());
        draw(
            &ctx,
            &mut app,
            vec![egui::Event::Key {
                key: egui::Key::Enter,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        assert!(matches!(
            requests.try_recv(),
            Ok(Request::Play { id: 1, .. })
        ));
    }

    #[test]
    fn virtual_rows_keep_original_positions_and_only_render_the_viewport() {
        let bounds = egui::Rect::from_min_size(pos2(0., -700.), vec2(600., 70000.));
        let clip = egui::Rect::from_min_size(pos2(0., 0.), vec2(600., 660.));
        assert_eq!(visible_rows(bounds, clip, 70., 1000), 10..20);
        assert_eq!(visible_rows(bounds, clip, 70., 15), 10..15);
        assert_eq!(visible_rows(bounds, clip, 70., 0), 0..0);
        assert_eq!(
            visible_rows(bounds.translate(vec2(0., 100000.)), clip, 70., 1000),
            0..0
        );
    }
}
