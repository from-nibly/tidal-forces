use super::*;
use crate::api::LibraryPage;
use std::collections::HashSet;

#[derive(Default)]
pub(super) struct Cache {
    serial: u64,
    job: Option<Job>,
    status: Option<String>,
    partial: Vec<String>,
}
struct Job {
    user: u64,
    kind: usize,
    offset: usize,
    total: Option<u64>,
    ids: HashSet<u64>,
    in_flight: bool,
    incomplete: Option<&'static str>,
}

impl App {
    pub(super) fn observe_library_membership(
        &mut self,
        kind: FavoriteKind,
        page: &LibraryPage,
        append: bool,
    ) {
        let ids: Vec<_> = page
            .data
            .tracks
            .iter()
            .map(|x| x.id)
            .chain(page.data.albums.iter().map(|x| x.id))
            .chain(page.data.artists.iter().map(|x| x.id))
            .collect();
        if !append {
            self.favorites.invalidate(kind);
            self.library_read_valid = page.total.is_some();
        } else {
            self.library_read_valid &= self.library_total == page.total;
        }
        let start = if append { self.library_offset } else { 0 };
        self.library_read_valid &=
            page.next_offset.checked_sub(start) == Some(ids.len()) && ids.iter().all(|id| *id != 0);
        for id in &ids {
            if *id != 0 {
                self.favorites.observe(Favorite::new(kind, *id));
            }
        }
        if !page.more && self.library_read_valid && page.total == Some(page.next_offset as u64) {
            let mut unique: HashSet<_> = ids.into_iter().collect();
            if append {
                unique.extend(
                    self.tracks
                        .iter()
                        .map(|x| x.id)
                        .chain(self.albums.iter().map(|x| x.id))
                        .chain(self.artists.iter().map(|x| x.id)),
                );
            }
            if unique.len() == page.next_offset {
                self.favorites.mark_complete(kind);
            }
        }
    }

