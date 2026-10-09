use super::*;
use std::time::{Duration, Instant};

enum PageStart {
    Track(usize),
    Collection { shuffle: bool },
}

pub(super) struct RestoreGuard {
    request: u64,
    queue_revision: u64,
    play_generation: u64,
    view_generation: u64,
    volume: f32,
    quality: String,
    explicit_link: bool,
    preserve_view: bool,
}

impl App {
    pub(super) fn play_current(&mut self) {
        self.play_at(0);
    }

    fn play_at(&mut self, position: u64) {
        let Some(track) = self.queue.current() else {
            return;
        };
        let id = track.id;
        let position = position.min(track.duration.saturating_sub(1));
        self.play_generation = self.backend.player.reserve();
        self.position = position;
        self.paused = false;
        self.buffering = true;
        self.waiting_context = false;
        self.ended = false;
        self.actual_quality.clear();
        self.error = None;
        self.send(Request::Play {
            generation: self.play_generation,
            id,
            quality: self.quality.clone(),
            position,
        });
    }

    pub(super) fn play_track(&mut self, index: usize) {
        self.start_page(PageStart::Track(index));
    }

    pub(super) fn play_collection(&mut self, shuffle: bool) {
        self.start_page(PageStart::Collection { shuffle });
    }

    fn start_page(&mut self, start: PageStart) {
        let continuation = if self.more {
            match &self.page {
                Page::Collection { kind, id, .. } if kind == "playlists" => self
                    .playlist_page
                    .as_ref()
                    .map(|page| Continuation::Playlist {
                        id: id.clone(),
                        etag: page.etag.clone(),
                        offset: page.next_offset,
                    }),
                Page::Collection { kind, id, .. } if kind == "albums" => {
                    id.parse().ok().map(|id| Continuation::Album {
                        id,
                        offset: self.tracks.len(),
                    })
                }
                Page::Library(FavoriteKind::Tracks) => Some(Continuation::Favorites {
                    offset: self.library_offset,
                    total: self.library_total,
                }),
                _ => None,
            }
        } else {
            None
        };
        let title = match &self.page {
            Page::Collection { title, .. } | Page::Mix { title, .. } => title.clone(),
            Page::Radio(seed) => seed.title(),
            Page::Library(_) => "Favorite tracks".into(),
            Page::Search => format!("Search · {}", self.search_query),
            Page::Home => "Daily Discovery".into(),
            Page::Artist(_) => self
                .artists
                .first()
                .map_or("Artist tracks".into(), |artist| artist.name.clone()),
            _ => "Selected tracks".into(),
        };
        let result = match start {
            PageStart::Track(index) => {
                self.queue
                    .start(self.tracks.clone(), index, title, continuation)
            }
            PageStart::Collection { shuffle } => {
                self.queue
                    .start_collection(self.tracks.clone(), title, continuation, shuffle)
            }
        };
        match result {
            Ok(()) => {
                self.cancel_context();
                self.play_current();
            }
            Err(error) => self.error = Some(error.to_string()),
        }
    }

    pub(super) fn queue_track(&mut self, track: Track, next: bool) {
        match self.queue.add(track, next) {
            Ok(()) => {
                self.queue_open = true;
                if self.waiting_context && !self.paused {
                    self.advance(false);
                }
            }
            Err(error) => self.error = Some(error.to_string()),
        }
    }

    pub(super) fn toggle(&mut self) {
        if self.queue.current().is_none() {
            if self.queue.advance(false) {
                self.play_current();
            } else if !self.tracks.is_empty() {
                self.play_track(0);
            }
            return;
        }
        if self.buffering {
            self.paused = !self.paused;
            return;
        }
        if self.ended && self.context_error.is_some() && self.queue.continuation().is_some() {
            self.retry_context();
            self.waiting_context = true;
            self.buffering = true;
            self.paused = false;
        } else if self.ended {
            self.play_current();
        } else if self.actual_quality.is_empty() {
            self.play_at(self.position);
        } else {
            self.paused = !self.paused;
            self.backend.player.pause(self.paused);
        }
    }

    pub(super) fn next(&mut self) {
        self.advance(false);
    }
    pub(super) fn advance(&mut self, natural: bool) {
        if self.queue.advance(natural) {
            self.play_current();
        } else if self.queue.continuation().is_some() {
            self.play_generation = self.backend.player.reserve();
            self.actual_quality.clear();
            if self.context_error.is_some() {
                self.paused = true;
                self.ended = true;
                self.buffering = false;
                self.waiting_context = false;
            } else {
                self.waiting_context = true;
                self.buffering = true;
                self.paused = false;
            }
        } else {
            self.play_generation = self.backend.player.reserve();
            self.paused = true;
            self.ended = true;
            self.buffering = false;
            self.waiting_context = false;
        }
    }
    pub(super) fn previous(&mut self) {
        if self.position > 3 {
            if self.buffering {
                self.play_current();
            } else {
                self.seek(0);
            }
        } else if self.queue.previous() {
            self.play_current();
        }
    }
    pub(super) fn cancel_context(&mut self) {
        self.context_generation = self.context_generation.wrapping_add(1);
        self.context_loading = false;
        self.context_error = None;
        self.context_retry = false;
        if self.waiting_context {
            self.buffering = false;
            self.paused = true;
        }
        self.waiting_context = false;
    }
    pub(super) fn retry_context(&mut self) {
        self.context_error = None;
        self.context_retry = true;
    }
    pub(super) fn clear_source(&mut self) {
        self.cancel_context();
        self.queue.clear_context();
    }
    pub(super) fn stop_and_clear(&mut self) {
        self.cancel_context();
        self.queue.clear();
        self.play_generation = self.backend.player.reserve();
        self.paused = true;
        self.buffering = false;
        self.ended = false;
        self.position = 0;
        self.actual_quality.clear();
    }
    pub(super) fn prefetch_context(&mut self) {
        // Run after foreground UI actions: a newly requested stream goes first.
        if self.context_loading
            || self.context_error.is_some()
            || (!self.context_retry && !self.waiting_context && (self.paused || self.buffering))
        {
            return;
        }
        self.context_retry = false;
        if let (Some(user), Some(source)) = (self.account, self.queue.continuation().cloned()) {
            self.context_loading = true;
            self.send(Request::ContextPage {
                user,
                generation: self.context_generation,
                source,
            });
        }
    }

