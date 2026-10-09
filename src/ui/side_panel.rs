use super::*;

#[derive(Clone)]
struct QueueDrag {
    user: u64,
    context: u64,
    revision: u64,
    occurrence: u64,
}

enum Action {
    Manual(u64),
    Context(u64),
    Move(u64, usize),
    Drop(QueueDrag, u64, bool),
    Remove(u64),
    ClearManual,
    ClearSource,
    ClearAll,
    Retry,
    Save,
}

fn queue_drag_handle(ui: &mut egui::Ui) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(vec2(28., 38.), egui::Sense::drag());
    let color = if response.hovered() || response.dragged() {
        ACCENT
    } else {
        MUTED
    };
    ui.painter().rect_filled(rect, 4., CARD);
    for x in [-3.5, 3.5] {
        for y in [-5., 0., 5.] {
            ui.painter()
                .circle_filled(rect.center() + vec2(x, y), 1.5, color);
        }
    }
    response.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::WidgetType::Button,
            ui.is_enabled(),
            "Reorder manual Up next; Up/Down buttons are also available",
        )
    });
    response
        .on_hover_cursor(egui::CursorIcon::Grab)
        .on_hover_text("Drag this grip to reorder manual Up next. Up/Down buttons also work.")
}

impl App {
    pub(super) fn queue_drag_cursor(&self, ctx: &egui::Context) {
        if egui::DragAndDrop::has_payload_of_type::<QueueDrag>(ctx) {
            ctx.set_cursor_icon(egui::CursorIcon::Grabbing);
        }
    }

    fn valid_queue_drag(&self, drag: &QueueDrag) -> bool {
        self.account == Some(drag.user)
            && self.context_generation == drag.context
            && self.queue.revision() == drag.revision
            && self
                .queue
                .manual()
                .iter()
                .any(|entry| entry.occurrence == drag.occurrence)
    }
    fn drop_manual(&mut self, drag: &QueueDrag, target: u64, after: bool) -> bool {
        if !self.valid_queue_drag(drag) {
            return false;
        }
        let Some(from) = self
            .queue
            .manual()
            .iter()
            .position(|entry| entry.occurrence == drag.occurrence)
        else {
            return false;
        };
        let Some(to) = self
            .queue
            .manual()
            .iter()
            .position(|entry| entry.occurrence == target)
        else {
            return false;
        };
        let insertion = to + usize::from(after);
        self.queue.move_manual(
            drag.occurrence,
            insertion.saturating_sub(usize::from(from < insertion)),
        )
    }
    pub(super) fn queue_panel(&mut self, ctx: &egui::Context) {
        if !self.queue_open {
            return;
        }
        if ctx.content_rect().width() < 1180. {
            let mut open = true;
            egui::Window::new("Queue / History")
                .open(&mut open)
                .default_width(360.)
                .default_height((ctx.available_rect().height() - 48.).max(120.))
                .anchor(egui::Align2::RIGHT_TOP, vec2(-16., 16.))
                .max_width((ctx.available_rect().width() - 32.).max(260.))
                .title_bar(false)
                .vscroll(true)
                .max_height((ctx.available_rect().height() - 48.).max(120.))
                .show(ctx, |ui| self.queue_contents(ui));
            self.queue_open &= open;
        } else {
            egui::SidePanel::right("queue")
                .default_width(310.)
                .width_range(280.0..=420.)
                .resizable(true)
                .frame(egui::Frame::new().fill(PANEL).inner_margin(16))
                .show(ctx, |ui| self.queue_contents(ui));
        }
    }