    pub(super) fn start_favorite_cache(&mut self) {
        if self.favorites.has_pending() {
            return;
        }
        self.favorite_cache.serial = self.favorite_cache.serial.wrapping_add(1);
        self.favorite_cache.job = self
            .account
            .filter(|user| *user != 0 && self.connected)
            .map(|user| Job {
                user,
                kind: 0,
                offset: 0,
                total: None,
                ids: HashSet::new(),
                in_flight: false,
                incomplete: None,
            });
        self.favorite_cache.status = None;
        self.favorite_cache.partial.clear();
        self.favorites = Favorites::default();
    }
    pub(super) fn stop_favorite_cache(&mut self, message: &str) {
        if self.favorite_cache.job.take().is_some() {
            self.favorite_cache.serial = self.favorite_cache.serial.wrapping_add(1);
            self.favorite_cache.status = Some(message.into());
        }
    }
    pub(super) fn favorite_cache_tick(&mut self) {
        let Some(job) = &self.favorite_cache.job else {
            return;
        };
        if !self.connected || self.account != Some(job.user) || self.favorites.has_pending() {
            self.stop_favorite_cache(
                "Saved-status check stopped after an account or favorites change.",
            );
            return;
        }
        if job.in_flight || self.loading || self.buffering {
            return;
        }
        let request = Request::FavoriteMembership {
            user: job.user,
            serial: self.favorite_cache.serial,
            kind: FavoriteKind::ALL[job.kind],
            offset: job.offset,
        };
        // Background reads yield rather than surfacing a full foreground queue as an error.
        match self.backend.tx.try_send(request) {
            Ok(()) => self.favorite_cache.job.as_mut().unwrap().in_flight = true,
            Err(tokio::sync::mpsc::error::TrySendError::Full(_)) => {}
            Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => self
                .stop_favorite_cache("Saved status could not be checked. Reopen the app to retry."),
        }
    }
    pub(super) fn favorite_cache_page(
        &mut self,
        user: u64,
        serial: u64,
        kind: FavoriteKind,
        offset: usize,
        result: Result<LibraryPage, String>,
    ) {
        if !self.connected || self.favorites.has_pending() {
            self.stop_favorite_cache(
                "Saved-status check stopped after an account or favorites change.",
            );
            return;
        }
        let Some(job) = &mut self.favorite_cache.job else {
            return;
        };
        if self.account != Some(user)
            || user != job.user
            || serial != self.favorite_cache.serial
            || kind != FavoriteKind::ALL[job.kind]
            || offset != job.offset
            || !job.in_flight
        {
            return;
        }
        let page = match result {
            Ok(page) => page,
            Err(message) => {
                self.stop_favorite_cache(&format!(
                    "Could not check saved {}: {message}",
                    kind.label().to_lowercase()
                ));
                return;
            }
        };
        let ids: Vec<u64> = match kind {
            FavoriteKind::Tracks => page.data.tracks.iter().map(|item| item.id).collect(),
            FavoriteKind::Albums => page.data.albums.iter().map(|item| item.id).collect(),
            FavoriteKind::Artists => page.data.artists.iter().map(|item| item.id).collect(),
        };
        let raw_count = page.next_offset.checked_sub(offset);
        let reason = if page
            .total
            .is_some_and(|total| total > crate::queue::MAX_ENTRIES as u64)
            || page.next_offset > crate::queue::MAX_ENTRIES
            || (page.more && page.next_offset == crate::queue::MAX_ENTRIES)
        {
            Some("the 50,000-entry check limit was reached")
        } else if offset != 0 && job.total != page.total {
            Some("the total changed during the check")
        } else if raw_count.is_none_or(|count| count > 100 || ids.len() > count) {
            Some("TIDAL returned an inconsistent page range")
        } else if page.more && raw_count == Some(0) {
            Some("TIDAL returned an empty unfinished page")
        } else if page.total.is_some_and(|total| {
            page.next_offset as u64 > total || page.more != ((page.next_offset as u64) < total)
        }) || (page.total.is_none() && page.more != (raw_count == Some(100)))
        {
            Some("TIDAL returned inconsistent page boundaries")
        } else {
            None
        };
        if let Some(reason) = reason {
            self.stop_favorite_cache(&format!(
                "Could not finish checking saved {}: {reason}. Unchecked items remain unknown.",
                kind.label().to_lowercase()
            ));
            return;
        }
        let mut added = 0;
        let mut duplicate = false;
        for id in ids.iter().filter(|id| **id != 0) {
            if job.ids.insert(*id) {
                added += 1;
            } else {
                duplicate = true;
            }
        }
        // Overlapping pages prove presence, but cannot prove that every item
        // was seen. A wholly repeated page must not drive further reads.
        let repeated_page = offset > 0 && duplicate && added == 0;
        if duplicate {
            job.incomplete = Some("TIDAL returned repeated entries");
        }
        if page.total.is_none() {
            job.incomplete = Some("TIDAL did not provide a total");
        } else if raw_count != Some(ids.len()) || ids.contains(&0) {
            job.incomplete = Some("some entries could not be identified");
        }
        job.offset = page.next_offset;
        job.total = page.total;
        job.in_flight = false;
        if page.more && !repeated_page {
            return;
        }
        // Identified items establish presence even with holes; absence requires
        // complete coverage. Never clear prior observations based on an incomplete read.
        if job.incomplete.is_none() {
            self.favorites.invalidate(kind);
        }
        for id in &job.ids {
            self.favorites.observe(Favorite::new(kind, *id));
        }
        if let Some(reason) = job.incomplete {
            self.favorites.mark_incomplete(kind);
            self.favorite_cache
                .partial
                .push(format!("Saved {}: {reason}.", kind.label().to_lowercase()));
        } else {
            self.favorites.mark_complete(kind);
        }
        job.kind += 1;
        job.offset = 0;
        job.total = None;
        job.ids.clear();
        job.incomplete = None;
        if job.kind == FavoriteKind::ALL.len() {
            self.favorite_cache.job = None;
            self.favorite_cache.status = Some(if self.favorite_cache.partial.is_empty() {
                "Saved status loaded. Refresh after changes in another app.".into()
            } else {
                format!(
                    "{} Available items were loaded; unchecked items remain unknown.",
                    self.favorite_cache.partial.join(" ")
                )
            });
        }
    }
    pub(super) fn favorite_cache_controls(&mut self, ui: &mut egui::Ui) {
        surfaces::section_title(ui, "Favorite status");
        ui.label("Checks saved tracks, albums and artists after sign-in. Kept in memory for this account, not on disk. Does not change your favorites.");
        if let Some(job) = &self.favorite_cache.job {
            ui.label(format!(
                "Checking {} · {} entries read",
                FavoriteKind::ALL[job.kind].label().to_lowercase(),
                job.offset
            ));
            if ui.button("Stop checking").clicked() {
                self.stop_favorite_cache(
                    "Saved-status check stopped. Unknown items keep their actions menu.",
                );
            }
        } else {
            if let Some(message) = &self.favorite_cache.status {
                ui.label(message);
            }
            if ui
                .add_enabled(
                    self.connected && !self.favorites.has_pending(),
                    egui::Button::new("Refresh saved status"),
                )
                .clicked()
            {
                self.start_favorite_cache();
            }
        }
    }
}

#[cfg(test)]
#[path = "favorite_cache_tests.rs"]
mod tests;
