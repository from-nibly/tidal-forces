use crate::{
    backend::{Backend, Event, Request},
    desktop::{DesktopControls, MediaControlEvent},
    model::{Album, Artist, LibraryEntry, Mix, RadioSeed, Track, cover_url, time},
    queue::Queue,
};
use eframe::egui::{self, Color32, RichText, Stroke, Vec2, pos2, vec2};
use std::collections::{HashMap, HashSet};

const BG: Color32 = Color32::from_rgb(12, 13, 15);
const PANEL: Color32 = Color32::from_rgb(18, 19, 22);
const CARD: Color32 = Color32::from_rgb(28, 30, 34);
const MUTED: Color32 = Color32::from_rgb(151, 155, 165);
const ACCENT: Color32 = Color32::from_rgb(81, 225, 219);

#[derive(Clone, PartialEq)]
enum Page {
    Home,
    Search,
    Favorites,
    Mix {
        id: String,
        title: String,
    },
    Radio(RadioSeed),
    Collection {
        kind: String,
        id: String,
        title: String,
    },
}

pub struct App {
    backend: Backend,
    screenshot: Option<std::path::PathBuf>,
    connected: bool,
    country: String,
    login: Option<(String, String)>,
    signing_in: bool,
    pkce_url: Option<String>,
    pkce_redirect: String,
    pkce_wait: bool,
    auth_pkce: bool,
    page: Page,
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
    play_generation: u64,
    buffering: bool,
    paused: bool,
    ended: bool,
    position: u64,
    volume: f32,
    quality: String,
    actual_quality: String,
    settings: bool,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, screenshot: Option<std::path::PathBuf>) -> Self {
        let mut visuals = egui::Visuals::dark();
        visuals.panel_fill = BG;
        visuals.window_fill = PANEL;
        visuals.extreme_bg_color = PANEL;
        visuals.faint_bg_color = CARD;
        visuals.selection.bg_fill = Color32::from_rgb(25, 67, 67);
        visuals.selection.stroke = Stroke::new(1.0_f32, ACCENT);
        visuals.widgets.inactive.bg_fill = CARD;
        visuals.widgets.hovered.bg_fill = Color32::from_rgb(44, 47, 53);
        visuals.widgets.active.bg_fill = Color32::from_rgb(40, 70, 70);
        visuals.widgets.noninteractive.fg_stroke.color = MUTED;
        visuals.override_text_color = Some(Color32::from_rgb(241, 242, 245));
        cc.egui_ctx.set_visuals(visuals);
        let mut fonts = egui::FontDefinitions::default();
        fonts.font_data.insert(
            "Inter".into(),
            egui::FontData::from_static(include_bytes!("../assets/fonts/Inter-Regular.ttf")).into(),
        );
        fonts.font_data.insert(
            "Inter Bold".into(),
            egui::FontData::from_static(include_bytes!("../assets/fonts/Inter-Bold.ttf")).into(),
        );
        fonts
            .families
            .get_mut(&egui::FontFamily::Proportional)
            .unwrap()
            .insert(0, "Inter".into());
        fonts.families.insert(
            egui::FontFamily::Name("heading".into()),
            vec!["Inter Bold".into()],
        );
        cc.egui_ctx.set_fonts(fonts);
        cc.egui_ctx.style_mut(|s| {
            s.spacing.item_spacing = vec2(12., 10.);
            s.spacing.button_padding = vec2(14., 9.);
            s.text_styles
                .insert(egui::TextStyle::Body, egui::FontId::proportional(15.));
            s.text_styles
                .insert(egui::TextStyle::Button, egui::FontId::proportional(14.));
            s.text_styles.insert(
                egui::TextStyle::Heading,
                egui::FontId::new(28., egui::FontFamily::Name("heading".into())),
            );
        });
        egui_extras::install_image_loaders(&cc.egui_ctx);
        let media_result = DesktopControls::new(cc.egui_ctx.clone());
        let media_error = media_result
            .as_ref()
            .err()
            .map(|e| format!("Desktop media controls unavailable: {e}"));
        Self {
            backend: Backend::new(cc.egui_ctx.clone()),
            screenshot,
            connected: false,
            country: String::new(),
            login: None,
            signing_in: false,
            pkce_url: None,
            pkce_redirect: String::new(),
            pkce_wait: false,
            auth_pkce: false,
            page: Page::Home,
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
            media: media_result.ok(),
            generation: 0,
            loading: false,
            more: false,
            error: media_error,
            audio_available: true,
            queue: Queue::default(),
            queue_open: false,
            play_generation: 0,
            buffering: false,
            paused: true,
            ended: false,
            position: 0,
            volume: 0.65,
            quality: "LOSSLESS".into(),
            actual_quality: String::new(),
            settings: false,
        }
    }

    fn send(&mut self, r: Request) {
        if self.backend.tx.try_send(r).is_err() {
            self.loading = false;
            self.buffering = false;
            self.signing_in = false;
            self.error = Some("The background worker is busy or unavailable. Please retry.".into());
        }
    }

    fn navigate(&mut self, page: Page) {
        self.page = page;
        self.generation += 1;
        self.tracks.clear();
        self.albums.clear();
        self.artists.clear();
        self.more = false;
        if !self.connected {
            return;
        }
        if self.page == Page::Search {
            self.loading = false;
            self.focus_search = true;
            if !self.query.trim().is_empty() {
                self.search();
            }
        } else {
            self.load_page(0);
        }
    }

    fn search(&mut self) {
        if self.query.trim().is_empty() {
            return;
        }
        self.page = Page::Search;
        self.generation += 1;
        self.loading = true;
        self.error = None;
        self.more = false;
        self.send(Request::Search {
            generation: self.generation,
            query: self.query.trim().to_owned(),
        });
    }

    fn load_page(&mut self, offset: usize) {
        self.loading = true;
        match &self.page {
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
            Page::Favorites => self.send(Request::Favorites {
                generation: self.generation,
                offset,
            }),
            Page::Collection { kind, id, .. } => self.send(Request::Collection {
                generation: self.generation,
                kind: kind.clone(),
                id: id.clone(),
                offset,
            }),
            Page::Search => self.search(),
        }
    }

    fn play_current(&mut self) {
        let Some(track) = self.queue.current() else {
            return;
        };
        let id = track.id;
        self.play_generation = self.backend.player.reserve();
        self.position = 0;
        self.paused = false;
        self.buffering = true;
        self.ended = false;
        self.actual_quality.clear();
        self.error = None;
        self.send(Request::Play {
            generation: self.play_generation,
            id,
            quality: self.quality.clone(),
        });
    }

    fn play_track(&mut self, index: usize) {
        self.queue.tracks = self.tracks.clone();
        self.queue.index = index;
        self.play_current();
    }

    fn toggle(&mut self) {
        if self.queue.current().is_none() {
            if !self.tracks.is_empty() {
                self.play_track(0);
            }
            return;
        }
        if self.buffering {
            self.paused = !self.paused;
            return;
        }
        if self.ended || self.actual_quality.is_empty() {
            self.play_current();
            return;
        }
        self.paused = !self.paused;
        self.backend.player.pause(self.paused);
    }

    fn next(&mut self) {
        if self.queue.next() {
            self.play_current();
        }
    }

    fn previous(&mut self) {
        if self.position > 3 {
            self.backend.player.seek(0);
            self.position = 0;
        } else if self.queue.previous() {
            self.play_current();
        }
    }

    fn logout(&mut self) {
        self.play_generation = self.backend.player.reserve();
        self.generation += 1;
        self.queue = Queue::default();
        self.paused = true;
        self.buffering = false;
        self.loading = false;
        self.connected = false;
        self.login = None;
        self.send(Request::Logout);
    }

    fn events(&mut self) {
        while let Ok(event) = self.backend.rx.try_recv() {
            match event {
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
                    self.quality = if pkce { "LOSSLESS" } else { "HIGH" }.into();
                }
                Event::Session(country) => {
                    self.pkce_url = None;
                    self.pkce_redirect.clear();
                    self.pkce_wait = false;
                    self.connected = country.is_some();
                    self.country = country.unwrap_or_default();
                    self.login = None;
                    self.signing_in = false;
                    self.folders.clear();
                    self.expanded.clear();
                    self.folder_pending.clear();
                    if self.connected {
                        self.request_folder("root");
                        self.navigate(Page::Home);
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
                    self.more = tracks.len() == 100
                        && matches!(self.page, Page::Favorites | Page::Collection { .. });
                    if append {
                        self.tracks.extend(tracks);
                    } else {
                        self.tracks = tracks;
                    }
                    self.loading = false;
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
                    self.play_track(0);
                }
                Event::Playing {
                    generation,
                    quality,
                } if generation == self.play_generation => {
                    self.buffering = false;
                    self.backend.player.pause(self.paused);
                    self.actual_quality = quality;
                }
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
                    } else if self.queue.next() {
                        self.play_current();
                    } else {
                        self.paused = true;
                        self.ended = true;
                    }
                }
                Event::PlaybackError {
                    generation,
                    message,
                } if generation == self.play_generation => {
                    self.buffering = false;
                    self.paused = true;
                    self.ended = true;
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
                MediaControlEvent::Stop => {
                    self.play_generation = self.backend.player.reserve();
                    self.queue = Queue::default();
                    self.paused = true;
                    self.buffering = false;
                    self.position = 0;
                }
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
                MediaControlEvent::Raise => ctx.send_viewport_cmd(egui::ViewportCommand::Focus),
                MediaControlEvent::Quit => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
                _ => {}
            }
        }
    }

    fn seek(&mut self, seconds: u64) {
        if self.buffering || self.ended {
            return;
        }
        if let Some(t) = self.queue.current() {
            self.position = seconds.min(t.duration.saturating_sub(1));
            self.backend.player.seek(self.position);
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
                    if folder_button(ui, &name, expanded)
                        .on_hover_text(format!("{count} items"))
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
                        ui.indent(&id, |ui| self.folder_tree(ui, &id, depth + 1));
                    }
                }
                LibraryEntry::Playlist(p) => {
                    let active = matches!(&self.page, Page::Collection { id, .. } if *id == p.uuid);
                    if ui
                        .add(
                            egui::Button::new(RichText::new(&p.title).color(if active {
                                ACCENT
                            } else {
                                MUTED
                            }))
                            .frame(active)
                            .min_size(vec2(ui.available_width(), 32.)),
                        )
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
        egui::SidePanel::left("sidebar")
            .exact_width(216.)
            .resizable(false)
            .frame(egui::Frame::new().fill(PANEL).inner_margin(20))
            .show(ctx, |ui| {
                ui.add_space(12.);
                ui.horizontal(|ui| {
                    mark(ui, 25., ACCENT);
                    ui.label(RichText::new("TIDAL FORCES").size(16.).strong());
                });
                ui.add_space(5.);
                ui.label(RichText::new("MUSIC. NOTHING ELSE.").size(10.).color(MUTED));
                ui.add_space(36.);
                for (page, icon, label) in [
                    (Page::Home, Icon::Home, "Home"),
                    (Page::Search, Icon::Search, "Search"),
                    (Page::Favorites, Icon::Library, "My collection"),
                ] {
                    if nav(ui, icon, label, self.page == page).clicked() {
                        self.navigate(page);
                    }
                }
                ui.add_space(26.);
                ui.label(
                    RichText::new("YOUR PLAYLISTS")
                        .size(10.)
                        .color(MUTED)
                        .strong(),
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
                        RichText::new("NATIVE RUST  /  NO BROWSER ENGINE")
                            .size(9.)
                            .color(MUTED),
                    );
                    ui.add_space(10.);
                    if nav(ui, Icon::Settings, "Settings", self.settings).clicked() {
                        self.settings = !self.settings;
                    }
                    if self.connected {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("●").color(ACCENT));
                            ui.label(
                                RichText::new(format!("Connected · {}", self.country)).size(12.),
                            );
                        });
                    }
                });
            });
    }

    fn player_bar(&mut self, ctx: &egui::Context) {
        let mut radio = None;
        egui::TopBottomPanel::bottom("player")
            .exact_height(112.)
            .frame(
                egui::Frame::new()
                    .fill(PANEL)
                    .inner_margin(18)
                    .stroke(Stroke::new(1.0_f32, CARD)),
            )
            .show(ctx, |ui| {
                let width = ui.available_width();
                ui.horizontal(|ui| {
                    ui.allocate_ui_with_layout(
                        vec2(width * 0.29, 74.),
                        egui::Layout::left_to_right(egui::Align::Center),
                        |ui| {
                            ui.set_min_size(vec2(width * 0.29, 74.));
                            if let Some(track) = self.queue.current() {
                                artwork(ui, track.cover_url(80), 60.)
                                    .context_menu(|ui| radio_menu(ui, track, &mut radio));
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
                                        ui.label(
                                            RichText::new(self.actual_quality.replace('_', " "))
                                                .size(9.)
                                                .color(ACCENT),
                                        );
                                    }
                                });
                            } else {
                                artwork(ui, None, 60.);
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
                                self.audio_available && self.queue.current().is_some(),
                                |ui| {
                                    ui.horizontal(|ui| {
                                        ui.add_space((width * 0.42 - 222.).max(0.) / 2.);
                                        if icon_button(
                                            ui,
                                            Icon::Shuffle,
                                            "Shuffle upcoming tracks",
                                            false,
                                            30.,
                                        )
                                        .clicked()
                                        {
                                            self.queue.shuffle_remaining();
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
                                        if self.buffering {
                                            ui.add_sized([42., 42.], egui::Spinner::new());
                                        } else if icon_button(
                                            ui,
                                            if self.paused { Icon::Play } else { Icon::Pause },
                                            "Play / pause · Space",
                                            true,
                                            42.,
                                        )
                                        .clicked()
                                        {
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
                                        if icon_button(
                                            ui,
                                            Icon::Repeat,
                                            "Repeat queue",
                                            self.queue.repeat,
                                            30.,
                                        )
                                        .clicked()
                                        {
                                            self.queue.repeat = !self.queue.repeat;
                                        }
                                    });
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
                                                self.position = position as u64;
                                                self.backend.player.seek(self.position);
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
                                ui.menu_button("...", |ui| radio_menu(ui, track, &mut radio));
                            }
                            ui.spacing_mut().slider_width = 75.;
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
                        },
                    );
                });
            });
        if let Some(seed) = radio {
            self.navigate(Page::Radio(seed));
        }
    }

    fn queue_panel(&mut self, ctx: &egui::Context) {
        if !self.queue_open {
            return;
        }
        let mut radio = None;
        egui::SidePanel::right("queue")
            .exact_width(250.)
            .resizable(false)
            .frame(egui::Frame::new().fill(PANEL).inner_margin(20))
            .show(ctx, |ui| {
                ui.add_space(16.);
                ui.heading("Play queue");
                ui.label(
                    RichText::new(format!("{} tracks", self.queue.tracks.len()))
                        .color(MUTED)
                        .size(12.),
                );
                ui.add_space(20.);
                let mut selected = None;
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for (i, track) in self.queue.tracks.iter().enumerate() {
                        if i < self.queue.index {
                            continue;
                        }
                        let text = format!("{}\n{}", track.title, track.artist.name);
                        let response = ui.add(
                            egui::Button::new(RichText::new(text).size(13.).color(
                                if i == self.queue.index {
                                    ACCENT
                                } else {
                                    Color32::WHITE
                                },
                            ))
                            .frame(i == self.queue.index)
                            .min_size(vec2(ui.available_width(), 58.)),
                        );
                        response.context_menu(|ui| radio_menu(ui, track, &mut radio));
                        if response.clicked() {
                            selected = Some(i);
                        }
                    }
                    if self.queue.tracks.is_empty() {
                        ui.label(RichText::new("Play a track or add one with +.").color(MUTED));
                    }
                });
                if let Some(i) = selected {
                    self.queue.index = i;
                    self.play_current();
                }
            });
        if let Some(seed) = radio {
            self.navigate(Page::Radio(seed));
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

    fn content(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(if self.connected {
                    "YOUR DAILY ROTATION"
                } else {
                    "LISTEN ON YOUR TERMS"
                })
                .size(10.)
                .strong()
                .color(MUTED),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if self.connected && ui.small_button("Refresh").clicked() {
                    self.error = None;
                    self.load_page(0);
                    self.folders.clear();
                    self.folder_pending.clear();
                    self.expanded.clear();
                    self.request_folder("root");
                }
                ui.label(RichText::new("TIDAL FORCES").size(10.).color(ACCENT));
            });
        });
        ui.add_space(18.);
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
        if !self.connected {
            self.welcome(ui);
            return;
        }
        match &self.page {
            Page::Home => {
                ui.horizontal(|ui| {
                    if let Some(daily) = &self.daily {
                        artwork(ui, daily.cover_url(), 88.);
                    }
                    ui.vertical(|ui| {
                        ui.label(
                            RichText::new(
                                self.daily
                                    .as_ref()
                                    .map_or("Made for you", |m| m.title.as_str()),
                            )
                            .size(34.)
                            .family(egui::FontFamily::Name("heading".into())),
                        );
                        ui.label(
                            RichText::new(
                                "Fresh discoveries and your personal mixes, straight from TIDAL.",
                            )
                            .size(15.)
                            .color(MUTED),
                        );
                    });
                });
            }
            Page::Mix { title, .. } => {
                ui.label(RichText::new(title).size(34.).strong());
            }
            Page::Radio(seed) => {
                ui.label(RichText::new(seed.title()).size(32.).strong());
                ui.label(
                    RichText::new("Radio selected by TIDAL · Starts automatically").color(MUTED),
                );
            }
            Page::Favorites => {
                ui.label(RichText::new("My collection").size(36.).strong());
                ui.label(RichText::new("Your favorite tracks, all in one place.").color(MUTED));
            }
            Page::Search => {
                ui.label(RichText::new("Search").size(36.).strong());
            }
            Page::Collection { title, kind, .. } => {
                ui.label(
                    RichText::new(kind.trim_end_matches('s').to_uppercase())
                        .size(10.)
                        .color(ACCENT),
                );
                ui.label(RichText::new(title).size(32.).strong());
            }
        }
        ui.add_space(18.);
        ui.horizontal(|ui| {
            let response = ui.add_sized(
                [ui.available_width() - 95., 40.],
                egui::TextEdit::singleline(&mut self.query)
                    .hint_text("Search artists, tracks, albums")
                    .margin(vec2(14., 10.)),
            );
            if self.focus_search {
                response.request_focus();
                self.focus_search = false;
            }
            let submit = response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if ui
                .add_sized([82., 40.], egui::Button::new("Search"))
                .clicked()
                || submit
            {
                self.search();
            }
        });
        ui.add_space(22.);
        if self.loading {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Loading your music…");
            });
        }
        if !self.artists.is_empty() {
            ui.heading("Artists");
            let mut radio = None;
            egui::ScrollArea::horizontal()
                .id_salt("artists")
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        for artist in &self.artists {
                            ui.vertical(|ui| {
                                ui.set_width(140.);
                                artwork(ui, cover_url(artist.picture.as_deref(), 320), 120.);
                                ui.add(egui::Label::new(&artist.name).truncate());
                                if ui.button("Start artist radio").clicked() {
                                    radio = Some(RadioSeed::Artist {
                                        id: artist.id,
                                        name: artist.name.clone(),
                                    });
                                }
                            });
                        }
                    });
                });
            if let Some(seed) = radio {
                self.navigate(Page::Radio(seed));
            }
            ui.add_space(20.);
        }
        if !self.albums.is_empty() {
            ui.heading("Albums");
            ui.add_space(8.);
            let mut selected = None;
            egui::ScrollArea::horizontal()
                .id_salt("albums")
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        for album in &self.albums {
                            ui.vertical(|ui| {
                                ui.set_width(150.);
                                if artwork(ui, cover_url(album.cover.as_deref(), 320), 150.)
                                    .clicked()
                                {
                                    selected = Some(album.clone());
                                }
                                if ui
                                    .add(egui::Button::new(&album.title).frame(false).wrap())
                                    .clicked()
                                {
                                    selected = Some(album.clone());
                                }
                                ui.label(RichText::new(&album.artist.name).size(12.).color(MUTED));
                            });
                        }
                    });
                });
            if let Some(a) = selected {
                self.navigate(Page::Collection {
                    kind: "albums".into(),
                    id: a.id.to_string(),
                    title: a.title,
                });
            }
            ui.add_space(26.);
        }
        if self.page == Page::Home && !self.mixes.is_empty() {
            ui.heading("My mixes");
            ui.add_space(10.);
            let mut selected = None;
            egui::ScrollArea::horizontal()
                .id_salt("mix_cards")
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        for mix in self.mixes.iter().filter(|m| m.mix_type != "DISCOVERY_MIX") {
                            ui.vertical(|ui| {
                                ui.set_width(155.);
                                if artwork(ui, mix.cover_url(), 155.).clicked() {
                                    selected = Some(mix.clone());
                                }
                                if ui.add(egui::Button::new(&mix.title).frame(false)).clicked() {
                                    selected = Some(mix.clone());
                                }
                                ui.add(
                                    egui::Label::new(
                                        RichText::new(&mix.sub_title).color(MUTED).size(12.),
                                    )
                                    .truncate(),
                                );
                            });
                        }
                    });
                });
            if let Some(mix) = selected {
                self.navigate(Page::Mix {
                    id: mix.id,
                    title: mix.title,
                });
            }
            ui.add_space(26.);
        }
        ui.horizontal(|ui| {
            ui.heading(match self.page {
                Page::Home => "Today's discovery",
                Page::Favorites => "Favorite tracks",
                _ => "Tracks",
            });
            if !self.tracks.is_empty() {
                ui.label(
                    RichText::new(format!("{} loaded", self.tracks.len()))
                        .size(12.)
                        .color(MUTED),
                );
                if ui
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
            self.load_page(self.tracks.len());
        }
    }

    fn track_list(&mut self, ui: &mut egui::Ui) {
        let mut play = None;
        let mut add = None;
        let mut radio = None;
        for (i, t) in self.tracks.iter().enumerate() {
            let playing = self
                .queue
                .current()
                .is_some_and(|current| current.id == t.id);
            let (rect, response) =
                ui.allocate_exact_size(vec2(ui.available_width(), 62.), egui::Sense::click());
            if response.hovered() || playing {
                ui.painter().rect_filled(
                    rect,
                    4.,
                    if playing {
                        Color32::from_rgb(21, 37, 39)
                    } else {
                        CARD
                    },
                );
            }
            let mut row = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(rect.shrink2(vec2(10., 7.)))
                    .layout(egui::Layout::left_to_right(egui::Align::Center)),
            );
            row.label(
                RichText::new(format!("{:02}", i + 1))
                    .color(if playing { ACCENT } else { MUTED })
                    .size(11.),
            );
            artwork(&mut row, t.cover_url(80), 44.);
            row.allocate_ui_with_layout(
                vec2((rect.width() * 0.40).max(100.), 44.),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    ui.add(
                        egui::Label::new(RichText::new(&t.title).strong().color(if playing {
                            ACCENT
                        } else {
                            Color32::WHITE
                        }))
                        .truncate(),
                    );
                    ui.add(
                        egui::Label::new(RichText::new(&t.artist.name).size(12.).color(MUTED))
                            .truncate(),
                    );
                },
            );
            row.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.menu_button("...", |ui| radio_menu(ui, t, &mut radio));
                if ui.small_button("+").on_hover_text("Add to queue").clicked() {
                    add = Some(t.clone());
                }
                ui.label(RichText::new(time(t.duration)).size(12.).color(MUTED));
                if t.explicit {
                    ui.label(RichText::new("E").size(9.).color(MUTED));
                }
                if rect.width() > 570. {
                    ui.add(
                        egui::Label::new(RichText::new(&t.album.title).size(12.).color(MUTED))
                            .truncate(),
                    );
                }
            });
            if response.double_clicked() && self.audio_available {
                play = Some(i);
            }
            response.context_menu(|ui| radio_menu(ui, t, &mut radio));
            response.on_hover_text("Double-click to play · Right-click for radio · + to queue");
        }
        if let Some(i) = play {
            self.play_track(i);
        }
        if let Some(t) = add {
            self.queue.tracks.push(t);
            self.queue_open = true;
        }
        if let Some(seed) = radio {
            self.navigate(Page::Radio(seed));
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

    fn settings(&mut self, ctx: &egui::Context) {
        if !self.settings {
            return;
        }
        let mut open = true;
        egui::Window::new("Settings").open(&mut open).resizable(false).default_width(430.).show(ctx, |ui| {
            ui.heading("Listening quality");
            if self.auth_pkce {
                ui.label(RichText::new("Lossless-capable sign-in connected").color(ACCENT));
            } else {
                ui.label("This device session is limited to AAC. Reauthorize with TIDAL to enable lossless.");
            }
            if ui.button(if self.auth_pkce { "Reconnect lossless account" } else { "Enable lossless sign-in" }).clicked() {
                self.send(Request::BeginPkce);
            }
            ui.add_space(8.);
            ui.radio_value(&mut self.quality, "LOSSLESS".into(), "Lossless · FLAC");
            ui.add_enabled_ui(!self.auth_pkce, |ui| {
                ui.radio_value(&mut self.quality, "HIGH".into(), "Compatibility High · AAC");
                ui.radio_value(&mut self.quality, "LOW".into(), "Compatibility Low · AAC");
            });
            ui.label(RichText::new("Applies to the next track. The player shows the quality TIDAL actually returns; it never claims unverified hi-res or bit-perfect output.").size(12.).color(MUTED));
            ui.separator();
            ui.label("Shortcuts");
            ui.label(RichText::new("Media play/pause, next, previous  System-wide (MPRIS)\nSpace  Play / pause in this window\nCtrl+K  Search\nCtrl+Left / Right  Previous / next").size(13.).color(MUTED));
            ui.separator();
            ui.label(RichText::new("Unofficial TIDAL client. Lossless sign-in streams unencrypted DASH FLAC. Compatibility sign-in supports BTS audio. If TIDAL only offers lossy audio in lossless mode, playback fails explicitly. Encrypted audio is not supported.").size(12.).color(MUTED));
            ui.label(RichText::new("Session tokens are stored locally in a private (0600) file. Signing out removes them. No passwords, telemetry, or third-party music proxies.").size(12.).color(MUTED));
            if self.connected && ui.button("Sign out and remove saved session").clicked() {
                self.logout(); self.settings = false;
            }
            ui.label(RichText::new(format!("Tidal Forces {}", env!("CARGO_PKG_VERSION"))).size(11.).color(MUTED));
        });
        if !open {
            self.settings = false;
        }
    }
}

