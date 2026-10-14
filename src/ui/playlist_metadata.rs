use super::*;

#[cfg(test)]
#[path = "playlist_metadata_tests.rs"]
mod tests;

pub(super) struct Editor {
    user: u64,
    generation: u64,
    operation: u64,
    id: String,
    etag: String,
    title: String,
    description: String,
    busy: bool,
    error: Option<String>,
}

impl App {
    pub(super) fn open_metadata_editor(&mut self) {
        let Some(page) = &self.playlist_page else {
            return;
        };
        let Some(description) = &page.playlist.description else {
            return;
        };
        let Some(user) = self.account.filter(|u| *u != 0) else {
            return;
        };
        if !self.connected
            || !self.browse_cache.ready
            || self.loading
            || !page.editable
            || self.metadata_editor.is_some()
            || !matches!(&self.page,Page::Collection{kind,id,..} if kind=="playlists" && id==&page.playlist.uuid)
        {
            return;
        }
        self.metadata_serial = self.metadata_serial.wrapping_add(1);
        self.metadata_editor = Some(Editor {
            user,
            generation: self.generation,
            operation: self.metadata_serial,
            id: page.playlist.uuid.clone(),
            etag: page.etag.clone(),
            title: page.playlist.title.clone(),
            description: description.clone(),
            busy: false,
            error: None,
        });
    }

    pub(super) fn save_metadata_editor(&mut self) {
        let Some(editor) = &mut self.metadata_editor else {
            return;
        };
        if editor.busy
            || editor.error.is_some()
            || !self.connected
            || self.account != Some(editor.user)
            || self.generation != editor.generation
            || editor.title.trim().is_empty()
            || editor.title.chars().count() > 100
            || editor.description.chars().count() > 1000
        {
            return;
        }
        editor.busy = true;
        let request = Request::EditPlaylistMetadata {
            user: editor.user,
            operation: editor.operation,
            id: editor.id.clone(),
            etag: editor.etag.clone(),
            title: editor.title.clone(),
            description: editor.description.clone(),
        };
        if !self.try_send(request)
            && let Some(editor) = &mut self.metadata_editor
        {
            editor.busy = false;
            editor.error =
                Some("The request could not be queued. Refresh before editing again.".into());
        }
    }

    pub(super) fn metadata_result(
        &mut self,
        user: u64,
        operation: u64,
        id: String,
        result: Result<Playlist, String>,
    ) {
        if self.account != Some(user) {
            return;
        }
        self.invalidate_browse_cache();
        let matching = self
            .metadata_editor
            .as_ref()
            .is_some_and(|e| e.user == user && e.operation == operation && e.id == id);
        match result {
            Ok(_) => {
                if matching {
                    self.metadata_editor = None;
                }
                self.notice = Some("Playlist name and description verified.".into());
                self.refresh_folders();
                if matches!(&self.page,Page::Collection{kind,id:current,..} if kind=="playlists" && current==&id)
                {
                    self.generation += 1;
                    self.load_page(0);
                }
            }
            Err(message) => {
                if matching && let Some(editor) = &mut self.metadata_editor {
                    editor.busy = false;
                    editor.error = Some(message.clone());
                }
                self.error = Some(message);
            }
        }
    }

    pub(super) fn metadata_window(&mut self, ctx: &egui::Context) {
        if self.metadata_editor.as_ref().is_some_and(|e| {
            !self.connected || self.account != Some(e.user) || self.generation != e.generation
        }) {
            self.metadata_editor = None;
        }
        let Some(editor) = &mut self.metadata_editor else {
            return;
        };
        let mut save = false;
        let mut close = false;
        let mut refresh = false;
        egui::Window::new("Edit playlist details")
            .id(egui::Id::new("playlist-metadata"))
            .collapsible(false)
            .resizable(false)
            .default_width(360.)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                ui.label("Changes are saved to your TIDAL account.");
                ui.add_enabled_ui(!editor.busy && editor.error.is_none(), |ui| {
                    ui.label("Name (1–100 characters)");
                    ui.add(egui::TextEdit::singleline(&mut editor.title).char_limit(100));
                    ui.label("Description (up to 1,000 characters)");
                    egui::ScrollArea::vertical()
                        .max_height(120.)
                        .show(ui, |ui| {
                            ui.add(
                                egui::TextEdit::multiline(&mut editor.description)
                                    .char_limit(1000)
                                    .desired_rows(4)
                                    .desired_width(340.),
                            );
                        });
                });
                if editor.busy {
                    ui.spinner();
                    ui.label("Saving and verifying…");
                } else if let Some(error) = &editor.error {
                    ui.colored_label(Color32::LIGHT_RED, error);
                    refresh = ui.button("Refresh before trying again").clicked();
                    close = ui.button("Close").clicked();
                } else {
                    ui.horizontal(|ui| {
                        let valid = !editor.title.trim().is_empty()
                            && editor.title.chars().count() <= 100
                            && editor.description.chars().count() <= 1000;
                        save = ui
                            .add_enabled(valid, egui::Button::new("Save changes"))
                            .clicked();
                        close = ui.button("Cancel").clicked();
                    });
                }
            });
        if save {
            self.save_metadata_editor();
        }
        if close || refresh {
            self.metadata_editor = None;
        }
        if refresh {
            self.generation += 1;
            self.load_page(0);
        }
    }
}
