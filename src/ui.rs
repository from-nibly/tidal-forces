use crate::{
    backend::{Backend, Event, Request},
    desktop::{DesktopControls, MediaControlEvent},
    library::{Favorite, FavoriteKind, Favorites},
    links::{Instance, Link},
    model::{
        Album, Artist, LibraryEntry, Mix, Playlist, PlaylistPage, RadioSeed, Track, cover_url, time,
    },
    player_state::{PlaybackState, Progress, StateStore},
    queue::{Continuation, Queue, Repeat},
    visualizer::Mode,
    visualizer_ui::Visualizer,
};
use eframe::egui::{self, Color32, RichText, Stroke, Vec2, pos2, vec2};
use std::collections::{HashMap, HashSet};

mod credentials;
mod export;
mod history;
mod library;
pub(crate) mod navigation;
mod playback;
mod side_panel;
use playback::RestoreGuard;
mod settings;
use library::favorite_button;
#[cfg(test)]
mod player_bar_tests;
mod surfaces;
#[cfg(test)]
mod tests;
mod theme;
#[cfg(test)]
mod toolbar_tests;
mod track_table;
#[cfg(debug_assertions)]
pub(crate) mod visual_fixture;
use navigation::{Location, Navigation, Page};
use settings::SettingsSection;
use theme::*;

enum TrackAction {
    Radio(RadioSeed),
    Add(Track),
    Queue(Track, bool),
}
struct Removal {
    playlist: String,
    title: String,
    index: usize,
    track: Track,
    etag: String,
}

pub struct App {
    instance: Option<Instance>,
    pending_link: Option<Link>,
    playlist_page: Option<PlaylistPage>,
    playlist_dialog: bool,
    playlist_target: Option<Track>,
    owned_playlists: Vec<Playlist>,
    playlist_filter: String,
    new_playlist_title: String,
    new_playlist_description: String,
    playlist_busy: bool,
    playlist_error: Option<String>,
    removal: Option<Removal>,
    notice: Option<String>,
    backend: Backend,
    screenshot: Option<std::path::PathBuf>,
    connected: bool,
    credentials: credentials::Credentials,
    export: export::Export,
    account: Option<u64>,
    favorites: Favorites,
    library_offset: usize,
    library_total: Option<u64>,
    country: String,
    login: Option<(String, String)>,
    signing_in: bool,
    pkce_url: Option<String>,
    pkce_redirect: String,
    pkce_wait: bool,
    auth_pkce: bool,
    page: Page,
    navigation: Navigation,
    search_query: String,
    autoplay_page: bool,
    query: String,
    focus_search: bool,
    tracks: Vec<Track>,
    albums: Vec<Album>,
    artists: Vec<Artist>,
    mixes: Vec<Mix>,
    daily: Option<Mix>,
    folders: HashMap<String, Vec<LibraryEntry>>,
    expanded: HashSet<String>,
    folder_pending: HashSet<String>,
    media: Option<DesktopControls>,
    generation: u64,
    loading: bool,
    more: bool,
    error: Option<String>,
    audio_available: bool,
    queue: Queue,
    queue_open: bool,
    context_generation: u64,
    context_loading: bool,
    context_error: Option<String>,
    context_retry: bool,
    waiting_context: bool,
    state_store: Option<StateStore>,
    listening: history::Listening,
    restore_request: u64,
    restore_guard: Option<RestoreGuard>,
    restore_ready: bool,
    saved_revision: Option<u64>,
    last_progress: Option<Progress>,
    last_checkpoint: std::time::Instant,
    play_generation: u64,
    buffering: bool,
    paused: bool,
    ended: bool,
    position: u64,
    volume: f32,
    quality: String,
    actual_quality: String,
    settings_section: SettingsSection,
    appearance: crate::store::Appearance,
    visualizer: Visualizer,
}

