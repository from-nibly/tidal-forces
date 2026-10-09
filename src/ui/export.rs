use super::*;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

#[derive(Default)]
pub(super) struct Export {
    open: bool,
    serial: u64,
    transfer: Option<Transfer>,
}
struct Transfer {
    user: u64,
    operation: u64,
    ids: Vec<u64>,
    title: String,
    description: String,
    started: bool,
    busy: bool,
    confirmed: usize,
    playlist: Option<Playlist>,
    etag: Option<String>,
    error: Option<String>,
    cancelled: Arc<AtomicBool>,
}
impl App {
    pub(super) fn cancel_export(&mut self, forget: bool) {
        if let Some(transfer) = &self.export.transfer {
            transfer.cancelled.store(true, Ordering::Release);
        }
        if forget {
            self.export.transfer = None;
            self.export.open = false;
        }
    }
    pub(super) fn open_queue_export(&mut self) {
        if self.export.transfer.is_some() {
            self.export.open = true;
            return;
        }
        let Some(user) = self.account else {
            return;
        };
        let ids = match self.queue.export_ids() {
            Ok(ids) => ids,
            Err(error) => {
                self.error = Some(error.to_string());
                return;
            }
        };
        self.export.serial = self.export.serial.wrapping_add(1);
        self.export.transfer = Some(Transfer {
            user,
            operation: self.export.serial,
            ids,
            title: format!("Queue · {}", self.queue.title())
                .chars()
                .take(200)
                .collect(),
            description: String::new(),
            started: false,
            busy: false,
            confirmed: 0,
            playlist: None,
            etag: None,
            error: None,
            cancelled: Arc::new(AtomicBool::new(false)),
        });
        self.export.open = true;
    }
    pub(super) fn export_error(&mut self, message: String) {
        if let Some(transfer) = &mut self.export.transfer {
            transfer.busy = false;
            transfer.error = Some(message);
        }
    }
    pub(super) fn export_created(
        &mut self,
        user: u64,
        operation: u64,
        result: Result<Playlist, String>,
    ) {
        if self.account != Some(user) {
            return;
        }
        let Some(transfer) = self.export.transfer.as_mut().filter(|transfer| {
            transfer.user == user
                && transfer.operation == operation
                && transfer.busy
                && transfer.playlist.is_none()
        }) else {
            return;
        };
        transfer.busy = false;
        match result {
            Ok(playlist) => transfer.playlist = Some(playlist),
            Err(error) => transfer.error = Some(error),
        }
        self.refresh_folders();
    }
    pub(super) fn export_saved(
        &mut self,
        user: u64,
        operation: u64,
        offset: usize,
        result: Result<String, String>,
    ) {
        if self.account != Some(user) {
            return;
        }
        let Some(transfer) = self.export.transfer.as_mut().filter(|transfer| {
            transfer.user == user
                && transfer.operation == operation
                && transfer.busy
                && transfer.playlist.is_some()
                && transfer.confirmed == offset
        }) else {
            return;
        };
        transfer.busy = false;
        match result {
            Ok(etag) => {
                transfer.confirmed = (offset + 100).min(transfer.ids.len());
                transfer.etag = Some(etag);
            }
            Err(error) => transfer.error = Some(error),
        }
        if transfer.confirmed == transfer.ids.len() {
            self.notice =
                Some("Queue saved and read-back verified in your new TIDAL playlist.".into());
        }
    }
    pub(super) fn export_tick(&mut self) {
        let Some(transfer) = &mut self.export.transfer else {
            return;
        };
        if self.account != Some(transfer.user)
            || !transfer.started
            || transfer.busy
            || transfer.cancelled.load(Ordering::Acquire)
            || transfer.error.is_some()
            || transfer.confirmed == transfer.ids.len()
        {
            return;
        }
        let request = if let Some(playlist) = &transfer.playlist {
            Request::AppendQueueBatch {
                user: transfer.user,
                operation: transfer.operation,
                id: playlist.uuid.clone(),
                tracks: transfer.ids
                    [transfer.confirmed..(transfer.confirmed + 100).min(transfer.ids.len())]
                    .to_vec(),
                offset: transfer.confirmed,
                etag: transfer.etag.clone(),
                cancelled: transfer.cancelled.clone(),
            }
        } else {
            Request::CreateQueuePlaylist {
                user: transfer.user,
                operation: transfer.operation,
                title: transfer.title.clone(),
                description: transfer.description.clone(),
                cancelled: transfer.cancelled.clone(),
            }
        };
        transfer.busy = true;
        self.try_send(request);
    }
    pub(super) fn export_window(&mut self, ctx: &egui::Context) {
        if !self.export.open {
            return;
        }
        let mut open = true;
        let mut cancel = false;
        let mut reset = false;
        let mut destination = None;
        egui::Window::new("Save queue as TIDAL playlist").open(&mut open).default_width(460.)
            .vscroll(true).max_height((ctx.content_rect().height() - 120.).max(160.)).show(ctx, |ui| {
                let Some(transfer) = &mut self.export.transfer else { return; };
                ui.label(format!("{} current/upcoming entries · duplicates preserved", transfer.ids.len()));
                ui.label("This is a fixed snapshot in playback order, not future repeat cycles or past tracks. Later queue edits do not change this export.");
                if !transfer.started {
                    ui.label("Playlist name"); ui.add(egui::TextEdit::singleline(&mut transfer.title).char_limit(200));
                    ui.label("Description"); ui.add(egui::TextEdit::multiline(&mut transfer.description).char_limit(1000).desired_rows(2));
                    ui.label("Creates a real playlist in your TIDAL account. Batches are revision-guarded and read back before continuing. If the server cannot confirm a write/revision, saving stops. A failed or cancelled export can leave a partial playlist; no automatic retry or deletion occurs.");
                    if ui.add_enabled(!transfer.title.trim().is_empty(), egui::Button::new("Create playlist and save snapshot")).clicked() { transfer.started = true; }
                } else {
                    ui.label(format!("{} / {} entries read-back verified", transfer.confirmed, transfer.ids.len()));
                    if transfer.busy { ui.spinner(); }
                    if let Some(error) = &transfer.error { ui.colored_label(Color32::LIGHT_RED, error); ui.label("The last request may have completed. Inspect the playlist or refresh Library before starting another export."); }
                    if transfer.cancelled.load(Ordering::Acquire) { ui.label("Cancelled: no more batches will be sent. The in-flight request may finish; inspect the destination."); }
                    if let Some(playlist) = &transfer.playlist && ui.button("Inspect destination playlist").clicked() {
                        destination = Some(Page::Collection { kind:"playlists".into(), id:playlist.uuid.clone(), title:playlist.title.clone() });
                    }
                    if transfer.confirmed != transfer.ids.len() && !transfer.cancelled.load(Ordering::Acquire) && ui.button("Cancel remaining export").clicked() { cancel = true; }
                }
                if !transfer.busy && ui.button("Start a new queue snapshot…").clicked() { reset = true; }
            });
        if !open || cancel {
            self.cancel_export(false);
        }
        self.export.open = open;
        if reset {
            self.cancel_export(true);
            self.open_queue_export();
        }
        if let Some(page) = destination {
            self.navigate(page);
        }
    }
}

#[cfg(test)]
#[path = "export_tests.rs"]
mod tests;
