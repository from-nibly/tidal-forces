use crate::{
    api::{Api, ContextPage, DeviceLogin, LibraryPage, LoginPoll, Search},
    audio::Player,
    library::{Favorite, FavoriteKind},
    model::{Home, LibraryEntry, Playlist, PlaylistPage, RadioSeed, Track},
    queue::Continuation,
};
use std::{
    sync::mpsc,
    time::{Duration, Instant},
};
use tokio::sync::mpsc as async_mpsc;

pub enum Request {
    BeginPkce,
    FinishPkce(String),
    CancelPkce,
    Login,
    Logout,
    MigrateCredentials {
        user: u64,
    },
    RetryCredentialSave,
    ReloadCredentials,
    CleanupCredentials,
    Search {
        generation: u64,
        query: String,
    },
    Library {
        generation: u64,
        kind: FavoriteKind,
        offset: usize,
    },
    SetFavorite {
        user: u64,
        item: Favorite,
        saved: bool,
    },
    Home {
        generation: u64,
    },
    Folder {
        id: String,
    },
    Mix {
        generation: u64,
        id: String,
    },
    Radio {
        generation: u64,
        seed: RadioSeed,
    },
    Collection {
        generation: u64,
        kind: String,
        id: String,
        offset: usize,
    },
    Playlist {
        generation: u64,
        id: String,
        offset: usize,
        etag: Option<String>,
    },
    Track {
        generation: u64,
        id: u64,
    },
    Artist {
        generation: u64,
        id: u64,
    },
    OwnedPlaylists,
    CreateQueuePlaylist {
        user: u64,
        operation: u64,
        title: String,
        description: String,
        cancelled: std::sync::Arc<std::sync::atomic::AtomicBool>,
    },
    CheckPlaylistDuplicates {
        user: u64,
        operation: u64,
        id: String,
        offset: usize,
        count: usize,
        etag: String,
        cancelled: std::sync::Arc<std::sync::atomic::AtomicBool>,
    },
    AppendQueueBatch {
        user: u64,
        operation: u64,
        id: String,
        tracks: Vec<u64>,
        offset: usize,
        etag: Option<String>,
        cancelled: std::sync::Arc<std::sync::atomic::AtomicBool>,
    },
    CreatePlaylist {
        title: String,
        description: String,
    },
    AddToPlaylist {
        id: String,
        track: u64,
    },
    RemoveFromPlaylist {
        id: String,
        index: usize,
        track: u64,
        etag: String,
    },
    ContextPage {
        user: u64,
        generation: u64,
        source: Continuation,
    },
    Play {
        generation: u64,
        id: u64,
        quality: String,
        position: u64,
    },
}

pub enum Event {
    Credentials {
        status: Result<crate::credentials::Status, String>,
        warning: Option<String>,
        dirty: bool,
        finished: bool,
    },
    ContextPage {
        generation: u64,
        result: Result<ContextPage, String>,
    },
    Playlist {
        generation: u64,
        page: PlaylistPage,
        append: bool,
    },
    CollectionTitle {
        generation: u64,
        title: String,
    },
    OwnedPlaylists(Vec<Playlist>),
    QueuePlaylistCreated {
        user: u64,
        operation: u64,
        result: Result<Playlist, String>,
    },
    PlaylistDuplicatesChecked {
        user: u64,
        operation: u64,
        offset: usize,
        result: Result<Vec<u64>, String>,
    },
    QueueBatchSaved {
        user: u64,
        operation: u64,
        offset: usize,
        result: Result<String, String>,
    },
    PlaylistCreated(Playlist),
    PlaylistEdited {
        id: String,
        message: String,
    },
    PlaylistError(String),
    PkceReady(String),
    AuthKind(bool),
    Session(Option<(u64, String)>),
    Library {
        generation: u64,
        kind: FavoriteKind,
        page: LibraryPage,
        append: bool,
    },
    Favorite {
        user: u64,
        item: Favorite,
        result: Result<bool, String>,
    },
    Login {
        url: String,
        code: String,
    },
    Search {
        generation: u64,
        data: Search,
    },
    Tracks {
        generation: u64,
        tracks: Vec<Track>,
        append: bool,
    },
    Home {
        generation: u64,
        home: Home,
    },
    Folder {
        id: String,
        entries: Vec<LibraryEntry>,
    },
    FolderError {
        id: String,
        message: String,
    },
    Radio {
        generation: u64,
        tracks: Vec<Track>,
    },
    Playing {
        generation: u64,
        quality: String,
    },
    Listening {
        generation: u64,
        rendered: std::time::Duration,
        elapsed: std::time::Duration,
    },
    Position {
        generation: u64,
        seconds: u64,
    },
    Ended(u64),
    PlaybackError {
        generation: u64,
        message: String,
    },
    RequestError {
        generation: u64,
        message: String,
    },
    AudioError(String),
    Error(String),
}