impl App {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        screenshot: Option<std::path::PathBuf>,
        instance: Option<Instance>,
        pending_link: Option<Link>,
    ) -> Self {
        if let Some(instance) = &instance {
            instance.attach(cc.egui_ctx.clone());
        }
        theme::configure(&cc.egui_ctx);
        egui_extras::install_image_loaders(&cc.egui_ctx);
        let appearance = crate::store::Appearance::load();
        let appearance_error = appearance.as_ref().err().map(|e| e.to_string());
        let appearance = appearance.unwrap_or_default();
        cc.egui_ctx.set_zoom_factor(appearance.zoom);
        let media_result = DesktopControls::new(cc.egui_ctx.clone());
        let media_error = media_result
            .as_ref()
            .err()
            .map(|e| format!("Desktop media controls unavailable: {e}"));
        let mut app = Self::with_backend(Backend::new(cc.egui_ctx.clone()), appearance);
        app.instance = instance;
        app.pending_link = pending_link;
        app.screenshot = screenshot;
        app.media = media_result.ok();
        app.error = media_error.or(appearance_error);
        match crate::store::config_dir().and_then(|root| StateStore::new(root, cc.egui_ctx.clone()))
        {
            Ok(store) => app.state_store = Some(store),
            Err(error) => app.error = Some(format!("Playback-state storage unavailable: {error}")),
        }
        app
    }

    fn with_backend(backend: Backend, appearance: crate::store::Appearance) -> Self {
        let visualizer = Visualizer::new(appearance.player_bar_visualizer);
        Self {
            instance: None,
            pending_link: None,
            playlist_page: None,
            playlist_dialog: false,
            playlist_target: None,
            owned_playlists: Vec::new(),
            playlist_filter: String::new(),
            new_playlist_title: String::new(),
            new_playlist_description: String::new(),
            playlist_busy: false,
            playlist_error: None,
            removal: None,
            notice: None,
            backend,
            screenshot: None,
            connected: false,
            credentials: credentials::Credentials::default(),
            export: export::Export::default(),
            account: None,
            favorites: Favorites::default(),
            library_offset: 0,
            library_total: None,
            country: String::new(),
            login: None,
            signing_in: false,
            pkce_url: None,
            pkce_redirect: String::new(),
            pkce_wait: false,
            auth_pkce: false,
            page: Page::Home,
            navigation: Navigation::default(),
            search_query: String::new(),
            autoplay_page: false,
            query: String::new(),
            focus_search: false,
            tracks: vec![],
            albums: vec![],
            artists: vec![],
            mixes: vec![],
            daily: None,
            folders: HashMap::new(),
            expanded: HashSet::new(),
            folder_pending: HashSet::new(),
            media: None,
            generation: 0,
            loading: false,
            more: false,
            error: None,
            audio_available: true,
            queue: Queue::default(),
            queue_open: false,
            context_generation: 0,
            context_loading: false,
            context_error: None,
            context_retry: false,
            waiting_context: false,
            state_store: None,
            listening: history::Listening::default(),
            restore_request: 0,
            restore_guard: None,
            restore_ready: false,
            saved_revision: None,
            last_progress: None,
            last_checkpoint: std::time::Instant::now(),
            play_generation: 0,
            buffering: false,
            paused: true,
            ended: false,
            position: 0,
            volume: 0.65,
            quality: "LOSSLESS".into(),
            actual_quality: String::new(),
            settings_section: SettingsSection::General,
            appearance,
            visualizer,
        }
    }

    fn send(&mut self, r: Request) {
        self.try_send(r);
    }

    fn try_send(&mut self, r: Request) -> bool {
        if let Err(error) = self.backend.tx.try_send(r) {
            match error.into_inner() {
                Request::CreateQueuePlaylist { .. } | Request::AppendQueueBatch { .. } => self
                    .export_error(
                        "The queue export request was not accepted; no new batch was sent.".into(),
                    ),
                Request::MigrateCredentials { .. }
                | Request::RetryCredentialSave
                | Request::ReloadCredentials
                | Request::CleanupCredentials
                | Request::Logout => self.credentials.busy = false,
                Request::Play { .. } => {
                    self.buffering = false;
                    self.paused = true;
                }
                Request::ContextPage { .. } => {
                    self.context_loading = false;
                    self.context_error =
                        Some("The worker is busy. Retry loading the source.".into());
                    if self.waiting_context {
                        self.waiting_context = false;
                        self.buffering = false;
                        self.paused = true;
                    }
                }
                Request::Search { .. }
                | Request::Library { .. }
                | Request::Home { .. }
                | Request::Mix { .. }
                | Request::Radio { .. }
                | Request::Collection { .. }
                | Request::Playlist { .. }
                | Request::Track { .. }
                | Request::Artist { .. } => self.loading = false,
                Request::BeginPkce | Request::FinishPkce(_) | Request::Login => {
                    self.signing_in = false;
                    self.pkce_wait = false;
                }
                Request::OwnedPlaylists
                | Request::CreatePlaylist { .. }
                | Request::AddToPlaylist { .. }
                | Request::RemoveFromPlaylist { .. } => self.playlist_busy = false,
                Request::Folder { id } => {
                    self.folder_pending.remove(&id);
                }
                _ => {}
            }
            self.error = Some("The background worker is busy or unavailable. Please retry.".into());
            return false;
        }
        true
    }

    fn location(&self) -> Location {
        Location {
            page: self.page.clone(),
            query: if self.page == Page::Search {
                self.search_query.clone()
            } else {
                String::new()
            },
        }
    }

    fn navigate(&mut self, page: Page) {
        let to = Location {
            query: if page == Page::Search {
                self.query.trim().into()
            } else {
                String::new()
            },
            page,
        };
        self.navigation.visit(self.location(), &to);
        self.show_location(to, true);
    }

    fn history(&mut self, forward: bool) {
        let current = self.location();
        let target = if forward {
            self.navigation.forward(current)
        } else {
            self.navigation.back(current)
        };
        if let Some(target) = target {
            self.show_location(target, false);
        }
    }

    fn show_location(&mut self, location: Location, autoplay: bool) {
        self.page = location.page;
        self.autoplay_page = autoplay;
        if self.page == Page::Search {
            self.search_query = location.query.clone();
            self.query = location.query;
            self.focus_search = true;
        }
        self.removal = None;
        self.playlist_page = None;
        self.generation += 1;
        self.tracks.clear();
        self.albums.clear();
        self.artists.clear();
        self.more = false;
        self.library_offset = 0;
        self.library_total = None;
        self.loading = false;
        if self.connected {
            self.load_page(0);
        }
    }

    fn search(&mut self) {
        if self.query.trim().is_empty() {
            return;
        }
        if self.query.starts_with("tidal:") || self.query.starts_with("https:") {
            match Link::parse(self.query.trim()) {
                Ok(link) => self.open_link(link),
                Err(e) => self.error = Some(e.to_string()),
            }
            return;
        }
        self.error = None;
        self.navigate(Page::Search);
    }

    fn load_page(&mut self, offset: usize) {
        self.loading = true;
        match &self.page {
            Page::Track(id) => self.send(Request::Track {
                generation: self.generation,
                id: *id,
            }),
            Page::Artist(id) => self.send(Request::Artist {
                generation: self.generation,
                id: *id,
            }),
            Page::Collection { kind, id, .. } if kind == "playlists" => {
                self.send(Request::Playlist {
                    generation: self.generation,
                    id: id.clone(),
                    offset,
                    etag: if offset > 0 {
                        self.playlist_page.as_ref().map(|p| p.etag.clone())
                    } else {
                        None
                    },
                })
            }
            Page::Home => self.send(Request::Home {
                generation: self.generation,
            }),
            Page::Mix { id, .. } => self.send(Request::Mix {
                generation: self.generation,
                id: id.clone(),
            }),
            Page::Radio(seed) => self.send(Request::Radio {
                generation: self.generation,
                seed: seed.clone(),
            }),
            Page::Library(kind) => self.send(Request::Library {
                generation: self.generation,
                kind: *kind,
                offset,
            }),
            Page::Collection { kind, id, .. } => self.send(Request::Collection {
                generation: self.generation,
                kind: kind.clone(),
                id: id.clone(),
                offset,
            }),
            Page::Search if !self.search_query.is_empty() => self.send(Request::Search {
                generation: self.generation,
                query: self.search_query.clone(),
            }),
            Page::Search | Page::Settings | Page::Playlists => {
                self.loading = false;
            }
        }
    }

    fn logout(&mut self) {
        if !self.try_send(Request::Logout) {
            return;
        }
        self.credentials.busy = true;
        self.credentials.confirm = false;
        self.cancel_export(true);
        self.history_account_changed();
        self.checkpoint(true);
        self.cancel_context();
        self.restore_guard = None;
        self.restore_ready = false;
        self.play_generation = self.backend.player.reserve();
        self.generation += 1;
        self.queue = Queue::default();
        self.paused = true;
        self.buffering = false;
        self.loading = false;
        self.connected = false;
        self.account = None;
        self.favorites = Favorites::default();
        self.navigation = Navigation::default();
        self.query.clear();
        self.search_query.clear();
        self.login = None;
        self.playlist_dialog = false;
        self.playlist_target = None;
        self.playlist_page = None;
        self.removal = None;
        self.owned_playlists.clear();
    }

    fn events(&mut self) {
        while let Ok(event) = self.backend.rx.try_recv() {
            match event {
                Event::QueuePlaylistCreated {
                    user,
                    operation,
                    result,
                } => self.export_created(user, operation, result),
                Event::QueueBatchSaved {
                    user,
                    operation,
                    offset,
                    result,
                } => self.export_saved(user, operation, offset, result),
                Event::Credentials {
                    status,
                    warning,
                    dirty,
                    finished,
                } => {
                    self.credentials.status = Some(status);
                    self.credentials.warning = warning;
                    self.credentials.dirty = dirty;
                    if finished {
                        self.credentials.busy = false;
                    }
                }
                Event::ContextPage { generation, result }
                    if generation == self.context_generation =>
                {
                    self.context_loading = false;
                    let result = result.and_then(|page| {
                        self.queue
                            .append(page.tracks, page.continuation)
                            .map_err(|error| error.to_string())
                    });
                    if let Err(error) = result {
                        self.context_error = Some(error);
                        if self.waiting_context {
                            self.waiting_context = false;
                            self.buffering = false;
                            self.paused = true;
                            self.ended = true;
                        }
                    } else if self.waiting_context {
                        let paused = self.paused;
                        self.waiting_context = false;
                        self.buffering = false;
                        if self.queue.advance(false) {
                            self.position = 0;
                            self.actual_quality.clear();
                            self.ended = false;
                            if !paused {
                                self.play_current();
                            }
                        } else if self.queue.continuation().is_some() {
                            self.waiting_context = true;
                            self.buffering = true;
                        } else {
                            self.paused = true;
                            self.ended = true;
                        }
                    }
                }
                Event::Playlist {
                    generation,
                    mut page,
                    append,
                } if generation == self.generation => {
                    if append && let Some(old) = self.playlist_page.take() {
                        let mut rows = old.rows;
                        rows.append(&mut page.rows);
                        page.rows = rows;
                    }
                    self.tracks = page.rows.iter().map(|(_, t)| t.clone()).collect();
                    self.more = page.more;
                    if let Page::Collection { title, .. } = &mut self.page {
                        *title = page.playlist.title.clone();
                    }
                    self.playlist_page = Some(page);
                    self.loading = false;
                }
                Event::CollectionTitle { generation, title } if generation == self.generation => {
                    if let Page::Collection { title: current, .. } = &mut self.page {
                        *current = title;
                    }
                }
                Event::OwnedPlaylists(playlists) => {
                    self.owned_playlists = playlists;
                    self.playlist_busy = false;
                }
                Event::PlaylistCreated(playlist) => {
                    self.new_playlist_title.clear();
                    self.new_playlist_description.clear();
                    self.notice = Some(format!("Created {}.", playlist.title));
                    self.owned_playlists.insert(0, playlist.clone());
                    self.refresh_folders();
                    if let Some(track) = &self.playlist_target {
                        self.send(Request::AddToPlaylist {
                            id: playlist.uuid,
                            track: track.id,
                        });
                    } else {
                        self.playlist_busy = false;
                        self.playlist_dialog = false;
                        self.navigate(Page::Collection {
                            kind: "playlists".into(),
                            id: playlist.uuid,
                            title: playlist.title,
                        });
                    }
                }
                Event::PlaylistEdited { id, message } => {
                    self.playlist_busy = false;
                    self.playlist_dialog = false;
                    self.playlist_target = None;
                    self.removal = None;
                    self.notice = Some(message);
                    self.refresh_folders();
                    if matches!(&self.page, Page::Collection { kind, id: current, .. } if kind == "playlists" && *current == id)
                    {
                        self.generation += 1;
                        self.load_page(0);
                    }
                }
                Event::PlaylistError(message) => {
                    self.playlist_busy = false;
                    self.playlist_error = Some(message.clone());
                    self.error = Some(message);
                }
                Event::PkceReady(url) => {
                    self.pkce_wait = false;
                    self.signing_in = false;
                    self.pkce_url = Some(url.clone());
                    self.pkce_redirect.clear();
                    if webbrowser::open(&url).is_err() {
                        self.error =
                            Some("Open the sign-in link from the lossless sign-in window.".into());
                    }
                }
                Event::AuthKind(pkce) => {
                    self.auth_pkce = pkce;
                    if self.account.is_none() {
                        self.quality = if pkce { "LOSSLESS" } else { "HIGH" }.into();
                    }
                }
                Event::Session(account) => {
                    let account_changed = self.account != account.as_ref().map(|(user, _)| *user);
                    if account_changed {
                        self.cancel_export(true);
                        self.credentials.confirm = false;
                        self.history_account_changed();
                        self.checkpoint(true);
                        self.quality = if self.auth_pkce { "LOSSLESS" } else { "HIGH" }.into();
                        self.cancel_context();
                        self.restore_guard = None;
                        self.restore_ready = false;
                        self.play_generation = self.backend.player.reserve();
                        self.queue = Queue::default();
                        self.paused = true;
                        self.buffering = false;
                        self.actual_quality.clear();
                        self.position = 0;
                    }
                    self.mixes.clear();
                    self.daily = None;
                    self.notice = None;
                    self.playlist_error = None;
                    self.pkce_url = None;
                    self.pkce_redirect.clear();
                    self.pkce_wait = false;
                    self.connected = account.is_some();
                    self.account = account.as_ref().map(|(user, _)| *user);
                    self.country = account.map(|(_, country)| country).unwrap_or_default();
                    self.favorites = Favorites::default();
                    self.navigation = Navigation::default();
                    self.query.clear();
                    self.search_query.clear();
                    self.login = None;
                    self.signing_in = false;
                    self.folders.clear();
                    self.expanded.clear();
                    self.folder_pending.clear();
                    self.playlist_page = None;
                    self.playlist_dialog = false;
                    self.playlist_target = None;
                    self.playlist_busy = false;
                    self.owned_playlists.clear();
                    self.removal = None;
                    if self.connected {
                        if account_changed {
                            self.request_history();
                        }
                        self.request_folder("root");
                        if let Some(link) = self.pending_link.take() {
                            self.open_link(link);
                        } else {
                            self.navigate(Page::Home);
                        }
                        if account_changed || !self.restore_ready {
                            self.request_restore();
                        }
                    } else {
                        self.tracks.clear();
                        self.albums.clear();
                        self.artists.clear();
                        self.mixes.clear();
                        self.daily = None;
                        self.queue = Queue::default();
                        self.paused = true;
                        self.buffering = false;
                    }
                }
                Event::Login { url, code } => {
                    self.signing_in = false;
                    self.login = Some((url.clone(), code));
                    if webbrowser::open(&url).is_err() {
                        self.error = Some(
                            "Could not open your browser. Copy the sign-in link below.".into(),
                        );
                    }
                }
                Event::Library {
                    generation,
                    kind,
                    page,
                    append,
                } if generation == self.generation => {
                    if !append {
                        self.favorites.invalidate(kind);
                    }
                    for id in page
                        .data
                        .tracks
                        .iter()
                        .map(|t| t.id)
                        .chain(page.data.albums.iter().map(|a| a.id))
                        .chain(page.data.artists.iter().map(|a| a.id))
                    {
                        self.favorites.observe(Favorite::new(kind, id));
                    }
                    if !page.more {
                        self.favorites.mark_complete(kind);
                    }
                    self.library_total = page.total;
                    self.library_offset = page.next_offset;
                    self.more = page.more;
                    if append {
                        self.tracks.extend(page.data.tracks);
                        self.albums.extend(page.data.albums);
                        self.artists.extend(page.data.artists);
                    } else {
                        self.tracks = page.data.tracks;
                        self.albums = page.data.albums;
                        self.artists = page.data.artists;
                    }
                    self.loading = false;
                }
                Event::Favorite { user, item, result } if self.account == Some(user) => {
                    self.favorites.finish(item, result.as_ref().ok().copied());
                    match result {
                        Ok(saved) => {
                            self.notice = Some(
                                if saved {
                                    "Saved to your TIDAL favorites."
                                } else {
                                    "Removed from your TIDAL favorites."
                                }
                                .into(),
                            );
                            if self.page == Page::Library(item.kind) {
                                self.generation += 1;
                                self.load_page(0);
                            }
                        }
                        Err(message) => self.error = Some(message),
                    }
                }
                Event::Search { generation, data } if generation == self.generation => {
                    self.tracks = data.tracks;
                    self.albums = data.albums;
                    self.artists = data.artists;
                    self.loading = false;
                }
                Event::Tracks {
                    generation,
                    tracks,
                    append,
                } if generation == self.generation => {
                    self.more = tracks.len() == 100 && matches!(self.page, Page::Collection { .. });
                    if append {
                        self.tracks.extend(tracks);
                    } else {
                        self.tracks = tracks;
                    }
                    self.loading = false;
                    if matches!(self.page, Page::Track(_))
                        && self.autoplay_page
                        && !self.tracks.is_empty()
                    {
                        self.play_track(0);
                    }
                }
                Event::Home { generation, home } if generation == self.generation => {
                    self.daily = home.daily;
                    self.mixes = home.mixes;
                    self.tracks = home.tracks;
                    self.loading = false;
                    self.more = false;
                }
                Event::Folder { id, entries } => {
                    self.folder_pending.remove(&id);
                    self.folders.insert(id, entries);
                }
                Event::FolderError { id, message } => {
                    self.folder_pending.remove(&id);
                    self.error = Some(message);
                }
                Event::Radio { generation, tracks } if generation == self.generation => {
                    self.tracks = tracks;
                    self.loading = false;
                    self.more = false;
                    if self.autoplay_page {
                        self.play_track(0);
                    }
                }
                Event::Playing {
                    generation,
                    quality,
                } if generation == self.play_generation => {
                    self.buffering = false;
                    self.backend.player.pause(self.paused);
                    self.actual_quality = quality;
                }
                Event::Listening {
                    generation,
                    rendered,
                    elapsed,
                } => self.observe_listening(generation, rendered, elapsed),
                Event::Position {
                    generation,
                    seconds,
                } if generation == self.play_generation => self.position = seconds,
                Event::Ended(generation) if generation == self.play_generation => {
                    if self
                        .queue
                        .current()
                        .is_some_and(|t| self.position + 3 < t.duration)
                    {
                        self.paused = true;
                        self.ended = true;
                        self.error = Some("The stream ended early. Check your connection and press play to retry.".into());
                    } else {
                        self.advance(true);
                    }
                }
                Event::PlaybackError {
                    generation,
                    message,
                } if generation == self.play_generation => {
                    self.buffering = false;
                    self.paused = true;
                    self.ended = false;
                    self.actual_quality.clear();
                    self.error = Some(message);
                }
                Event::RequestError {
                    generation,
                    message,
                } if generation == self.generation => {
                    self.loading = false;
                    self.error = Some(message);
                }
                Event::AudioError(message) => {
                    self.audio_available = false;
                    self.error = Some(message);
                }
                Event::Error(message) => {
                    self.pkce_wait = false;
                    self.login = None;
                    self.signing_in = false;
                    self.error = Some(message);
                }
                _ => {}
            }
        }
    }

    fn desktop_events(&mut self, ctx: &egui::Context) {
        let events: Vec<_> = self
            .media
            .as_ref()
            .map(|m| m.events.try_iter().collect())
            .unwrap_or_default();
        for event in events {
            match event {
                MediaControlEvent::Toggle => self.toggle(),
                MediaControlEvent::Play if self.paused => self.toggle(),
                MediaControlEvent::Pause if !self.paused => self.toggle(),
                MediaControlEvent::Next => self.next(),
                MediaControlEvent::Previous => self.previous(),
                MediaControlEvent::Stop => self.stop_and_clear(),
                MediaControlEvent::SetPosition(p) => self.seek(p.0.as_secs()),
                MediaControlEvent::SeekBy(direction, duration) => {
                    let pos = if direction == souvlaki::SeekDirection::Forward {
                        self.position.saturating_add(duration.as_secs())
                    } else {
                        self.position.saturating_sub(duration.as_secs())
                    };
                    self.seek(pos);
                }
                MediaControlEvent::Seek(direction) => {
                    self.seek(if direction == souvlaki::SeekDirection::Forward {
                        self.position.saturating_add(10)
                    } else {
                        self.position.saturating_sub(10)
                    });
                }
                MediaControlEvent::SetVolume(v) if v.is_finite() => {
                    self.volume = v.clamp(0., 1.) as f32;
                    self.backend.player.volume(self.volume);
                }
                MediaControlEvent::OpenUri(uri) => match Link::parse(&uri) {
                    Ok(link) => self.open_link(link),
                    Err(e) => self.error = Some(e.to_string()),
                },
                MediaControlEvent::Raise => ctx.send_viewport_cmd(egui::ViewportCommand::Focus),
                MediaControlEvent::Quit => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
                _ => {}
            }
        }
    }

    fn seek(&mut self, seconds: u64) {
        if self.buffering {
            return;
        }
        if let Some(t) = self.queue.current() {
            self.position = seconds.min(t.duration.saturating_sub(1));
            if !self.actual_quality.is_empty() && !self.ended {
                self.backend.player.seek(self.position);
            } else {
                self.ended = false;
                self.actual_quality.clear();
            }
        }
    }

    fn open_link(&mut self, link: Link) {
        if !self.connected {
            self.pending_link = Some(link);
            return;
        }
        self.error = None;
        match link {
            Link::Track(id) => self.navigate(Page::Track(id)),
            Link::Artist(id) => self.navigate(Page::Artist(id)),
            Link::Album(id) => self.navigate(Page::Collection {
                kind: "albums".into(),
                id: id.to_string(),
                title: "Album".into(),
            }),
            Link::Playlist(id) => self.navigate(Page::Collection {
                kind: "playlists".into(),
                id,
                title: "Playlist".into(),
            }),
            Link::Mix(id) => self.navigate(Page::Mix {
                id,
                title: "TIDAL mix".into(),
            }),
        }
    }

    fn link_events(&mut self, ctx: &egui::Context) {
        let events: Vec<_> = self
            .instance
            .as_ref()
            .map(|i| i.events.try_iter().collect())
            .unwrap_or_default();
        for link in events {
            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
            if let Some(link) = link {
                self.open_link(link);
            }
        }
    }

    fn refresh_folders(&mut self) {
        let mut ids: Vec<_> = self.expanded.iter().cloned().collect();
        ids.push("root".into());
        for id in ids {
            self.request_folder(&id);
        }
    }

    fn apply_track_action(&mut self, action: TrackAction) {
        match action {
            TrackAction::Radio(seed) => self.navigate(Page::Radio(seed)),
            TrackAction::Add(track) => self.open_playlist_dialog(Some(track)),
            TrackAction::Queue(track, next) => self.queue_track(track, next),
        }
    }

    fn open_playlist_dialog(&mut self, track: Option<Track>) {
        if !self.connected || self.playlist_busy {
            return;
        }
        self.playlist_dialog = true;
        self.playlist_target = track;
        self.playlist_error = None;
        self.playlist_filter.clear();
        self.new_playlist_title.clear();
        self.new_playlist_description.clear();
        if self.playlist_target.is_some() {
            self.owned_playlists.clear();
            self.playlist_busy = true;
            self.send(Request::OwnedPlaylists);
        }
    }

    fn request_folder(&mut self, id: &str) {
        if self.folder_pending.insert(id.to_owned()) {
            self.send(Request::Folder { id: id.to_owned() });
        }
    }

    fn folder_tree(&mut self, ui: &mut egui::Ui, id: &str, depth: usize) {
        if depth > 20 {
            ui.label("Folder nesting limit reached");
            return;
        }
        let Some(entries) = self.folders.get(id).cloned() else {
            if self.folder_pending.contains(id) {
                ui.spinner();
            } else if ui.small_button("Load folder / retry").clicked() {
                self.request_folder(id);
            }
            return;
        };
        if entries.is_empty() {
            ui.label(RichText::new("Empty folder").size(12.).color(MUTED));
        }
        for entry in entries {
            match entry {
                LibraryEntry::Folder { id, name, count } => {
                    let expanded = self.expanded.contains(&id);
                    if ui
                        .push_id(("folder", &id), |ui| folder_button(ui, &name, expanded))
                        .inner
                        .on_hover_text(format!(
                            "{name} · {count} items · Folder level {}",
                            depth + 1
                        ))
                        .clicked()
                    {
                        if expanded {
                            self.expanded.remove(&id);
                        } else {
                            self.expanded.insert(id.clone());
                            if !self.folders.contains_key(&id) {
                                self.request_folder(&id);
                            }
                        }
                    }
                    if self.expanded.contains(&id) {
                        if ui.available_width() > 128. {
                            ui.scope(|ui| {
                                ui.spacing_mut().indent = 12.;
                                ui.indent(&id, |ui| self.folder_tree(ui, &id, depth + 1));
                            });
                        } else {
                            ui.push_id(&id, |ui| self.folder_tree(ui, &id, depth + 1));
                        }
                    }
                }
                LibraryEntry::Playlist(p) => {
                    let active = matches!(&self.page, Page::Collection { id, .. } if *id == p.uuid);
                    if ui
                        .push_id(("playlist", &p.uuid), |ui| playlist_button(ui, &p, active))
                        .inner
                        .on_hover_text(format!("{} · {} tracks", p.title, p.number_of_tracks))
                        .clicked()
                    {
                        self.navigate(Page::Collection {
                            kind: "playlists".into(),
                            id: p.uuid,
                            title: p.title,
                        });
                    }
                }
            }
        }
    }

    fn sidebar(&mut self, ctx: &egui::Context) {
        let compact = ctx.content_rect().width() < 1100.;
        egui::SidePanel::left("sidebar")
            .exact_width(if compact { 76. } else { 216. })
            .resizable(false)
            .frame(
                egui::Frame::new()
                    .fill(PANEL)
                    .inner_margin(if compact { 12 } else { 20 }),
            )
            .show(ctx, |ui| {
                ui.add_space(12.);
                ui.horizontal(|ui| {
                    mark(ui, 25., ACCENT);
                    if !compact {
                        ui.label(
                            RichText::new("Tidal Forces")
                                .size(17.)
                                .family(egui::FontFamily::Name("heading".into())),
                        );
                    }
                });
                ui.add_space(5.);
                if !compact {
                    ui.label(
                        RichText::new("Your music. Nothing else.")
                            .size(11.)
                            .color(MUTED),
                    );
                }
                ui.add_space(24.);
                for (page, icon, label) in [
                    (Page::Home, Icon::Home, "Home"),
                    (Page::Search, Icon::Search, "Search"),
                    (
                        Page::Library(FavoriteKind::Tracks),
                        Icon::Library,
                        "Library",
                    ),
                ] {
                    let active = self.page == page || (page.is_library() && self.page.is_library());
                    let response = if compact {
                        icon_button(ui, icon, label, active, 48.)
                    } else {
                        nav(ui, icon, label, active)
                    };
                    if response.clicked() {
                        self.navigate(page);
                    }
                }
                if compact {
                    ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
                        if icon_button(
                            ui,
                            Icon::Settings,
                            "Settings",
                            self.page == Page::Settings,
                            48.,
                        )
                        .clicked()
                        {
                            self.navigate(Page::Settings);
                        }
                    });
                    return;
                }
                ui.add_space(26.);
                ui.allocate_ui_with_layout(
                    vec2(ui.available_width(), 28.),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| {
                        ui.label(
                            RichText::new("YOUR PLAYLISTS")
                                .size(10.)
                                .color(MUTED)
                                .strong(),
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui
                                .add_enabled_ui(self.connected && !self.playlist_busy, |ui| {
                                    icon_button(ui, Icon::Add, "New playlist", false, 26.)
                                })
                                .inner
                                .clicked()
                            {
                                self.open_playlist_dialog(None);
                            }
                        });
                    },
                );
                ui.add_space(6.);
                egui::ScrollArea::vertical()
                    .id_salt("playlists")
                    .max_height((ui.available_height() - 145.).max(50.))
                    .show(ui, |ui| {
                        if self.connected {
                            self.folder_tree(ui, "root", 0);
                        } else {
                            ui.label(
                                RichText::new("Sign in to see your music")
                                    .size(12.)
                                    .color(MUTED),
                            );
                        }
                    });
                ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
                    ui.label(
                        RichText::new("TIDAL FORCES  ·  NATIVE PLAYER")
                            .size(10.)
                            .color(MUTED),
                    );
                    ui.add_space(10.);
                    if nav(ui, Icon::Settings, "Settings", self.page == Page::Settings).clicked() {
                        self.navigate(Page::Settings);
                    }
                    if self.connected {
                        ui.horizontal(|ui| {
                            let (dot, _) =
                                ui.allocate_exact_size(vec2(8., 8.), egui::Sense::hover());
                            ui.painter().circle_filled(dot.center(), 3., ACCENT);
                            ui.label(
                                RichText::new(format!("Connected · {}", self.country)).size(12.),
                            );
                        });
                    }
                });
            });
    }

    fn set_visualizer(&mut self, mode: Mode) {
        self.visualizer.set_mode(mode);
        self.backend.player.visualizer.enable(mode != Mode::Off);
        self.appearance.player_bar_visualizer = mode;
        self.save_appearance();
    }

    fn save_appearance(&mut self) {
        if let Err(e) = self.appearance.save() {
            self.error = Some(format!("Could not save appearance settings: {e}"));
        }
    }

    fn player_bar(&mut self, ctx: &egui::Context) {
        let mut radio = None;
        let mut favorite = None;
        let visible = !ctx.input(|i| i.viewport().minimized.unwrap_or(false));
        self.backend
            .player
            .visualizer
            .enable(visible && self.visualizer.mode != Mode::Off);
        let active = visible
            && !self.paused
            && !self.buffering
            && !self.ended
            && self.queue.current().is_some();
        egui::TopBottomPanel::bottom("player")
            .exact_height(112.)
            .frame(
                egui::Frame::new()
                    .fill(PANEL)
                    .inner_margin(18)
                    .stroke(Stroke::new(1.0_f32, CARD)),
            )
            .show(ctx, |ui| {
                let rect = ui.max_rect().expand(18.);
                let background = ui.interact(
                    rect,
                    ui.id().with("visualizer_toggle"),
                    egui::Sense::click(),
                );
                self.visualizer.paint(
                    ui,
                    rect,
                    &self.backend.player.visualizer,
                    active,
                    self.queue.current().and_then(|t| t.cover_url(80)),
                );
                if self.visualizer.mode != Mode::Off {
                    ui.painter()
                        .rect_filled(rect, 0., Color32::from_black_alpha(80));
                }
                let width = ui.available_width();
                let cover_size = if width < 900. { 48. } else { 60. };
                ui.horizontal(|ui| {
                    ui.allocate_ui_with_layout(
                        vec2(width * 0.29, 74.),
                        egui::Layout::left_to_right(egui::Align::Center),
                        |ui| {
                            ui.set_min_size(vec2(width * 0.29, 74.));
                            if let Some(track) = self.queue.current() {
                                artwork(ui, track.cover_url(80), cover_size)
                                    .context_menu(|ui| track_menu(ui, track, &mut radio));
                                ui.vertical(|ui| {
                                    ui.add_space(9.);
                                    ui.add(
                                        egui::Label::new(RichText::new(&track.title).strong())
                                            .truncate(),
                                    );
                                    ui.add(
                                        egui::Label::new(
                                            RichText::new(&track.artist.name)
                                                .size(12.)
                                                .color(MUTED),
                                        )
                                        .truncate(),
                                    );
                                    if !self.actual_quality.is_empty() {
                                        ui.add(
                                            egui::Label::new(
                                                RichText::new(
                                                    self.actual_quality.replace('_', " "),
                                                )
                                                .size(10.)
                                                .color(ACCENT),
                                            )
                                            .truncate(),
                                        )
                                        .on_hover_text(&self.actual_quality);
                                    }
                                });
                            } else {
                                artwork(ui, None, cover_size);
                                ui.vertical(|ui| {
                                    ui.label(
                                        RichText::new("Find your next favorite").strong().size(13.),
                                    );
                                    ui.label(
                                        RichText::new("Your music starts here")
                                            .size(11.)
                                            .color(MUTED),
                                    );
                                });
                            }
                        },
                    );
                    ui.allocate_ui_with_layout(
                        vec2(width * 0.42, 74.),
                        egui::Layout::top_down(egui::Align::Center),
                        |ui| {
                            ui.set_min_size(vec2(width * 0.42, 74.));
                            ui.add_enabled_ui(
                                self.audio_available
                                    && (self.queue.current().is_some()
                                        || self.queue.upcoming_len() > 0),
                                |ui| {
                                    ui.allocate_ui_with_layout(
                                        vec2(width * 0.42, 42.),
                                        egui::Layout::left_to_right(egui::Align::Center),
                                        |ui| {
                                            let controls_width =
                                                162. + 4. * ui.spacing().item_spacing.x;
                                            ui.add_space(
                                                (ui.available_width() - controls_width).max(0.)
                                                    / 2.,
                                            );
                                            if icon_button(
                                            ui,
                                            Icon::Shuffle,
                                            "Shuffle source tracks · Manual Up next is unchanged",
                                            self.queue.shuffled(),
                                            30.,
                                        )
                                        .clicked()
                                        {
                                            self.queue.toggle_shuffle();
                                        }
                                            if icon_button(
                                                ui,
                                                Icon::Previous,
                                                "Previous · Ctrl+Left",
                                                false,
                                                30.,
                                            )
                                            .clicked()
                                            {
                                                self.previous();
                                            }
                                            let transport = icon_button(
                                                ui,
                                                if self.paused { Icon::Play } else { Icon::Pause },
                                                if self.buffering {
                                                    "Loading · Click to pause / resume"
                                                } else {
                                                    "Play / pause · Space"
                                                },
                                                true,
                                                42.,
                                            );
                                            if self.buffering {
                                                egui::Spinner::new().size(12.).paint_at(
                                                    ui,
                                                    egui::Rect::from_min_size(
                                                        transport.rect.right_top() - vec2(8., 0.),
                                                        vec2(12., 12.),
                                                    ),
                                                );
                                            }
                                            if transport.clicked() {
                                                self.toggle();
                                            }
                                            if icon_button(
                                                ui,
                                                Icon::Next,
                                                "Next · Ctrl+Right",
                                                false,
                                                30.,
                                            )
                                            .clicked()
                                            {
                                                self.next();
                                            }
                                            let repeat = icon_button(
                                                ui,
                                                Icon::Repeat,
                                                self.queue.repeat().label(),
                                                self.queue.repeat() != Repeat::Off,
                                                30.,
                                            );
                                            if self.queue.repeat() == Repeat::Track {
                                                ui.painter().text(
                                                    repeat.rect.right_bottom() - vec2(2., 2.),
                                                    egui::Align2::RIGHT_BOTTOM,
                                                    "1",
                                                    egui::FontId::proportional(10.),
                                                    ACCENT,
                                                );
                                            }
                                            if repeat.clicked() {
                                                self.queue.set_repeat(self.queue.repeat().next());
                                            }
                                        },
                                    );
                                    ui.horizontal(|ui| {
                                        ui.label(
                                            RichText::new(time(self.position))
                                                .size(10.)
                                                .color(MUTED),
                                        );
                                        let duration =
                                            self.queue.current().map_or(0, |t| t.duration);
                                        let mut position = self.position as f64;
                                        ui.scope(|ui| {
                                            ui.spacing_mut().slider_width =
                                                (width * 0.42 - 105.).max(60.);
                                            let response = ui.add_enabled(
                                                duration > 0 && !self.buffering && !self.ended,
                                                egui::Slider::new(
                                                    &mut position,
                                                    0.0..=duration.max(1) as f64,
                                                )
                                                .show_value(false),
                                            );
                                            if response.drag_stopped()
                                                || (response.changed() && !response.dragged())
                                            {
                                                self.seek(position as u64);
                                            }
                                        });
                                        ui.label(
                                            RichText::new(time(duration)).size(10.).color(MUTED),
                                        );
                                    });
                                },
                            );
                        },
                    );
                    ui.allocate_ui_with_layout(
                        vec2(width * 0.25, 74.),
                        egui::Layout::right_to_left(egui::Align::Center),
                        |ui| {
                            ui.set_min_size(vec2(width * 0.25, 74.));
                            if icon_button(ui, Icon::Queue, "Show queue", self.queue_open, 32.)
                                .clicked()
                            {
                                self.queue_open = !self.queue_open;
                            }
                            if let Some(track) = self.queue.current() {
                                let more = icon_button(
                                    ui,
                                    Icon::More,
                                    "Playing track actions",
                                    false,
                                    28.,
                                );
                                egui::Popup::menu(&more).show(|ui| {
                                    if width < 900. {
                                        ui.horizontal(|ui| {
                                            ui.label("Favorite");
                                            favorite_button(
                                                ui,
                                                &self.favorites,
                                                Favorite::new(FavoriteKind::Tracks, track.id),
                                                &mut favorite,
                                            );
                                        });
                                        ui.separator();
                                    }
                                    track_menu(ui, track, &mut radio);
                                });
                                if width >= 900. {
                                    favorite_button(
                                        ui,
                                        &self.favorites,
                                        Favorite::new(FavoriteKind::Tracks, track.id),
                                        &mut favorite,
                                    );
                                }
                            }
                            if width >= 900. {
                                ui.spacing_mut().slider_width = 55.;
                                if ui
                                    .add(
                                        egui::Slider::new(&mut self.volume, 0.0..=1.0)
                                            .show_value(false),
                                    )
                                    .on_hover_text("Volume")
                                    .changed()
                                {
                                    self.backend.player.volume(self.volume);
                                }
                                icon_button(ui, Icon::Volume, "Volume", false, 24.);
                            } else {
                                ui.menu_button("Vol", |ui| {
                                    if ui
                                        .add(
                                            egui::Slider::new(&mut self.volume, 0.0..=1.0)
                                                .text("Volume"),
                                        )
                                        .changed()
                                    {
                                        self.backend.player.volume(self.volume);
                                    }
                                });
                            }
                        },
                    );
                });
                if background.clicked() {
                    self.set_visualizer(self.visualizer.mode.next());
                    ctx.request_repaint();
                }
                background.on_hover_text(format!(
                    "Player bar visualizer: {} · Click empty space to change",
                    self.visualizer.mode.label()
                ));
            });
        if let Some(seed) = radio {
            self.apply_track_action(seed);
        }
        if let Some((item, saved)) = favorite {
            self.set_favorite(item, saved);
        }
    }

    fn welcome(&mut self, ui: &mut egui::Ui) {
        ui.add_space(24.);
        egui::Frame::new()
            .fill(Color32::from_rgb(18, 30, 33))
            .inner_margin(36)
            .corner_radius(8)
            .show(ui, |ui| {
                ui.set_min_width((ui.available_width() - 2.).max(0.));
                ui.label(
                    RichText::new("A CLOSER CONNECTION TO MUSIC")
                        .size(11.)
                        .color(ACCENT)
                        .strong(),
                );
                ui.add_space(22.);
                ui.label(
                    RichText::new("All the feeling.\nNone of the weight.")
                        .size(46.)
                        .family(egui::FontFamily::Name("heading".into()))
                        .strong()
                        .line_height(Some(51.)),
                );
                ui.add_space(16.);
                ui.label(
                    RichText::new("Your TIDAL library, in a fast, native desktop player.")
                        .size(17.)
                        .color(Color32::from_rgb(198, 210, 215)),
                );
                ui.label(
                    RichText::new("Built in Rust. Designed for listening.")
                        .size(17.)
                        .color(Color32::from_rgb(198, 210, 215)),
                );
                ui.add_space(24.);
                if let Some((url, code)) = self.login.clone() {
                    ui.label(
                        RichText::new("Finish signing in with TIDAL in your browser").strong(),
                    );
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(&code).size(24.).monospace().color(ACCENT));
                        if ui.button("Copy code").clicked() {
                            ui.ctx().copy_text(code.clone());
                        }
                        if ui.button("Open sign-in").clicked() {
                            let _ = webbrowser::open(&url);
                        }
                        if ui.button("Copy link").clicked() {
                            ui.ctx().copy_text(url.clone());
                        }
                    });
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label("Waiting for authorization…");
                    });
                    if ui.small_button("Cancel sign-in").clicked() {
                        self.logout();
                    }
                } else {
                    ui.horizontal(|ui| {
                        if ui
                            .add_enabled(
                                !self.signing_in,
                                egui::Button::new(
                                    RichText::new("Connect with TIDAL").color(BG).strong(),
                                )
                                .fill(ACCENT)
                                .min_size(vec2(200., 46.)),
                            )
                            .clicked()
                        {
                            self.signing_in = true;
                            self.error = None;
                            self.send(Request::BeginPkce);
                        }
                        if self.signing_in {
                            ui.spinner();
                        }
                    });
                    if ui
                        .small_button("Compatibility sign-in (AAC only)")
                        .clicked()
                    {
                        self.quality = "HIGH".into();
                        self.send(Request::Login);
                    }
                    ui.add_space(8.);
                    ui.label(
                        RichText::new("A TIDAL subscription is required for full-track playback.")
                            .size(12.)
                            .color(MUTED),
                    );
                }
                ui.add_space(10.);
            });
        ui.add_space(32.);
        ui.heading("Made for the music");
        ui.add_space(10.);
        ui.columns(3, |columns| {
            for (ui, (title, body)) in columns.iter_mut().zip([
                (
                    "Your collection",
                    "Search tracks and albums. Bring your favorites and playlists with you.",
                ),
                (
                    "Lossless listening",
                    "Native FLAC and AAC playback. Direct audio, without a browser engine.",
                ),
                (
                    "A lighter desktop",
                    "Event-driven rendering. Background streaming. One executable.",
                ),
            ]) {
                egui::Frame::new()
                    .fill(CARD)
                    .inner_margin(18)
                    .corner_radius(6)
                    .show(ui, |ui| {
                        ui.set_min_height(90.);
                        ui.label(RichText::new(title).size(17.).strong());
                        ui.add_space(6.);
                        ui.label(RichText::new(body).size(13.).color(MUTED));
                    });
            }
        });
        ui.add_space(24.);
        ui.label(
            RichText::new("Independent client · Not affiliated with TIDAL · No DRM bypass")
                .size(11.)
                .color(MUTED),
        );
    }

    fn toolbar(&mut self, ui: &mut egui::Ui) {
        let toolbar = ui.allocate_ui_with_layout(
            vec2(ui.available_width(), 38.),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                ui.spacing_mut().item_spacing.x = 6.;
                if ui
                    .add_enabled_ui(self.navigation.can_back(), |ui| {
                        icon_button(ui, Icon::Back, "Back · Alt+Left", false, 32.)
                    })
                    .inner
                    .clicked()
                {
                    self.history(false);
                }
                if ui
                    .add_enabled_ui(self.navigation.can_forward(), |ui| {
                        icon_button(ui, Icon::Forward, "Forward · Alt+Right", false, 32.)
                    })
                    .inner
                    .clicked()
                {
                    self.history(true);
                }
                let compact = ui.ctx().content_rect().width() <= 1050.;
                let width = (if compact { 200.0_f32 } else { 210. }).min(ui.available_width());
                let gap = (ui.available_width() - width).max(0.);
                if !compact && gap >= 140. {
                    let label = match &self.page {
                        Page::Home => "Home",
                        Page::Search => "Search",
                        Page::Settings => "Preferences",
                        Page::Playlists => "Library / Playlists",
                        Page::Library(FavoriteKind::Tracks) => "Library / Tracks",
                        Page::Library(FavoriteKind::Albums) => "Library / Albums",
                        Page::Library(FavoriteKind::Artists) => "Library / Artists",
                        Page::Collection { kind, .. } if kind == "playlists" => {
                            "Library / Playlists"
                        }
                        Page::Collection { .. } => "Albums",
                        Page::Artist(_) => "Artists",
                        Page::Track(_) => "Tracks",
                        Page::Mix { .. } => "Mixes",
                        Page::Radio(_) => "Radio",
                    };
                    let (rect, _) =
                        ui.allocate_exact_size(vec2(gap - 6., 32.), egui::Sense::hover());
                    let mut crumb = ui.new_child(
                        egui::UiBuilder::new()
                            .id_salt("breadcrumb")
                            .max_rect(rect)
                            .layout(egui::Layout::left_to_right(egui::Align::Center)),
                    );
                    crumb.add(
                        egui::Label::new(RichText::new(label).size(11.).color(MUTED))
                            .truncate()
                            .selectable(false),
                    );
                } else {
                    ui.add_space(gap);
                }
                ui.add_enabled_ui(self.connected, |ui| self.search_field(ui, width));
            },
        );
        let rect = toolbar.response.rect;
        ui.painter().line_segment(
            [
                pos2(rect.left(), rect.bottom() + 8.),
                pos2(rect.right(), rect.bottom() + 8.),
            ],
            Stroke::new(1.0_f32, BORDER),
        );
        ui.add_space(20.);
    }

    fn search_field(&mut self, ui: &mut egui::Ui, width: f32) {
        let (rect, _) = ui.allocate_exact_size(vec2(width, 32.), egui::Sense::hover());
        ui.painter()
            .rect_filled(rect, 6., Color32::from_rgb(24, 27, 31));
        let mut icon = ui.new_child(egui::UiBuilder::new().id_salt("search-icon").max_rect(
            egui::Rect::from_center_size(pos2(rect.left() + 16., rect.center().y), vec2(28., 28.)),
        ));
        let clicked = icon_button(
            &mut icon,
            Icon::Search,
            "Search TIDAL or open a TIDAL link",
            false,
            28.,
        )
        .clicked();
        let edit_rect = egui::Rect::from_min_max(
            pos2(rect.left() + 34., rect.center().y - 11.),
            pos2(rect.right() - 49., rect.center().y + 11.),
        );
        let mut editor = ui.new_child(
            egui::UiBuilder::new()
                .id_salt("search-editor")
                .max_rect(edit_rect),
        );
        let response = editor
            .add_sized(
                edit_rect.size(),
                egui::TextEdit::singleline(&mut self.query)
                    .id(egui::Id::new("global-search"))
                    .char_limit(512)
                    .font(egui::FontId::proportional(12.))
                    .hint_text("Search TIDAL")
                    .frame(false)
                    .margin(vec2(0., 3.)),
            )
            .on_hover_text("Search music or paste a TIDAL link · Ctrl+K");
        if self.focus_search {
            if ui.is_enabled() {
                if ui.ctx().content_rect().width() < 1180. {
                    self.queue_open = false;
                }
                response.request_focus();
            }
            self.focus_search = false;
        }
        let badge =
            egui::Rect::from_center_size(pos2(rect.right() - 26., rect.center().y), vec2(34., 17.));
        ui.painter().rect_stroke(
            badge,
            3.,
            Stroke::new(1.0_f32, BORDER),
            egui::StrokeKind::Inside,
        );
        ui.painter().text(
            badge.center(),
            egui::Align2::CENTER_CENTER,
            "Ctrl K",
            egui::FontId::proportional(9.),
            MUTED,
        );
        ui.painter().rect_stroke(
            rect,
            6.,
            Stroke::new(1.0_f32, if response.has_focus() { ACCENT } else { BORDER }),
            egui::StrokeKind::Inside,
        );
        if clicked || (response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))) {
            self.search();
        }
    }

    fn content(&mut self, ui: &mut egui::Ui) {
        let short = ui.ctx().content_rect().height() < 600.;
        self.credential_notice(ui);
        if let Some(error) = self.error.clone() {
            egui::Frame::new()
                .fill(Color32::from_rgb(61, 30, 33))
                .inner_margin(12)
                .corner_radius(5)
                .show(ui, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(RichText::new(error).color(Color32::from_rgb(255, 196, 192)));
                        if ui.small_button("Dismiss").clicked() {
                            self.error = None;
                        }
                    });
                });
            ui.add_space(12.);
        }
        if let Some(notice) = self.notice.clone() {
            ui.horizontal_wrapped(|ui| {
                ui.colored_label(ACCENT, notice);
                if ui.small_button("Dismiss").clicked() {
                    self.notice = None;
                }
            });
        }
        if self.page == Page::Settings {
            self.settings_page(ui);
            return;
        }
        if !self.connected {
            if self.pending_link.is_some() {
                ui.label("Sign in to open your TIDAL link.");
            }
            ui.label(
                RichText::new(self.credential_label())
                    .size(12.)
                    .color(MUTED),
            );
            self.welcome(ui);
            return;
        }
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(match self.page {
                    Page::Home => "MADE FOR YOU",
                    Page::Library(_) | Page::Playlists => "YOUR LIBRARY",
                    Page::Search => "EXPLORE TIDAL",
                    _ => "YOUR MUSIC",
                })
                .size(10.)
                .color(MUTED),
            );
            if ui
                .add_enabled(!self.loading, egui::Button::new("Refresh").small())
                .clicked()
            {
                self.error = None;
                self.autoplay_page = false;
                self.generation += 1;
                self.load_page(0);
                self.refresh_folders();
            }
        });
        ui.add_space(if short { 4. } else { 16. });
        match &self.page {
            Page::Track(_) => {
                surfaces::page_title(
                    ui,
                    self.tracks.first().map_or("Track", |t| t.title.as_str()),
                );
            }
            Page::Artist(_) => {
                surfaces::page_title(
                    ui,
                    self.artists.first().map_or("Artist", |a| a.name.as_str()),
                );
            }
            Page::Home => {
                surfaces::hero(
                    ui,
                    self.daily.as_ref().and_then(Mix::cover_url),
                    "DAILY DISCOVERY",
                    self.daily
                        .as_ref()
                        .map_or("Made for you", |mix| mix.title.as_str()),
                    "Fresh discoveries and personal mixes, selected by TIDAL.",
                    true,
                );
            }
            Page::Mix { title, .. } => {
                surfaces::page_title(ui, title);
            }
            Page::Radio(seed) => {
                surfaces::page_title(ui, &seed.title());
                ui.label(
                    RichText::new("Radio selected by TIDAL · Starts automatically").color(MUTED),
                );
            }
            Page::Library(_) | Page::Playlists => {
                surfaces::page_title(ui, "Your library");
                if !short {
                    ui.label(
                        RichText::new("Your saved music and playlists, together.").color(MUTED),
                    );
                }
            }
            Page::Settings => unreachable!(),
            Page::Search => {
                surfaces::page_title(ui, "Find your next favorite");
            }
            Page::Collection { title, kind, .. } => {
                let cover = if kind == "albums" {
                    self.tracks.first().and_then(|track| track.cover_url(320))
                } else {
                    self.playlist_page
                        .as_ref()
                        .and_then(|page| page.playlist.artwork_url())
                };
                let subtitle = if self.loading {
                    "Loading your music…".into()
                } else {
                    format!(
                        "{} tracks loaded{}",
                        self.tracks.len(),
                        if self
                            .playlist_page
                            .as_ref()
                            .is_some_and(|page| page.editable)
                        {
                            " · Your playlist"
                        } else {
                            ""
                        }
                    )
                };
                surfaces::hero(
                    ui,
                    cover,
                    &kind.trim_end_matches('s').to_uppercase(),
                    title,
                    &subtitle,
                    false,
                );
            }
        }
        let collection = matches!(self.page, Page::Collection { .. });
        if collection {
            ui.add_space(12.);
            ui.horizontal(|ui| {
                ui.add_enabled_ui(self.audio_available && !self.loading && !self.tracks.is_empty(), |ui| {
                    if surfaces::action_button(ui,Icon::Play,"Play",true).on_hover_text("Play this collection in source order · Manual Up next is preserved").clicked() { self.play_collection(false); }
                    if surfaces::action_button(ui,Icon::Shuffle,"Shuffle",false).on_hover_text("Start with a random loaded track; remaining source pages join the shuffle as they load. Manual Up next stays ordered.").clicked() { self.play_collection(true); }
                });
                if let Some(page) = &self.playlist_page {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center),|ui| {
                        ui.label(RichText::new(if page.editable { "Owned" } else { "Read-only" }).size(12.).color(MUTED));
                    });
                }
            });
        }
        if self.page == Page::Home {
            self.history_shelf(ui);
        }
        let header_item = match &self.page {
            Page::Artist(id) => Some(Favorite::new(FavoriteKind::Artists, *id)),
            Page::Collection { kind, id, .. } if kind == "albums" => id
                .parse()
                .ok()
                .map(|id| Favorite::new(FavoriteKind::Albums, id)),
            _ => None,
        };
        if let Some(item) = header_item {
            let mut favorite = None;
            ui.horizontal(|ui| {
                favorite_button(ui, &self.favorites, item, &mut favorite);
                ui.label(RichText::new("TIDAL favorites").size(12.).color(MUTED));
            });
            if let Some((item, saved)) = favorite {
                self.set_favorite(item, saved);
            }
        }
        ui.add_space(if short { 8. } else { 18. });
        if self.page.is_library() {
            self.library_tabs(ui);
        }
        if self.page == Page::Playlists {
            if ui
                .add_enabled(!self.playlist_busy, egui::Button::new("+ New playlist"))
                .clicked()
            {
                self.open_playlist_dialog(None);
            }
            self.folder_tree(ui, "root", 0);
            return;
        }
        let mut favorite = None;
        if self.loading {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Loading your music…");
            });
        }
        if !self.artists.is_empty() {
            if !self.page.is_library() {
                surfaces::section_title(ui, "Artists");
            }
            let mut radio = None;
            let mut selected_artist = None;
            surfaces::media_shelf(
                ui,
                "artists",
                &self.artists,
                self.page.is_library(),
                100.,
                |ui, artist, width| {
                    surfaces::card(ui, width, |ui| {
                        if rounded_artwork(
                            ui,
                            cover_url(artist.picture.as_deref(), 320),
                            width - 26.,
                            ((width - 26.) / 2.) as u8,
                        )
                        .clicked()
                        {
                            selected_artist = Some(artist.id);
                        }
                        if ui
                            .add(egui::Button::new(&artist.name).frame(false).truncate())
                            .clicked()
                        {
                            selected_artist = Some(artist.id);
                        }
                        favorite_button(
                            ui,
                            &self.favorites,
                            Favorite::new(FavoriteKind::Artists, artist.id),
                            &mut favorite,
                        );
                        if ui.small_button("Start artist radio").clicked() {
                            radio = Some(RadioSeed::Artist {
                                id: artist.id,
                                name: artist.name.clone(),
                            });
                        }
                    });
                },
            );
            if let Some(seed) = radio {
                self.navigate(Page::Radio(seed));
            } else if let Some(id) = selected_artist {
                self.navigate(Page::Artist(id));
            }
            ui.add_space(20.);
        }
        if !self.albums.is_empty() {
            if !self.page.is_library() {
                surfaces::section_title(ui, "Albums");
                ui.add_space(8.);
            }
            let mut selected = None;
            surfaces::media_shelf(
                ui,
                "albums",
                &self.albums,
                self.page.is_library(),
                92.,
                |ui, album, width| {
                    surfaces::card(ui, width, |ui| {
                        if artwork(ui, cover_url(album.cover.as_deref(), 320), width - 26.)
                            .clicked()
                        {
                            selected = Some(album.clone());
                        }
                        if ui
                            .add(egui::Button::new(&album.title).frame(false).truncate())
                            .clicked()
                        {
                            selected = Some(album.clone());
                        }
                        ui.add(
                            egui::Label::new(
                                RichText::new(&album.artist.name).size(12.).color(MUTED),
                            )
                            .truncate(),
                        );
                        favorite_button(
                            ui,
                            &self.favorites,
                            Favorite::new(FavoriteKind::Albums, album.id),
                            &mut favorite,
                        );
                    });
                },
            );
            if let Some(a) = selected {
                self.navigate(Page::Collection {
                    kind: "albums".into(),
                    id: a.id.to_string(),
                    title: a.title,
                });
            }
            ui.add_space(26.);
        }
        if let Some((item, saved)) = favorite {
            self.set_favorite(item, saved);
        }
        if matches!(
            self.page,
            Page::Library(FavoriteKind::Albums | FavoriteKind::Artists)
        ) {
            if !self.loading && self.albums.is_empty() && self.artists.is_empty() {
                ui.label(
                    RichText::new(
                        "Nothing saved here yet. Search for music to add to your library.",
                    )
                    .color(MUTED),
                );
            }
            if self.more
                && ui
                    .add_enabled(!self.loading, egui::Button::new("Load more"))
                    .clicked()
            {
                self.load_page(self.library_offset);
            }
            return;
        }
        if self.page == Page::Home && !self.mixes.is_empty() {
            surfaces::section_title(ui, "Made for your day");
            ui.add_space(10.);
            let mut selected = None;
            let mixes: Vec<_> = self
                .mixes
                .iter()
                .filter(|mix| mix.mix_type != "DISCOVERY_MIX")
                .collect();
            surfaces::media_shelf(ui, "mix_cards", &mixes, false, 64., |ui, mix, width| {
                surfaces::card(ui, width, |ui| {
                    let cover = artwork(ui, mix.cover_url(), width - 26.);
                    let title = ui.add(egui::Button::new(&mix.title).frame(false).truncate());
                    if cover.clicked() || title.clicked() {
                        selected = Some((mix.id.clone(), mix.title.clone()));
                    }
                    ui.add(
                        egui::Label::new(RichText::new(&mix.sub_title).color(MUTED).size(12.))
                            .truncate(),
                    );
                });
            });
            if let Some((id, title)) = selected {
                self.navigate(Page::Mix { id, title });
            }
            ui.add_space(26.);
        }
        ui.horizontal(|ui| {
            surfaces::section_title(
                ui,
                match self.page {
                    Page::Home => "Today's discovery",
                    Page::Library(FavoriteKind::Tracks) => "Favorite tracks",
                    _ => "Tracks",
                },
            );
            if !self.tracks.is_empty() {
                ui.label(
                    RichText::new(format!("{} loaded", self.tracks.len()))
                        .size(12.)
                        .color(MUTED),
                );
                if !collection
                    && ui
                        .add_enabled(
                            self.audio_available,
                            egui::Button::new(RichText::new("Play all").color(BG)).fill(ACCENT),
                        )
                        .clicked()
                {
                    self.play_track(0);
                }
            }
        });
        ui.add_space(8.);
        if self.tracks.is_empty() && !self.loading {
            ui.add_space(20.);
            ui.label(
                RichText::new(if self.page == Page::Search {
                    if self.query.is_empty() {
                        "What do you want to listen to?"
                    } else {
                        "No tracks found. Try another search."
                    }
                } else {
                    "No tracks here yet. Search for something you love."
                })
                .color(MUTED),
            );
        }
        self.track_list(ui);
        if self.more
            && ui
                .add_enabled(!self.loading, egui::Button::new("Load more tracks"))
                .clicked()
        {
            let offset = if matches!(self.page, Page::Library(_)) {
                self.library_offset
            } else {
                self.playlist_page
                    .as_ref()
                    .map_or(self.tracks.len(), |p| p.next_offset)
            };
            self.load_page(offset);
        }
    }

    fn playlist_windows(&mut self, ctx: &egui::Context) {
        if self.playlist_dialog {
            let mut open = true;
            let mut add = None;
            let mut create = false;
            egui::Window::new(if self.playlist_target.is_some() {
                "Add to playlist"
            } else {
                "Create playlist"
            })
            .id(egui::Id::new("playlist_editor"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(420.)
            .show(ctx, |ui| {
                if let Some(track) = &self.playlist_target {
                    ui.label(RichText::new(&track.title).strong());
                    ui.label(RichText::new(&track.artist.name).color(MUTED));
                    ui.add(
                        egui::TextEdit::singleline(&mut self.playlist_filter)
                            .hint_text("Find one of your playlists"),
                    );
                    egui::ScrollArea::vertical()
                        .max_height(230.)
                        .show(ui, |ui| {
                            for playlist in &self.owned_playlists {
                                if !playlist
                                    .title
                                    .to_lowercase()
                                    .contains(&self.playlist_filter.to_lowercase())
                                {
                                    continue;
                                }
                                if ui
                                    .add_enabled(
                                        !self.playlist_busy,
                                        egui::Button::new(format!(
                                            "{}  ·  {} tracks",
                                            playlist.title, playlist.number_of_tracks
                                        )),
                                    )
                                    .clicked()
                                {
                                    add = Some(playlist.uuid.clone());
                                }
                            }
                            if self.owned_playlists.is_empty() && !self.playlist_busy {
                                ui.label("No editable playlists yet. Create one below.");
                            }
                        });
                    ui.separator();
                    ui.label("Or create a new playlist");
                }
                ui.add(
                    egui::TextEdit::singleline(&mut self.new_playlist_title)
                        .hint_text("Playlist name")
                        .char_limit(200),
                );
                ui.add(
                    egui::TextEdit::multiline(&mut self.new_playlist_description)
                        .hint_text("Description (optional)")
                        .desired_rows(2)
                        .char_limit(1000),
                );
                if let Some(error) = &self.playlist_error {
                    ui.colored_label(Color32::LIGHT_RED, error);
                }
                if self.playlist_busy {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label("Waiting for TIDAL…");
                    });
                }
                let label = if self.playlist_target.is_some() {
                    "Create and add track"
                } else {
                    "Create playlist"
                };
                create = ui
                    .add_enabled(
                        !self.playlist_busy && !self.new_playlist_title.trim().is_empty(),
                        egui::Button::new(label),
                    )
                    .clicked();
            });
            self.playlist_dialog = open;
            if let Some(id) = add
                && let Some(track) = &self.playlist_target
            {
                self.playlist_busy = true;
                self.playlist_error = None;
                self.send(Request::AddToPlaylist {
                    id,
                    track: track.id,
                });
            } else if create {
                self.playlist_busy = true;
                self.playlist_error = None;
                self.send(Request::CreatePlaylist {
                    title: self.new_playlist_title.trim().into(),
                    description: self.new_playlist_description.clone(),
                });
            }
        }
        if let Some(removal) = &self.removal {
            let mut open = true;
            let mut confirm = false;
            let mut cancel = false;
            egui::Window::new("Remove track?")
                .default_width(420.)
                .open(&mut open)
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.label(format!(
                        "Remove “{}” from “{}”?",
                        removal.track.title, removal.title
                    ));
                    ui.label("Only this occurrence is removed. Your playback queue is unchanged.");
                    if let Some(error) = &self.playlist_error {
                        ui.colored_label(Color32::LIGHT_RED, error);
                    }
                    ui.horizontal(|ui| {
                        confirm = ui
                            .add_enabled(!self.playlist_busy, egui::Button::new("Remove track"))
                            .clicked();
                        cancel = ui.button("Cancel").clicked();
                    });
                });
            if confirm {
                self.playlist_busy = true;
                self.playlist_error = None;
                self.send(Request::RemoveFromPlaylist {
                    id: removal.playlist.clone(),
                    index: removal.index,
                    track: removal.track.id,
                    etag: removal.etag.clone(),
                });
            }
            if !open || cancel {
                self.removal = None;
            }
        }
    }

    fn lossless_sign_in(&mut self, ctx: &egui::Context) {
        let Some(url) = self.pkce_url.clone() else {
            return;
        };
        let mut open = true;
        egui::Window::new("Connect lossless playback").open(&mut open).default_width(520.).resizable(false).show(ctx, |ui| {
            ui.label("1. Sign in on TIDAL's website in the browser opened for you.");
            ui.label("2. The final page may say ‘Oops’. That is expected. Copy its entire address from the browser address bar.");
            ui.label("3. Paste that address here, not in chat. The authorization code is single-use.");
            ui.horizontal(|ui| {
                if ui.button("Open TIDAL sign-in").clicked() { let _ = webbrowser::open(&url); }
                if ui.button("Copy sign-in link").clicked() { ctx.copy_text(url.clone()); }
            });
            ui.add(egui::TextEdit::singleline(&mut self.pkce_redirect).password(true).desired_width(f32::INFINITY).hint_text("Paste the complete redirected TIDAL URL"));
            if ui.add_enabled(!self.pkce_wait && !self.pkce_redirect.trim().is_empty(), egui::Button::new("Finish lossless sign-in").fill(ACCENT)).clicked() {
                self.pkce_wait = true;
                let redirect = std::mem::take(&mut self.pkce_redirect);
                self.send(Request::FinishPkce(redirect));
            }
            if self.pkce_wait { ui.spinner(); }
            if let Some(error) = &self.error { ui.colored_label(Color32::LIGHT_RED, error); }
            ui.label(RichText::new("Your current session stays connected until authorization succeeds. This does not change your subscription.").size(12.).color(MUTED));
        });
        if !open {
            self.pkce_url = None;
            self.pkce_redirect.clear();
            self.send(Request::CancelPkce);
        }
    }
}

