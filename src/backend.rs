use crate::{
    api::{Api, DeviceLogin, LoginPoll, Search},
    audio::Player,
    model::{Home, LibraryEntry, RadioSeed, Track},
    store,
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
    Search {
        generation: u64,
        query: String,
    },
    Favorites {
        generation: u64,
        offset: usize,
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
    Play {
        generation: u64,
        id: u64,
        quality: String,
    },
}

pub enum Event {
    PkceReady(String),
    AuthKind(bool),
    Session(Option<String>),
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
                match store::load() {
                    Ok(session) => api.session = session,
                    Err(e) => events.send(Event::Error(e.to_string())),
                }
                if api.session.is_some() {
                    match api.identify().await {
                        Ok(()) => {
                            events.send(Event::AuthKind(api.session.as_ref().is_some_and(|s| s.pkce)));
                            events.send(Event::Session(api.session.as_ref().map(|s| s.country.clone())));
                        },
                        Err(e) => events.send(Event::Error(e.to_string())),
                    }
                } else { events.send(Event::Session(None)); }
                let mut pkce = None;
                let mut login: Option<(DeviceLogin, Instant, Instant)> = None;
                let mut ticker = tokio::time::interval(Duration::from_secs(1));
                loop {
                    tokio::select! {
                        request = requests.recv() => {
                            let Some(request) = request else { break; };
                            let request_generation = match &request {
                                Request::Search { generation, .. } | Request::Favorites { generation, .. } |
                                Request::Collection { generation, .. } | Request::Home { generation } |
                                Request::Mix { generation, .. } | Request::Radio { generation, .. } => Some(*generation),
                                _ => None,
                            };
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
                                        events.send(Event::Session(api.session.as_ref().map(|s| s.country.clone())));
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
                                        store::clear()?;
                                        api.session = None;
                                        events.send(Event::Session(None));
                                    }
                                    Request::Search { generation, query } => {
                                        let data = api.search(&query).await?;
                                        events.send(Event::Search { generation, data });
                                    }
                                    Request::Favorites { generation, offset } => {
                                        let tracks = api.favorites(offset).await?;
                                        events.send(Event::Tracks { generation, tracks, append: offset > 0 });
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
                                    Request::Collection { generation, kind, id, offset } => {
                                        let tracks = api.collection(&kind, &id, offset).await?;
                                        events.send(Event::Tracks { generation, tracks, append: offset > 0 });
                                    }
                                    Request::Play { generation, id, quality } => {
                                        if !audio.current(generation) { return Ok(()); }
                                        let stream = api.stream(id, &quality).await?;
                                        let audio = audio.clone();
                                        let events = events.clone();
                                        tokio::spawn(async move {
                                            if let Err(e) = audio.load(generation, stream).await {
                                                events.send(Event::PlaybackError { generation, message: e.to_string() });
                                            }
                                        });
                                    }
                                }
                                Ok(())
                            }.await;
                            if let Err(e) = result {
                                events.send(if let Some(id) = folder_id {
                                    Event::FolderError { id, message: e.to_string() }
                                } else if let Some(generation) = request_generation {
                                    Event::RequestError { generation, message: e.to_string() }
                                } else if let Some(generation) = playback_generation {
                                    Event::PlaybackError { generation, message: e.to_string() }
                                } else { Event::Error(e.to_string()) });
                            }
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
                                    events.send(Event::Session(api.session.as_ref().map(|s| s.country.clone())));
                                }
                                Ok(LoginPoll::Pending) => *next = Instant::now() + Duration::from_secs(device.interval.max(1)),
                                Ok(LoginPoll::SlowDown) => {
                                    device.interval += 5;
                                    *next = Instant::now() + Duration::from_secs(device.interval);
                                }
                                Err(e) => { login = None; events.send(Event::Error(e.to_string())); }
                            }
                        }
                    }
                }
            });
        });
        Self { tx, rx, player }
    }
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
