use super::*;
use crate::folders::{Change, Folder, Outcome};

#[cfg(test)]
#[path = "folder_edit_tests.rs"]
mod tests;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

pub(super) struct Dialog {
    user: u64,
    generation: u64,
    operation: u64,
    change: Change,
    destinations: Vec<Folder>,
    busy: bool,
    error: Option<String>,
    cancelled: Arc<AtomicBool>,
}
impl Drop for Dialog {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Release);
    }
}
fn folder_popup(response: &egui::Response) -> egui::Popup<'_> {
    let mut popup = egui::Popup::context_menu(response).at_position(response.rect.right_bottom());
    if response.has_focus()
        && response
            .ctx
            .input(|i| i.modifiers.shift && i.key_pressed(egui::Key::F10))
    {
        popup = popup.open_memory(Some(egui::SetOpenCommand::Bool(true)));
    }
    popup
}

impl App {
    pub(super) fn can_edit_folders(&self) -> bool {
        self.connected
            && self.account.is_some_and(|u| u != 0)
            && !self.buffering
            && !self.playlist_busy
            && !self.playlist_dialog
            && self.folder_dialog.is_none()
            && self.metadata_editor.is_none()
            && self.delete_dialog.is_none()
            && self.reorder_dialog.is_none()
            && !self.folder_refresh_required
            && self
                .folders
                .get("root")
                .is_some_and(|entries| entries.len() < 1000)
            && !self.folder_pending.contains_key("root")
    }
    pub(super) fn open_folder_edit(&mut self, change: Change) {
        if !self.can_edit_folders() {
            return;
        }
        let mut destinations = vec![Folder::root()];
        if let Some(entries) = self.folders.get("root") {
            for entry in entries {
                if let LibraryEntry::Folder { id, name, .. } = entry
                    && entries
                        .iter()
                        .filter(|e| matches!(e,LibraryEntry::Folder{name:n,..} if n==name))
                        .count()
                        == 1
                {
                    destinations.push(Folder {
                        id: id.clone(),
                        parent: Some("root".into()),
                        name: name.clone(),
                    });
                }
            }
        }
        destinations[1..].sort_by(|a, b| a.name.cmp(&b.name).then(a.id.cmp(&b.id)));
        self.folder_operation = self.folder_operation.wrapping_add(1);
        self.folder_dialog = Some(Dialog {
            user: self.account.unwrap(),
            generation: self.generation,
            operation: self.folder_operation,
            change,
            destinations,
            busy: false,
            error: None,
            cancelled: Arc::new(AtomicBool::new(false)),
        });
    }
    pub(super) fn new_folder(&mut self) {
        self.open_folder_edit(Change::Create {
            parent: Folder::root(),
            name: String::new(),
        });
    }
    pub(super) fn folder_menu(&mut self, response: &egui::Response, folder: Folder) {
        folder_popup(response).show(|ui| {
            if ui
                .add_enabled(self.can_edit_folders(), egui::Button::new("Rename folder…"))
                .clicked()
            {
                self.open_folder_edit(Change::Rename {
                    name: folder.name.clone(),
                    folder,
                });
                ui.close();
            }
            ui.label("Folder deletion and nesting are not enabled.");
        });
    }
    pub(super) fn playlist_folder_menu(
        &mut self,
        response: &egui::Response,
        playlist: &Playlist,
        parent: &str,
    ) {
        folder_popup(response).show(|ui| {
            if ui
                .add_enabled(
                    self.can_edit_folders(),
                    egui::Button::new("Move to folder…"),
                )
                .clicked()
            {
                self.open_folder_edit(Change::MovePlaylist {
                    id: playlist.uuid.clone(),
                    title: playlist.title.clone(),
                    source: parent.into(),
                    destination: Folder::root(),
                });
                ui.close();
            }
        });
    }
    pub(super) fn cancel_folder_edit(&mut self) {
        if let Some(dialog) = self.folder_dialog.take()
            && dialog.busy
        {
            self.folder_refresh_required = true;
            self.folders.clear();
            self.folder_pending.clear();
            self.notice = Some(
                "Folder change may have completed. Refresh folders before another edit.".into(),
            );
        }
    }
    fn folder_scope_current(&self) -> bool {
        self.folder_dialog.as_ref().is_some_and(|d| {
            self.connected && self.account == Some(d.user) && self.generation == d.generation
        })
    }
    fn submit_folder_edit(&mut self) {
        if !self.folder_scope_current() {
            self.cancel_folder_edit();
            return;
        }
        if self.buffering || self.folder_refresh_required {
            return;
        }
        let dialog = self.folder_dialog.as_mut().unwrap();
        if dialog.busy || dialog.error.is_some() || dialog.change.validate().is_err() {
            return;
        }
        let request = Request::ManageFolder {
            user: dialog.user,
            operation: dialog.operation,
            change: dialog.change.clone(),
            cancelled: dialog.cancelled.clone(),
        };
        dialog.busy = true;
        if !self.try_send(request)
            && let Some(dialog) = &mut self.folder_dialog
        {
            dialog.busy = false;
            dialog.error =
                Some("Folder request was not queued. Close and Refresh before retrying.".into());
        }
    }
    pub(super) fn folder_result(
        &mut self,
        user: u64,
        operation: u64,
        result: Result<Outcome, String>,
    ) {
        if !self.folder_scope_current()
            || !self
                .folder_dialog
                .as_ref()
                .is_some_and(|d| d.user == user && d.operation == operation && d.busy)
        {
            return;
        }
        self.folders.clear();
        self.folder_pending.clear();
        match result {
            Ok(outcome) if outcome.verified => {
                if let Some(folder) = outcome.created {
                    self.expanded.insert(folder.id);
                }
                self.folder_dialog = None;
                self.notice = Some("Folder change verified. Playback queue unchanged.".into());
                self.refresh_folders();
            }
            result => {
                let dialog = self.folder_dialog.as_mut().unwrap();
                dialog.busy = false;
                dialog.error=Some(result.err().unwrap_or_else(||"The change may have completed, but read-back was inconclusive. Refresh and inspect before retrying.".into()));
                dialog.cancelled.store(true, Ordering::Release);
                self.folder_refresh_required = true;
            }
        }
    }
    pub(super) fn folder_window(&mut self, ctx: &egui::Context) {
        if self.folder_dialog.is_some() && !self.folder_scope_current() {
            self.cancel_folder_edit();
        }
        let Some(dialog) = &mut self.folder_dialog else {
            return;
        };
        let mut submit = false;
        let mut close = false;
        let mut refresh = false;
        egui::Window::new("Organize folders").id(egui::Id::new("folder-edit")).anchor(egui::Align2::CENTER_CENTER,egui::Vec2::ZERO).collapsible(false).resizable(false).default_width(410.).show(ctx,|ui| {
            ui.add_enabled_ui(!dialog.busy && dialog.error.is_none(),|ui|match &mut dialog.change {
                Change::Create {name,..}=>{ui.label("Create a folder in your library root:");ui.add(egui::TextEdit::singleline(name).char_limit(512));}
                Change::Rename {folder,name}=>{ui.label(format!("Rename {}:",folder.name));ui.add(egui::TextEdit::singleline(name).char_limit(512));}
                Change::MovePlaylist {title,destination,source,..}=>{
                    ui.label(format!("Move {title} to:"));
                    egui::ComboBox::from_id_salt("folder-destination").selected_text(&destination.name).show_ui(ui,|ui| {
                        for folder in &dialog.destinations {
                            if ui.add_enabled(&folder.id!=source,egui::Button::new(&folder.name).selected(folder==destination)).clicked() {*destination=folder.clone();}
                        }
                    });
                    ui.label("Only uniquely named root folders are listed. Rename duplicates before choosing a destination.");
                }
            });
            ui.label("Your playback queue stays unchanged. Changes made in another app can conflict. We verify the result; if it is uncertain, Refresh and inspect before trying again.");
            if let Some(error)=&dialog.error {
                ui.colored_label(Color32::LIGHT_RED,error);refresh=ui.button("Refresh folders and inspect").clicked();close=ui.button("Close").clicked();
            } else if dialog.busy {
                ui.spinner();ui.label("Checking and applying folder change…");close=ui.button("Stop waiting").clicked();
            } else {
                let label=match dialog.change {Change::Create {..}=>"Create folder",Change::Rename {..}=>"Save folder name",Change::MovePlaylist {..}=>"Move playlist"};
                ui.horizontal(|ui| {submit=ui.add_enabled(!self.buffering && dialog.change.validate().is_ok(),egui::Button::new(label)).clicked();close=ui.button("Cancel").clicked();});
            }
        });
        if submit {
            self.submit_folder_edit();
        }
        if close || refresh {
            self.cancel_folder_edit();
        }
        if refresh {
            self.refresh_folders();
        }
    }
}