impl eframe::App for App {
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.cancel_export(true);
        self.checkpoint_history(true);
        self.checkpoint(true);
        self.state_store.take();
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if let Some(path) = &self.screenshot {
            ctx.request_repaint_after(std::time::Duration::from_millis(16));
            for event in ctx.input(|i| i.events.clone()) {
                if let egui::Event::Screenshot { image, .. } = event {
                    let rgba: Vec<u8> = image.pixels.iter().flat_map(|p| p.to_array()).collect();
                    if let Err(e) = image::save_buffer(
                        path,
                        &rgba,
                        image.width() as u32,
                        image.height() as u32,
                        image::ColorType::Rgba8,
                    ) {
                        eprintln!("Screenshot failed: {e}");
                    }
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
        }
        self.link_events(ctx);
        self.events();
        self.state_events();
        self.desktop_events(ctx);
        self.credential_close_guard(ctx);
        if ctx.input(|input| input.viewport().close_requested()) {
            self.cancel_export(false);
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, egui::Key::K)) {
            if self.page != Page::Search {
                self.navigate(Page::Search);
            }
            self.focus_search = true;
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::ALT, egui::Key::ArrowLeft)) {
            self.history(false);
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::ALT, egui::Key::ArrowRight)) {
            self.history(true);
        }
        if !ctx.wants_keyboard_input() {
            if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Space)) {
                self.toggle();
            }
            if ctx.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, egui::Key::ArrowRight)) {
                self.next();
            }
            if ctx.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, egui::Key::ArrowLeft)) {
                self.previous();
            }
        }
        self.player_bar(ctx);
        self.sidebar(ctx);
        self.queue_panel(ctx);
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(BG).inner_margin(
                if ctx.content_rect().width() < 1100. {
                    16
                } else {
                    24
                },
            ))
            .show(ctx, |ui| {
                self.toolbar(ui);
                egui::ScrollArea::vertical()
                    .id_salt(("content", &self.page, &self.search_query))
                    .show(ui, |ui| {
                        self.content(ui);
                    });
            });
        self.lossless_sign_in(ctx);
        self.playlist_windows(ctx);
        self.export_window(ctx);
        self.prefetch_context();
        self.export_tick();
        self.queue_drag_cursor(ctx);
        self.checkpoint_history(false);
        self.checkpoint(false);
        if let Some(media) = &mut self.media
            && let Err(e) = media.update(
                self.queue.current(),
                self.paused || self.buffering,
                self.position,
                self.volume,
            )
        {
            self.error = Some(format!("Desktop media controls: {e}"));
            self.media = None;
        }
    }
}

