use super::*;
use crate::api::LibraryPage;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Position {
    pub id: String,
    pub offset: usize,
    pub total: Option<u64>,
}

pub(super) struct AlbumLoad {
    user: u64,
    generation: u64,
    position: Position,
    in_flight: bool,
}

impl App {
    pub(super) fn album_page(
        &mut self,
        generation: u64,
        kind: &str,
        id: &str,
        offset: usize,
        mut page: LibraryPage,
    ) {
        if generation != self.generation
            || !self.connected
            || self.account.is_none_or(|user| user == 0)
            || !matches!(&self.page,Page::Collection {kind:k,id:i,..} if k==kind && i==id)
        {
            return;
        }
        let advance = page.next_offset.checked_sub(offset);
        let valid = kind == "albums"
            && offset < crate::queue::MAX_ENTRIES
            && page.next_offset <= crate::queue::MAX_ENTRIES
            && page.data.albums.is_empty()
            && page.data.artists.is_empty()
            && advance
                .is_some_and(|n| n <= 100 && page.data.tracks.len() <= n && (n > 0 || !page.more))
            && page.total.is_none_or(|total| {
                page.next_offset as u64 <= total && page.more == ((page.next_offset as u64) < total)
            })
            && (offset == 0
                || self
                    .album_position
                    .as_ref()
                    .is_some_and(|p| p.id == id && p.offset == offset && p.total == page.total))
            && self.album_load.as_ref().is_none_or(|load| {
                load.user == self.account.unwrap_or(0)
                    && load.generation == generation
                    && load.in_flight
                    && load.position.offset == offset
                    && self.album_position.as_ref() == Some(&load.position)
            });
        if !valid {
            self.album_load = None;
            self.loading = false;
            self.browse_cache.ready = false;
            self.error=Some("The album changed or returned an incomplete page. Loaded tracks were kept; Refresh before trying again.".into());
            return;
        }
        page.data.tracks.retain(|track| track.id != 0);
        if offset == 0 {
            self.track_selection.clear();
            self.track_view.invalidate();
            self.browse_cache.restored = false;
            self.tracks = page.data.tracks;
        } else {
            self.tracks.extend(page.data.tracks);
        }
        let position = Position {
            id: id.into(),
            offset: page.next_offset,
            total: page.total,
        };
        self.album_position = Some(position.clone());
        self.more = page.more;
        self.loading = false;
        self.browse_loaded();
        if let Some(load) = &mut self.album_load {
            load.position = position;
            load.in_flight = false;
            if !self.more || page.next_offset == crate::queue::MAX_ENTRIES {
                self.album_load = None;
                self.notice = Some(
                    if self.more {
                        "Loading stopped at 50,000 entries. Loaded tracks were kept."
                    } else {
                        "Finished loading available album tracks. Refresh to check for changes."
                    }
                    .into(),
                );
            }
        }
    }

    fn can_load_album(&self) -> bool {
        self.connected
            && self.account.is_some_and(|id| id != 0)
            && self.browse_cache.ready
            && !self.loading
            && self.more && self.album_position.as_ref().is_some_and(|p| {
            p.offset > 0
                && p.offset < crate::queue::MAX_ENTRIES
                && p.total
                    .is_none_or(|n| n <= crate::queue::MAX_ENTRIES as u64 && n > p.offset as u64)
                && matches!(&self.page,Page::Collection {kind,id,..} if kind=="albums" && id==&p.id)
        })
    }
    pub(super) fn start_album_load(&mut self) {
        if self.album_load.is_none() && self.can_load_album() {
            self.album_load = Some(AlbumLoad {
                user: self.account.unwrap(),
                generation: self.generation,
                position: self.album_position.clone().unwrap(),
                in_flight: false,
            });
            self.error = None;
        }
    }
    pub(super) fn album_load_controls(&mut self, ui: &mut egui::Ui) {
        if self.album_load.is_some() {
            ui.spinner();ui.label("Loading…");
            if ui.button("Stop loading").clicked() {self.album_load=None;}
        } else if matches!(&self.page,Page::Collection{kind,..} if kind=="albums") && self.more
            && ui.add_enabled(self.can_load_album(),egui::Button::new("Load all album tracks"))
            .on_hover_text("Read remaining album pages, up to 50,000 entries. Unavailable entries still count toward pagination. Does not change playback; an in-flight page may finish after stopping.").clicked() {
            self.start_album_load();
        }
    }
    pub(super) fn album_load_tick(&mut self) {
        let Some(load) = &self.album_load else { return };
        if self.account != Some(load.user)
            || !self.connected
            || self.generation != load.generation
            || !matches!(&self.page,Page::Collection {kind,id,..} if kind=="albums" && id==&load.position.id)
        {
            self.album_load = None;
            return;
        }
        if self.loading || load.in_flight || self.buffering {
            return;
        }
        if !self.more || self.album_position.as_ref() != Some(&load.position) {
            self.album_load = None;
            return;
        }
        let offset = load.position.offset;
        self.album_load.as_mut().unwrap().in_flight = true;
        self.load_page(offset);
        if !self.loading {
            self.album_load = None;
        }
    }
}

#[cfg(test)]
#[path = "album_load_tests.rs"]
mod tests;
