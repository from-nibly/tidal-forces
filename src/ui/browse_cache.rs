use super::*;
use std::collections::VecDeque;

const MAX_VIEWS: usize = 8;
const MAX_ROWS: usize = 50_000;

#[derive(Default)]
pub(super) struct Cache {
    pub(super) owner: Option<u64>,
    entries: VecDeque<Snapshot>,
    pub ready: bool,
    pub restored: bool,
    pub scroll: f32,
    pub pending_scroll: Option<f32>,
}
struct Snapshot {
    location: Location,
    tracks: Vec<Track>,
    albums: Vec<Album>,
    album_position: Option<album_load::Position>,
    artists: Vec<Artist>,
    playlist: Option<PlaylistPage>,
    more: bool,
    library_offset: usize,
    library_total: Option<u64>,
    selection: selection::Selection,
    filter: String,
    sort: track_view::Sort,
    reverse: bool,
    card_filter: String,
    card_sort: library_view::Sort,
    card_reverse: bool,
    scroll: f32,
}
impl Snapshot {
    fn rows(&self) -> usize {
        self.tracks.len() + self.albums.len() + self.artists.len()
    }
}
fn same_location(a: &Location, b: &Location) -> bool {
    a.query == b.query
        && match (&a.page, &b.page) {
            (
                Page::Collection {
                    kind: ak, id: ai, ..
                },
                Page::Collection {
                    kind: bk, id: bi, ..
                },
            ) => ak == bk && ai == bi,
            (Page::Mix { id: a, .. }, Page::Mix { id: b, .. }) => a == b,
            _ => a.page == b.page,
        }
}
impl App {
    pub(super) fn invalidate_browse_cache(&mut self) {
        self.library_load = None;
        self.album_load = None;
        self.browse_cache.entries.clear();
        if self.browse_cache.owner != self.account || !self.connected {
            self.browse_cache.restored = false;
        }
        self.browse_cache.owner = self.account;
        self.browse_cache.ready = false;
    }
    pub(super) fn browse_loaded(&mut self) {
        if self.browse_cache.owner != self.account {
            self.invalidate_browse_cache();
        }
        self.browse_cache.ready = true;
    }
    pub(super) fn fresh_browse_read(&mut self) {
        let location = self.location();
        self.browse_cache
            .entries
            .retain(|entry| !same_location(&entry.location, &location));
        self.browse_cache.ready = false;
    }
    pub(super) fn save_browse_view(&mut self) {
        if !self.connected
            || self.account.is_none_or(|user| user == 0)
            || self.browse_cache.owner != self.account
            || !self.browse_cache.ready
            || self.loading
            || matches!(self.page, Page::Home | Page::Settings | Page::Playlists)
        {
            return;
        }
        let location = self.location();
        self.browse_cache
            .entries
            .retain(|entry| !same_location(&entry.location, &location));
        let rows = self.tracks.len() + self.albums.len() + self.artists.len();
        if rows > MAX_ROWS {
            return;
        }
        while self.browse_cache.entries.len() >= MAX_VIEWS
            || self
                .browse_cache
                .entries
                .iter()
                .map(Snapshot::rows)
                .sum::<usize>()
                + rows
                > MAX_ROWS
        {
            self.browse_cache.entries.pop_front();
        }
        self.track_selection
            .sync(self.account, self.generation, self.tracks.len());
        self.sync_track_view();
        self.browse_cache.entries.push_back(Snapshot {
            location,
            tracks: std::mem::take(&mut self.tracks),
            albums: std::mem::take(&mut self.albums),
            album_position: self.album_position.take(),
            artists: std::mem::take(&mut self.artists),
            playlist: self.playlist_page.take(),
            more: self.more,
            library_offset: self.library_offset,
            library_total: self.library_total,
            selection: std::mem::take(&mut self.track_selection),
            filter: std::mem::take(&mut self.track_view.query),
            sort: self.track_view.sort,
            reverse: self.track_view.reverse,
            card_filter: std::mem::take(&mut self.library_view.query),
            card_sort: self.library_view.sort,
            card_reverse: self.library_view.reverse,
            scroll: self.browse_cache.scroll,
        });
        self.browse_cache.ready = false;
    }
    pub(super) fn restore_browse_view(&mut self, location: &Location) -> bool {
        if !self.connected || self.browse_cache.owner != self.account {
            self.invalidate_browse_cache();
            return false;
        }
        let Some(index) = self
            .browse_cache
            .entries
            .iter()
            .position(|entry| same_location(&entry.location, location))
        else {
            return false;
        };
        let snapshot = self.browse_cache.entries.remove(index).unwrap();
        self.page = snapshot.location.page;
        self.tracks = snapshot.tracks;
        self.albums = snapshot.albums;
        self.album_position = snapshot.album_position;
        self.artists = snapshot.artists;
        self.playlist_page = snapshot.playlist;
        self.more = snapshot.more;
        self.library_offset = snapshot.library_offset;
        self.library_total = snapshot.library_total;
        // Cached rows cannot establish membership completeness after a newer
        // independent favorite check has changed or cleared that knowledge.
        self.library_read_valid = false;
        self.track_selection = snapshot.selection;
        self.track_selection
            .restore_scope(self.account, self.generation, self.tracks.len());
        self.track_view = track_view::View::default();
        self.track_view.query = snapshot.filter;
        self.track_view.sort = snapshot.sort;
        self.track_view.reverse = snapshot.reverse;
        self.library_view = library_view::View::default();
        self.library_view.query = snapshot.card_filter;
        self.library_view.sort = snapshot.card_sort;
        self.library_view.reverse = snapshot.card_reverse;
        self.browse_cache.scroll = snapshot.scroll;
        self.browse_cache.pending_scroll = Some(snapshot.scroll);
        self.browse_cache.ready = true;
        self.browse_cache.restored = true;
        self.autoplay_page = false;
        true
    }
}

#[cfg(test)]
#[path = "browse_cache_tests.rs"]
mod tests;