fn library_row(
    ui: &mut egui::Ui,
    name: &str,
    active: bool,
    trailing: f32,
) -> (egui::Rect, egui::Response) {
    let (rect, response) =
        ui.allocate_exact_size(vec2(ui.available_width(), 40.), egui::Sense::click());
    if response.clicked() {
        response.request_focus();
    }
    if response.gained_focus() {
        response.scroll_to_me(None);
    }
    if ui.is_rect_visible(rect) {
        if active || response.hovered() {
            ui.painter()
                .rect_filled(rect, 6., if active { PLAYING } else { CARD });
        }
        if response.has_focus() {
            ui.painter().rect_stroke(
                rect,
                6.,
                Stroke::new(1.0_f32, ACCENT),
                egui::StrokeKind::Inside,
            );
        }
        let mut title = ui.new_child(egui::UiBuilder::new().id_salt(response.id).max_rect(
            egui::Rect::from_min_max(
                pos2(rect.left() + 44., rect.center().y - 8.),
                pos2(rect.right() - trailing, rect.bottom()),
            ),
        ));
        title.add(
            egui::Label::new(RichText::new(name).size(13.).color(if active {
                ACCENT
            } else {
                MUTED
            }))
            .truncate()
            .selectable(false),
        );
    }
    (
        rect,
        response.on_hover_cursor(egui::CursorIcon::PointingHand),
    )
}