impl eframe::App for App {
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
        self.events();
        self.desktop_events(ctx);
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, egui::Key::K)) {
            self.navigate(Page::Search);
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
            .frame(egui::Frame::new().fill(BG).inner_margin(30))
            .show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("content")
                    .show(ui, |ui| {
                        self.content(ui);
                    });
            });
        self.settings(ctx);
        self.lossless_sign_in(ctx);
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

fn folder_button(ui: &mut egui::Ui, name: &str, expanded: bool) -> egui::Response {
    let (r, response) =
        ui.allocate_exact_size(vec2(ui.available_width(), 30.), egui::Sense::click());
    if response.hovered() {
        ui.painter().rect_filled(r, 4., CARD);
    }
    let c = pos2(r.left() + 7., r.center().y);
    let points = if expanded {
        vec![c + vec2(-4., -2.), c + vec2(4., -2.), c + vec2(0., 3.)]
    } else {
        vec![c + vec2(-2., -4.), c + vec2(-2., 4.), c + vec2(3., 0.)]
    };
    ui.painter()
        .add(egui::Shape::convex_polygon(points, MUTED, Stroke::NONE));
    ui.painter().with_clip_rect(r).text(
        pos2(r.left() + 20., r.center().y),
        egui::Align2::LEFT_CENTER,
        name,
        egui::FontId::proportional(14.),
        Color32::WHITE,
    );
    response
        .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), name));
    response
}

