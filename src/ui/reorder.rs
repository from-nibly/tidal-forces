use super::*;
use crate::reorder::{Check, MAX_TRACKS, Move, Target};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

#[cfg(test)]
#[path = "reorder_tests.rs"]
mod tests;

#[derive(Clone)]
enum Phase {
    Preflight(usize),
    Write,
    Readback { offset: usize, etag: String },
}

pub(super) struct Dialog {
    change: Move,
    expected: Vec<u64>,
    choosing_position: bool,
    position: String,
    generation: u64,
    operation: u64,
    title: String,
    phase: Phase,
    busy: bool,
    write_sent: bool,
    error: Option<String>,
    cancelled: Arc<AtomicBool>,
}
impl Dialog {
    fn position_target(&self) -> Option<Target> {
        // Below the widget limit: truncating an oversized paste must not turn it
        // into a different valid destination (including long zero/space prefixes).
        if self.position.len() > 16 {
            return None;
        }
        let text = self.position.trim();
        if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        text.parse::<usize>()
            .ok()
            .filter(|p| *p > 0 && *p <= self.change.ids.len() && *p != self.change.from + 1)
            .map(|p| Target::Position(p - 1))
    }
}
impl Drop for Dialog {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Release);
    }
}
impl App {
    fn reorder_page_scope(&self) -> Option<&PlaylistPage> {
        let page = self.playlist_page.as_ref()?;
        let user = self.account.filter(|user| *user != 0)?;
        let count = self.tracks.len();
        if !self.connected
            || self.loading
            || self.buffering
            || !self.browse_cache.ready
            || self.browse_cache.owner != Some(user)
            || self.track_view.active()
            || self.playlist_busy
            || self.metadata_editor.is_some()
            || self.delete_dialog.is_some()
            || self.folder_dialog.is_some()
            || !(2..=MAX_TRACKS).contains(&count)
            || !page.editable
            || page.more
            || page.next_offset != count
            || page.rows.len() != count
            || page.playlist.number_of_tracks != count as u64
            || page.playlist.number_of_videos != 0
            || !matches!(&self.page,Page::Collection{kind,id,..} if kind=="playlists" && id==&page.playlist.uuid)
        {
            return None;
        }
        Some(page)
    }
    pub(super) fn can_reorder_playlist(&self) -> bool {
        self.reorder_dialog.is_none() && self.reorder_page_scope().is_some()
    }
    pub(super) fn open_reorder(&mut self, from: usize, target: Target) {
        self.begin_reorder(from, target, false);
    }
    pub(super) fn open_position_reorder(&mut self, from: usize) {
        self.begin_reorder(
            from,
            Target::Position(if from == 0 { 1 } else { from - 1 }),
            true,
        );
    }
    fn begin_reorder(&mut self, from: usize, target: Target, choosing_position: bool) {
        if self.reorder_dialog.is_some() {
            return;
        }
        let Some(page) = self.reorder_page_scope() else {
            return;
        };
        let change = Move {
            user: self.account.unwrap(),
            id: page.playlist.uuid.clone(),
            etag: page.etag.clone(),
            ids: self.tracks.iter().map(|t| t.id).collect(),
            from,
            target,
        };
        let expected = match change.after() {
            Ok(expected) => expected,
            Err(error) => {
                self.error = Some(error.to_string());
                return;
            }
        };
        if !change.matches(page, &change.ids) {
            self.error = Some(
                "The loaded playlist positions are incomplete. Refresh before reordering.".into(),
            );
            return;
        }
        self.reorder_serial = self.reorder_serial.wrapping_add(1);
        self.reorder_dialog = Some(Dialog {
            change,
            expected,
            choosing_position,
            position: String::new(),
            generation: self.generation,
            operation: self.reorder_serial,
            title: self.tracks[from].title.clone(),
            phase: Phase::Preflight(0),
            busy: false,
            write_sent: false,
            error: None,
            cancelled: Arc::new(AtomicBool::new(false)),
        });
        self.reorder_tick();
    }
    fn submit_position_reorder(&mut self) {
        if !self.reorder_scope_current() {
            self.cancel_reorder();
            return;
        }
        let Some(page) = self.reorder_page_scope() else {
            return;
        };
        let Some(dialog) = &self.reorder_dialog else {
            return;
        };
        if !dialog.choosing_position || dialog.busy || dialog.error.is_some() {
            return;
        }
        if page.etag != dialog.change.etag
            || !dialog.change.matches(page, &dialog.change.ids)
            || !self
                .tracks
                .iter()
                .map(|t| t.id)
                .eq(dialog.change.ids.iter().copied())
        {
            self.reorder_failure(
                "The loaded playlist changed. Refresh before choosing a position.".into(),
            );
            return;
        }
        let Some(target) = dialog.position_target() else {
            return;
        };
        let dialog = self.reorder_dialog.as_mut().unwrap();
        dialog.change.target = target;
        match dialog.change.after() {
            Ok(expected) => {
                dialog.expected = expected;
                dialog.choosing_position = false;
            }
            Err(error) => {
                self.reorder_failure(error.to_string());
                return;
            }
        }
        self.reorder_tick();
    }
    fn reorder_scope_current(&self) -> bool {
        self.reorder_dialog.as_ref().is_some_and(|d| self.connected && self.account==Some(d.change.user) && self.generation==d.generation
            && matches!(&self.page,Page::Collection{kind,id,..} if kind=="playlists" && id==&d.change.id))
    }
    pub(super) fn cancel_reorder(&mut self) {
        if let Some(dialog) = self.reorder_dialog.take() {
            self.notice=Some(if !dialog.write_sent {"Playlist move cancelled before writing."} else {"The move was not fully verified. Refresh and inspect the playlist before trying again."}.into());
        }
    }
    fn reorder_failure(&mut self, message: String) {
        self.invalidate_browse_cache();
        if let Some(dialog) = &mut self.reorder_dialog {
            dialog.busy = false;
            dialog.cancelled.store(true, Ordering::Release);
            let message = if !dialog.write_sent {
                format!("The move was not sent. {message}")
            } else {
                format!(
                    "The move may have completed. Refresh and inspect before retrying. {message}"
                )
            };
            dialog.error = Some(message.clone());
            self.error = Some(message);
        }
    }
    pub(super) fn reorder_tick(&mut self) {
        if self.reorder_dialog.is_none() {
            return;
        }
        if !self.reorder_scope_current() {
            self.cancel_reorder();
            return;
        }
        let dialog = self.reorder_dialog.as_ref().unwrap();
        if dialog.error.is_some() || dialog.busy {
            return;
        }
        if !matches!(dialog.phase, Phase::Readback { .. }) && !self.browse_cache.ready {
            self.cancel_reorder();
            return;
        }
        if self.buffering || self.loading || dialog.choosing_position {
            return;
        }
        let request = match &dialog.phase {
            Phase::Write => Request::ReorderPlaylist {
                operation: dialog.operation,
                change: dialog.change.clone(),
                cancelled: dialog.cancelled.clone(),
            },
            Phase::Preflight(offset) | Phase::Readback { offset, .. } => Request::CheckReorder {
                operation: dialog.operation,
                check: Check {
                    user: dialog.change.user,
                    id: dialog.change.id.clone(),
                    etag: match &dialog.phase {
                        Phase::Readback { etag, .. } => etag.clone(),
                        _ => dialog.change.etag.clone(),
                    },
                    count: dialog.change.ids.len(),
                    offset: *offset,
                },
                cancelled: dialog.cancelled.clone(),
            },
        };
        let writing = matches!(request, Request::ReorderPlaylist { .. });
        self.reorder_dialog.as_mut().unwrap().busy = true;
        if self.try_send(request) {
            if writing {
                self.reorder_dialog.as_mut().unwrap().write_sent = true;
            }
        } else {
            self.reorder_failure("The request could not be queued.".into());
        }
    }
    pub(super) fn reorder_checked(
        &mut self,
        operation: u64,
        check: Check,
        result: Result<Vec<u64>, String>,
    ) {
        let Some(dialog) = &self.reorder_dialog else {
            return;
        };
        if dialog.operation != operation
            || dialog.change.user != check.user
            || dialog.change.id != check.id
        {
            return;
        }
        if !self.reorder_scope_current() {
            self.cancel_reorder();
            return;
        }
        let dialog = self.reorder_dialog.as_ref().unwrap();
        if dialog.error.is_some() {
            return;
        }
        let (offset, etag, expected) = match &dialog.phase {
            Phase::Preflight(offset) => {
                if !self.browse_cache.ready {
                    self.cancel_reorder();
                    return;
                }
                (*offset, &dialog.change.etag, &dialog.change.ids)
            }
            Phase::Readback { offset, etag } => (*offset, etag, &dialog.expected),
            Phase::Write => {
                self.reorder_failure("Unexpected verification response.".into());
                return;
            }
        };
        if !dialog.busy
            || check.offset != offset
            || check.etag != *etag
            || check.count != expected.len()
        {
            self.reorder_failure("Verification scope changed.".into());
            return;
        }
        let ids = match result {
            Ok(ids) => ids,
            Err(message) => {
                self.reorder_failure(message);
                return;
            }
        };
        let end = (offset + 100).min(expected.len());
        if ids.as_slice() != &expected[offset..end] {
            self.reorder_failure("Playlist order changed or a page was incomplete.".into());
            return;
        }
        let complete = end == expected.len();
        let dialog = self.reorder_dialog.as_mut().unwrap();
        dialog.busy = false;
        match &mut dialog.phase {
            Phase::Preflight(offset) => {
                if complete {
                    dialog.phase = Phase::Write;
                } else {
                    *offset = end;
                }
            }
            Phase::Readback { offset, .. } => {
                if complete {
                    self.finish_reorder();
                } else {
                    *offset = end;
                }
            }
            Phase::Write => unreachable!(),
        }
    }
    pub(super) fn reorder_result(
        &mut self,
        user: u64,
        operation: u64,
        id: String,
        result: Result<String, String>,
    ) {
        if self.account != Some(user) {
            return;
        }
        self.invalidate_browse_cache();
        let matching = self.reorder_dialog.as_ref().is_some_and(|d| {
            d.change.user == user
                && d.operation == operation
                && d.change.id == id
                && matches!(d.phase, Phase::Write)
                && d.busy
                && d.error.is_none()
        });
        if !matching || !self.reorder_scope_current() {
            if matching {
                self.cancel_reorder();
            }
            self.notice =
                Some("The move was not fully verified. Refresh and inspect the playlist.".into());
            return;
        }
        match result {
            Ok(etag) => {
                let dialog = self.reorder_dialog.as_mut().unwrap();
                if etag.trim().is_empty() || etag == dialog.change.etag {
                    self.reorder_failure("No new write revision was confirmed.".into());
                    return;
                }
                dialog.phase = Phase::Readback { offset: 0, etag };
                dialog.busy = false;
            }
            Err(message) => self.reorder_failure(message),
        }
    }
    fn finish_reorder(&mut self) {
        let Some(dialog) = self.reorder_dialog.take() else {
            return;
        };
        let Phase::Readback { etag, .. } = &dialog.phase else {
            return;
        };
        self.invalidate_browse_cache();
        self.notice =
            Some("Playlist order verified. The current playback queue was not changed.".into());
        if !self.playlist_page.as_ref().is_some_and(|page| {
            page.etag == dialog.change.etag && dialog.change.matches(page, &dialog.change.ids)
        }) || self.tracks.len() != dialog.change.ids.len()
            || !self
                .tracks
                .iter()
                .zip(&dialog.change.ids)
                .all(|(t, id)| t.id == *id)
        {
            self.generation += 1;
            self.load_page(0);
            return;
        }
        let (from, to) = dialog.change.indices().expect("validated move");
        let track = self.tracks.remove(from);
        self.tracks.insert(to.min(self.tracks.len()), track);
        if let Some(page) = &mut self.playlist_page {
            page.etag = etag.clone();
            page.rows = self.tracks.iter().cloned().enumerate().collect();
        }
        self.track_selection.clear();
        let query = self.track_view.query.clone();
        let sort = self.track_view.sort;
        let reverse = self.track_view.reverse;
        self.generation += 1;
        self.sync_track_view();
        self.track_view.query = query;
        self.track_view.sort = sort;
        self.track_view.reverse = reverse;
        self.track_view.invalidate();
        self.browse_loaded();
    }
    pub(super) fn reorder_window(&mut self, ctx: &egui::Context) {
        if self.reorder_dialog.is_some() && !self.reorder_scope_current() {
            self.cancel_reorder();
        }
        let can_choose = self.reorder_page_scope().is_some_and(|page| {
            self.reorder_dialog
                .as_ref()
                .is_none_or(|d| page.etag == d.change.etag)
        });
        let Some(dialog) = &mut self.reorder_dialog else {
            return;
        };
        let mut close = false;
        let mut refresh = false;
        let mut submit = false;
        egui::Window::new("Reorder playlist")
            .id(egui::Id::new("playlist-reorder"))
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .collapsible(false)
            .resizable(false)
            .default_width(380.)
            .show(ctx, |ui| {
                ui.label(if dialog.choosing_position {
                    format!(
                        "Choose a new position for original position {}.",
                        dialog.change.from + 1
                    )
                } else if let Target::Position(position) = dialog.change.target {
                    format!(
                        "Moving original position {} to position {}.",
                        dialog.change.from + 1,
                        position + 1
                    )
                } else {
                    format!(
                        "Moving original position {} {}.",
                        dialog.change.from + 1,
                        dialog.change.target.label()
                    )
                });
                ui.add(egui::Label::new(RichText::new(&dialog.title).strong()).truncate());
                ui.label("This changes your TIDAL playlist, not the current playback queue.");
                if let Some(error) = &dialog.error {
                    ui.colored_label(Color32::LIGHT_RED, error);
                    refresh = ui.button("Refresh and inspect playlist").clicked();
                    close = ui.button("Close").clicked();
                } else if dialog.choosing_position {
                    ui.label(format!("Final position (1–{}):", dialog.change.ids.len()));
                    ui.add(
                        egui::TextEdit::singleline(&mut dialog.position)
                            .desired_width(120.)
                            .char_limit(64),
                    );
                    ui.label("Positions refer to Original order, not a filtered or sorted view.");
                    let valid = dialog.position_target().is_some();
                    if !valid && !dialog.position.is_empty() {
                        ui.colored_label(
                            Color32::LIGHT_RED,
                            "Enter a different whole-number position within the range.",
                        );
                    }
                    if !can_choose {
                        ui.label(if self.buffering {
                            "Waiting for playback buffering…"
                        } else {
                            "This view is not ready. Use the unchanged, fully loaded Original-order playlist."
                        });
                    }
                    ui.horizontal(|ui| {
                        submit = ui
                            .add_enabled(can_choose && valid, egui::Button::new("Move to position"))
                            .clicked();
                        close = ui.button("Cancel").clicked();
                    });
                } else {
                    ui.spinner();
                    match &dialog.phase {
                        Phase::Preflight(offset) => {
                            ui.label(format!(
                                "Checking source: {offset} / {}",
                                dialog.change.ids.len()
                            ));
                            close = ui.button("Cancel before move").clicked();
                        }
                        Phase::Write => {
                            ui.label("Moving track…");
                        }
                        Phase::Readback { offset, .. } => {
                            ui.label(format!(
                                "Verifying order: {offset} / {}",
                                dialog.expected.len()
                            ));
                        }
                    }
                    if self.buffering {
                        ui.label("Waiting for playback buffering…");
                    }
                }
            });
        if submit {
            self.submit_position_reorder();
        }
        if close || refresh {
            self.cancel_reorder();
        }
        if refresh {
            self.generation += 1;
            self.load_page(0);
        }
    }
}