fn folder_button(ui: &mut egui::Ui, name: &str, expanded: bool) -> egui::Response {
    let (rect, response) = library_row(ui, name, false, 26.);
    if ui.is_rect_visible(rect) {
        paint_icon(
            ui.painter(),
            Icon::Folder,
            pos2(rect.left() + 20., rect.center().y),
            22.,
            if expanded { ACCENT } else { MUTED },
        );
        paint_icon(
            ui.painter(),
            if expanded { Icon::Down } else { Icon::Forward },
            pos2(rect.right() - 12., rect.center().y),
            14.,
            MUTED,
        );
    }
    response.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::WidgetType::Button,
            ui.is_enabled(),
            format!(
                "{} folder {name}",
                if expanded { "Collapse" } else { "Expand" }
            ),
        )
    });
    response
}

fn playlist_button(ui: &mut egui::Ui, playlist: &Playlist, active: bool) -> egui::Response {
    let (rect, response) = library_row(ui, &playlist.title, active, 8.);
    if ui.is_rect_visible(rect) {
        let cover =
            egui::Rect::from_center_size(pos2(rect.left() + 20., rect.center().y), vec2(32., 32.));
        ui.painter().rect_filled(cover, 4., CARD);
        let mut loaded = false;
        if let Some(url) = playlist.artwork_url() {
            let wide = playlist.square_image.as_deref().is_none_or(str::is_empty);
            let uv = if wide {
                egui::Rect::from_min_max(pos2(1. / 6., 0.), pos2(5. / 6., 1.))
            } else {
                egui::Rect::from_min_max(pos2(0., 0.), pos2(1., 1.))
            };
            let image = egui::Image::new(url)
                .fit_to_exact_size(cover.size())
                .maintain_aspect_ratio(false)
                .uv(uv)
                .corner_radius(4);
            if matches!(
                image.load_for_size(ui.ctx(), cover.size()),
                Ok(egui::load::TexturePoll::Ready { .. })
            ) {
                image.paint_at(ui, cover);
                loaded = true;
            }
        }
        if !loaded {
            paint_icon(ui.painter(), Icon::Playlist, cover.center(), 18., MUTED);
        }
    }
    response.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::WidgetType::Button,
            ui.is_enabled(),
            format!("Open playlist {}", playlist.title),
        )
    });
    response
}

