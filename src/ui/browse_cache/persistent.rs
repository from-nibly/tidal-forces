use super::*;
use crate::metadata_cache::{self, Record};

impl App {
    pub(in crate::ui) fn has_persisted_view(&self, location: &Location) -> bool {
        self.appearance.metadata_cache
            && !matches!(location.page, Page::Track(_) | Page::Radio(_))
            && self.browse_cache.owner == self.account
            && self
                .browse_cache
                .entries
                .iter()
                .any(|entry| entry.persisted && same_location(&entry.location, location))
    }
    pub(in crate::ui) fn load_metadata_cache(&mut self) {
        if !self.appearance.metadata_cache || !self.connected {
            return;
        }
        let (Some(user), Some(store)) = (self.account, &self.state_store) else {
            return;
        };
        self.browse_cache.disk_serial = self.browse_cache.disk_serial.wrapping_add(1);
        self.browse_cache.disk_loaded = None;
        store.load_cache(user, self.browse_cache.disk_serial);
    }
    pub(in crate::ui) fn persist_metadata(&mut self, current: bool) {
        if !self.appearance.metadata_cache || !self.connected {
            return;
        }
        let (Some(user), Some(_)) = (self.account, &self.state_store) else {
            return;
        };
        if user == 0
            || self.browse_cache.owner != Some(user)
            || self.browse_cache.disk_loaded != Some(user)
        {
            return;
        }
        let mut entries = Vec::new();
        let mut count = 0;
        if current && self.browse_cache.ready && !self.loading {
            add_snapshot(
                &mut entries,
                &mut count,
                &self.location(),
                &self.tracks,
                &self.albums,
                &self.artists,
            );
        }
        for snapshot in self.browse_cache.entries.iter().rev() {
            if entries
                .iter()
                .any(|e| same_location(&e.location, &snapshot.location))
            {
                continue;
            }
            add_snapshot(
                &mut entries,
                &mut count,
                &snapshot.location,
                &snapshot.tracks,
                &snapshot.albums,
                &snapshot.artists,
            );
        }
        entries.reverse();
        self.write_metadata_cache(Record::new(user, entries));
    }
    fn write_metadata_cache(&mut self, record: Record) {
        let Some(store) = &self.state_store else {
            return;
        };
        self.browse_cache.disk_serial = self.browse_cache.disk_serial.wrapping_add(1);
        self.browse_cache.disk_status = if store.cache(self.browse_cache.disk_serial, record) {
            "Updating metadata cache…"
        } else {
            "Metadata cache is busy; this update was not queued."
        }
        .into();
    }
    pub(in crate::ui) fn metadata_cache_loaded(
        &mut self,
        user: u64,
        request: u64,
        result: Result<Record, String>,
    ) {
        if self.account != Some(user)
            || !self.appearance.metadata_cache
            || self.browse_cache.disk_serial != request
        {
            return;
        }
        match result {
            Err(error) => self.browse_cache.disk_status = error,
            Ok(record) => {
                if record.user != user {
                    return;
                }
                self.browse_cache.disk_loaded = Some(user);
                for item in record.entries.into_iter().rev() {
                    if self.browse_cache.entries.len() >= MAX_VIEWS
                        || self
                            .browse_cache
                            .entries
                            .iter()
                            .map(Snapshot::rows)
                            .sum::<usize>()
                            + item.rows()
                            > MAX_ROWS
                    {
                        continue;
                    }
                    if self
                        .browse_cache
                        .entries
                        .iter()
                        .any(|e| same_location(&e.location, &item.location))
                    {
                        continue;
                    }
                    self.browse_cache.entries.push_front(Snapshot {
                        persisted: true,
                        location: item.location,
                        tracks: item.tracks,
                        albums: item.albums,
                        artists: item.artists,
                        // Never restore remote-write authority or pagination from disk.
                        playlist: None,
                        album_position: None,
                        more: false,
                        library_offset: 0,
                        library_total: None,
                        selection: Default::default(),
                        filter: String::new(),
                        sort: Default::default(),
                        reverse: false,
                        card_filter: String::new(),
                        card_sort: Default::default(),
                        card_reverse: false,
                        scroll: 0.,
                    });
                }
                self.browse_cache.disk_status =
                    "Saved metadata loaded. Snapshots require Refresh for current data.".into();
            }
        }
    }
    pub(in crate::ui) fn metadata_cache_saved(
        &mut self,
        user: u64,
        request: u64,
        result: Result<(), String>,
    ) {
        if self.account == Some(user) && self.browse_cache.disk_serial == request {
            self.browse_cache.disk_status = result
                .err()
                .unwrap_or_else(|| "Metadata cache updated.".into());
        }
    }
    pub(in crate::ui) fn metadata_cache_controls(&mut self, ui: &mut egui::Ui) {
        surfaces::section_title(ui, "Persistent browsing cache");
        if ui
            .checkbox(
                &mut self.appearance.metadata_cache,
                "Remember loaded metadata across restarts",
            )
            .changed()
        {
            self.browse_cache.disk_serial = self.browse_cache.disk_serial.wrapping_add(1);
            self.save_appearance();
            if self.appearance.metadata_cache {
                self.load_metadata_cache();
            } else {
                self.browse_cache.entries.retain(|s| !s.persisted);
            }
        }
        ui.label("Private per-account snapshots include loaded metadata and search queries: up to 8 views, 10,000 rows and 8 MiB. Larger views may be skipped. Filters and selection reset; cached views cannot edit playlists or continue loading until Refresh.");
        ui.label("Disabling stops new caching but retains files; in-flight saves may finish. Clear empties this account's saved cache; current displayed data stays. No audio, tokens or stream URLs are cached.");
        if ui
            .add_enabled(
                self.connected && self.state_store.is_some(),
                egui::Button::new("Clear this account's cached metadata"),
            )
            .clicked()
        {
            self.browse_cache.entries.clear();
            if let Some(user) = self.account {
                self.browse_cache.disk_loaded = Some(user);
                self.write_metadata_cache(Record::new(user, vec![]));
            }
        }
        if !self.browse_cache.disk_status.is_empty() {
            ui.label(&self.browse_cache.disk_status);
        }
    }
}
fn add_snapshot(
    entries: &mut Vec<metadata_cache::Snapshot>,
    count: &mut usize,
    location: &Location,
    tracks: &[Track],
    albums: &[Album],
    artists: &[Artist],
) {
    let rows = tracks.len() + albums.len() + artists.len();
    if entries.len() >= metadata_cache::MAX_VIEWS || *count + rows > metadata_cache::MAX_ROWS {
        return;
    }
    let snapshot = metadata_cache::Snapshot {
        location: location.clone(),
        tracks: tracks.to_vec(),
        albums: albums.to_vec(),
        artists: artists.to_vec(),
    };
    if snapshot.valid() {
        *count += rows;
        entries.push(snapshot);
    }
}
