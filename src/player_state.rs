use crate::{
    queue::Queue,
    store,
    ui::navigation::{Location, Page},
};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fs,
    io::Read,
    path::{Path, PathBuf},
    sync::{Arc, Condvar, Mutex, mpsc},
    thread,
};

const VERSION: u32 = 1;
const MAX_BYTES: u64 = 32 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub struct Progress {
    pub occurrence: Option<u64>,
    pub position: u64,
    pub volume: f32,
    pub quality: String,
    pub location: Location,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct PlaybackState {
    version: u32,
    pub user: u64,
    pub queue: Queue,
    pub progress: Progress,
}
impl PlaybackState {
    pub fn new(user: u64, queue: Queue, progress: Progress) -> Self {
        Self {
            version: VERSION,
            user,
            queue,
            progress,
        }
    }
    pub fn empty(user: u64) -> Self {
        Self::new(
            user,
            Queue::default(),
            Progress {
                occurrence: None,
                position: 0,
                volume: 0.65,
                quality: "LOSSLESS".into(),
                location: Location {
                    page: Page::Home,
                    query: String::new(),
                },
            },
        )
    }
    pub fn validate(&self, user: u64) -> Result<()> {
        ensure!(
            self.version == VERSION,
            "Unsupported saved playback-state version"
        );
        ensure!(
            user != 0 && self.user == user,
            "Saved playback state belongs to a different account"
        );
        self.queue.validate()?;
        ensure!(
            self.progress.occurrence == self.queue.current_entry().map(|entry| entry.occurrence),
            "Saved position has a different queue occurrence"
        );
        ensure!(
            self.progress.volume.is_finite() && (0.0..=1.0).contains(&self.progress.volume),
            "Invalid saved volume"
        );
        ensure!(
            ["LOSSLESS", "HIGH", "LOW"].contains(&self.progress.quality.as_str()),
            "Invalid saved listening quality"
        );
        ensure!(
            self.progress.position <= self.queue.current().map_or(0, |track| track.duration),
            "Invalid saved position"
        );
        validate_location(&self.progress.location)
    }
}

fn validate_location(location: &Location) -> Result<()> {
    ensure!(location.query.len() <= 2048, "Saved search is too long");
    let (kind, id, title) = match &location.page {
        Page::Home | Page::Search | Page::Settings | Page::Playlists | Page::Library(_) => {
            return Ok(());
        }
        Page::Track(id) => ("track", id.to_string(), ""),
        Page::Artist(id) => ("artist", id.to_string(), ""),
        Page::Radio(crate::model::RadioSeed::Track { id, title }) => {
            ("track", id.to_string(), title.as_str())
        }
        Page::Radio(crate::model::RadioSeed::Artist { id, name }) => {
            ("artist", id.to_string(), name.as_str())
        }
        Page::Mix { id, title } => ("mix", id.clone(), title.as_str()),
        Page::Collection { kind, id, title } => {
            ensure!(
                kind == "albums" || kind == "playlists",
                "Invalid saved collection type"
            );
            (kind.trim_end_matches('s'), id.clone(), title.as_str())
        }
    };
    ensure!(title.len() <= 4096, "Saved title is too long");
    crate::links::Link::parse(&format!("tidal://{kind}/{id}"))?;
    Ok(())
}

fn state_dir(root: &Path, user: u64) -> PathBuf {
    root.join("accounts").join(user.to_string())
}
fn load(root: &Path, user: u64) -> Result<Option<PlaybackState>> {
    let file = match fs::File::open(state_dir(root, user).join("player-state.json")) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let mut bytes = Vec::new();
    file.take(MAX_BYTES + 1).read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() as u64 <= MAX_BYTES,
        "Saved playback state exceeds its size limit"
    );
    let state: PlaybackState =
        serde_json::from_slice(&bytes).context("Saved playback state is invalid")?;
    state.validate(user)?;
    Ok(Some(state))
}
fn save(root: &Path, state: &PlaybackState) -> Result<()> {
    state.validate(state.user)?;
    let bytes = serde_json::to_vec(state)?;
    ensure!(
        bytes.len() as u64 <= MAX_BYTES,
        "Playback state exceeds its storage limit"
    );
    store::private_dir(root)?;
    store::private_dir(&root.join("accounts"))?;
    store::save_bytes(&state_dir(root, state.user), "player-state.json", &bytes)
}

pub enum Event {
    HistoryLoaded {
        user: u64,
        request: u64,
        result: std::result::Result<crate::history::History, String>,
    },
    HistorySaved {
        user: u64,
        revision: u64,
        result: std::result::Result<(), String>,
    },
    Loaded {
        user: u64,
        request: u64,
        result: std::result::Result<Option<Box<PlaybackState>>, String>,
    },
    Error {
        user: u64,
        message: String,
    },
}
#[derive(Default)]
struct Update {
    snapshot: Option<PlaybackState>,
    progress: Option<Progress>,
}
#[derive(Default)]
struct Pending {
    updates: HashMap<u64, Update>,
    load: Option<(u64, u64)>,
    histories: HashMap<u64, crate::history::History>,
    history_load: Option<(u64, u64)>,
    shutdown: bool,
}