fn track_menu(ui: &mut egui::Ui, track: &Track, radio: &mut Option<TrackAction>) {
    if ui.button("Play next").clicked() {
        *radio = Some(TrackAction::Queue(track.clone(), true));
        ui.close();
    }
    if ui.button("Add to Up next").clicked() {
        *radio = Some(TrackAction::Queue(track.clone(), false));
        ui.close();
    }
    ui.separator();
    if ui.button("Add to playlist…").clicked() {
        *radio = Some(TrackAction::Add(track.clone()));
        ui.close();
    }
    ui.separator();
    if ui.button("Start track radio").clicked() {
        *radio = Some(TrackAction::Radio(RadioSeed::Track {
            id: track.id,
            title: track.title.clone(),
        }));
        ui.close();
    }
    if track.artist.id != 0
        && ui
            .button(format!("Start artist radio · {}", track.artist.name))
            .clicked()
    {
        *radio = Some(TrackAction::Radio(RadioSeed::Artist {
            id: track.artist.id,
            name: track.artist.name.clone(),
        }));
        ui.close();
    }
}

fn artwork(ui: &mut egui::Ui, url: Option<String>, size: f32) -> egui::Response {
    rounded_artwork(ui, url, size, 8)
}

fn rounded_artwork(
    ui: &mut egui::Ui,
    url: Option<String>,
    size: f32,
    radius: u8,
) -> egui::Response {
    if let Some(url) = url {
        ui.add(
            egui::Image::new(url)
                .fit_to_exact_size(vec2(size, size))
                .corner_radius(radius)
                .sense(egui::Sense::click()),
        )
    } else {
        let (r, response) = ui.allocate_exact_size(vec2(size, size), egui::Sense::click());
        ui.painter().rect_filled(r, radius, CARD);
        let c = r.center();
        for (i, h) in [0.20, 0.42, 0.65, 0.35, 0.52].iter().enumerate() {
            let x = c.x + (i as f32 - 2.) * size * 0.12;
            ui.painter().line_segment(
                [pos2(x, c.y - size * h * 0.4), pos2(x, c.y + size * h * 0.4)],
                Stroke::new(2.0_f32, MUTED),
            );
        }
        response
    }
}