    fn queue_contents(&mut self, ui: &mut egui::Ui) {
        let mut action = None;
        let mut track_action = None;
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.listening.show_panel, false, "Queue");
            ui.selectable_value(&mut self.listening.show_panel, true, "History");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if icon_button(ui, Icon::Close, "Close Queue / History", false, 24.).clicked() {
                    self.queue_open = false;
                }
            });
        });
        ui.add_space(8.);
        if self.listening.show_panel {
            self.history_panel(ui);
            return;
        }
        ui.horizontal(|ui| {
            surfaces::section_title(ui, "Up next");
            ui.label(
                RichText::new(format!("{} tracks", self.queue.upcoming_len()))
                    .size(12.)
                    .color(MUTED),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let menu = icon_button(ui, Icon::More, "Queue actions", false, 28.);
                egui::Popup::menu(&menu).show(|ui| {
                    if ui.button("Save queue as TIDAL playlist…").clicked() {
                        action = Some(Action::Save);
                        ui.close();
                    }
                    if ui.button("Clear manual Up next").clicked() {
                        action = Some(Action::ClearManual);
                        ui.close();
                    }
                    if ui.button("Clear remaining source tracks").clicked() {
                        action = Some(Action::ClearSource);
                        ui.close();
                    }
                    if ui.button("Stop and clear entire queue").clicked() {
                        action = Some(Action::ClearAll);
                        ui.close();
                    }
                });
            });
        });
        ui.add_space(8.);
        ui.label(RichText::new("NOW PLAYING").size(10.).color(ACCENT));
        if let Some(track) = self.queue.current() {
            surfaces::track_tile(ui, track, false, true)
                .context_menu(|ui| track_menu(ui, track, &mut track_action));
        } else {
            ui.label("Choose a track, or press Play to start Up next.");
        }
        if self.context_loading {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Loading source continuation…");
            });
        }
        if let Some(error) = &self.context_error {
            ui.colored_label(Color32::LIGHT_RED, error);
            if ui.button("Retry source loading").clicked() {
                action = Some(Action::Retry);
            }
        } else if self.queue.continuation().is_some() && !self.context_loading && self.paused {
            ui.label(
                RichText::new("More source tracks load when playback starts.")
                    .size(11.)
                    .color(MUTED),
            );
        }
        ui.separator();
        if !self.queue.title().is_empty() {
            ui.add(
                egui::Label::new(
                    RichText::new(format!("Source · {}", self.queue.title()))
                        .size(12.)
                        .color(MUTED),
                )
                .truncate(),
            );
        }
        let manual_count = self.queue.manual().len();
        let entries: Vec<_> = self
            .queue
            .manual()
            .iter()
            .chain(self.queue.context_upcoming())
            .collect();
        let rows = entries.len();
        if rows == 0 {
            ui.label(
                RichText::new("No upcoming tracks. Use + or Play next to add one.").color(MUTED),
            );
        }
        egui::ScrollArea::vertical()
            .id_salt("queue_rows")
            .auto_shrink([false, false])
            .max_height(
                ui.available_height()
                    .min((ui.clip_rect().bottom() - ui.cursor().top() - 8.).max(100.))
                    .min((ui.ctx().content_rect().height() - 280.).max(100.)),
            )
            .show_rows(ui, 90., rows, |ui, range| {
                for row in range {
                    let entry = entries[row];
                    let row_response = ui
                        .push_id(
                            (self.account, self.context_generation, entry.occurrence),
                            |ui| {
                                ui.spacing_mut().item_spacing.y = 4.;
                                ui.set_min_height(90.);
                                let manual = row < manual_count;
                                let index = row;
                                ui.label(
                                    RichText::new(if manual {
                                        "UP NEXT · MANUAL"
                                    } else if row < manual_count + self.queue.redo_len() {
                                        "RETURN PATH"
                                    } else {
                                        "FROM SOURCE"
                                    })
                                    .size(10.)
                                    .color(MUTED),
                                );
                                let response = ui
                                    .horizontal(|ui| {
                                        if manual && let Some(user) = self.account {
                                            let handle = queue_drag_handle(ui);
                                            handle.dnd_set_drag_payload(QueueDrag {
                                                user,
                                                context: self.context_generation,
                                                revision: self.queue.revision(),
                                                occurrence: entry.occurrence,
                                            });
                                        }
                                        surfaces::track_tile(ui, &entry.track, true, false)
                                    })
                                    .inner;
                                if response.clicked() {
                                    action = Some(if manual {
                                        Action::Manual(entry.occurrence)
                                    } else {
                                        Action::Context(entry.occurrence)
                                    });
                                }
                                response.context_menu(|ui| {
                                    track_menu(ui, &entry.track, &mut track_action)
                                });
                                ui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing.x = 4.;
                                    if manual {
                                        if ui
                                            .add_enabled_ui(index > 0, |ui| {
                                                icon_button(ui, Icon::Up, "Move up", false, 24.)
                                            })
                                            .inner
                                            .clicked()
                                        {
                                            action =
                                                Some(Action::Move(entry.occurrence, index - 1));
                                        }
                                        if ui
                                            .add_enabled_ui(index + 1 < manual_count, |ui| {
                                                icon_button(ui, Icon::Down, "Move down", false, 24.)
                                            })
                                            .inner
                                            .clicked()
                                        {
                                            action =
                                                Some(Action::Move(entry.occurrence, index + 1));
                                        }
                                        if icon_button(
                                            ui,
                                            Icon::Close,
                                            "Remove from Up next",
                                            false,
                                            24.,
                                        )
                                        .clicked()
                                        {
                                            action = Some(Action::Remove(entry.occurrence));
                                        }
                                    } else {
                                        ui.label(
                                            RichText::new(time(entry.track.duration))
                                                .size(11.)
                                                .color(MUTED),
                                        );
                                    }
                                    let more = icon_button(
                                        ui,
                                        Icon::More,
                                        "Queued track actions",
                                        false,
                                        24.,
                                    );
                                    egui::Popup::menu(&more)
                                        .show(|ui| track_menu(ui, &entry.track, &mut track_action));
                                });
                            },
                        )
                        .response;
                    if row < manual_count
                        && let Some(drag) = row_response.dnd_hover_payload::<QueueDrag>()
                        && self.valid_queue_drag(&drag)
                    {
                        let after = ui.input(|input| {
                            input
                                .pointer
                                .hover_pos()
                                .is_some_and(|point| point.y > row_response.rect.center().y)
                        });
                        let y = if after {
                            row_response.rect.bottom()
                        } else {
                            row_response.rect.top()
                        };
                        ui.painter().line_segment(
                            [
                                pos2(row_response.rect.left(), y),
                                pos2(row_response.rect.right(), y),
                            ],
                            Stroke::new(2.0_f32, ACCENT),
                        );
                        if let Some(drag) = row_response.dnd_release_payload::<QueueDrag>() {
                            action = Some(Action::Drop((*drag).clone(), entry.occurrence, after));
                        }
                    }
                }
            });
        match action {
            Some(Action::Manual(id)) if self.queue.jump_manual(id) => self.play_current(),
            Some(Action::Context(id)) if self.queue.jump_context(id) => self.play_current(),
            Some(Action::Move(id, index)) => {
                self.queue.move_manual(id, index);
            }
            Some(Action::Drop(drag, target, after)) => {
                self.drop_manual(&drag, target, after);
            }
            Some(Action::Remove(id)) => {
                self.queue.remove_manual(id);
            }
            Some(Action::ClearManual) => self.queue.clear_manual(),
            Some(Action::ClearSource) => self.clear_source(),
            Some(Action::ClearAll) => self.stop_and_clear(),
            Some(Action::Retry) => self.retry_context(),
            Some(Action::Save) => self.open_queue_export(),
            _ => {}
        }
        if let Some(action) = track_action {
            self.apply_track_action(action);
        }
    }
}

#[cfg(test)]
#[path = "queue_tests.rs"]
mod tests;