#[derive(Clone)]
pub struct Events {
    tx: mpsc::Sender<Event>,
    ctx: eframe::egui::Context,
}
impl Events {
    pub fn send(&self, event: Event) {
        let _ = self.tx.send(event);
        self.ctx.request_repaint();
    }
}

pub struct Backend {
    pub tx: async_mpsc::Sender<Request>,
    pub rx: mpsc::Receiver<Event>,
    pub player: Player,
}

impl Backend {
    pub fn new(ctx: eframe::egui::Context) -> Self {
        let (tx, mut requests) = async_mpsc::channel(32);
        let (event_tx, rx) = mpsc::channel();
        let events = Events { tx: event_tx, ctx };
        let player = Player::new(events.clone());
        let audio = player.clone();
        std::thread::spawn(move || {
            let runtime = match tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
            {
                Ok(r) => r,
                Err(e) => {
                    events.send(Event::Error(e.to_string()));
                    return;
                }
            };
            runtime.block_on(async move {
                let mut api = match Api::new() {
                    Ok(a) => a,
                    Err(e) => { events.send(Event::Error(e.to_string())); return; }
                };
                match api.credentials.load().await {
                    Ok(session) => api.session = session,
                    Err(e) => api.credential_warning = Some(e.to_string()),
                }
                if api.session.is_some() {
                    match api.identify().await {
                        Ok(()) => {
                            events.send(Event::AuthKind(api.session.as_ref().is_some_and(|s| s.pkce)));
                            events.send(Event::Session(api.session.as_ref().map(|s| (s.user_id, s.country.clone()))));
                        },
                        Err(e) => events.send(Event::Error(e.to_string())),
                    }
                } else { events.send(Event::Session(None)); }
                credential_event(&api, &events, false);
                let mut pkce = None;
                let mut login: Option<(DeviceLogin, Instant, Instant)> = None;
                let mut ticker = tokio::time::interval(Duration::from_secs(1));
                loop {
                    tokio::select! {
                        request = requests.recv() => {
                            let Some(request) = request else { break; };
                            let request_generation = match &request {
                                Request::Search { generation, .. } | Request::Library { generation, .. } |
                                Request::Collection { generation, .. } | Request::Home { generation } |
                                Request::Mix { generation, .. } | Request::Radio { generation, .. } |
                                Request::Playlist { generation, .. } | Request::Track { generation, .. } | Request::Artist { generation, .. } => Some(*generation),
                                _ => None,
                            };
                            let credential_request = matches!(&request, Request::MigrateCredentials { .. } | Request::RetryCredentialSave | Request::ReloadCredentials | Request::CleanupCredentials | Request::Logout);
                            let playlist_edit = matches!(&request, Request::OwnedPlaylists | Request::CreatePlaylist { .. } | Request::AddToPlaylist { .. } | Request::RemoveFromPlaylist { .. });
                            let folder_id = match &request { Request::Folder { id } => Some(id.clone()), _ => None };
                            let playback_generation = match &request { Request::Play { generation, .. } => Some(*generation), _ => None };
                            let result: anyhow::Result<()> = async {
                                match request {
                                    Request::BeginPkce => {
                                        let p = crate::auth::Pkce::new();
                                        events.send(Event::PkceReady(p.url()));
                                        pkce = Some(p);
                                    }
                                    Request::CancelPkce => pkce = None,
                                    Request::FinishPkce(redirect) => {
                                        let p = pkce.as_ref().ok_or_else(|| anyhow::anyhow!("Start lossless sign-in first"))?;
                                        api.finish_pkce(p, &redirect).await?;
                                        pkce = None;
                                        events.send(Event::AuthKind(true));
                                        events.send(Event::Session(api.session.as_ref().map(|s| (s.user_id, s.country.clone()))));
                                    }
                                    Request::Login => {
                                        let device = api.begin_login().await?;
                                        let url = login_url(&device.verification_uri_complete)?;
                                        events.send(Event::Login { url, code: device.user_code.clone() });
                                        let next = Instant::now() + Duration::from_secs(device.interval.max(1));
                                        login = Some((device, Instant::now(), next));
                                    }
                                    Request::Logout => {
                                        login = None;
                                        pkce = None;
                                        audio.stop();
                                        api.session = None;
                                        api.credentials_dirty = false;
                                        api.credential_warning = api.credentials.clear().await.err().map(|error| format!("Saved credential removal could not be completed: {error}. Retry removal; a failed settings write may leave automatic sign-in enabled."));
                                        events.send(Event::Session(None));
                                    }
                                    Request::MigrateCredentials { user } => {
                                        let session = api.session.as_ref().ok_or_else(|| anyhow::anyhow!("Sign in before migrating credentials"))?;
                                        anyhow::ensure!(session.user_id == user && user != 0, "Account changed; credential migration was not started");
                                        api.credential_warning = api.credentials.migrate(session).await.err().map(|error| format!("Credential migration needs attention: {error}. Previous credential copies were retained unless a verified replacement was committed."));
                                        if api.credential_warning.is_none() { api.credentials_dirty = false; }
                                    }
                                    Request::RetryCredentialSave => api.persist_session().await,
                                    Request::ReloadCredentials => {
                                        if api.session.is_none() { api.session = api.credentials.load().await?; api.credential_warning = None; }
                                        if api.session.is_some() {
                                            api.identify().await?;
                                            events.send(Event::AuthKind(api.session.as_ref().is_some_and(|s| s.pkce)));
                                            events.send(Event::Session(api.session.as_ref().map(|s| (s.user_id, s.country.clone()))));
                                        }
                                    }
                                    Request::CleanupCredentials => {
                                        api.credential_warning = api.credentials.cleanup().await.err().map(|error| format!("Credential cleanup needs attention: {error}"));
                                    }
                                    Request::Search { generation, query } => {
                                        let data = api.search(&query).await?;
                                        events.send(Event::Search { generation, data });
                                    }
                                    Request::Library { generation, kind, offset } => {
                                        let page = api.library(kind, offset).await?;
                                        events.send(Event::Library { generation, kind, page, append: offset > 0 });
                                    }
                                    Request::SetFavorite { user, item, saved } => {
                                        let result = api.set_favorite(user, item, saved).await.map(|()| saved).map_err(|e| e.to_string());
                                        events.send(Event::Favorite { user, item, result });
                                    }
                                    Request::Home { generation } => events.send(Event::Home { generation, home: api.home().await? }),
                                    Request::Folder { id } => {
                                        let entries = api.folder(&id).await?;
                                        events.send(Event::Folder { id, entries });
                                    }
                                    Request::Mix { generation, id } => {
                                        let tracks = api.mix_tracks(&id).await?;
                                        events.send(Event::Tracks { generation, tracks, append: false });
                                    }
                                    Request::Radio { generation, seed } => {
                                        let tracks = api.radio(&seed).await?;
                                        events.send(Event::Radio { generation, tracks });
                                    }
                                    Request::Playlist { generation, id, offset, etag } => {
                                        let page = api.playlist_page(&id, offset, etag.as_deref()).await?;
                                        events.send(Event::Playlist { generation, page, append: offset > 0 });
                                    }
                                    Request::Track { generation, id } => events.send(Event::Tracks { generation, tracks: vec![api.track(id).await?], append: false }),
                                    Request::Artist { generation, id } => events.send(Event::Search { generation, data: api.artist(id).await? }),
                                    Request::OwnedPlaylists => events.send(Event::OwnedPlaylists(api.owned_playlists().await?)),
                                    Request::CreateQueuePlaylist { user, operation, title, description, cancelled } => {
                                        let result = api.create_queue_playlist(user, &title, &description, &cancelled).await.map_err(|error| error.to_string());
                                        events.send(Event::QueuePlaylistCreated { user, operation, result });
                                    }
                                    Request::CheckPlaylistDuplicates { user, operation, id, offset, count, etag, cancelled } => {
                                        let result = api.playlist_duplicate_page(user, &id, offset, count, &etag, &cancelled).await.map_err(|e| e.to_string());
                                        events.send(Event::PlaylistDuplicatesChecked { user, operation, offset, result });
                                    }
                                    Request::AppendQueueBatch { user, operation, id, tracks, offset, etag, cancelled } => {
                                        let result = api.append_queue_batch(user, &id, &tracks, offset, etag.as_deref(), &cancelled).await.map_err(|error| error.to_string());
                                        events.send(Event::QueueBatchSaved { user, operation, offset, result });
                                    }
                                    Request::CreatePlaylist { title, description } => events.send(Event::PlaylistCreated(api.create_playlist(&title, &description).await?)),
                                    Request::AddToPlaylist { id, track } => {
                                        let added = api.add_to_playlist(&id, track).await?;
                                        events.send(Event::PlaylistEdited { id, message: if added { "Track added to playlist." } else { "That track is already in the playlist." }.into() });
                                    }
                                    Request::RemoveFromPlaylist { id, index, track, etag } => {
                                        api.remove_from_playlist(&id, index, track, &etag).await?;
                                        events.send(Event::PlaylistEdited { id, message: "Track removed from playlist.".into() });
                                    }
                                    Request::Collection { generation, kind, id, offset } => {
                                        if offset == 0 { events.send(Event::CollectionTitle { generation, title: api.collection_title(&kind, &id).await? }); }
                                        let tracks = api.collection(&kind, &id, offset).await?;
                                        events.send(Event::Tracks { generation, tracks, append: offset > 0 });
                                    }
                                    Request::ContextPage { user, generation, source } => {
                                        let result = api.context_page(user, &source).await.map_err(|e| e.to_string());
                                        events.send(Event::ContextPage { generation, result });
                                    }
                                    Request::Play { generation, id, quality, position } => {
                                        if !audio.current(generation) { return Ok(()); }
                                        let stream = api.stream(id, &quality).await?;
                                        let audio = audio.clone();
                                        let events = events.clone();
                                        tokio::spawn(async move {
                                            if let Err(e) = audio.load(generation, stream, position).await {
                                                events.send(Event::PlaybackError { generation, message: e.to_string() });
                                            }
                                        });
                                    }
                                }
                                Ok(())
                            }.await;
                            if let Err(e) = result {
                                if credential_request { api.credential_warning = Some(e.to_string()); }
                                events.send(if playlist_edit { Event::PlaylistError(e.to_string()) } else if let Some(id) = folder_id {
                                    Event::FolderError { id, message: e.to_string() }
                                } else if let Some(generation) = request_generation {
                                    Event::RequestError { generation, message: e.to_string() }
                                } else if let Some(generation) = playback_generation {
                                    Event::PlaybackError { generation, message: e.to_string() }
                                } else { Event::Error(e.to_string()) });
                            }
                            credential_event(&api, &events, credential_request);
                        }
                        _ = ticker.tick(), if login.is_some() => {
                            let (device, start, next) = login.as_mut().unwrap();
                            if start.elapsed().as_secs() >= device.expires_in {
                                login = None;
                                events.send(Event::Error("Sign-in expired. Please try again.".into()));
                                continue;
                            }
                            if Instant::now() < *next { continue; }
                            match api.poll_login(&device.device_code).await {
                                Ok(LoginPoll::Complete) => {
                                    login = None;
                                    events.send(Event::AuthKind(false));
                                    events.send(Event::Session(api.session.as_ref().map(|s| (s.user_id, s.country.clone()))));
                                }
                                Ok(LoginPoll::Pending) => *next = Instant::now() + Duration::from_secs(device.interval.max(1)),
                                Ok(LoginPoll::SlowDown) => {
                                    device.interval += 5;
                                    *next = Instant::now() + Duration::from_secs(device.interval);
                                }
                                Err(e) => { login = None; events.send(Event::Error(e.to_string())); }
                            }
                            credential_event(&api, &events, false);
                        }
                    }
                }
            });
        });
        Self { tx, rx, player }
    }
}

fn credential_event(api: &Api, events: &Events, finished: bool) {
    events.send(Event::Credentials {
        status: api.credentials.status().map_err(|error| error.to_string()),
        warning: api.credential_warning.clone(),
        dirty: api.credentials_dirty,
        finished,
    });
}

fn login_url(value: &str) -> anyhow::Result<String> {
    let value = if value.starts_with("https://") {
        value.to_owned()
    } else {
        format!("https://{value}")
    };
    let url = reqwest::Url::parse(&value)?;
    anyhow::ensure!(
        url.scheme() == "https"
            && url
                .host_str()
                .is_some_and(|h| h == "tidal.com" || h.ends_with(".tidal.com")),
        "Untrusted sign-in URL"
    );
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_opens_tidal_login() {
        assert!(login_url("link.tidal.com/abc").is_ok());
        assert!(login_url("https://login.tidal.com/abc").is_ok());
        assert!(login_url("https://tidal.com.evil.test/abc").is_err());
        assert!(login_url("file:///etc/passwd").is_err());
    }
}