fn mark(ui: &mut egui::Ui, size: f32, color: Color32) {
    let (r, _) = ui.allocate_exact_size(vec2(size, size), egui::Sense::hover());
    let c = r.center();
    let s = size / 5.;
    // An original four-point current mark, not TIDAL's trademark diamond logo.
    ui.painter().add(egui::Shape::convex_polygon(
        vec![
            pos2(c.x, c.y - 2.3 * s),
            pos2(c.x + s * 0.5, c.y - s * 0.5),
            pos2(c.x + 2.3 * s, c.y),
            pos2(c.x + s * 0.5, c.y + s * 0.5),
            pos2(c.x, c.y + 2.3 * s),
            pos2(c.x - s * 0.5, c.y + s * 0.5),
            pos2(c.x - 2.3 * s, c.y),
            pos2(c.x - s * 0.5, c.y - s * 0.5),
        ],
        color,
        Stroke::NONE,
    ));
}

#[derive(Clone, Copy)]
enum Icon {
    Home,
    Search,
    Library,
    Settings,
    Play,
    Pause,
    Previous,
    Next,
    Queue,
    Shuffle,
    Repeat,
    Volume,
    Heart,
    Back,
    Forward,
    Up,
    Down,
    Close,
    More,
    Add,
    Folder,
    Playlist,
}

