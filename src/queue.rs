use crate::model::Track;
use anyhow::{Result, ensure};
use rand::{Rng, seq::SliceRandom};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    sync::Arc,
};

pub const MAX_ENTRIES: usize = 50_000;
const BACK_LIMIT: usize = 100;

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
pub enum Repeat {
    #[default]
    Off,
    Context,
    Track,
}
impl Repeat {
    pub fn next(self) -> Self {
        match self {
            Self::Off => Self::Context,
            Self::Context => Self::Track,
            Self::Track => Self::Off,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "Repeat off",
            Self::Context => "Repeat source",
            Self::Track => "Repeat track",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub enum Continuation {
    Playlist {
        id: String,
        etag: String,
        offset: usize,
    },
    Album {
        id: u64,
        offset: usize,
    },
    Favorites {
        offset: usize,
        total: Option<u64>,
    },
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Entry {
    pub occurrence: u64,
    pub track: Arc<Track>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Queue {
    current: Option<Entry>,
    manual: VecDeque<Entry>,
    context: Vec<Entry>,
    upcoming: VecDeque<usize>,
    back: Vec<Entry>,
    redo: Vec<Entry>,
    title: String,
    continuation: Option<Continuation>,
    repeat: Repeat,
    shuffled: bool,
    next_id: u64,
    #[serde(skip)]
    revision: u64,
}

impl Default for Queue {
    fn default() -> Self {
        Self {
            current: None,
            manual: VecDeque::new(),
            context: Vec::new(),
            upcoming: VecDeque::new(),
            back: Vec::new(),
            redo: Vec::new(),
            title: String::new(),
            continuation: None,
            repeat: Repeat::Off,
            shuffled: false,
            next_id: 1,
            revision: 0,
        }
    }
}

impl Queue {
    pub fn current(&self) -> Option<&Track> {
        self.current.as_ref().map(|entry| entry.track.as_ref())
    }
    pub fn current_entry(&self) -> Option<&Entry> {
        self.current.as_ref()
    }
    pub fn manual(&self) -> &VecDeque<Entry> {
        &self.manual
    }
    pub fn context_upcoming(&self) -> impl Iterator<Item = &Entry> {
        self.redo
            .iter()
            .rev()
            .chain(self.upcoming.iter().map(|&index| &self.context[index]))
    }
    pub fn export_ids(&self) -> Result<Vec<u64>> {
        ensure!(
            self.continuation.is_none(),
            "Finish loading the source before saving the queue; unloaded tracks will not be silently omitted"
        );
        let ids: Vec<_> = self
            .current
            .iter()
            .chain(self.manual.iter())
            .chain(self.context_upcoming())
            .map(|entry| entry.track.id)
            .collect();
        ensure!(
            !ids.is_empty() && ids.len() <= MAX_ENTRIES && ids.iter().all(|id| *id != 0),
            "Save between 1 and 50,000 valid current/upcoming tracks"
        );
        Ok(ids)
    }
    pub fn redo_len(&self) -> usize {
        self.redo.len()
    }
    pub fn title(&self) -> &str {
        &self.title
    }
    pub fn continuation(&self) -> Option<&Continuation> {
        self.continuation.as_ref()
    }
    pub fn revision(&self) -> u64 {
        self.revision
    }
    pub fn repeat(&self) -> Repeat {
        self.repeat
    }
    pub fn shuffled(&self) -> bool {
        self.shuffled
    }
    pub fn set_repeat(&mut self, repeat: Repeat) {
        self.repeat = repeat;
        self.changed();
    }
    pub fn upcoming_len(&self) -> usize {
        self.manual.len() + self.redo.len() + self.upcoming.len()
    }
    fn changed(&mut self) {
        self.revision = self.revision.wrapping_add(1);
    }

    fn entry(&mut self, track: Track) -> Entry {
        let occurrence = self.next_id;
        self.next_id += 1;
        Entry {
            occurrence,
            track: Arc::new(track),
        }
    }
    fn capacity(&self, count: usize) -> Result<()> {
        ensure!(
            count <= MAX_ENTRIES && self.manual.len() + self.context.len() + count <= MAX_ENTRIES,
            "The queue is limited to 50,000 source/manual entries. Clear its source or Up next before adding more."
        );
        ensure!(
            self.next_id.checked_add(count as u64).is_some(),
            "Queue occurrence IDs exhausted; clear saved playback state."
        );
        Ok(())
    }
    fn remember_current(&mut self) {
        if let Some(entry) = self.current.take() {
            if self.back.len() + self.redo.len() == BACK_LIMIT {
                if self.back.is_empty() {
                    self.redo.remove(0);
                } else {
                    self.back.remove(0);
                }
            }
            self.back.push(entry);
        }
    }

    pub fn start(
        &mut self,
        tracks: Vec<Track>,
        index: usize,
        title: String,
        continuation: Option<Continuation>,
    ) -> Result<()> {
        ensure!(index < tracks.len(), "No playable track at that position");
        ensure!(
            tracks.len() + self.manual.len() <= MAX_ENTRIES,
            "This playback source exceeds the queue limit"
        );
        ensure!(
            tracks.iter().all(|track| track.id != 0),
            "Invalid track in playback source"
        );
        ensure!(
            self.next_id.checked_add(tracks.len() as u64).is_some(),
            "Queue occurrence IDs exhausted"
        );
        self.remember_current();
        self.redo.clear();
        self.context = tracks.into_iter().map(|track| self.entry(track)).collect();
        self.current = Some(self.context[index].clone());
        self.upcoming = (index + 1..self.context.len()).collect();
        self.title = title;
        self.continuation = continuation;
        if self.shuffled {
            self.upcoming.make_contiguous().shuffle(&mut rand::rng());
        }
        self.changed();
        Ok(())
    }

    /// Start from the beginning of an explicitly ordered or shuffled collection.
    /// Unloaded continuation pages join the existing order as they arrive.
    pub fn start_collection(
        &mut self,
        tracks: Vec<Track>,
        title: String,
        continuation: Option<Continuation>,
        shuffle: bool,
    ) -> Result<()> {
        self.start(tracks, 0, title, continuation)?;
        self.shuffled = shuffle;
        self.upcoming = (0..self.context.len()).collect();
        if shuffle {
            self.upcoming.make_contiguous().shuffle(&mut rand::rng());
        }
        self.current = self
            .upcoming
            .pop_front()
            .map(|index| self.context[index].clone());
        self.changed();
        Ok(())
    }

    pub fn add(&mut self, track: Track, play_next: bool) -> Result<()> {
        self.capacity(1)?;
        ensure!(track.id != 0, "This track has no valid TIDAL ID");
        let entry = self.entry(track);
        if play_next {
            self.manual.push_front(entry);
        } else {
            self.manual.push_back(entry);
        }
        self.changed();
        Ok(())
    }

    pub fn append(&mut self, tracks: Vec<Track>, continuation: Option<Continuation>) -> Result<()> {
        self.capacity(tracks.len())?;
        ensure!(
            tracks.iter().all(|track| track.id != 0),
            "Invalid track in source continuation"
        );
        if matches!(self.continuation, Some(Continuation::Favorites { .. })) {
            let mut seen: HashSet<_> = self.context.iter().map(|entry| entry.track.id).collect();
            ensure!(
                tracks.iter().all(|track| seen.insert(track.id)),
                "Favorites changed while loading the queue. Refresh the library and play it again."
            );
        }
        let mut rng = rand::rng();
        for track in tracks {
            let index = self.context.len();
            let entry = self.entry(track);
            self.context.push(entry);
            if self.shuffled {
                // New pages join the shuffle without rearranging already queued entries.
                self.upcoming
                    .insert(rng.random_range(0..=self.upcoming.len()), index);
            } else {
                self.upcoming.push_back(index);
            }
        }
        self.continuation = continuation;
        self.changed();
        Ok(())
    }

    pub fn toggle_shuffle(&mut self) {
        self.shuffled = !self.shuffled;
        if self.shuffled {
            self.upcoming.make_contiguous().shuffle(&mut rand::rng());
        } else {
            self.upcoming.make_contiguous().sort_unstable();
        }
        self.changed();
    }

    pub fn advance(&mut self, natural_end: bool) -> bool {
        if natural_end && self.repeat == Repeat::Track && self.current.is_some() {
            return true;
        }
        let next = if let Some(entry) = self.manual.pop_front() {
            Some(entry)
        } else if let Some(entry) = self.redo.pop() {
            Some(entry)
        } else if let Some(index) = self.upcoming.pop_front() {
            Some(self.context[index].clone())
        } else if self.repeat == Repeat::Context
            && self.continuation.is_none()
            && !self.context.is_empty()
            && self
                .next_id
                .checked_add(self.context.len() as u64)
                .is_some()
        {
            // A new cycle has fresh occurrence IDs; previous-cycle history stays unambiguous.
            for entry in &mut self.context {
                entry.occurrence = self.next_id;
                self.next_id += 1;
            }
            self.upcoming = (0..self.context.len()).collect();
            if self.shuffled {
                self.upcoming.make_contiguous().shuffle(&mut rand::rng());
            }
            self.upcoming
                .pop_front()
                .map(|index| self.context[index].clone())
        } else {
            None
        };
        let Some(next) = next else {
            return false;
        };
        self.remember_current();
        self.current = Some(next);
        self.changed();
        true
    }

    pub fn previous(&mut self) -> bool {
        let Some(previous) = self.back.pop() else {
            return false;
        };
        if let Some(current) = self.current.take() {
            if self.redo.len() == BACK_LIMIT {
                self.redo.remove(0);
            }
            self.redo.push(current);
        }
        self.current = Some(previous);
        self.changed();
        true
    }

    pub fn jump_manual(&mut self, occurrence: u64) -> bool {
        let Some(index) = self
            .manual
            .iter()
            .position(|entry| entry.occurrence == occurrence)
        else {
            return false;
        };
        let next = self.manual.remove(index).unwrap();
        self.remember_current();
        self.current = Some(next);
        self.changed();
        true
    }

    pub fn jump_context(&mut self, occurrence: u64) -> bool {
        if let Some(index) = self
            .redo
            .iter()
            .position(|entry| entry.occurrence == occurrence)
        {
            let next = self.redo[index].clone();
            self.redo.truncate(index);
            self.remember_current();
            self.current = Some(next);
        } else if let Some(position) = self
            .upcoming
            .iter()
            .position(|&index| self.context[index].occurrence == occurrence)
        {
            let index = self.upcoming[position];
            self.upcoming.drain(..=position);
            self.redo.clear();
            self.remember_current();
            self.current = Some(self.context[index].clone());
        } else {
            return false;
        }
        self.changed();
        true
    }

    pub fn remove_manual(&mut self, occurrence: u64) -> bool {
        let Some(index) = self
            .manual
            .iter()
            .position(|entry| entry.occurrence == occurrence)
        else {
            return false;
        };
        self.manual.remove(index);
        self.changed();
        true
    }
    pub fn move_manual(&mut self, occurrence: u64, destination: usize) -> bool {
        let Some(index) = self
            .manual
            .iter()
            .position(|entry| entry.occurrence == occurrence)
        else {
            return false;
        };
        if destination >= self.manual.len() || destination == index {
            return false;
        }
        let entry = self.manual.remove(index).unwrap();
        self.manual.insert(destination, entry);
        self.changed();
        true
    }
    pub fn clear_manual(&mut self) {
        self.manual.clear();
        self.changed();
    }
    pub fn clear_context(&mut self) {
        self.context.clear();
        self.upcoming.clear();
        self.redo.clear();
        self.continuation = None;
        self.title.clear();
        self.changed();
    }
    pub fn clear(&mut self) {
        self.clear_context();
        self.manual.clear();
        self.back.clear();
        self.current = None;
    }

    pub fn validate(&self) -> Result<()> {
        ensure!(self.next_id != 0, "Invalid saved occurrence counter");
        ensure!(
            self.context.len() + self.manual.len() <= MAX_ENTRIES
                && self.back.len() + self.redo.len() <= BACK_LIMIT,
            "Saved queue exceeds its size limit"
        );
        let mut ids = HashMap::new();
        for entry in self
            .context
            .iter()
            .chain(&self.manual)
            .chain(&self.back)
            .chain(&self.redo)
            .chain(&self.current)
        {
            ensure!(
                entry.occurrence != 0 && entry.occurrence < self.next_id && entry.track.id != 0,
                "Invalid saved queue identity"
            );
            let track = &entry.track;
            ensure!(
                track.title.len() <= 4096
                    && track.artist.name.len() <= 4096
                    && track.album.title.len() <= 4096
                    && track.album.artist.name.len() <= 4096,
                "Saved track metadata exceeds its size limit"
            );
            if let Some(track) = ids.insert(entry.occurrence, entry.track.id) {
                ensure!(
                    track == entry.track.id,
                    "Conflicting saved queue identities"
                );
            }
        }
        let mut canonical = HashSet::new();
        ensure!(
            self.context
                .iter()
                .chain(&self.manual)
                .all(|entry| canonical.insert(entry.occurrence)),
            "Duplicate saved queue occurrence"
        );
        let mut upcoming = HashSet::new();
        ensure!(
            self.upcoming
                .iter()
                .all(|&index| index < self.context.len() && upcoming.insert(index)),
            "Invalid saved context order"
        );
        let mut active = HashSet::new();
        ensure!(
            self.manual
                .iter()
                .chain(self.context_upcoming())
                .chain(&self.current)
                .all(|entry| active.insert(entry.occurrence)),
            "Duplicate active queue occurrence"
        );
        ensure!(self.title.len() <= 4096, "Invalid source title");
        match &self.continuation {
            Some(Continuation::Playlist { id, etag, offset }) => ensure!(
                !id.is_empty()
                    && id.len() <= 100
                    && id.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-')
                    && !etag.is_empty()
                    && etag.len() <= 256
                    && !etag.contains(['\r', '\n'])
                    && *offset <= 1_000_000,
                "Invalid saved playlist continuation"
            ),
            Some(Continuation::Album { id, offset }) => ensure!(
                *id != 0 && *offset <= 1_000_000,
                "Invalid saved album continuation"
            ),
            Some(Continuation::Favorites { offset, .. }) => {
                ensure!(*offset <= 1_000_000, "Invalid saved favorites continuation")
            }
            None => {}
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "queue_tests.rs"]
mod tests;
