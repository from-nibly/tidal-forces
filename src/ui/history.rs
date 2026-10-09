use super::*;
use crate::history::{History, Listener};
use std::time::{Duration, Instant};

pub(super) struct Listening {
    pub show_panel: bool,
    state: Option<History>,
    listener: Listener,
    request: u64,
    loading: bool,
    error: Option<String>,
    saved_revision: Option<u64>,
    last_save: Instant,
    last_prune: Instant,
    confirm_clear: bool,
}
impl Default for Listening {
    fn default() -> Self {
        Self {
            show_panel: false,
            state: None,
            listener: Listener::default(),
            request: 0,
            loading: false,
            error: None,
            saved_revision: None,
            last_save: Instant::now(),
            last_prune: Instant::now(),
            confirm_clear: false,
        }
    }
}
impl App {
    pub(super) fn history_account_changed(&mut self) {
        self.checkpoint_history(true);
        if self.listening.error.is_some() && self.error == self.listening.error {
            self.error = None;
        }
        self.listening = Listening {
            request: self.listening.request.wrapping_add(1),
            ..Listening::default()
        };
    }
    pub(super) fn request_history(&mut self) {
        let (Some(user), Some(store)) = (self.account, &self.state_store) else {
            return;
        };
        self.listening.request = self.listening.request.wrapping_add(1);
        self.listening.loading = true;
        store.load_history(user, self.listening.request);
    }
    pub(super) fn history_loaded(
        &mut self,
        user: u64,
        request: u64,
        result: Result<History, String>,
    ) {
        if self.account != Some(user) || self.listening.request != request {
            return;
        }
        self.listening.loading = false;
        if self.listening.error.is_some() && self.error == self.listening.error {
            self.error = None;
        }
        match result {
            Ok(history) => {
                self.listening.saved_revision = Some(0);
                self.listening.state = Some(history);
                self.listening.error = None;
            }
            Err(error) => {
                self.listening.state = None;
                self.error = Some(error.clone());
                self.listening.error = Some(error);
            }
        }
    }
    pub(super) fn history_saved(&mut self, user: u64, revision: u64, result: Result<(), String>) {
        if self.account == Some(user) && self.listening.saved_revision == Some(revision) {
            if self.listening.error.is_some() && self.error == self.listening.error {
                self.error = None;
            }
            self.listening.error = result.err().map(|message| format!("{message}. This change is active in memory only; the previous history and recording preference may return after restart."));
            if let Some(error) = &self.listening.error {
                self.error = Some(error.clone());
            }
        }
    }
    pub(super) fn observe_listening(
        &mut self,
        generation: u64,
        rendered: Duration,
        elapsed: Duration,
    ) {
        if generation != self.play_generation || self.account.is_none() {
            return;
        }
        if let Some(entry) = self.queue.current_entry() {
            self.listening.listener.observe(
                self.listening.state.as_mut(),
                generation,
                entry.track.clone(),
                rendered,
                elapsed,
                crate::history::now(),
            );
        }
    }
    pub(super) fn checkpoint_history(&mut self, force: bool) {
        let (Some(user), Some(store), Some(history)) =
            (self.account, &self.state_store, &mut self.listening.state)
        else {
            return;
        };
        if history.user != user {
            return;
        }
        if self.listening.last_prune.elapsed() >= Duration::from_secs(60) {
            history.prune(crate::history::now());
            self.listening.last_prune = Instant::now();
        }
        if self.listening.saved_revision != Some(history.revision())
            && (force
                || self.paused
                || self.listening.last_save.elapsed() >= Duration::from_secs(5))
        {
            store.history(history.clone());
            self.listening.saved_revision = Some(history.revision());
            self.listening.last_save = Instant::now();
        }
    }
    fn set_history_enabled(&mut self, enabled: bool) {
        if let Some(history) = &mut self.listening.state {
            history.set_enabled(enabled);
            self.listening.listener.reset_collection();
            self.checkpoint_history(true);
        }
    }
    fn clear_history(&mut self) {
        if let Some(user) = self.account {
            if let Some(history) = &mut self.listening.state {
                history.clear();
            } else {
                self.listening.state = Some(History::new(user));
                self.listening.saved_revision = None;
            }
            self.listening.request = self.listening.request.wrapping_add(1);
            self.listening.confirm_clear = false;
            self.listening.listener.reset_collection();
            self.checkpoint_history(true);
        }
    }
    pub(super) fn history_controls(&mut self, ui: &mut egui::Ui, details: bool) {
        let history = self
            .listening
            .state
            .as_ref()
            .filter(|history| Some(history.user) == self.account);
        let mut enabled = history.is_some_and(|history| history.enabled);
        let available = history.is_some() && !self.listening.loading && self.state_store.is_some();
        if ui
            .add_enabled_ui(available, |ui| recording_switch(ui, &mut enabled))
            .inner
            .changed()
        {
            self.set_history_enabled(enabled);
        }
        if details {
            ui.label(
                RichText::new("Local to this account. Nothing is uploaded to TIDAL.").color(MUTED),
            );
            ui.label(RichText::new("After 30 seconds or half a short track. Pauses, seeks and buffering do not add listening time. Retains up to 1,000 plays for 90 days.").size(11.).color(MUTED));
        }
        if self.account.is_none() {
            ui.label("Sign in to view this account's history.");
            return;
        }
        if self.listening.loading {
            ui.spinner();
            return;
        }
        if self.state_store.is_none() {
            ui.label("History storage is unavailable.");
            return;
        }
        if let Some(error) = self.listening.error.clone() {
            ui.colored_label(Color32::LIGHT_RED, error);
            if self.listening.state.is_some() && ui.button("Retry saving history").clicked() {
                self.listening.saved_revision = None;
                self.checkpoint_history(true);
            } else if self.listening.state.is_none() && ui.button("Retry loading history").clicked()
            {
                self.request_history();
            }
        }
        if self.listening.confirm_clear {
            ui.label("Delete this account's local listening records? Queue and TIDAL data stay unchanged.");
            if self
                .listening
                .state
                .as_ref()
                .is_some_and(|history| history.enabled)
            {
                ui.label("Recording stays on; future listening can add new records.");
            }
            ui.horizontal_wrapped(|ui| {
                if ui.button("Confirm clear history").clicked() {
                    self.clear_history();
                }
                if ui.button("Cancel").clicked() {
                    self.listening.confirm_clear = false;
                }
            });
        } else if ui
            .button(if self.listening.state.is_some() {
                "Clear local history…"
            } else {
                "Reset unreadable history…"
            })
            .clicked()
        {
            self.listening.confirm_clear = true;
        }
    }
    pub(super) fn history_panel(&mut self, ui: &mut egui::Ui) {
        surfaces::section_title(ui, "Listening history");
        self.history_controls(ui, false);
        ui.separator();
        let mut play = None;
        let mut action = None;
        if let Some(history) = &self.listening.state {
            if history.entries.is_empty() {
                ui.label("No recorded listening yet.");
            }
            let now = crate::history::now();
            egui::ScrollArea::vertical()
                .id_salt("listening_history")
                .auto_shrink([false, false])
                .max_height(
                    ui.available_height()
                        .min((ui.clip_rect().bottom() - ui.cursor().top() - 8.).max(100.))
                        .min((ui.ctx().content_rect().height() - 280.).max(100.)),
                )
                .show_rows(ui, 80., history.entries.len(), |ui, range| {
                    for row in range {
                        let entry = &history.entries[row];
                        ui.push_id((history.user, self.listening.request, entry.id), |ui| {
                            ui.set_min_height(80.);
                            let button = surfaces::track_tile(ui, &entry.track, true, false);
                            if button.clicked() {
                                play = Some((*entry.track).clone());
                            }
                            button
                                .on_hover_text("Play this track")
                                .context_menu(|ui| track_menu(ui, &entry.track, &mut action));
                            ui.label(
                                RichText::new(format!(
                                    "{} · {} listened",
                                    age(now.saturating_sub(entry.started_at)),
                                    time(entry.listened_ms / 1000)
                                ))
                                .size(11.)
                                .color(MUTED),
                            );
                        });
                    }
                });
        }
        if let Some(track) = play {
            self.play_history_track(track);
        }
        if let Some(action) = action {
            self.apply_track_action(action);
        }
    }
    pub(super) fn history_shelf(&mut self, ui: &mut egui::Ui) {
        let Some(history) = &self.listening.state else {
            return;
        };
        if history.entries.is_empty() {
            return;
        }
        ui.add_space(20.);
        ui.horizontal(|ui| {
            surfaces::section_title(ui, "Recently listened");
            if ui.small_button("View history").clicked() {
                self.listening.show_panel = true;
                self.queue_open = true;
            }
        });
        let mut play = None;
        let columns = ((ui.available_width() + 12.) / 240.).floor().clamp(1., 3.) as usize;
        let width = (ui.available_width() - (columns - 1) as f32 * 12.) / columns as f32;
        let entries: Vec<_> = history.entries.iter().take(3).collect();
        for row in entries.chunks(columns) {
            ui.horizontal(|ui| {
                for entry in row {
                    ui.push_id((history.user, self.listening.request, entry.id), |ui| {
                        egui::Frame::new()
                            .fill(PANEL)
                            .corner_radius(8)
                            .inner_margin(8)
                            .show(ui, |ui| {
                                ui.set_width(width - 16.);
                                if surfaces::track_tile(ui, &entry.track, true, false).clicked() {
                                    play = Some((*entry.track).clone());
                                }
                            });
                    });
                }
            });
        }
        if let Some(track) = play {
            self.play_history_track(track);
        }
    }
    fn play_history_track(&mut self, track: Track) {
        match self
            .queue
            .start(vec![track], 0, "Listening history".into(), None)
        {
            Ok(()) => {
                self.cancel_context();
                self.play_current();
            }
            Err(error) => self.error = Some(error.to_string()),
        }
    }
}
fn recording_switch(ui: &mut egui::Ui, enabled: &mut bool) -> egui::Response {
    let (rect, mut response) =
        ui.allocate_exact_size(vec2(ui.available_width(), 28.), egui::Sense::click());
    if response.clicked() {
        *enabled = !*enabled;
        response.mark_changed();
        response.request_focus();
    }
    response.widget_info(|| {
        egui::WidgetInfo::selected(
            egui::WidgetType::Checkbox,
            ui.is_enabled(),
            *enabled,
            "Record listening history",
        )
    });
    let pill =
        egui::Rect::from_center_size(pos2(rect.right() - 18., rect.center().y), vec2(36., 20.));
    let mut label = ui.new_child(
        egui::UiBuilder::new()
            .id_salt(response.id)
            .max_rect(egui::Rect::from_min_max(
                rect.min,
                pos2(pill.left() - 8., rect.bottom()),
            ))
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    label.add(
        egui::Label::new(RichText::new("Record listening history").size(13.))
            .truncate()
            .selectable(false),
    );
    let t = ui.ctx().animate_bool_responsive(response.id, *enabled);
    ui.painter()
        .rect_filled(pill, 10., if *enabled { ACCENT } else { BORDER });
    ui.painter().circle_filled(
        pos2(
            egui::lerp((pill.left() + 10.)..=(pill.right() - 10.), t),
            pill.center().y,
        ),
        7.,
        if *enabled { PANEL } else { MUTED },
    );
    if response.has_focus() {
        ui.painter().rect_stroke(
            rect.expand(2.),
            4.,
            Stroke::new(1.0_f32, ACCENT),
            egui::StrokeKind::Inside,
        );
    }
    response.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text("Local to this account on this device; nothing is uploaded to TIDAL. Turning recording off keeps existing entries. Muted playback counts; pauses, seeks and buffering do not.")
}

fn age(seconds: u64) -> String {
    match seconds {
        0..60 => "Just now".into(),
        60..3600 => format!("{}m ago", seconds / 60),
        3600..86400 => format!("{}h ago", seconds / 3600),
        _ => format!("{}d ago", seconds / 86400),
    }
}

#[cfg(test)]
#[path = "history_tests.rs"]
mod tests;