fn nav(ui: &mut egui::Ui, icon: Icon, label: &str, selected: bool) -> egui::Response {
    let (r, response) =
        ui.allocate_exact_size(vec2(ui.available_width(), 44.), egui::Sense::click());
    if response.hovered() || selected {
        ui.painter()
            .rect_filled(r, 8., if selected { PLAYING } else { CARD });
    }
    if response.has_focus() {
        ui.painter().rect_stroke(
            r,
            5.,
            Stroke::new(1.0_f32, ACCENT),
            egui::StrokeKind::Inside,
        );
    }
    if selected {
        ui.painter().rect_filled(
            egui::Rect::from_min_size(r.min, vec2(3., r.height())),
            2.,
            ACCENT,
        );
    }
    paint_icon(
        ui.painter(),
        icon,
        pos2(r.min.x + 22., r.center().y),
        19.,
        if selected { ACCENT } else { MUTED },
    );
    ui.painter().text(
        pos2(r.min.x + 45., r.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        egui::FontId::proportional(14.),
        if selected { Color32::WHITE } else { MUTED },
    );
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), label)
    });
    response
}

fn icon_button(
    ui: &mut egui::Ui,
    icon: Icon,
    label: &str,
    active: bool,
    size: f32,
) -> egui::Response {
    let (r, response) = ui.allocate_exact_size(Vec2::splat(size), egui::Sense::click());
    let transport = matches!(icon, Icon::Play | Icon::Pause);
    if response.has_focus() {
        ui.painter()
            .circle_stroke(r.center(), size / 2., Stroke::new(1.0_f32, ACCENT));
    }
    if active || response.hovered() {
        ui.painter().circle_filled(
            r.center(),
            size / 2.,
            if transport { Color32::WHITE } else { CARD },
        );
    }
    paint_icon(
        ui.painter(),
        icon,
        r.center(),
        size * 0.52,
        if active && transport {
            BG
        } else if active {
            ACCENT
        } else {
            MUTED
        },
    );
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), label)
    });
    response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(label)
}

fn paint_icon(p: &egui::Painter, icon: Icon, c: egui::Pos2, size: f32, color: Color32) {
    let s = size / 2.;
    let at = |x: f32, y: f32| c + vec2(x * s, y * s);
    let stroke = Stroke::new(1.6_f32, color);
    let line = |a: (f32, f32), b: (f32, f32)| {
        p.line_segment([at(a.0, a.1), at(b.0, b.1)], stroke);
    };
    match icon {
        Icon::Folder => {
            p.add(egui::Shape::closed_line(
                vec![
                    at(-0.85, -0.65),
                    at(-0.3, -0.65),
                    at(-0.05, -0.3),
                    at(0.85, -0.3),
                    at(0.85, 0.7),
                    at(-0.85, 0.7),
                ],
                stroke,
            ));
        }
        Icon::Playlist => {
            line((-0.8, -0.6), (0.7, -0.6));
            line((-0.8, -0.05), (0.2, -0.05));
            line((-0.8, 0.5), (0.2, 0.5));
            line((0.7, -0.05), (0.7, 0.7));
            p.circle_filled(at(0.48, 0.7), s * 0.22, color);
        }
        Icon::Back | Icon::Forward | Icon::Up | Icon::Down => {
            let points = match icon {
                Icon::Back => [at(0.3, -0.65), at(-0.35, 0.), at(0.3, 0.65)],
                Icon::Forward => [at(-0.3, -0.65), at(0.35, 0.), at(-0.3, 0.65)],
                Icon::Up => [at(-0.65, 0.3), at(0., -0.35), at(0.65, 0.3)],
                _ => [at(-0.65, -0.3), at(0., 0.35), at(0.65, -0.3)],
            };
            p.add(egui::Shape::line(points.to_vec(), stroke));
        }
        Icon::Close => {
            line((-0.6, -0.6), (0.6, 0.6));
            line((-0.6, 0.6), (0.6, -0.6));
        }
        Icon::Add => {
            line((-0.7, 0.), (0.7, 0.));
            line((0., -0.7), (0., 0.7));
        }
        Icon::More => {
            for x in [-0.65, 0., 0.65] {
                p.circle_filled(at(x, 0.), 1.25, color);
            }
        }
        Icon::Play => {
            p.add(egui::Shape::convex_polygon(
                vec![at(-0.55, -0.85), at(0.8, 0.), at(-0.55, 0.85)],
                color,
                Stroke::NONE,
            ));
        }
        Icon::Pause => {
            for x in [-0.45, 0.35] {
                p.rect_filled(
                    egui::Rect::from_min_max(at(x - 0.15, -0.8), at(x + 0.15, 0.8)),
                    1.,
                    color,
                );
            }
        }
        Icon::Next | Icon::Previous => {
            let f = if matches!(icon, Icon::Next) { 1. } else { -1. };
            p.add(egui::Shape::convex_polygon(
                vec![at(-0.7 * f, -0.7), at(0.5 * f, 0.), at(-0.7 * f, 0.7)],
                color,
                Stroke::NONE,
            ));
            line((0.75 * f, -0.8), (0.75 * f, 0.8));
        }
        Icon::Home => {
            for (a, b) in [
                ((-0.9, -0.1), (0., -0.9)),
                ((0., -0.9), (0.9, -0.1)),
                ((-0.65, -0.25), (-0.65, 0.8)),
                ((-0.65, 0.8), (0.65, 0.8)),
                ((0.65, 0.8), (0.65, -0.25)),
            ] {
                line(a, b);
            }
        }
        Icon::Search => {
            p.circle_stroke(at(-0.2, -0.2), s * 0.6, stroke);
            line((0.3, 0.3), (0.9, 0.9));
        }
        Icon::Library => {
            for x in [-0.75, -0.25, 0.55] {
                line((x, -0.85), (x, 0.85));
            }
        }
        Icon::Queue => {
            for y in [-0.6, 0., 0.6] {
                line((-0.8, y), (0.8, y));
            }
        }
        Icon::Volume => {
            line((-0.8, -0.3), (-0.35, -0.3));
            line((-0.35, -0.3), (0.15, -0.8));
            line((0.15, -0.8), (0.15, 0.8));
            line((0.15, 0.8), (-0.35, 0.3));
            line((-0.35, 0.3), (-0.8, 0.3));
            line((-0.8, 0.3), (-0.8, -0.3));
            line((0.65, -0.45), (0.85, 0.));
            line((0.85, 0.), (0.65, 0.45));
        }
        Icon::Heart => {
            p.add(egui::Shape::line(
                vec![
                    at(0., 0.85),
                    at(-0.85, 0.),
                    at(-0.85, -0.5),
                    at(-0.45, -0.8),
                    at(0., -0.4),
                    at(0.45, -0.8),
                    at(0.85, -0.5),
                    at(0.85, 0.),
                    at(0., 0.85),
                ],
                stroke,
            ));
        }
        Icon::Settings => {
            p.circle_stroke(c, s * 0.65, stroke);
            p.circle_stroke(c, s * 0.22, stroke);
            for (x, y) in [(1., 0.), (-1., 0.), (0., 1.), (0., -1.)] {
                line((x * 0.65, y * 0.65), (x, y));
            }
        }
        Icon::Shuffle => {
            line((-0.85, -0.6), (0.85, 0.6));
            line((-0.85, 0.6), (0.85, -0.6));
            line((0.3, -0.7), (0.85, -0.6));
            line((0.85, -0.6), (0.8, -0.1));
            line((0.3, 0.7), (0.85, 0.6));
        }
        Icon::Repeat => {
            line((-0.8, -0.5), (0.8, -0.5));
            line((0.8, -0.5), (0.4, -0.85));
            line((0.8, -0.5), (0.4, -0.15));
            line((0.8, 0.5), (-0.8, 0.5));
            line((-0.8, 0.5), (-0.4, 0.85));
            line((-0.8, 0.5), (-0.4, 0.15));
        }
    }
}
