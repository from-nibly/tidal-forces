use super::*;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

#[cfg(test)]
#[path = "playlist_delete_tests.rs"]
mod tests;

pub(super) struct Dialog {
    user: u64,
    generation: u64,
    operation: u64,
    id: String,
    etag: String,
    title: String,
    confirmation: String,
    busy: bool,
    error: Option<String>,
    cancelled: Arc<AtomicBool>,
}

impl Drop for Dialog {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Release);
    }
}

impl App {
    fn delete_scope(&self) -> Option<&PlaylistPage> {
        let user = self.account.filter(|u| *u != 0)?;
        let page = self.playlist_page.as_ref()?;
        (self.connected && self.browse_cache.owner == Some(user) && self.browse_cache.ready
            && !self.loading && !self.buffering && !self.playlist_busy
            && self.metadata_editor.is_none() && self.reorder_dialog.is_none()
            && page.editable && !page.playlist.title.trim().is_empty() && !page.etag.trim().is_empty()
            && matches!(&self.page, Page::Collection {kind,id,..} if kind=="playlists" && id==&page.playlist.uuid))
            .then_some(page)
    }

    pub(super) fn open_delete_playlist(&mut self) {
        if self.delete_dialog.is_some() {
            return;
        }
        let Some(page) = self.delete_scope() else {
            return;
        };
        let (id, etag, title) = (
            page.playlist.uuid.clone(),
            page.etag.clone(),
            page.playlist.title.clone(),
        );
        self.delete_serial = self.delete_serial.wrapping_add(1);
        self.delete_dialog = Some(Dialog {
            user: self.account.unwrap(),
            generation: self.generation,
            operation: self.delete_serial,
            id,
            etag,
            title,
            confirmation: String::new(),
            busy: false,
            error: None,
            cancelled: Arc::new(AtomicBool::new(false)),
        });
    }

    pub(super) fn cancel_delete_playlist(&mut self) {
        if self.delete_dialog.take().is_some_and(|d| d.busy) {
            self.notice = Some("Deletion may have completed. Refresh and inspect the playlist before trying again.".into());
        }
    }

    fn confirm_delete_playlist(&mut self) {
        let Some(page) = self.delete_scope() else {
            return;
        };
        let Some(dialog) = &self.delete_dialog else {
            return;
        };
        if dialog.busy
            || dialog.error.is_some()
            || dialog.confirmation != "DELETE"
            || self.account != Some(dialog.user)
            || self.generation != dialog.generation
            || page.playlist.uuid != dialog.id
            || page.etag != dialog.etag
        {
            return;
        }
        let request = Request::DeletePlaylist {
            user: dialog.user,
            operation: dialog.operation,
            id: dialog.id.clone(),
            etag: dialog.etag.clone(),
            cancelled: dialog.cancelled.clone(),
        };
        self.delete_dialog.as_mut().unwrap().busy = true;
        if !self.try_send(request)
            && let Some(dialog) = &mut self.delete_dialog
        {
            dialog.busy = false;
            dialog.cancelled.store(true, Ordering::Release);
            dialog.error =
                Some("The request could not be queued. Refresh before trying again.".into());
        }
    }

    pub(super) fn playlist_deleted(
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
        let matching = self.delete_dialog.as_ref().is_some_and(|d| {
            d.user == user
                && d.operation == operation
                && d.id == id
                && d.generation == self.generation
                && d.busy
        });
        if !matching {
            self.notice = Some(
                "A previous playlist deletion finished. Refresh and inspect your library.".into(),
            );
            return;
        }
        match result {
            Ok(()) => {
                self.delete_dialog = None;
                self.owned_playlists.retain(|p| p.uuid != id);
                for entries in self.folders.values_mut() {
                    entries.retain(|e| !matches!(e, LibraryEntry::Playlist(p) if p.uuid==id));
                }
                self.notice = Some(
                    "Playlist deletion verified. Your playback queue and local pins are unchanged."
                        .into(),
                );
                self.refresh_folders();
                if matches!(&self.page,Page::Collection{kind,id:current,..} if kind=="playlists" && current==&id)
                {
                    self.navigate(Page::Playlists);
                }
            }
            Err(error) => {
                let dialog = self.delete_dialog.as_mut().unwrap();
                dialog.busy = false;
                dialog.cancelled.store(true, Ordering::Release);
                dialog.error = Some(format!(
                    "{error} No automatic retry was made. Refresh and inspect before trying again."
                ));
            }
        }
    }

    pub(super) fn delete_playlist_window(&mut self, ctx: &egui::Context) {
        if self.delete_dialog.as_ref().is_some_and(|d| !self.connected || self.account!=Some(d.user)
            || self.generation!=d.generation
            || !matches!(&self.page,Page::Collection{kind,id,..} if kind=="playlists" && id==&d.id)) {
            self.cancel_delete_playlist();
        }
        let can_confirm = self.delete_scope().is_some();
        let Some(dialog) = &mut self.delete_dialog else {
            return;
        };
        let (mut confirm, mut close, mut refresh) = (false, false, false);
        egui::Window::new("Delete your playlist")
            .id(egui::Id::new("playlist-delete")).collapsible(false).resizable(false)
            .default_width(360.).anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                ui.strong(&dialog.title);
                ui.label("Permanently delete the entire playlist from TIDAL. This is not unfollowing and cannot be undone here.");
                ui.label("Songs, queued tracks and local pins are kept. Further loading from this playlist may fail.");
                if dialog.busy {
                    ui.spinner(); ui.label("Deleting and verifying…");
                } else if let Some(error) = &dialog.error {
                    ui.colored_label(Color32::LIGHT_RED, error);
                    refresh = ui.button("Refresh and inspect").clicked();
                    close = ui.button("Close").clicked();
                } else {
                    ui.label("Type DELETE to confirm:");
                    ui.add(egui::TextEdit::singleline(&mut dialog.confirmation).char_limit(6));
                    ui.horizontal(|ui| {
                        confirm = ui.add_enabled(can_confirm && dialog.confirmation=="DELETE",
                            egui::Button::new(egui::RichText::new("Delete playlist permanently").color(Color32::LIGHT_RED))).clicked();
                        close = ui.button("Cancel").clicked();
                    });
                }
            });
        if confirm {
            self.confirm_delete_playlist();
        }
        if close || refresh {
            self.cancel_delete_playlist();
        }
        if refresh {
            self.generation += 1;
            self.load_page(0);
            self.refresh_folders();
        }
    }
}