fn radio_menu(ui: &mut egui::Ui, track: &Track, radio: &mut Option<RadioSeed>) {
    if ui.button("Start track radio").clicked() {
        *radio = Some(RadioSeed::Track {
            id: track.id,
            title: track.title.clone(),
        });
        ui.close();
    }
    if track.artist.id != 0
        && ui
            .button(format!("Start artist radio · {}", track.artist.name))
            .clicked()
    {
        *radio = Some(RadioSeed::Artist {
            id: track.artist.id,
            name: track.artist.name.clone(),
        });
        ui.close();
    }
}

fn artwork(ui: &mut egui::Ui, url: Option<String>, size: f32) -> egui::Response {
    if let Some(url) = url {
        ui.add(
            egui::Image::new(url)
                .fit_to_exact_size(vec2(size, size))
                .corner_radius(4)
                .sense(egui::Sense::click()),
        )
    } else {
        let (r, response) = ui.allocate_exact_size(vec2(size, size), egui::Sense::hover());
        ui.painter().rect_filled(r, 4., CARD);
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
}

fn nav(ui: &mut egui::Ui, icon: Icon, label: &str, selected: bool) -> egui::Response {
    let (r, response) =
        ui.allocate_exact_size(vec2(ui.available_width(), 44.), egui::Sense::click());
    if response.hovered() || selected {
        ui.painter().rect_filled(r, 5., CARD);
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
        if selected { Color32::WHITE } else { MUTED },
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
    response.on_hover_text(label)
}

fn paint_icon(p: &egui::Painter, icon: Icon, c: egui::Pos2, size: f32, color: Color32) {
    let s = size / 2.;
    let at = |x: f32, y: f32| c + vec2(x * s, y * s);
    let stroke = Stroke::new(1.6_f32, color);
    let line = |a: (f32, f32), b: (f32, f32)| {
        p.line_segment([at(a.0, a.1), at(b.0, b.1)], stroke);
    };
    match icon {
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
