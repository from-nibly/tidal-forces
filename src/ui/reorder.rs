use super::*;
#[cfg(test)]
#[path = "reorder_tests.rs"]
mod tests;
use crate::reorder::{MAX_TRACKS, Move};

pub(super) struct Dialog {
    change: Move,
    generation: u64,
    operation: u64,
    title: String,
    busy: bool,
    error: Option<String>,
}
impl App {
    fn reorder_scope(&self) -> Option<Move> {
        let page = self.playlist_page.as_ref()?;
        let user = self.account.filter(|u| *u != 0)?;
        if !self.connected
            || self.loading
            || self.buffering
            || !self.browse_cache.ready
            || self.browse_cache.owner != Some(user)
            || self.track_view.active()
            || self.playlist_busy
            || self.metadata_editor.is_some()
            || !(2..=MAX_TRACKS).contains(&self.tracks.len())
            || !matches!(&self.page,Page::Collection{kind,id,..} if kind=="playlists" && id==&page.playlist.uuid)
        {
            return None;
        }
        let change = Move {
            user,
            id: page.playlist.uuid.clone(),
            etag: page.etag.clone(),
            ids: self.tracks.iter().map(|track| track.id).collect(),
            from: 0,
            to_end: true,
        };
        (change.after().is_ok() && change.matches(page, &change.ids)).then_some(change)
    }
    pub(super) fn can_reorder_playlist(&self) -> bool {
        self.reorder_dialog.is_none() && self.reorder_scope().is_some()
    }
    pub(super) fn open_reorder(&mut self, from: usize, to_end: bool) {
        if self.reorder_dialog.is_some() {
            return;
        }
        let Some(mut change) = self.reorder_scope() else {
            return;
        };
        change.from = from;
        change.to_end = to_end;
        if change.after().is_err() {
            return;
        }
        self.reorder_serial = self.reorder_serial.wrapping_add(1);
        self.reorder_dialog = Some(Dialog {
            change,
            generation: self.generation,
            operation: self.reorder_serial,
            title: self.tracks[from].title.clone(),
            busy: false,
            error: None,
        });
        self.dispatch_reorder();
    }
    fn dispatch_reorder(&mut self) {
        let scope = self.reorder_scope();
        let Some(dialog) = &mut self.reorder_dialog else {
            return;
        };
        if dialog.busy || dialog.error.is_some() {
            return;
        }
        if dialog.generation != self.generation
            || scope.as_ref().is_none_or(|scope| {
                scope.user != dialog.change.user
                    || scope.id != dialog.change.id
                    || scope.etag != dialog.change.etag
                    || scope.ids != dialog.change.ids
            })
        {
            dialog.error = Some("The playlist view changed. Refresh before reordering.".into());
            return;
        }
        dialog.busy = true;
        let request = Request::ReorderPlaylist {
            operation: dialog.operation,
            change: dialog.change.clone(),
        };
        if !self.try_send(request)
            && let Some(dialog) = &mut self.reorder_dialog
        {
            dialog.busy = false;
            dialog.error =
                Some("The move could not be queued. Refresh before trying again.".into());
        }
    }
    pub(super) fn reorder_result(
        &mut self,
        user: u64,
        operation: u64,
        id: String,
        result: Result<(), String>,
    ) {
        if self.account != Some(user) {
            return;
        }
        self.invalidate_browse_cache();
        let matching = self.reorder_dialog.as_ref().is_some_and(|d| {
            d.change.user == user && d.operation == operation && d.change.id == id
        });
        match result {
            Ok(()) => {
                if matching {
                    self.reorder_dialog = None;
                }
                self.notice = Some(
                    "Playlist order verified. The current playback queue was not changed.".into(),
                );
                if matches!(&self.page,Page::Collection{kind,id:current,..} if kind=="playlists" && current==&id)
                {
                    self.generation += 1;
                    self.load_page(0);
                }
            }
            Err(message) => {
                if matching && let Some(dialog) = &mut self.reorder_dialog {
                    dialog.busy = false;
                    dialog.error = Some(message.clone());
                }
                self.error = Some(message);
            }
        }
    }
    pub(super) fn reorder_window(&mut self, ctx: &egui::Context) {
        if self.reorder_dialog.as_ref().is_some_and(|d| {
            !self.connected
                || self.account != Some(d.change.user)
                || self.generation != d.generation
        }) {
            self.reorder_dialog = None;
        }
        let Some(dialog) = &mut self.reorder_dialog else {
            return;
        };
        let mut close = false;
        let mut refresh = false;
        egui::Window::new("Reorder playlist")
            .id(egui::Id::new("playlist-reorder"))
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .collapsible(false)
            .resizable(false)
            .default_width(380.)
            .show(ctx, |ui| {
                ui.label(format!(
                    "Moving original position {} to the {}.",
                    dialog.change.from + 1,
                    if dialog.change.to_end {
                        "bottom"
                    } else {
                        "top"
                    }
                ));
                ui.add(egui::Label::new(RichText::new(&dialog.title).strong()).truncate());
                ui.label("This changes your TIDAL playlist, not the current playback queue.");
                if dialog.busy {
                    ui.spinner();
                    ui.label("Moving and verifying…");
                } else if let Some(error) = &dialog.error {
                    ui.colored_label(Color32::LIGHT_RED, error);
                    refresh = ui.button("Refresh and inspect playlist").clicked();
                    close = ui.button("Close").clicked();
                }
            });
        if close || refresh {
            self.reorder_dialog = None;
        }
        if refresh {
            self.generation += 1;
            self.load_page(0);
        }
    }
}
