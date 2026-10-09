use super::*;

#[derive(Clone, Copy)]
enum SelectionAction {
    Copy,
    Next,
    Append,
    Clear,
}

fn selection_checkbox(
    ui: &mut egui::Ui,
    selected: &mut bool,
    partial: bool,
    enabled: bool,
) -> egui::Response {
    let widgets = &mut ui.visuals_mut().widgets;
    for visual in [
        &mut widgets.inactive,
        &mut widgets.hovered,
        &mut widgets.active,
    ] {
        visual.corner_radius = 3.into();
        if *selected || partial {
            visual.fg_stroke.color = ACCENT;
        }
    }
    ui.add_enabled(
        enabled,
        egui::Checkbox::new(selected, "").indeterminate(partial),
    )
}

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
    fn apply_selection_action(&mut self, ctx: &egui::Context, action: SelectionAction) {
        if matches!(action, SelectionAction::Clear) {
            self.track_selection.clear();
            return;
        }
        if self.loading || self.track_selection.rows.is_empty() {
            return;
        }
        let result: anyhow::Result<()> = (|| {
            anyhow::ensure!(
                self.track_selection.rows.len() <= crate::queue::MAX_ENTRIES,
                "Selection actions are limited to 50,000 tracks; select fewer tracks."
            );
            let tracks: Vec<_> = self
                .track_selection
                .rows
                .iter()
                .map(|index| {
                    self.tracks
                        .get(*index)
                        .filter(|track| track.id != 0)
                        .cloned()
                        .ok_or_else(|| {
                            anyhow::anyhow!("Selection changed; select the tracks again.")
                        })
                })
                .collect::<anyhow::Result<_>>()?;
            if matches!(action, SelectionAction::Copy) {
                ctx.copy_text(
                    tracks
                        .iter()
                        .map(|track| format!("https://tidal.com/browse/track/{}", track.id))
                        .collect::<Vec<_>>()
                        .join("\n"),
                );
            } else {
                self.queue
                    .add_many(tracks, matches!(action, SelectionAction::Next))?;
                self.queue_open = true;
                if self.waiting_context && !self.paused {
                    self.advance(false);
                }
            }
            Ok(())
        })();
        if let Err(error) = result {
            self.error = Some(error.to_string());
        }
    }

    pub(super) fn track_list(&mut self, ui: &mut egui::Ui) {
        self.track_selection
            .sync(self.account, self.generation, self.tracks.len());
        // A queued keyboard scroll must not reclaim focus after newer pointer,
        // scrolling or Tab intent elsewhere in the interface.
        if ui.input(|input| {
            input.pointer.any_pressed()
                || input.key_pressed(egui::Key::Tab)
                || input.raw_scroll_delta != Vec2::ZERO
        }) {
            self.track_selection.pending_focus = None;
        }
        if self.tracks.is_empty() {
            return;
        }
        let mut selection_action = None;
        ui.horizontal_wrapped(|ui| {
            ui.add_sized(vec2(100.,28.),egui::Label::new(RichText::new(format!("{} selected",self.track_selection.rows.len())).size(12.).color(MUTED)))
                .on_hover_text("Selection refers to loaded track occurrences, including duplicates. Ctrl/Shift-click to select; Ctrl+A selects loaded tracks; Ctrl+C copies links when the table has focus.");
            ui.add_enabled_ui(!self.loading && !self.track_selection.rows.is_empty(),|ui| {
                if ui.button("Copy links").clicked() { selection_action=Some(SelectionAction::Copy); }
                ui.menu_button("Selection actions",|ui| {
                    for (label,action) in [("Play next",SelectionAction::Next),("Add to Up next",SelectionAction::Append),("Clear selection",SelectionAction::Clear)] {
                        if ui.button(label).clicked() {selection_action=Some(action);ui.close();}
                    }
                });
            });
        });
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
        let title_offset = 64. + cover_size + 12.;
        let actions_width = 152.;
        let artist_width = if width > 640. {
            (width * 0.23).min(220.)
        } else {
            0.
        };
        let (header, _) = ui.allocate_exact_size(vec2(width, 28.), egui::Sense::hover());
        let mut all = self.track_selection.rows.len() == self.tracks.len();
        let mut selector = ui.new_child(
            egui::UiBuilder::new()
                .id_salt("select-loaded")
                .max_rect(egui::Rect::from_min_max(
                    header.min + vec2(4., 0.),
                    header.min + vec2(28., 28.),
                ))
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        let partial = !all && !self.track_selection.rows.is_empty();
        let check_all = selection_checkbox(&mut selector, &mut all, partial, !self.loading);
        check_all.widget_info(|| {
            if partial {
                egui::WidgetInfo::labeled(
                    egui::WidgetType::Checkbox,
                    !self.loading,
                    "Select all loaded tracks (partially selected)",
                )
            } else {
                egui::WidgetInfo::selected(
                    egui::WidgetType::Checkbox,
                    !self.loading,
                    all,
                    "Select all loaded tracks",
                )
            }
        });
        if check_all.changed() {
            if all {
                self.track_selection.all();
            } else {
                self.track_selection.clear();
            }
        }
        let mut table_focused = check_all.has_focus();
        check_all.on_hover_text("Select all loaded tracks (not unloaded pages)");
        for (x, label) in [
            (header.left() + 42., "#"),
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
                if !self.loading {
                    let modifiers = ui.input(|input| input.modifiers);
                    self.track_selection.select(
                        i,
                        modifiers.command || modifiers.ctrl,
                        modifiers.shift,
                    );
                }
            }
            if self.track_selection.pending_focus == Some(i) {
                response.request_focus();
                self.track_selection.pending_focus = None;
            }
            let mut selected = self.track_selection.rows.contains(&i);
            if selected || response.hovered() || playing {
                ui.painter().rect_filled(
                    rect,
                    6.,
                    if selected {
                        SELECTED
                    } else if playing {
                        PLAYING
                    } else {
                        CARD
                    },
                );
            }
            let mut selector = ui.new_child(
                egui::UiBuilder::new()
                    .id_salt(("track_select", i))
                    .max_rect(egui::Rect::from_min_size(
                        pos2(rect.left() + 4., rect.center().y - 14.),
                        vec2(24., 28.),
                    ))
                    .layout(egui::Layout::left_to_right(egui::Align::Center)),
            );
            let check = selection_checkbox(&mut selector, &mut selected, false, !self.loading);
            check.widget_info(|| {
                egui::WidgetInfo::selected(
                    egui::WidgetType::Checkbox,
                    !self.loading,
                    selected,
                    format!("Select track {}: {} by {}", i + 1, t.title, t.artist.name),
                )
            });
            if check.changed() {
                let modifiers = ui.input(|input| input.modifiers);
                self.track_selection.select(
                    i,
                    !modifiers.shift || modifiers.command || modifiers.ctrl,
                    modifiers.shift,
                );
            }
            table_focused |= response.has_focus() || check.has_focus();
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
                    pos2(rect.left() + 44., rect.center().y),
                    10.,
                    ACCENT,
                );
            } else {
                ui.painter().text(
                    pos2(rect.left() + 44., rect.center().y),
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
                egui::WidgetInfo::selected(
                    egui::WidgetType::SelectableLabel,
                    ui.is_enabled(),
                    selected,
                    format!("{} by {}. Enter plays this track.", t.title, t.artist.name),
                )
            });
            let mut cover_ui =
                ui.new_child(egui::UiBuilder::new().id_salt(("track_cover", i)).max_rect(
                    egui::Rect::from_min_size(
                        pos2(rect.left() + 64., rect.center().y - cover_size / 2.),
                        Vec2::splat(cover_size),
                    ),
                ));
            let cover = artwork(&mut cover_ui, t.cover_url(80), cover_size);
            if cover.clicked() && !response.clicked() && !self.loading {
                response.request_focus();
                let modifiers = ui.input(|input| input.modifiers);
                self.track_selection.select(
                    i,
                    modifiers.command || modifiers.ctrl,
                    modifiers.shift,
                );
            }
            table_focused |= cover.has_focus();
            let unmodified = ui.input(|input| input.modifiers.is_none());
            if cover.double_clicked() && self.audio_available && unmodified {
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
                && unmodified
                && (response.double_clicked()
                    || (response.has_focus()
                        && ui.input(|input| input.key_pressed(egui::Key::Enter))))
            {
                play = Some(i);
            }
            cover.context_menu(&mut menu);
            response.context_menu(menu);
            response.on_hover_text("Click to select · Ctrl/Shift-click for multiple tracks · Shift+Up/Down selects a range · Ctrl+A selects loaded tracks · Ctrl+C copies links · Double-click or Enter to play");
        }
        if table_focused && !self.loading {
            if ui.input_mut(|input| {
                input.consume_key(egui::Modifiers::COMMAND, egui::Key::A)
                    || input.consume_key(egui::Modifiers::CTRL, egui::Key::A)
            }) {
                self.track_selection.all();
            }
            if ui.input(|input| {
                input
                    .events
                    .iter()
                    .any(|event| matches!(event, egui::Event::Copy))
            }) || ui.input_mut(|input| {
                input.consume_key(egui::Modifiers::COMMAND, egui::Key::C)
                    || input.consume_key(egui::Modifiers::CTRL, egui::Key::C)
            }) {
                selection_action = Some(SelectionAction::Copy);
            }
            if ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
                self.track_selection.clear();
            }
            let navigation = ui.input(|input| {
                [
                    egui::Key::ArrowUp,
                    egui::Key::ArrowDown,
                    egui::Key::Home,
                    egui::Key::End,
                ]
                .into_iter()
                .find(|key| input.key_pressed(*key))
                .map(|key| (key, input.modifiers))
            });
            if let Some((key, modifiers)) = navigation {
                let current = self.track_selection.focus.unwrap_or(0);
                let target = match key {
                    egui::Key::ArrowUp => current.saturating_sub(1),
                    egui::Key::ArrowDown => self
                        .track_selection
                        .focus
                        .map_or(0, |current| (current + 1).min(self.tracks.len() - 1)),
                    egui::Key::End => self.tracks.len() - 1,
                    _ => 0,
                };
                if (modifiers.command || modifiers.ctrl) && !modifiers.shift {
                    self.track_selection.focus = Some(target);
                } else {
                    self.track_selection.select(
                        target,
                        modifiers.command || modifiers.ctrl,
                        modifiers.shift,
                    );
                }
                self.track_selection.pending_focus = Some(target);
                ui.scroll_to_rect(
                    egui::Rect::from_min_size(
                        bounds.min + vec2(0., height * target as f32),
                        vec2(width, height),
                    ),
                    Some(egui::Align::Center),
                );
                ui.ctx().request_repaint();
            }
        }
        if let Some(action) = selection_action {
            self.apply_selection_action(ui.ctx(), action);
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
#[path = "selection_tests.rs"]
mod selection_tests;

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
