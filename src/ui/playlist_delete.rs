use super::*;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

#[cfg(test)]
#[path = "playlist_delete_tests.rs"]
mod tests;
#[cfg(test)]
#[path = "playlist_unfollow_tests.rs"]
mod unfollow_tests;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Action {
    Delete,
    Unfollow,
}

pub(super) struct Dialog {
    action: Action,
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
    fn removal_scope(&self, action: Action) -> Option<&PlaylistPage> {
        let user = self.account.filter(|u| *u != 0)?;
        let page = self.playlist_page.as_ref()?;
        let eligible = match action {
            Action::Delete => page.editable,
            Action::Unfollow => {
                !page.editable
                    && page.playlist.is_known_non_owned(user)
                    && self.folders.values().any(|entries| {
                        entries.iter().any(
                            |e| matches!(e,LibraryEntry::Playlist(p) if p.uuid==page.playlist.uuid),
                        )
                    })
            }
        };
        (self.connected && self.browse_cache.owner == Some(user) && self.browse_cache.ready
            && !self.loading && !self.buffering && !self.playlist_busy
            && self.metadata_editor.is_none() && self.reorder_dialog.is_none()
            && eligible && !page.playlist.title.trim().is_empty() && !page.etag.trim().is_empty()
            && matches!(&self.page, Page::Collection {kind,id,..} if kind=="playlists" && id==&page.playlist.uuid))
            .then_some(page)
    }
    pub(super) fn can_unfollow_playlist(&self) -> bool {
        self.removal_scope(Action::Unfollow).is_some()
    }
    pub(super) fn open_delete_playlist(&mut self) {
        self.open_playlist_removal(Action::Delete);
    }
    pub(super) fn open_unfollow_playlist(&mut self) {
        self.open_playlist_removal(Action::Unfollow);
    }
    fn open_playlist_removal(&mut self, action: Action) {
        if self.delete_dialog.is_some() {
            return;
        }
        let Some(page) = self.removal_scope(action) else {
            return;
        };
        let (id, etag, title) = (
            page.playlist.uuid.clone(),
            page.etag.clone(),
            page.playlist.title.clone(),
        );
        self.delete_serial = self.delete_serial.wrapping_add(1);
        self.delete_dialog = Some(Dialog {
            action,
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
        if let Some(d) = self.delete_dialog.take()
            && d.busy
        {
            self.notice = Some(match d.action {
                Action::Delete => "Deletion may have completed. Refresh and inspect the playlist before trying again.",
                Action::Unfollow => "Unfollow may have completed. Refresh and inspect your library before trying again.",
            }.into());
        }
    }
    fn confirm_delete_playlist(&mut self) {
        let Some(dialog) = &self.delete_dialog else {
            return;
        };
        let Some(page) = self.removal_scope(dialog.action) else {
            return;
        };
        if dialog.busy
            || dialog.error.is_some()
            || (dialog.action == Action::Delete && dialog.confirmation != "DELETE")
            || self.account != Some(dialog.user)
            || self.generation != dialog.generation
            || page.playlist.uuid != dialog.id
            || page.etag != dialog.etag
        {
            return;
        }
        let request = match dialog.action {
            Action::Delete => Request::DeletePlaylist {
                user: dialog.user,
                operation: dialog.operation,
                id: dialog.id.clone(),
                etag: dialog.etag.clone(),
                cancelled: dialog.cancelled.clone(),
            },
            Action::Unfollow => Request::UnfollowPlaylist {
                user: dialog.user,
                operation: dialog.operation,
                id: dialog.id.clone(),
                etag: dialog.etag.clone(),
                cancelled: dialog.cancelled.clone(),
            },
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
        self.playlist_removal_result(Action::Delete, user, operation, id, result);
    }
    pub(super) fn playlist_unfollowed(
        &mut self,
        user: u64,
        operation: u64,
        id: String,
        result: Result<crate::api::UnfollowVerification, String>,
    ) {
        let result = result.and_then(|verification| match verification {
            crate::api::UnfollowVerification::Absent => Ok(()),
            crate::api::UnfollowVerification::Unknown => Err("TIDAL acknowledged the request, but removal could not be verified. Large or incomplete library reads do not establish absence.".into()),
        });
        self.playlist_removal_result(Action::Unfollow, user, operation, id, result);
    }
    fn playlist_removal_result(
        &mut self,
        action: Action,
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
            d.action == action
                && d.user == user
                && d.operation == operation
                && d.id == id
                && d.generation == self.generation
                && d.busy
        });
        if !matching {
            self.notice = Some(
                "A previous playlist removal finished. Refresh and inspect your library.".into(),
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
                self.notice = Some(match action {
                    Action::Delete => "Playlist deletion verified. Your playback queue and local pins are unchanged.",
                    Action::Unfollow => "Playlist removal from your library verified. The owner's playlist, your queue and local pins are unchanged.",
                }.into());
                self.refresh_folders();
                if action == Action::Delete
                    && matches!(&self.page,Page::Collection{kind,id:current,..} if kind=="playlists" && current==&id)
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
        let Some(action) = self.delete_dialog.as_ref().map(|d| d.action) else {
            return;
        };
        let can_confirm = self.removal_scope(action).is_some();
        let dialog = self.delete_dialog.as_mut().unwrap();
        let (mut confirm, mut close, mut refresh) = (false, false, false);
        egui::Window::new(if action==Action::Delete {"Delete your playlist"} else {"Unfollow saved playlist"})
            .id(egui::Id::new("playlist-delete")).collapsible(false).resizable(false)
            .default_width(360.).anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                ui.strong(&dialog.title);
                if action==Action::Delete {
                    ui.label("Permanently delete the entire playlist from TIDAL. This is not unfollowing and cannot be undone here.");
                    ui.label("Songs, queued tracks and local pins are kept. Further loading from this playlist may fail.");
                } else {
                    ui.label("Stop following this playlist in your TIDAL library. This does not delete the owner's playlist.");
                    ui.label("Your current queue, loaded tracks and local pins are kept.");
                    ui.label("Removal can only be verified from a complete library response. Large or incomplete libraries may require manual inspection.");
                }
                if dialog.busy {
                    ui.spinner(); ui.label(if action==Action::Delete {"Deleting and verifying…"} else {"Unfollowing and checking…"});
                } else if let Some(error) = &dialog.error {
                    ui.colored_label(Color32::LIGHT_RED, error);
                    refresh = ui.button("Refresh and inspect").clicked();
                    close = ui.button("Close").clicked();
                } else {
                    if action==Action::Delete {
                        ui.label("Type DELETE to confirm:");
                        ui.add(egui::TextEdit::singleline(&mut dialog.confirmation).char_limit(6));
                    }
                    ui.horizontal(|ui| {
                        let button = if action==Action::Delete {
                            egui::Button::new(egui::RichText::new("Delete playlist permanently").color(Color32::LIGHT_RED))
                        } else {egui::Button::new("Unfollow playlist")};
                        confirm = ui.add_enabled(can_confirm && (action==Action::Unfollow || dialog.confirmation=="DELETE"),button).clicked();
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
            if action == Action::Unfollow {
                self.folders.clear();
            }
            self.generation += 1;
            self.load_page(0);
            self.refresh_folders();
        }
    }
}