    pub(super) fn request_restore(&mut self) {
        let (Some(user), Some(store)) = (self.account, &self.state_store) else {
            return;
        };
        self.restore_request = self.restore_request.wrapping_add(1);
        self.restore_guard = Some(RestoreGuard {
            request: self.restore_request,
            queue_revision: self.queue.revision(),
            play_generation: self.play_generation,
            view_generation: self.generation,
            volume: self.volume,
            quality: self.quality.clone(),
            explicit_link: self.autoplay_page
                && matches!(self.page, Page::Track(_) | Page::Radio(_)),
            preserve_view: self.page != Page::Home,
        });
        self.restore_ready = false;
        store.load(user, self.restore_request);
    }
    fn progress(&self) -> Progress {
        Progress {
            occurrence: self.queue.current_entry().map(|entry| entry.occurrence),
            position: if self.ended {
                0
            } else {
                self.position
                    .min(self.queue.current().map_or(0, |track| track.duration))
            },
            volume: self.volume,
            quality: self.quality.clone(),
            location: self.location(),
        }
    }
    pub(super) fn checkpoint(&mut self, force: bool) {
        let (Some(user), Some(store)) = (self.account, &self.state_store) else {
            return;
        };
        if !self.restore_ready {
            if force
                && self.restore_guard.as_ref().is_some_and(|guard| {
                    self.queue.revision() != guard.queue_revision
                        || self.play_generation != guard.play_generation
                })
            {
                store.snapshot(PlaybackState::new(
                    user,
                    self.queue.clone(),
                    self.progress(),
                ));
            }
            return;
        }
        let progress = self.progress();
        if self.saved_revision != Some(self.queue.revision()) {
            store.snapshot(PlaybackState::new(
                user,
                self.queue.clone(),
                progress.clone(),
            ));
            self.saved_revision = Some(self.queue.revision());
        } else if self.last_progress.as_ref() != Some(&progress)
            && (force || self.paused || self.last_checkpoint.elapsed() >= Duration::from_secs(5))
        {
            store.progress(user, progress.clone());
        } else {
            return;
        }
        self.last_progress = Some(progress);
        self.last_checkpoint = Instant::now();
    }
    pub(super) fn state_events(&mut self) {
        let events: Vec<_> = self
            .state_store
            .as_ref()
            .map(|store| store.events.try_iter().collect())
            .unwrap_or_default();
        for event in events {
            match event {
                crate::player_state::Event::HistoryLoaded {
                    user,
                    request,
                    result,
                } => self.history_loaded(user, request, result),
                crate::player_state::Event::HistorySaved {
                    user,
                    revision,
                    result,
                } => self.history_saved(user, revision, result),
                crate::player_state::Event::Loaded {
                    user,
                    request,
                    result,
                } if self.account == Some(user)
                    && self
                        .restore_guard
                        .as_ref()
                        .is_some_and(|guard| guard.request == request) =>
                {
                    let guard = self.restore_guard.take().unwrap();
                    let unchanged = self.queue.revision() == guard.queue_revision
                        && self.play_generation == guard.play_generation;
                    let skip_restore = guard.explicit_link
                        || (self.generation != guard.view_generation
                            && self.autoplay_page
                            && matches!(self.page, Page::Track(_) | Page::Radio(_)));
                    let saved_progress = match &result {
                        Ok(Some(state)) => Some(state.progress.clone()),
                        Ok(None) => Some(PlaybackState::empty(user).progress),
                        Err(_) => None,
                    };
                    match result {
                        Ok(Some(state)) if unchanged && !skip_restore => {
                            self.cancel_context();
                            self.queue = state.queue;
                            self.position = state.progress.position;
                            self.paused = true;
                            self.buffering = false;
                            self.ended = false;
                            self.actual_quality.clear();
                            if self.volume == guard.volume {
                                self.volume = state.progress.volume;
                            }
                            if self.quality == guard.quality {
                                self.quality = state.progress.quality;
                            }
                            self.backend.player.volume(self.volume);
                            if !guard.preserve_view && self.generation == guard.view_generation {
                                self.show_location(state.progress.location, false);
                            }
                        }
                        Err(error) => self.error = Some(error),
                        _ => {}
                    }
                    self.restore_ready = true;
                    self.saved_revision = if unchanged {
                        Some(self.queue.revision())
                    } else {
                        None
                    };
                    self.last_progress = if unchanged && !skip_restore {
                        saved_progress.or_else(|| Some(self.progress()))
                    } else {
                        Some(self.progress())
                    };
                }
                crate::player_state::Event::Error { user, message }
                    if self.account == Some(user) =>
                {
                    self.error = Some(message)
                }
                _ => {}
            }
        }
    }
}
