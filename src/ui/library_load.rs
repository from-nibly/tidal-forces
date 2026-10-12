use super::*;
use crate::api::LibraryPage;
use std::collections::HashSet;

pub(super) struct LibraryLoad {
    user: u64,
    generation: u64,
    kind: FavoriteKind,
    total: Option<u64>,
    offset: usize,
    seen: HashSet<u64>,
    in_flight: bool,
    partial: bool,
}

impl App {
    fn can_load_library(&self) -> bool {
        matches!(self.page, Page::Library(_))
            && self.account.is_some_and(|user| user != 0)
            && self.connected
            && self.browse_cache.ready
            && !self.loading
            && self.more
            && !self.favorites.has_pending()
            && self.library_offset > 0
            && self.library_offset < crate::queue::MAX_ENTRIES
            && self.library_total.is_none_or(|total| {
                total <= crate::queue::MAX_ENTRIES as u64 && total > self.library_offset as u64
            })
            && self.tracks.len() + self.albums.len() + self.artists.len() <= self.library_offset
    }
    fn library_load_target(&self) -> Option<LibraryLoad> {
        if !self.can_load_library() {
            return None;
        }
        let user = self.account?;
        let Page::Library(kind) = self.page else {
            return None;
        };
        let ids: Vec<_> = match kind {
            FavoriteKind::Tracks => self.tracks.iter().map(|x| x.id).collect(),
            FavoriteKind::Albums => self.albums.iter().map(|x| x.id).collect(),
            FavoriteKind::Artists => self.artists.iter().map(|x| x.id).collect(),
        };
        if ids.len() > self.library_offset {
            return None;
        }
        let seen: HashSet<_> = ids.iter().copied().filter(|id| *id != 0).collect();
        let partial = seen.len() != self.library_offset;
        Some(LibraryLoad {
            user,
            generation: self.generation,
            kind,
            total: self.library_total,
            offset: self.library_offset,
            seen,
            in_flight: false,
            partial,
        })
    }

    pub(super) fn start_library_load(&mut self) {
        if self.library_load.is_none() {
            self.library_load = self.library_load_target();
            if self.library_load.is_some() {
                self.error = None;
                // Favorites have no atomic revision. A long, possibly cached read
                // contributes positive observations, never inferred absence.
                self.library_read_valid = false;
            }
        }
    }

    pub(super) fn library_load_controls(&mut self, ui: &mut egui::Ui) {
        let Page::Library(kind) = self.page else {
            return;
        };
        if self.library_load.is_some() {
            ui.horizontal_wrapped(|ui| {
                ui.spinner();
                ui.label(format!("Loading saved {} · {} entries read", kind.label().to_lowercase(), self.library_offset));
                if ui.button("Stop loading").clicked() { self.library_load = None; }
            });
        } else if self.more && ui.add_enabled(self.can_load_library(),
            egui::Button::new(format!("Load all saved {}", kind.label().to_lowercase())))
            .on_hover_text("Read the remaining saved items, up to 50,000 entries. Does not change playback or favorites. Changes made elsewhere may require Refresh; an in-flight page may finish after stopping.")
            .clicked() { self.start_library_load(); }
    }

    pub(super) fn library_load_tick(&mut self) {
        let Some(load) = &self.library_load else {
            return;
        };
        if self.account != Some(load.user)
            || !self.connected
            || self.generation != load.generation
            || self.page != Page::Library(load.kind)
            || self.favorites.has_pending()
        {
            self.library_load = None;
            return;
        }
        if self.loading || load.in_flight || self.buffering {
            return;
        }
        if !self.more || self.library_offset != load.offset || self.library_total != load.total {
            self.library_load = None;
            return;
        }
        let offset = load.offset;
        self.library_load.as_mut().unwrap().in_flight = true;
        self.load_page(offset);
        if !self.loading {
            self.library_load = None;
        }
    }

    pub(super) fn accept_library_load_page(
        &mut self,
        generation: u64,
        kind: FavoriteKind,
        page: &mut LibraryPage,
        append: bool,
    ) -> bool {
        let Some(load) = &mut self.library_load else {
            return true;
        };
        let advance = page.next_offset.checked_sub(load.offset);
        let size = page.data.tracks.len() + page.data.albums.len() + page.data.artists.len();
        let right_kind = match kind {
            FavoriteKind::Tracks => page.data.albums.is_empty() && page.data.artists.is_empty(),
            FavoriteKind::Albums => page.data.tracks.is_empty() && page.data.artists.is_empty(),
            FavoriteKind::Artists => page.data.tracks.is_empty() && page.data.albums.is_empty(),
        };
        let valid = self.account == Some(load.user)
            && self.connected
            && generation == load.generation
            && self.page == Page::Library(load.kind)
            && kind == load.kind
            && append
            && load.in_flight
            && right_kind
            && page.total == load.total
            && page.next_offset <= crate::queue::MAX_ENTRIES
            && advance.is_some_and(|n| n <= 100 && size <= n && (n > 0 || !page.more))
            && page.total.is_none_or(|total| {
                page.next_offset as u64 <= total && page.more == ((page.next_offset as u64) < total)
            });
        if !valid {
            self.library_load = None;
            self.loading = false;
            self.browse_cache.ready = false;
            self.error = Some("The saved library changed or returned an incomplete page. Loaded items were kept. Refresh before trying again.".into());
            return false;
        }
        let before = load.seen.len();
        page.data
            .tracks
            .retain(|x| x.id != 0 && load.seen.insert(x.id));
        page.data
            .albums
            .retain(|x| x.id != 0 && load.seen.insert(x.id));
        page.data
            .artists
            .retain(|x| x.id != 0 && load.seen.insert(x.id));
        let added = load.seen.len() - before;
        load.partial |= Some(added) != advance;
        load.offset = page.next_offset;
        load.in_flight = false;
        let repeated = size > 0 && added == 0;
        let limit = page.more && page.next_offset == crate::queue::MAX_ENTRIES;
        if !page.more || repeated || limit {
            self.notice = Some(if repeated {
                "Loading stopped because TIDAL repeated a page. Loaded items were kept; Refresh to check for changes.".into()
            } else if limit {
                "Loading stopped at 50,000 entries. Loaded items were kept.".into()
            } else if load.partial || load.total.is_none() {
                "Loaded the available saved items. Some entries could not be verified; unchecked items remain unknown.".into()
            } else {
                "Finished loading saved items. Refresh to check for changes made elsewhere.".into()
            });
            self.library_load = None;
        }
        true
    }
}

#[cfg(test)]
#[path = "library_load_tests.rs"]
mod tests;
