use super::*;
use std::time::{Duration, Instant, SystemTime};

// A conservative idle policy, not a claim about TIDAL's signed-URL lifetime.
// Wall time includes suspend on Linux; a backwards clock also forces renewal.
fn refresh_after_pause(paused: Option<SystemTime>, now: SystemTime) -> bool {
    paused.is_some_and(|start| {
        now.duration_since(start)
            .map_or(true, |elapsed| elapsed >= Duration::from_secs(60))
    })
}

#[cfg(test)]
#[test]
fn pause_refresh_policy_covers_boundary_suspend_and_clock_rollback() {
    let start = SystemTime::UNIX_EPOCH + Duration::from_secs(100);
    assert!(!refresh_after_pause(None, start));
    assert!(!refresh_after_pause(
        Some(start),
        start + Duration::from_secs(59)
    ));
    assert!(refresh_after_pause(
        Some(start),
        start + Duration::from_secs(60)
    ));
    assert!(refresh_after_pause(
        Some(start),
        start + Duration::from_secs(86400)
    ));
    assert!(refresh_after_pause(
        Some(start),
        start - Duration::from_secs(1)
    ));
}

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

    pub(super) fn play_at(&mut self, position: u64) {
        self.sync_next(true);
        let Some(track) = self.queue.current() else {
            return;
        };
        let id = track.id;
        let position = position.min(track.duration.saturating_sub(1));
        if self.appearance.output == crate::output_devices::Target::RequireSelection {
            self.play_generation = self.backend.player.reserve();
            self.position = position;
            self.paused = true;
            self.paused_since = None;
            self.buffering = false;
            self.waiting_context = false;
            self.ended = false;
            self.starved_generation = None;
            self.actual_quality.clear();
            self.error = Some(
                "Choose an output in Playback settings, then press Play. No output was opened."
                    .into(),
            );
            self.settings_section = SettingsSection::Playback;
            self.navigate(Page::Settings);
            return;
        }
        self.play_generation = self.backend.player.reserve();
        self.position = position;
        self.paused = false;
        self.paused_since = None;
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
        self.unavailable_skips = 0;
        self.start_page(PageStart::Track(index));
    }

    pub(super) fn play_collection(&mut self, shuffle: bool) {
        self.unavailable_skips = 0;
        self.start_page(PageStart::Collection { shuffle });
    }

    fn start_page(&mut self, start: PageStart) {
        self.sync_track_view();
        let view = self.track_view.active();
        let continuation = if self.more && !view {
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
                        offset: self
                            .album_position
                            .as_ref()
                            .map_or(self.tracks.len(), |page| page.offset),
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
        let (tracks, title) = if view {
            (
                self.track_view
                    .rows
                    .iter()
                    .map(|&i| self.tracks[i].clone())
                    .collect(),
                format!("{title} · loaded view"),
            )
        } else {
            (self.tracks.clone(), title)
        };
        let result = match start {
            PageStart::Track(index) => {
                let index = if view {
                    self.track_view.rows.iter().position(|&i| i == index)
                } else {
                    Some(index)
                };
                if let Some(index) = index {
                    self.queue_mut().start(tracks, index, title, continuation)
                } else {
                    Err(anyhow::anyhow!("That song is not in the current view"))
                }
            }
            PageStart::Collection { shuffle } => {
                self.queue_mut()
                    .start_collection(tracks, title, continuation, shuffle)
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
        match self.queue_mut().add(track, next) {
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
        self.unavailable_skips = 0;
        if self.paused
            && let Some(epoch) = self.output_fault.take()
        {
            self.backend.player.retry_output(epoch);
        }
        self.sync_next(true);
        self.next_attempt = None;
        if self.queue.current().is_none() {
            if self.queue_mut().advance(false) {
                self.play_current();
            } else if !self.tracks.is_empty() {
                self.play_collection(false);
            }
            return;
        }
        if self.buffering {
            self.paused = !self.paused;
            self.paused_since = self.paused.then(SystemTime::now);
            return;
        }
        if self.ended && self.context_error.is_some() && self.queue.continuation().is_some() {
            self.retry_context();
            self.waiting_context = true;
            self.buffering = true;
            self.paused = false;
        } else if self.ended {
            self.play_current();
        } else if self.actual_quality.is_empty()
            || (self.paused && refresh_after_pause(self.paused_since, SystemTime::now()))
        {
            self.play_at(self.position);
        } else {
            self.paused = !self.paused;
            self.paused_since = self.paused.then(SystemTime::now);
            self.backend.player.pause(self.paused);
        }
    }

    pub(super) fn next(&mut self) {
        self.unavailable_skips = 0;
        self.advance(false);
    }
    pub(super) fn track_unavailable(&mut self, user: u64, generation: u64, id: u64) {
        self.sync_next(false);
        if self.account != Some(user)
            || self.play_generation != generation
            || self.queue.current().map(|t| t.id) != Some(id)
        {
            return;
        }
        self.sync_next(true);
        if self.play_generation != generation {
            return;
        }
        if self.unavailable.len() >= 512 {
            self.unavailable.clear();
        }
        self.unavailable.insert(id);
        self.unavailable_expiry.get_or_insert_with(|| {
            std::time::Instant::now() + std::time::Duration::from_secs(15 * 60)
        });
        self.unavailable_skips = self.unavailable_skips.saturating_add(1);
        self.buffering = false;
        self.starved_generation = None;
        self.actual_quality.clear();
        if self.paused || self.output_fault.is_some() || self.unavailable_skips >= 5 {
            self.play_generation = self.backend.player.reserve();
            self.paused = true;
            self.ended = false;
            self.waiting_context = false;
            self.error = Some(if self.unavailable_skips >= 5 { "Stopped after five unavailable tracks. Choose another track or explicitly retry." } else { "This track is unavailable for this account. Playback remains paused." }.into());
            return;
        }
        self.advance(false);
        if self.paused && self.ended {
            self.error = Some("This track is unavailable and no further tracks are queued.".into());
        } else {
            self.notice = Some(format!(
                "Skipped an unavailable track ({} of 5 consecutive attempts).",
                self.unavailable_skips
            ));
        }
    }
    pub(super) fn advance(&mut self, natural: bool) {
        if self.queue_mut().advance(natural) {
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
        self.unavailable_skips = 0;
        self.sync_next(true);
        if self.position > 3 {
            if self.buffering {
                self.play_current();
            } else {
                self.seek(0);
            }
        } else if self.queue_mut().previous() {
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
        self.queue_mut().clear_context();
    }
    pub(super) fn stop_and_clear(&mut self) {
        self.cancel_context();
        self.queue_mut().clear();
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
            || (!self.context_retry
                && !self.waiting_context
                && (self.paused
                    || self.audio_buffering()
                    || self.backend.player.has_prepared_next()))
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
        self.load_metadata_cache();
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
        self.sync_next(false);
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
                crate::player_state::Event::CacheLoaded {
                    user,
                    request,
                    result,
                } => self.metadata_cache_loaded(user, request, result),
                crate::player_state::Event::CacheSaved {
                    user,
                    request,
                    result,
                } => self.metadata_cache_saved(user, request, result),
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
                            self.replace_queue(state.queue);
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
