use super::*;

pub(super) struct PlaylistLoad {
    user: u64,
    generation: u64,
    id: String,
    etag: String,
    total: usize,
    offset: usize,
    in_flight: bool,
}
impl App {
    fn playlist_load_target(&self) -> Option<PlaylistLoad> {
        let user = self.account?;
        let Page::Collection { kind, id, .. } = &self.page else {
            return None;
        };
        let page = self.playlist_page.as_ref()?;
        let total = page
            .playlist
            .number_of_tracks
            .checked_add(page.playlist.number_of_videos)?;
        if user == 0
            || !self.connected
            || self.loading
            || !self.more
            || !page.more
            || kind != "playlists"
            || *id != page.playlist.uuid
            || page.etag.trim().is_empty()
            || total > crate::queue::MAX_ENTRIES as u64
            || page.next_offset == 0
            || page.next_offset >= total as usize
        {
            return None;
        }
        Some(PlaylistLoad {
            user,
            generation: self.generation,
            id: id.clone(),
            etag: page.etag.clone(),
            total: total as usize,
            offset: page.next_offset,
            in_flight: false,
        })
    }
    pub(super) fn start_playlist_load(&mut self) {
        if self.playlist_load.is_none() {
            self.playlist_load = self.playlist_load_target();
            if self.playlist_load.is_some() {
                self.error = None;
            }
        }
    }
    pub(super) fn playlist_load_controls(&mut self, ui: &mut egui::Ui) {
        if self.playlist_load.is_some() {
            ui.horizontal(|ui| {ui.spinner();ui.label(format!("Loading playlist · {} songs available",self.tracks.len()));if ui.button("Stop loading").clicked() {self.playlist_load=None;}});
        } else if self.playlist_page.is_some() && self.more && ui.add_enabled(self.playlist_load_target().is_some(),egui::Button::new("Load all playlist songs")).on_hover_text("Read the remaining playlist pages for filtering and sorting. Does not change playback. Limited to 50,000 playlist entries.").clicked() {
            self.start_playlist_load();
        }
    }
    pub(super) fn playlist_load_tick(&mut self) {
        let Some(load) = &self.playlist_load else {
            return;
        };
        let matches = self.account == Some(load.user)
            && self.generation == load.generation
            && matches!(&self.page,Page::Collection {kind,id,..} if kind=="playlists" && *id==load.id);
        if !matches {
            self.playlist_load = None;
            return;
        }
        if self.loading || load.in_flight {
            return;
        }
        let valid = self.playlist_page.as_ref().is_some_and(|page| {
            page.playlist.uuid == load.id
                && page.etag == load.etag
                && page.next_offset == load.offset
                && page
                    .playlist
                    .number_of_tracks
                    .checked_add(page.playlist.number_of_videos)
                    == Some(load.total as u64)
        });
        if !valid || !self.more {
            self.playlist_load = None;
            return;
        }
        let offset = load.offset;
        self.playlist_load.as_mut().unwrap().in_flight = true;
        self.load_page(offset);
        if !self.loading {
            self.playlist_load = None;
        }
    }
    /// Validate before merging a page so a failed full read leaves prior rows intact.
    pub(super) fn accept_playlist_load_page(
        &mut self,
        generation: u64,
        page: &PlaylistPage,
        append: bool,
    ) -> bool {
        let Some(load) = &mut self.playlist_load else {
            return true;
        };
        let valid = self.account == Some(load.user)
            && generation == load.generation
            && append
            && load.in_flight
            && page.playlist.uuid == load.id
            && page.etag == load.etag
            && page
                .playlist
                .number_of_tracks
                .checked_add(page.playlist.number_of_videos)
                == Some(load.total as u64)
            && page.next_offset > load.offset
            && page.next_offset <= load.total
            && page.more == (page.next_offset < load.total)
            && page
                .rows
                .iter()
                .all(|(i, t)| *i >= load.offset && *i < page.next_offset && t.id != 0)
            && page.rows.windows(2).all(|pair| pair[0].0 < pair[1].0);
        if !valid {
            self.playlist_load = None;
            self.loading = false;
            self.error=Some("The playlist changed or couldn’t be loaded completely. Refresh it before trying again.".into());
            return false;
        }
        load.offset = page.next_offset;
        load.in_flight = false;
        if !page.more {
            self.playlist_load = None;
        }
        true
    }
}

#[cfg(test)]
#[path = "playlist_load_tests.rs"]
mod tests;
