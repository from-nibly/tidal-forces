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
    FavoriteMembership {
        user: u64,
        serial: u64,
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
        user: u64,
        serial: u64,
        id: String,
    },
    ManageFolder {
        user: u64,
        operation: u64,
        change: crate::folders::Change,
        cancelled: std::sync::Arc<std::sync::atomic::AtomicBool>,
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
    CheckReorder {
        operation: u64,
        check: crate::reorder::Check,
        cancelled: std::sync::Arc<std::sync::atomic::AtomicBool>,
    },
    ReorderPlaylist {
        operation: u64,
        change: crate::reorder::Move,
        cancelled: std::sync::Arc<std::sync::atomic::AtomicBool>,
    },
    UnfollowPlaylist {
        user: u64,
        operation: u64,
        id: String,
        etag: String,
        cancelled: std::sync::Arc<std::sync::atomic::AtomicBool>,
    },
    DeletePlaylist {
        user: u64,
        operation: u64,
        id: String,
        etag: String,
        cancelled: std::sync::Arc<std::sync::atomic::AtomicBool>,
    },
    EditPlaylistMetadata {
        user: u64,
        operation: u64,
        id: String,
        etag: String,
        title: String,
        description: String,
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
    PrepareNext(crate::audio::NextRequest),
    SeekBuffered {
        generation: u64,
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
    CollectionPage {
        generation: u64,
        kind: String,
        id: String,
        offset: usize,
        page: LibraryPage,
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
    ReorderChecked {
        operation: u64,
        check: crate::reorder::Check,
        result: Result<Vec<u64>, String>,
    },
    PlaylistReordered {
        user: u64,
        operation: u64,
        id: String,
        result: Result<String, String>,
    },
    PlaylistUnfollowed {
        user: u64,
        operation: u64,
        id: String,
        result: Result<crate::api::UnfollowVerification, String>,
    },
    PlaylistDeleted {
        user: u64,
        operation: u64,
        id: String,
        result: Result<(), String>,
    },
    PlaylistMetadataEdited {
        user: u64,
        operation: u64,
        id: String,
        result: Result<Playlist, String>,
    },
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
    FavoriteMembership {
        user: u64,
        serial: u64,
        kind: FavoriteKind,
        offset: usize,
        result: Result<LibraryPage, String>,
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
    FolderManaged {
        user: u64,
        operation: u64,
        result: Result<crate::folders::Outcome, String>,
    },
    Folder {
        user: u64,
        serial: u64,
        id: String,
        entries: Vec<LibraryEntry>,
    },
    FolderError {
        user: u64,
        serial: u64,
        id: String,
        message: String,
    },
    Radio {
        generation: u64,
        tracks: Vec<Track>,
    },
    Transition(u64),
    Playing {
        generation: u64,
        quality: String,
    },
    StreamBuffering {
        generation: u64,
        buffering: bool,
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
    OutputLost {
        epoch: u64,
        generation: u64,
        position: Option<u64>,
    },
    Error(String),
}

#[derive(Clone)]
pub struct Events {
    tx: mpsc::Sender<Event>,
    ctx: eframe::egui::Context,
}
impl Events {
    #[cfg(test)]
    pub(crate) fn fixture(tx: mpsc::Sender<Event>) -> Self {
        Self {
            tx,
            ctx: eframe::egui::Context::default(),
        }
    }
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
                audio.set_seek_runtime(events.clone());
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
                            let folder_id = match &request { Request::Folder { user,serial,id } => Some((*user,*serial,id.clone())), _ => None };
                            let playback_generation = match &request { Request::Play { generation, .. } | Request::SeekBuffered { generation, .. } => Some(*generation), _ => None };
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
                                    Request::FavoriteMembership { user, serial, kind, offset } => {
                                        let result = api.favorite_membership_page(user, kind, offset).await.map_err(|e| e.to_string());
                                        events.send(Event::FavoriteMembership { user, serial, kind, offset, result });
                                    }
                                    Request::SetFavorite { user, item, saved } => {
                                        let result = api.set_favorite(user, item, saved).await.map(|()| saved).map_err(|e| e.to_string());
                                        events.send(Event::Favorite { user, item, result });
                                    }
                                    Request::Home { generation } => events.send(Event::Home { generation, home: api.home().await? }),
                                    Request::Folder { user,serial,id } => {
                                        let entries = api.folder_for(user,&id).await?;
                                        events.send(Event::Folder { user,serial,id, entries });
                                    }
                                    Request::ManageFolder {user,operation,change,cancelled} => {
                                        let result=api.manage_folder(user,&change,&cancelled).await.map_err(|_|"Folder change could not be confirmed. Refresh and inspect before retrying.".into());
                                        events.send(Event::FolderManaged {user,operation,result});
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
                                    Request::CheckReorder {operation,check,cancelled} => {
                                        let result=api.check_reorder_page(&check,&cancelled).await.map_err(|e|e.to_string());
                                        events.send(Event::ReorderChecked {operation,check,result});
                                    }
                                    Request::ReorderPlaylist {operation,change,cancelled} => {
                                        let result=api.write_reorder(&change,&cancelled).await.map_err(|e|e.to_string());
                                        events.send(Event::PlaylistReordered {user:change.user,operation,id:change.id,result});
                                    }
                                    Request::UnfollowPlaylist { user, operation, id, etag, cancelled } => {
                                        let result = api.unfollow_playlist(user, &id, &etag, &cancelled).await.map_err(|e| e.to_string());
                                        events.send(Event::PlaylistUnfollowed { user, operation, id, result });
                                    }
                                    Request::DeletePlaylist { user, operation, id, etag, cancelled } => {
                                        let result = api.delete_playlist(user, &id, &etag, &cancelled).await.map_err(|e| e.to_string());
                                        events.send(Event::PlaylistDeleted { user, operation, id, result });
                                    }
                                    Request::EditPlaylistMetadata { user, operation, id, etag, title, description } => {
                                        let result=api.edit_playlist_metadata(user,&id,&etag,&title,&description).await.map_err(|e|e.to_string());
                                        events.send(Event::PlaylistMetadataEdited {user,operation,id,result});
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
                                        let page = api.collection(&kind, &id, offset).await?;
                                        events.send(Event::CollectionPage { generation, kind, id, offset, page });
                                    }
                                    Request::ContextPage { user, generation, source } => {
                                        let result = api.context_page(user, &source).await.map_err(|e| e.to_string());
                                        events.send(Event::ContextPage { generation, result });
                                    }
                                    Request::PrepareNext(request) => {
                                        if !audio.is_preparing(&request) || api.session.as_ref().map(|s| s.user_id) != Some(request.key.user) {
                                            audio.fail_next(&request);
                                            return Ok(());
                                        }
                                        match api.stream(request.key.track, &request.key.quality).await {
                                            Ok(stream) => {
                                                let audio = audio.clone();
                                                tokio::spawn(async move {
                                                    if audio.prepare_next(request.clone(), stream).await.is_err() { audio.fail_next(&request); }
                                                });
                                            }
                                            Err(_) => audio.fail_next(&request),
                                        }
                                    }
                                    Request::SeekBuffered { generation, position } => {
                                        let audio = audio.clone();
                                        let events = events.clone();
                                        tokio::spawn(async move {
                                            if let Err(e) = audio.seek_buffered(generation, position).await {
                                                events.send(Event::PlaybackError { generation, message: e.to_string() });
                                            }
                                        });
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
                                events.send(if playlist_edit { Event::PlaylistError(e.to_string()) } else if let Some((user,serial,id)) = folder_id {
                                    Event::FolderError { user,serial,id, message: "Could not load this folder. Refresh to retry.".into() }
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
    #[tokio::test]
    async fn buffered_seek_dispatch_does_not_wait_for_the_catalog_queue() {
        let player = Player::inert();
        player.seed_buffered_fixture();
        let (tx, rx) = mpsc::channel();
        player.set_seek_runtime(Events {
            tx,
            ctx: eframe::egui::Context::default(),
        });
        let (catalog, _blocked) = async_mpsc::channel(1);
        catalog.send(Request::Home { generation: 1 }).await.unwrap();
        let generation = player.reserve_seek(0).unwrap();
        assert!(player.dispatch_seek(generation, 1));
        let event =
            tokio::task::spawn_blocking(move || rx.recv_timeout(std::time::Duration::from_secs(3)))
                .await
                .unwrap()
                .unwrap();
        // The inert output is deliberately closed; receiving its error proves
        // the seek ran without draining the blocked catalog request.
        assert!(
            matches!(event, Event::PlaybackError { generation: id, message } if id == generation && message == "Audio output is unavailable")
        );
        assert_eq!(catalog.capacity(), 0);
        assert!(player.reserve_seek(generation).is_none());
    }

    #[test]
    fn only_opens_tidal_login() {
        assert!(login_url("link.tidal.com/abc").is_ok());
        assert!(login_url("https://login.tidal.com/abc").is_ok());
        assert!(login_url("https://tidal.com.evil.test/abc").is_err());
        assert!(login_url("file:///etc/passwd").is_err());
    }
}
