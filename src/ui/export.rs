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
    clipboard: Option<PasteTarget>,
}
struct PasteTarget {
    user: u64,
    generation: u64,
    playlist: Playlist,
    etag: String,
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
    base_offset: usize,
    paste: bool,
    original_ids: Vec<u64>,
    unique_ids: Option<Vec<u64>>,
    checked_offset: usize,
    existing_ids: std::collections::HashSet<u64>,
    include_duplicates: bool,
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
            // Keep an outstanding clipboard target until its reply arrives, so
            // a late response cannot become a fresh paste into another account.
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
            base_offset: 0,
            paste: false,
            original_ids: Vec::new(),
            checked_offset: 0,
            existing_ids: Default::default(),
            unique_ids: None,
            include_duplicates: false,
            playlist: None,
            etag: None,
            error: None,
            cancelled: Arc::new(AtomicBool::new(false)),
        });
        self.export.open = true;
    }
    fn paste_target(&self) -> Option<PasteTarget> {
        let user = self.account?;
        let Page::Collection { kind, id, .. } = &self.page else {
            return None;
        };
        let page = self.playlist_page.as_ref()?;
        if !self.connected
            || self.loading
            || self.playlist_busy
            || kind != "playlists"
            || *id != page.playlist.uuid
            || !page.editable
            || page.etag.trim().is_empty()
            || page.playlist.number_of_videos != 0
            || page.playlist.number_of_tracks > crate::queue::MAX_ENTRIES as u64
        {
            return None;
        }
        Some(PasteTarget {
            user,
            generation: self.generation,
            playlist: page.playlist.clone(),
            etag: page.etag.clone(),
        })
    }
    pub(super) fn can_paste_tracks(&self) -> bool {
        self.paste_target().is_some()
    }
    pub(super) fn request_track_paste(&mut self, ctx: &egui::Context) {
        if self.export.transfer.is_some() {
            self.export.open = true;
            return;
        }
        if self.export.clipboard.is_some() {
            self.error=Some("A clipboard read is already pending. After copying track links, use Ctrl+V to paste them.".into());
            return;
        }
        if let Some(target) = self.paste_target() {
            if let Some(id) = ctx.memory(|memory| memory.focused()) {
                ctx.memory_mut(|memory| memory.surrender_focus(id));
            }
            self.export.clipboard = Some(target);
            ctx.send_viewport_cmd(egui::ViewportCommand::RequestPaste);
        }
    }
    pub(super) fn handle_track_paste(&mut self, ctx: &egui::Context) {
        if !ctx.input(|input| {
            input
                .events
                .iter()
                .any(|event| matches!(event, egui::Event::Paste(_)))
        }) {
            return;
        }
        let requested = self.export.clipboard.take();
        let editing = ctx
            .memory(|memory| memory.focused())
            .is_some_and(|id| egui::text_edit::TextEditState::load(ctx, id).is_some());
        if editing
            || self.playlist_dialog
            || self.removal.is_some()
            || self.export.open
            || self.credential_close_pending()
            || ctx.input(|input| input.viewport().close_requested())
            || egui::Popup::is_any_open(ctx)
        {
            return;
        }
        let Some(current) = self.paste_target() else {
            return;
        };
        let target = if let Some(requested) = requested {
            if requested.user != current.user
                || requested.generation != current.generation
                || requested.playlist.uuid != current.playlist.uuid
                || requested.etag != current.etag
            {
                self.error=Some("Playlist changed while reading the clipboard. Paste again to choose the current destination.".into());
                return;
            }
            requested
        } else {
            current
        };
        if self.export.transfer.is_some() {
            self.export.open = true;
            ctx.request_repaint();
            return;
        }
        let parsed = ctx
            .input(|input| {
                input.events.iter().find_map(|event| {
                    if let egui::Event::Paste(text) = event {
                        Some(Link::parse_tracks(text))
                    } else {
                        None
                    }
                })
            })
            .unwrap();
        let ids = match parsed {
            Ok(ids) => ids,
            Err(error) => {
                self.error = Some(error.to_string());
                return;
            }
        };
        let base_offset = target.playlist.number_of_tracks as usize;
        self.export.serial = self.export.serial.wrapping_add(1);
        self.export.transfer = Some(Transfer {
            user: target.user,
            operation: self.export.serial,
            original_ids: ids.clone(),
            checked_offset: 0,
            existing_ids: Default::default(),
            unique_ids: None,
            include_duplicates: false,
            ids,
            title: target.playlist.title.clone(),
            description: String::new(),
            started: false,
            busy: false,
            confirmed: 0,
            base_offset,
            paste: true,
            playlist: Some(target.playlist),
            etag: Some(target.etag),
            error: None,
            cancelled: Arc::new(AtomicBool::new(false)),
        });
        self.export.open = true;
        ctx.request_repaint();
    }

    pub(super) fn playlist_duplicates_checked(
        &mut self,
        user: u64,
        operation: u64,
        offset: usize,
        result: Result<Vec<u64>, String>,
    ) {
        if self.account != Some(user) {
            return;
        }
        let Some(transfer) = self.export.transfer.as_mut().filter(|transfer| {
            transfer.user == user
                && transfer.operation == operation
                && transfer.paste
                && !transfer.started
                && transfer.busy
                && transfer.unique_ids.is_none()
                && transfer.checked_offset == offset
                && !transfer.cancelled.load(Ordering::Acquire)
        }) else {
            return;
        };
        transfer.busy = false;
        match result {
            Ok(ids) => {
                if ids.len() > transfer.base_offset - offset
                    || (ids.is_empty() && offset < transfer.base_offset)
                    || ids.contains(&0)
                {
                    transfer.error = Some(
                        "Couldn’t finish checking this playlist. Paste again to retry.".into(),
                    );
                    return;
                }
                transfer.checked_offset += ids.len();
                transfer.existing_ids.extend(ids);
                if transfer.checked_offset == transfer.base_offset {
                    let unique: Vec<_> = transfer
                        .original_ids
                        .iter()
                        .copied()
                        .filter(|id| transfer.existing_ids.insert(*id))
                        .collect();
                    transfer.ids = unique.clone();
                    transfer.unique_ids = Some(unique);
                    transfer.existing_ids.clear();
                }
            }
            Err(error) => transfer.error = Some(error),
        }
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
                && transfer.started
                && transfer.base_offset + transfer.confirmed == offset
        }) else {
            return;
        };
        transfer.busy = false;
        match result {
            Ok(etag) => {
                transfer.confirmed = (offset - transfer.base_offset + 100).min(transfer.ids.len());
                transfer.etag = Some(etag);
            }
            Err(error) => transfer.error = Some(error),
        }
        if transfer.confirmed == transfer.ids.len() {
            self.notice = Some(
                if transfer.paste {
                    "Songs added to your playlist."
                } else {
                    "Queue saved as a playlist."
                }
                .into(),
            );
            if transfer.paste {
                let reload = matches!(&self.page,Page::Collection {kind,id,..} if kind=="playlists" && transfer.playlist.as_ref().is_some_and(|playlist| &playlist.uuid==id));
                self.refresh_folders();
                if reload {
                    self.generation += 1;
                    self.removal = None;
                    self.track_selection.clear();
                    self.load_page(0);
                }
            }
        }
    }
    pub(super) fn export_tick(&mut self) {
        let Some(transfer) = &mut self.export.transfer else {
            return;
        };
        if self.account != Some(transfer.user)
            || transfer.busy
            || transfer.cancelled.load(Ordering::Acquire)
            || transfer.error.is_some()
            || transfer.confirmed == transfer.ids.len()
        {
            return;
        }
        if transfer.paste && transfer.unique_ids.is_none() {
            let request = Request::CheckPlaylistDuplicates {
                user: transfer.user,
                operation: transfer.operation,
                id: transfer.playlist.as_ref().unwrap().uuid.clone(),
                offset: transfer.checked_offset,
                count: transfer.base_offset,
                etag: transfer.etag.clone().unwrap(),
                cancelled: transfer.cancelled.clone(),
            };
            transfer.busy = true;
            self.try_send(request);
            return;
        }
        if !transfer.started
            || transfer.base_offset + transfer.ids.len() > crate::queue::MAX_ENTRIES
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
                offset: transfer.base_offset + transfer.confirmed,
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
        let mut dismiss = false;
        let mut destination = None;
        let paste = self
            .export
            .transfer
            .as_ref()
            .is_some_and(|transfer| transfer.paste);
        egui::Window::new(if paste {"Add songs to playlist"} else {"Save queue as TIDAL playlist"}).open(&mut open).default_width(460.)
            .vscroll(true).max_height((ctx.content_rect().height() - 120.).max(160.)).show(ctx, |ui| {
                let Some(transfer) = &mut self.export.transfer else { return; };
                if paste {
                    if !transfer.started && transfer.unique_ids.is_none() {ui.label(format!("Add songs to ‘{}’",transfer.title));}
                    else if transfer.started {ui.label(format!("Adding songs to ‘{}’",transfer.title));}
                    else if transfer.ids.is_empty() {ui.label(format!("No new songs for ‘{}’",transfer.title));}
                    else {ui.label(format!("Add {} {} to ‘{}’?",transfer.ids.len(),if transfer.ids.len()==1 {"song"} else {"songs"},transfer.title));}
                }
                else {ui.label(format!("{} songs · includes repeated songs",transfer.ids.len()));ui.label("Saves your current queue. Later queue changes won’t affect this playlist.");}
                if !transfer.started {
                    if paste {
                        if let Some(unique)=&transfer.unique_ids {
                            let duplicates=transfer.original_ids.len()-unique.len();
                            if duplicates>0 {
                                ui.label(if transfer.include_duplicates {format!("Including {duplicates} repeated {}.",if duplicates==1 {"song"} else {"songs"})} else {format!("Skipping {duplicates} {}.",if duplicates==1 {"duplicate"} else {"duplicates"})});
                                if ui.checkbox(&mut transfer.include_duplicates,"Add duplicates too").changed() {
                                    transfer.ids=if transfer.include_duplicates {transfer.original_ids.clone()} else {unique.clone()};
                                }
                            }
                            if transfer.ids.is_empty() {ui.label("These songs are already in this playlist.");}
                            if transfer.base_offset+transfer.ids.len()>crate::queue::MAX_ENTRIES {ui.label("This would exceed the 50,000-song playlist limit.");}
                        } else if transfer.error.is_none() {ui.horizontal(|ui|{ui.spinner();ui.label("Checking for duplicates…");});}
                        if let Some(error)=&transfer.error {ui.colored_label(Color32::LIGHT_RED,"Couldn’t check these songs. Nothing has been added.");ui.collapsing("Details",|ui|{ui.label(error);});}
                    } else {
                        ui.label("Playlist name"); ui.add(egui::TextEdit::singleline(&mut transfer.title).char_limit(200));
                        ui.label("Description"); ui.add(egui::TextEdit::multiline(&mut transfer.description).char_limit(1000).desired_rows(2));
                        ui.label("Creates a real playlist in your TIDAL account.");
                    }
                    let ready=if paste {transfer.unique_ids.is_some() && !transfer.ids.is_empty() && transfer.base_offset+transfer.ids.len()<=crate::queue::MAX_ENTRIES && transfer.error.is_none()} else {!transfer.title.trim().is_empty()};
                    if ui.add_enabled(ready, egui::Button::new(if paste {"Add songs"} else {"Create playlist"})).clicked() { transfer.started = true; }
                } else {
                    ui.label(format!("{} of {} {} added", transfer.confirmed, transfer.ids.len(),if transfer.ids.len()==1 {"song"} else {"songs"}));
                    if transfer.busy { ui.spinner(); }
                    if let Some(error) = &transfer.error { ui.colored_label(Color32::LIGHT_RED, error); ui.label("Some songs may already have been added. Check the playlist before trying again."); }
                    if transfer.cancelled.load(Ordering::Acquire) { ui.label("Stopped. Songs already being added may still appear in the playlist."); }
                    if let Some(playlist) = &transfer.playlist && ui.button("View playlist").clicked() {
                        destination = Some(Page::Collection { kind:"playlists".into(), id:playlist.uuid.clone(), title:playlist.title.clone() });
                    }
                    if transfer.confirmed != transfer.ids.len() && !transfer.cancelled.load(Ordering::Acquire) && ui.button("Stop adding songs").clicked() { cancel = true; }
                }
                if (!transfer.busy || !transfer.started) && ui.button(if paste {"Close"} else {"Start a new queue snapshot…"}).clicked() { if paste {dismiss=true;} else {reset=true;} }
            });
        if !open || cancel {
            let forget_draft = !open
                && self
                    .export
                    .transfer
                    .as_ref()
                    .is_some_and(|transfer| !transfer.started);
            self.cancel_export(forget_draft);
        }
        self.export.open = open;
        if dismiss {
            self.cancel_export(true);
        }
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