/// Coalesce frequent edits and serialize/fsync away from both UI and audio.
/// Shutdown drains the last snapshot instead of racing process exit.
pub struct StateStore {
    shared: Arc<(Mutex<Pending>, Condvar)>,
    pub events: mpsc::Receiver<Event>,
    worker: Option<thread::JoinHandle<()>>,
}
impl StateStore {
    pub fn new(root: PathBuf, ctx: eframe::egui::Context) -> Result<Self> {
        let shared = Arc::new((Mutex::new(Pending::default()), Condvar::new()));
        let pending = shared.clone();
        let (tx, events) = mpsc::channel();
        let worker = thread::Builder::new().name("tidal-state".into()).spawn(move || {
            let mut cached: Option<PlaybackState> = None;
            loop {
                let (updates, request, histories, history_request, shutdown) = {
                    let (lock, wake) = &*pending;
                    let mut work = lock.lock().unwrap();
                    while work.updates.is_empty() && work.load.is_none() && work.histories.is_empty() && work.history_load.is_none() && !work.shutdown { work = wake.wait(work).unwrap(); }
                    (std::mem::take(&mut work.updates), work.load.take(), std::mem::take(&mut work.histories), work.history_load.take(), work.shutdown)
                };
                for (user, update) in updates {
                    if let Some(state) = update.snapshot { cached = Some(state); }
                    if let Some(state) = cached.as_mut().filter(|state| state.user == user) {
                        if let Some(progress) = update.progress
                            && progress.occurrence == state.queue.current_entry().map(|entry| entry.occurrence) {
                            state.progress = progress;
                        }
                        if let Err(error) = save(&root, state) {
                            let _ = tx.send(Event::Error { user, message: format!("Could not save playback state: {error}") });
                            ctx.request_repaint();
                        }
                    }
                }
                for (user, history) in histories {
                    let result = crate::history::save(&root, &history).map_err(|error| format!("Could not save listening history: {error}"));
                    let _ = tx.send(Event::HistorySaved { user, revision:history.revision(), result });
                    ctx.request_repaint();
                }
                if let Some((user, request)) = history_request {
                    let result = crate::history::load(&root, user, crate::history::now()).map_err(|error| format!("Could not load listening history: {error}. Recording is disabled; the existing file has not been replaced."));
                    let _ = tx.send(Event::HistoryLoaded { user, request, result });
                    ctx.request_repaint();
                }
                if let Some((user, request)) = request {
                    let result = load(&root, user);
                    cached = match &result {
                        Ok(Some(state)) => Some(state.clone()),
                        Ok(None) => Some(PlaybackState::empty(user)),
                        Err(_) => None,
                    };
                    let _ = tx.send(Event::Loaded { user, request, result: result.map(|state| state.map(Box::new)).map_err(|error| format!("Could not restore playback state: {error}. Your credentials are unchanged; starting a new queue replaces this state.")) });
                    ctx.request_repaint();
                }
                if shutdown { break; }
            }
        })?;
        Ok(Self {
            shared,
            events,
            worker: Some(worker),
        })
    }
    pub fn load_history(&self, user: u64, request: u64) {
        let (lock, wake) = &*self.shared;
        lock.lock().unwrap().history_load = Some((user, request));
        wake.notify_one();
    }
    pub fn history(&self, history: crate::history::History) {
        let (lock, wake) = &*self.shared;
        lock.lock().unwrap().histories.insert(history.user, history);
        wake.notify_one();
    }
    pub fn load(&self, user: u64, request: u64) {
        let (lock, wake) = &*self.shared;
        lock.lock().unwrap().load = Some((user, request));
        wake.notify_one();
    }
    pub fn snapshot(&self, state: PlaybackState) {
        let (lock, wake) = &*self.shared;
        let mut work = lock.lock().unwrap();
        // This snapshot already includes the newest progress for this occurrence.
        work.updates.insert(
            state.user,
            Update {
                snapshot: Some(state),
                progress: None,
            },
        );
        wake.notify_one();
    }
    pub fn progress(&self, user: u64, progress: Progress) {
        let (lock, wake) = &*self.shared;
        lock.lock()
            .unwrap()
            .updates
            .entry(user)
            .or_default()
            .progress = Some(progress);
        wake.notify_one();
    }
}
impl Drop for StateStore {
    fn drop(&mut self) {
        let (lock, wake) = &*self.shared;
        lock.lock().unwrap().shutdown = true;
        wake.notify_one();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

#[cfg(test)]
#[path = "player_state_tests.rs"]
mod tests;
