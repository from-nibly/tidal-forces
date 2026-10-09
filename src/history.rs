//! Local listening records, distinct from queue navigation and TIDAL's own history.
use crate::{model::Track, store};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashSet, VecDeque},
    fs,
    io::Read,
    path::Path,
    sync::Arc,
    time::Duration,
};

const VERSION: u32 = 1;
const MAX_ENTRIES: usize = 1000;
const RETENTION: u64 = 90 * 24 * 60 * 60;
const MAX_BYTES: u64 = 16 * 1024 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Entry {
    pub id: u64,
    pub track: Arc<Track>,
    pub started_at: u64,
    pub listened_ms: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct History {
    version: u32,
    pub user: u64,
    pub enabled: bool,
    next_id: u64,
    pub entries: VecDeque<Entry>,
    #[serde(skip)]
    revision: u64,
}
impl History {
    pub fn new(user: u64) -> Self {
        Self {
            version: VERSION,
            user,
            enabled: false,
            next_id: 1,
            entries: VecDeque::new(),
            revision: 0,
        }
    }
    pub fn revision(&self) -> u64 {
        self.revision
    }
    pub fn set_enabled(&mut self, enabled: bool) {
        if self.enabled != enabled {
            self.enabled = enabled;
            self.revision += 1;
        }
    }
    pub fn clear(&mut self) {
        self.entries.clear();
        self.revision += 1;
    }
    pub fn prune(&mut self, now: u64) {
        let before = self.entries.len();
        self.entries
            .retain(|entry| entry.started_at >= now.saturating_sub(RETENTION));
        self.entries.truncate(MAX_ENTRIES);
        if before != self.entries.len() {
            self.revision += 1;
        }
    }
    fn validate(&self, user: u64) -> Result<()> {
        ensure!(
            self.version == VERSION,
            "Unsupported listening-history version"
        );
        ensure!(
            user != 0 && user == self.user,
            "Listening history belongs to a different account"
        );
        ensure!(
            self.next_id > 0 && self.entries.len() <= MAX_ENTRIES,
            "Invalid listening-history limits"
        );
        let mut ids = HashSet::new();
        for entry in &self.entries {
            ensure!(
                entry.id > 0 && entry.id < self.next_id && ids.insert(entry.id),
                "Invalid listening-history identity"
            );
            let track = &entry.track;
            ensure!(
                track.id > 0
                    && track.title.len() <= 4096
                    && track.artist.name.len() <= 4096
                    && track.album.title.len() <= 4096
                    && track.album.artist.name.len() <= 4096,
                "Invalid listening-history metadata"
            );
        }
        Ok(())
    }
}

/// Takes monotonic output consumption and monotonic elapsed time, never seek position.
/// Keeping baselines even while disabled prevents retroactive collection on opt-in.
#[derive(Default)]
pub struct Listener {
    generation: Option<u64>,
    rendered: Duration,
    elapsed: Duration,
    heard: Duration,
    started_at: u64,
    record: Option<u64>,
    skip_next: bool,
}
impl Listener {
    pub fn reset_collection(&mut self) {
        self.heard = Duration::ZERO;
        self.record = None;
        self.skip_next = true;
    }
    pub fn observe(
        &mut self,
        history: Option<&mut History>,
        generation: u64,
        track: Arc<Track>,
        rendered: Duration,
        elapsed: Duration,
        now: u64,
    ) {
        if self.generation != Some(generation) {
            *self = Self {
                generation: Some(generation),
                started_at: now,
                skip_next: self.skip_next,
                ..Self::default()
            };
        }
        let delta = if rendered < self.rendered || elapsed < self.elapsed {
            self.reset_collection();
            Duration::ZERO
        } else {
            (rendered - self.rendered).min(elapsed - self.elapsed)
        };
        self.rendered = rendered;
        self.elapsed = elapsed;
        if self.skip_next {
            self.skip_next = false;
            self.started_at = now;
            return;
        }
        let Some(history) = history.filter(|history| history.enabled) else {
            self.heard = Duration::ZERO;
            self.record = None;
            self.started_at = now;
            return;
        };
        if delta.is_zero() {
            return;
        }
        self.heard = self.heard.saturating_add(delta);
        let threshold = if track.duration == 0 {
            30_000
        } else {
            track.duration.saturating_mul(500).min(30_000)
        };
        if self.heard < Duration::from_millis(threshold) {
            return;
        }
        history.prune(now);
        let listened_ms = self.heard.as_millis().min(u128::from(u64::MAX)) as u64;
        if let Some(id) = self.record {
            if let Some(entry) = history.entries.iter_mut().find(|entry| entry.id == id)
                && entry.listened_ms != listened_ms
            {
                entry.listened_ms = listened_ms;
                history.revision += 1;
            }
        } else if let Some(next) = history.next_id.checked_add(1) {
            let id = history.next_id;
            history.next_id = next;
            history.entries.push_front(Entry {
                id,
                track,
                started_at: self.started_at,
                listened_ms,
            });
            history.entries.truncate(MAX_ENTRIES);
            history.revision += 1;
            self.record = Some(id);
        }
    }
}

pub fn load(root: &Path, user: u64, now: u64) -> Result<History> {
    ensure!(user != 0, "Invalid listening-history account");
    let path = root
        .join("accounts")
        .join(user.to_string())
        .join("history.json");
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(History::new(user));
        }
        Err(error) => return Err(error.into()),
    };
    let mut bytes = Vec::new();
    file.take(MAX_BYTES + 1).read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() as u64 <= MAX_BYTES,
        "Listening history exceeds its size limit"
    );
    let mut history: History =
        serde_json::from_slice(&bytes).context("Invalid listening history")?;
    history.validate(user)?;
    history.prune(now);
    Ok(history)
}
pub fn save(root: &Path, history: &History) -> Result<()> {
    history.validate(history.user)?;
    let bytes = serde_json::to_vec(history)?;
    ensure!(
        bytes.len() as u64 <= MAX_BYTES,
        "Listening history exceeds its storage limit"
    );
    store::private_dir(root)?;
    store::private_dir(&root.join("accounts"))?;
    store::save_bytes(
        &root.join("accounts").join(history.user.to_string()),
        "history.json",
        &bytes,
    )
}
pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
#[path = "history_tests.rs"]
mod tests;
