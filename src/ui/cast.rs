use super::*;
#[cfg(test)]
#[path = "cast_tests.rs"]
mod tests;
use crate::cast_control::{Command, End, Event as CastEvent, Handle, Snapshot, State};
use std::{
    net::SocketAddrV4,
    sync::mpsc,
    time::{Duration, Instant},
};

#[derive(Clone, Copy)]
struct Start {
    position: Duration,
    autoplay: bool,
}
#[derive(Clone, Copy)]
struct Scope {
    user: u64,
    generation: u64,
    occurrence: u64,
}
struct Running {
    scope: Scope,
    handle: Handle,
}
struct Quiet {
    scope: Scope,
    request: Start,
    reply: mpsc::Receiver<bool>,
    deadline: Instant,
}
#[derive(Default)]
pub(super) struct Cast {
    repaint: Option<egui::Context>,
    picker: Option<egui::ViewportId>,
    picker_mini: bool,
    device_id: Option<String>,
    #[cfg(test)]
    pub(super) discovery_disabled: bool,
    device_name: Option<String>,
    devices: Vec<crate::cast_discovery::Device>,
    discovery: Option<crate::cast_discovery::Browser>,
    discovery_stats: crate::cast_discovery::Stats,
    discovery_error: Option<&'static str>,
    endpoint: Option<SocketAddrV4>,
    pub enabled: bool,
    unknown: bool,
    returning: bool,
    closing: bool,
    allow_close: bool,
    worker: Option<Running>,
    checking: Option<mpsc::Receiver<Result<bool, &'static str>>>,
    pending: Option<Start>,
    switching: Option<(crate::cast_discovery::Device, Start)>,
    quiet: Option<Quiet>,
    preparing: Option<Scope>,
    pub local_volume: Option<f32>,
    local_details: Option<String>,
    seek_serial: u64,
    pause_intent: Option<bool>,
    volume_intent: Option<f32>,
    phase: Option<crate::cast_control::Phase>,
    previous: Option<(Snapshot, Instant)>,
    credit: Duration,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ConnectionState {
    Local,
    Selected,
    Connecting,
    Connected,
    Releasing,
    Unconfirmed,
}
impl ConnectionState {
    fn label(self) -> &'static str {
        match self {
            Self::Local => "This computer",
            Self::Selected => "Selected",
            Self::Connecting => "Connecting…",
            Self::Connected => "Connected",
            Self::Releasing => "Stopping casting…",
            Self::Unconfirmed => "Check receiver",
        }
    }
}
impl Scope {
    fn matches(self, app: &App) -> bool {
        app.account == Some(self.user)
            && app.play_generation == self.generation
            && app
                .queue
                .current_entry()
                .is_some_and(|entry| entry.occurrence == self.occurrence)
    }
}
impl App {
    #[cfg(debug_assertions)]
    pub(super) fn cast_idle_fixture(
        &mut self,
        ctx: &egui::Context,
        populated: bool,
    ) -> mpsc::SyncSender<()> {
        let (browser, worker) = crate::cast_discovery::Browser::fixture();
        self.cast.discovery = Some(browser);
        self.cast.picker = Some(ctx.viewport_id());
        if populated {
            self.cast.devices = vec![crate::cast_discovery::Device {
                id: "12345678123412341234123456789abc".into(),
                name: "Synthetic receiver".into(),
                model: "Fixture".into(),
                endpoint: "192.168.99.1:8009".parse().unwrap(),
            }];
            self.cast.discovery_stats.found = 1;
        }
        worker
    }
    fn cast_state(&self) -> ConnectionState {
        if self.cast.unknown {
            return ConnectionState::Unconfirmed;
        }
        if !self.cast.enabled {
            return ConnectionState::Local;
        }
        if self.cast.returning
            || self.cast.switching.is_some()
            || matches!(self.cast.phase, Some(crate::cast_control::Phase::Releasing))
        {
            return ConnectionState::Releasing;
        }
        if self.cast.quiet.is_some() || self.cast.preparing.is_some() || self.cast.pending.is_some()
        {
            return ConnectionState::Connecting;
        }
        if self.cast.worker.is_some() {
            if matches!(
                self.cast.phase,
                Some(crate::cast_control::Phase::Monitoring)
            ) {
                ConnectionState::Connected
            } else {
                ConnectionState::Connecting
            }
        } else {
            ConnectionState::Selected
        }
    }
    pub(super) fn cast_picker_open(&self, ctx: &egui::Context) -> bool {
        self.cast.picker == Some(ctx.viewport_id())
    }
    pub(super) fn cast_mini_closed(&mut self) {
        if self.cast.picker_mini {
            self.cast.picker = None;
            self.cast.discovery.take();
        }
    }
    pub(super) fn cast_button(&mut self, ui: &mut egui::Ui, mini: bool) -> egui::Response {
        self.cast.repaint = Some(ui.ctx().clone());
        let state = self.cast_state();
        let (rect, response) = ui.allocate_exact_size(egui::vec2(30., 28.), egui::Sense::click());
        response.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::WidgetType::Button,
                ui.is_enabled(),
                tr("Playback devices"),
            )
        });
        let color = match state {
            ConnectionState::Local => ui.visuals().text_color(),
            ConnectionState::Unconfirmed => ui.visuals().error_fg_color,
            _ => theme_accent(),
        };
        let origin = rect.min + egui::vec2(5., 5.);
        let p = ui.painter();
        let screen = egui::Rect::from_min_size(origin, egui::vec2(21., 15.));
        if state == ConnectionState::Connected {
            p.rect_filled(screen, 2., color);
        } else {
            p.rect_stroke(
                screen,
                2.,
                egui::Stroke::new(1.5_f32, color),
                egui::StrokeKind::Inside,
            );
        }
        let animated = matches!(
            state,
            ConnectionState::Connecting | ConnectionState::Releasing
        );
        let waves = if animated {
            ui.ctx().request_repaint_after(Duration::from_millis(120));
            ui.input(|i| ((i.time * 3.) as usize) % 3 + 1)
        } else {
            3
        };
        // Clear the screen corner behind the conventional Cast waves.
        p.rect_filled(
            egui::Rect::from_min_size(origin + egui::vec2(-1., 5.), egui::vec2(11., 12.)),
            0.,
            ui.visuals().panel_fill,
        );
        for radius in [4., 8.].into_iter().take(waves.saturating_sub(1)) {
            let points = (0..=12)
                .map(|i| {
                    let angle = std::f32::consts::FRAC_PI_2 * i as f32 / 12.;
                    origin + egui::vec2(radius * angle.sin(), 15. - radius * angle.cos())
                })
                .collect();
            p.add(egui::Shape::line(points, egui::Stroke::new(1.5_f32, color)));
        }
        p.rect_filled(
            egui::Rect::from_min_size(origin + egui::vec2(0., 13.), egui::vec2(3., 3.)),
            0.,
            color,
        );
        if response.has_focus() {
            p.rect_stroke(
                rect,
                3.,
                egui::Stroke::new(1_f32, color),
                egui::StrokeKind::Inside,
            );
        }
        let response = response
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .on_hover_text(format!(
                "{}\n{} · {}",
                tr("Playback devices"),
                self.cast
                    .device_name
                    .as_deref()
                    .filter(|_| self.cast.enabled)
                    .unwrap_or(tr("This computer")),
                tr(state.label())
            ));
        if response.clicked() {
            let viewport = ui.ctx().viewport_id();
            if self.cast.picker == Some(viewport) {
                self.cast.picker = None;
                self.cast.discovery.take();
            } else {
                self.cast.picker = Some(viewport);
                self.cast.picker_mini = mini;
                self.cast_search(ui.ctx());
            }
        }
        response
    }
    pub(super) fn cast_picker(&mut self, ctx: &egui::Context) {
        if !self.cast_picker_open(ctx) {
            return;
        }
        let mut open = !ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
        if open {
            egui::Window::new(tr("Playback devices"))
                .id(egui::Id::new("cast-picker"))
                .open(&mut open)
                .collapsible(false)
                .resizable(false)
                .default_width(320.)
                .max_width((ctx.content_rect().width() - 32.).max(180.))
                .show(ctx, |ui| {
                    egui::ScrollArea::vertical()
                        .max_height((ctx.content_rect().height() - 110.).max(100.))
                        .show(ui, |ui| self.cast_controls(ui));
                });
        }
        if !open {
            self.cast.picker = None;
        }
        if self.cast.picker.is_none() {
            self.cast.discovery.take();
        }
    }
    fn cast_search(&mut self, ctx: &egui::Context) {
        #[cfg(test)]
        if self.cast.discovery_disabled {
            return;
        }
        if self.cast.discovery.is_some() {
            return;
        }
        self.cast.devices.clear();
        self.cast.discovery_error = None;
        self.cast.discovery_stats = Default::default();
        let ctx = ctx.clone();
        match crate::cast_discovery::browse(move || ctx.request_repaint()) {
            Ok(rx) => self.cast.discovery = Some(rx),
            Err(error) => self.cast.discovery_error = Some(error),
        }
    }
    fn cast_select(&mut self, device: crate::cast_discovery::Device) {
        let request = Start {
            position: self.position_time(),
            autoplay: !self.paused,
        };
        self.cast_select_at(device, request);
    }
    fn cast_select_at(&mut self, device: crate::cast_discovery::Device, request: Start) {
        if self.cast.unknown || self.cast.returning || self.cast.closing {
            return;
        }
        if self.cast.enabled {
            if self.cast.device_id.as_deref() == Some(&device.id)
                && self.cast.endpoint == Some(device.endpoint)
            {
                self.cast.picker = None;
                self.cast.discovery.take();
                return;
            }
            self.cast.pending = None;
            self.cast.quiet = None;
            self.cast.preparing = None;
            self.play_generation = self.backend.player.reserve();
            if let Some(worker) = &self.cast.worker {
                worker.handle.stop();
                self.cast.switching = Some((device, request));
                self.cast.picker = None;
                self.cast.discovery.take();
                return;
            }
            self.cast_returned();
        }
        self.sync_next(true);
        self.cast.endpoint = Some(device.endpoint);
        self.cast.device_id = Some(device.id);
        self.cast.device_name = Some(device.name);
        self.cast.local_volume = Some(self.volume);
        self.cast.local_details = Some(self.output_details.clone());
        self.cast.enabled = true;
        self.cast.picker = None;
        self.cast.discovery.take();
        if self.queue.current().is_some() {
            self.cast_start(request.position, request.autoplay);
        }
    }
    fn cast_return_local(&mut self) {
        if !self.cast.enabled || self.cast.unknown {
            return;
        }
        let position = self.position_time();
        self.cast.returning = true;
        self.cast.switching = None;
        self.cast.pending = None;
        self.cast.quiet = None;
        self.cast.preparing = None;
        self.play_generation = self.backend.player.reserve();
        self.position = position.as_secs();
        self.position_clock
            .target(self.play_generation, position, Instant::now());
        self.paused = true;
        self.buffering = false;
        if let Some(worker) = &self.cast.worker {
            worker.handle.stop();
        } else {
            self.cast_returned();
        }
    }
    pub(super) fn cast_controls(&mut self, ui: &mut egui::Ui) {
        self.cast.repaint = Some(ui.ctx().clone());
        let switching = self.cast.unknown
            || self.cast.returning
            || self.cast.switching.is_some()
            || self.cast.closing;
        if ui
            .add_enabled(
                !switching,
                egui::Button::new(tr("This computer"))
                    .selected(!self.cast.enabled)
                    .min_size(egui::vec2(ui.available_width(), 34.)),
            )
            .clicked()
        {
            self.cast_return_local();
        }
        if self.cast.enabled {
            ui.label(
                egui::RichText::new(self.cast.device_name.as_deref().unwrap_or("Chromecast"))
                    .strong(),
            );
            ui.label(tr(self.cast_state().label()));
            ui.label(tr("Your play queue plays on this device."));
            if self.cast_state() == ConnectionState::Selected && self.queue.current().is_some() {
                ui.label(tr("Press Play to start casting."));
            }
            if ui
                .add_enabled(
                    !switching,
                    egui::Button::new(tr("Stop casting · return here paused")),
                )
                .clicked()
            {
                self.cast_return_local();
            }
        }
        ui.separator();
        let devices = self.cast.devices.clone();
        for device in devices {
            let selected = self.cast.enabled && self.cast.device_id.as_deref() == Some(&device.id);
            let clicked = ui
                .push_id(&device.id, |ui| {
                    ui.add_enabled(
                        !switching && self.account.is_some(),
                        egui::Button::new(format!("{}\n{}", device.name, device.model))
                            .truncate()
                            .selected(selected)
                            .min_size(egui::vec2(ui.available_width(), 44.)),
                    )
                    .clicked()
                })
                .inner;
            if clicked {
                self.cast_select(device);
            }
        }
        if self.cast.discovery.is_some() {
            ui.label(tr(if self.cast.devices.is_empty() {
                "Looking for Cast devices…"
            } else {
                "Devices update automatically while this window is open."
            }));
        }
        if self.cast.devices.is_empty() && self.cast.discovery_stats.found > 0 {
            ui.label(tr(
                "Cast advertisements were found, but no supported device has resolved yet.",
            ));
        } else if self.cast.devices.is_empty() && self.cast.discovery.is_none() {
            ui.label(tr("No Cast devices have been discovered on this computer."));
        }
        if let Some(error) = self.cast.discovery_error {
            ui.label(error);
        }
        if ui
            .add_enabled(
                self.cast.discovery.is_none(),
                egui::Button::new(tr("Refresh devices")),
            )
            .clicked()
        {
            self.cast_search(ui.ctx());
        }
        if self.cast.unknown {
            ui.colored_label(theme_accent(),tr("Receiver release is unconfirmed. Stop casting on the receiver; local playback stays blocked."));
            if ui
                .add_enabled(
                    self.cast.checking.is_none(),
                    egui::Button::new(tr("Verify receiver is idle · read only")),
                )
                .clicked()
                && let Some(endpoint) = self.cast.endpoint
            {
                let legacy = true;
                let (tx, rx) = mpsc::sync_channel(1);
                let ctx = ui.ctx().clone();
                match std::thread::Builder::new()
                    .name("cast-idle-check".into())
                    .spawn(move || {
                        let result = crate::cast_auth::verify_idle(endpoint, legacy);
                        let _ = tx.try_send(result);
                        ctx.request_repaint();
                    }) {
                    Ok(_) => self.cast.checking = Some(rx),
                    Err(_) => {
                        self.error = Some("Could not start the read-only receiver check".into())
                    }
                }
            }
        }
        if self.cast.enabled
            && let Some(error) = &self.error
        {
            ui.colored_label(ui.visuals().error_fg_color, error);
        }
        ui.separator();
        ui.label(tr(
            "Devices are discovered on your local network. Returning here keeps playback paused.",
        ));
    }
    pub(super) fn cast_close_guard(&mut self, ctx: &egui::Context, requested: bool) {
        if self.cast.allow_close || !self.cast.enabled {
            return;
        }
        if requested && (self.cast.worker.is_some() || self.cast.unknown) {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.cast.closing = true;
            self.cast.pending = None;
            self.cast.quiet = None;
            self.cast.preparing = None;
            self.play_generation = self.backend.player.reserve();
            self.paused = true;
            self.buffering = false;
            if let Some(worker) = &self.cast.worker {
                worker.handle.stop();
            }
        }
        if self.cast.closing {
            if self.cast.worker.is_none() && !self.cast.unknown {
                self.cast.allow_close = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            } else if self.cast.unknown {
                egui::Window::new(tr("Cast release unconfirmed")).collapsible(false).show(ctx,|ui| {
                    ui.label(tr("The receiver may still be playing. Stop it manually before starting another output."));
                    if ui.button(tr("Keep application open")).clicked() {self.cast.closing=false;self.quitting=false;}
                    if ui.button(tr("Close without confirmed receiver stop")).clicked() {self.cast.allow_close=true;ctx.send_viewport_cmd(egui::ViewportCommand::Close);}
                });
            }
        }
    }
    fn cast_returned(&mut self) {
        self.cast.switching = None;
        self.cast.enabled = false;
        self.cast.phase = None;
        self.cast.device_id = None;
        self.cast.device_name = None;
        self.cast.returning = false;
        self.cast.volume_intent = None;
        self.cast.pending = None;
        self.cast.quiet = None;
        self.cast.preparing = None;
        self.paused = true;
        self.buffering = false;
        self.actual_quality.clear();
        if let Some(volume) = self.cast.local_volume.take() {
            self.volume = volume;
            self.backend.player.volume(volume);
        }
        if let Some(details) = self.cast.local_details.take() {
            self.output_details = details;
        }
    }
    pub(super) fn cast_start(&mut self, position: Duration, autoplay: bool) {
        self.sync_next(true);
        self.next_attempt = None;
        if self.cast.unknown {
            self.error =
                Some("Receiver release remains unconfirmed; no new output started.".into());
            return;
        }
        let Some(user) = self.account else {
            return;
        };
        let Some(occurrence) = self.queue.current_entry().map(|e| e.occurrence) else {
            return;
        };
        self.paused = !autoplay;
        self.buffering = true;
        self.ended = false;
        self.waiting_context = false;
        self.starved_generation = None;
        self.actual_quality.clear();
        self.error = None;
        self.position = position.as_secs();
        self.cast.pause_intent = None;
        self.cast.previous = None;
        self.cast.credit = Duration::ZERO;
        let request = Start { position, autoplay };
        if let Some(worker) = &self.cast.worker {
            self.play_generation = self.backend.player.reserve();
            worker.handle.stop();
            self.cast.pending = Some(request);
            return;
        }
        let (generation, reply) = self.backend.player.quiesce_for_transfer();
        self.play_generation = generation;
        self.position_clock
            .target(generation, position, Instant::now());
        self.cast.quiet = Some(Quiet {
            scope: Scope {
                user,
                generation,
                occurrence,
            },
            request,
            reply,
            deadline: Instant::now() + Duration::from_secs(15),
        });
        self.cast.preparing = None;
    }
    pub(super) fn cast_stop(&mut self) {
        self.cast.switching = None;
        self.cast.pending = None;
        self.cast.quiet = None;
        self.cast.preparing = None;
        if let Some(worker) = &self.cast.worker {
            self.cast.phase = Some(crate::cast_control::Phase::Releasing);
            worker.handle.stop();
        }
    }
    pub(super) fn cast_pause(&mut self, paused: bool) {
        if let Some(worker) = &self.cast.worker {
            match worker.handle.command(Command::Pause(paused)) {
                Ok(()) => {
                    self.cast.pause_intent = Some(paused);
                    self.paused = paused;
                    self.buffering = true;
                }
                Err(error) => self.error = Some(error.into()),
            }
        } else {
            self.paused = paused;
            if let Some(request) = &mut self.cast.pending {
                request.autoplay = !paused;
            }
            if let Some(quiet) = &mut self.cast.quiet {
                quiet.request.autoplay = !paused;
            }
        }
    }
    pub(super) fn cast_toggle(&mut self) {
        if self.cast.worker.is_some() || self.cast.quiet.is_some() || self.cast.preparing.is_some()
        {
            self.cast_pause(!self.paused);
        } else if self.waiting_context {
            self.advance(true);
        } else {
            self.cast_start(self.position_time(), true);
        }
    }
    pub(super) fn cast_seek(&mut self, position: Duration) {
        let position = position.min(Duration::from_secs(
            self.queue
                .current()
                .map_or(0, |t| t.duration.saturating_sub(1)),
        ));
        if let Some(worker) = &self.cast.worker {
            let Some(serial) = self.cast.seek_serial.checked_add(1) else {
                self.error = Some("Cast control serial exhausted.".into());
                return;
            };
            match worker.handle.command(Command::Seek { serial, position }) {
                Ok(()) => {
                    self.cast.seek_serial = serial;
                    self.position = position.as_secs();
                    self.cast.previous = None;
                    self.buffering = true;
                    self.position_clock
                        .target(self.play_generation, position, Instant::now());
                }
                Err(error) => self.error = Some(error.into()),
            }
        } else {
            self.cast_start(position, !self.paused);
        }
    }
    pub(super) fn volume_target(&self) -> f32 {
        self.cast.volume_intent.unwrap_or(self.volume)
    }
    pub(super) fn playback_volume(&mut self, volume: f32) {
        if self.cast.enabled {
            if let Some(worker) = &self.cast.worker {
                match worker
                    .handle
                    .command(Command::Volume(f64::from(volume.clamp(0., 1.))))
                {
                    Ok(()) => self.cast.volume_intent = Some(volume.clamp(0., 1.)),
                    Err(error) => self.error = Some(error.into()),
                }
            }
        } else {
            self.volume = volume.clamp(0., 1.);
            self.backend.player.volume(self.volume);
        }
    }
    fn cast_track_metadata(&self) -> crate::cast_control::Metadata {
        self.queue
            .current()
            .map_or_else(Default::default, |track| crate::cast_control::Metadata {
                title: track.title.clone(),
                artist: track.artist.name.clone(),
                album: track.album.title.clone(),
                artwork: track.cover_url(640),
            })
    }
    pub(super) fn cast_prepared(
        &mut self,
        user: u64,
        generation: u64,
        result: Result<(crate::cast_control::Media, String), String>,
    ) {
        let Some(scope) = self.cast.preparing else {
            return;
        };
        if scope.user != user
            || scope.generation != generation
            || !scope.matches(self)
            || !self.cast.enabled
        {
            return;
        }
        self.cast.preparing = None;
        let (mut media, quality) = match result {
            Ok(value) => value,
            Err(error) => {
                self.error = Some(error);
                self.paused = true;
                self.buffering = false;
                return;
            }
        };
        media.autoplay = !self.paused;
        media.metadata = self.cast_track_metadata();
        let Some(endpoint) = self.cast.endpoint else {
            self.error = Some("Invalid Cast receiver address".into());
            return;
        };
        let legacy = true;
        let (handle, mut control) = crate::cast_control::channel();
        if let Some(ctx) = self.cast.repaint.clone() {
            control.set_waker(move || ctx.request_repaint());
        }
        let audio = self.backend.player.clone();
        match std::thread::Builder::new()
            .name("cast-control".into())
            .spawn(move || {
                let _ =
                    crate::cast_auth::run_runtime(endpoint, legacy, media, control, &mut || {
                        if audio.current(generation) {
                            Ok(())
                        } else {
                            Err("Playback context changed")
                        }
                    });
            }) {
            Ok(_) => {
                self.cast.seek_serial = 0;
                self.cast.worker = Some(Running { scope, handle });
                self.actual_quality = quality;
                self.output_details =
                    "Google Cast · receiver output format unknown; local DSP bypassed".into();
            }
            Err(_) => {
                self.error = Some("Could not start the Cast worker".into());
                self.paused = true;
                self.buffering = false;
            }
        }
    }
    pub(super) fn cast_tick(&mut self) {
        if self.cast.picker.is_none() {
            self.cast.discovery.take();
        }
        if let Some(update) = self
            .cast
            .discovery
            .as_mut()
            .and_then(|browser| browser.next())
        {
            self.cast.devices = update.devices;
            self.cast.discovery_stats = update.stats;
            self.cast.discovery_error = update.error;
            if update.finished {
                self.cast.discovery.take();
            }
        }
        if let Some(check) = self.cast.checking.take() {
            match check.try_recv() {
                Ok(Ok(true)) => {
                    self.cast.unknown = false;
                    self.error = None;
                    self.notice = Some("Receiver idle confirmed. Playback remains paused.".into());
                }
                Ok(Ok(false)) => {
                    self.error = Some("Receiver is still active; no controls were sent.".into())
                }
                Ok(Err(error)) => self.error = Some(error.into()),
                Err(mpsc::TryRecvError::Empty) => self.cast.checking = Some(check),
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.error = Some("Receiver idle check did not complete.".into())
                }
            }
        }
        if let Some(quiet) = self.cast.quiet.take()
            && quiet.scope.matches(self)
            && self.cast.enabled
        {
            match quiet.reply.try_recv() {
                Ok(true) => {
                    self.cast.preparing = Some(quiet.scope);
                    let id = self.queue.current().unwrap().id;
                    if !self.try_send(Request::CastPrepare {
                        user: quiet.scope.user,
                        generation: quiet.scope.generation,
                        id,
                        quality: self.quality.clone(),
                        position: quiet.request.position,
                        autoplay: quiet.request.autoplay,
                    }) {
                        self.cast.preparing = None;
                        self.paused = true;
                        self.buffering = false;
                    }
                }
                Err(mpsc::TryRecvError::Empty) if Instant::now() < quiet.deadline => {
                    self.cast.quiet = Some(quiet)
                }
                _ => {
                    self.paused = true;
                    self.buffering = false;
                    self.error = Some(
                        "Local output release was not confirmed; Cast was not started.".into(),
                    );
                }
            }
        }
        if let Some(worker) = &self.cast.worker {
            if self.account != Some(worker.scope.user) {
                self.cast.returning = true;
                self.cast.pending = None;
            }
            if !worker.scope.matches(self) {
                worker.handle.stop();
            }
        }
        for _ in 0..34 {
            let Some((scope, event)) = self
                .cast
                .worker
                .as_ref()
                .and_then(|w| w.handle.next_event().map(|event| (w.scope, event)))
            else {
                break;
            };
            match event {
                CastEvent::Phase(phase) if scope.matches(self) => self.cast.phase = Some(phase),
                CastEvent::Snapshot(snapshot)
                    if scope.matches(self) && snapshot.seek_serial == self.cast.seek_serial =>
                {
                    let now = snapshot.observed;
                    if let Some((previous, time)) = self.cast.previous {
                        let elapsed = now.saturating_duration_since(time);
                        if previous.state == State::Playing
                            && snapshot.state == State::Playing
                            && previous.seek_serial == snapshot.seek_serial
                            && snapshot.position >= previous.position
                        {
                            let delta = snapshot.position - previous.position;
                            if delta <= elapsed + Duration::from_millis(250) {
                                self.cast.credit += delta.min(elapsed);
                            }
                        }
                    }
                    self.cast.previous = Some((snapshot, now));
                    self.observe_listening(scope.generation, self.cast.credit, self.cast.credit);
                    let paused = snapshot.state == State::Paused;
                    if self.cast.pause_intent == Some(paused) {
                        self.cast.pause_intent = None;
                    }
                    if self.cast.pause_intent.is_none() && snapshot.state != State::Buffering {
                        self.paused = paused;
                    }
                    self.buffering =
                        snapshot.state == State::Buffering || self.cast.pause_intent.is_some();
                    self.position = snapshot.position.as_secs();
                    self.position_clock
                        .observe(scope.generation, snapshot.position, now);
                }
                CastEvent::Volume(level) if scope.matches(self) => {
                    self.volume = level as f32;
                    if self
                        .cast
                        .volume_intent
                        .is_some_and(|v| (v - self.volume).abs() < 0.005)
                    {
                        self.cast.volume_intent = None;
                    }
                }
                CastEvent::Closed { result, released } => {
                    self.cast.phase = None;
                    self.cast.volume_intent = None;
                    self.cast.worker.take();
                    self.cast.previous = None;
                    self.cast.unknown = !released;
                    if !released {
                        self.cast.switching = None;
                        self.cast.pending = None;
                        self.paused = true;
                        self.buffering = false;
                        self.error=Some("Cast release is unconfirmed. Check the receiver; local playback remains blocked.".into());
                        break;
                    }
                    if self.cast.closing {
                        break;
                    }
                    if let Some((device, request)) = self.cast.switching.take() {
                        self.cast_returned();
                        if self.account == Some(scope.user) {
                            self.cast_select_at(device, request);
                        }
                        break;
                    }
                    if self.cast.returning {
                        self.cast_returned();
                        break;
                    }
                    if let Some(request) = self.cast.pending.take() {
                        self.cast_start(request.position, request.autoplay);
                        break;
                    }
                    if !scope.matches(self) {
                        break;
                    }
                    self.buffering = false;
                    match result {
                        Ok(End::Finished) => {
                            if !self.paused {
                                self.advance(true);
                            } else {
                                let advanced = self.queue_mut().advance(true);
                                self.play_generation = self.backend.player.reserve();
                                self.position = 0;
                                self.position_clock.target(
                                    self.play_generation,
                                    Duration::ZERO,
                                    Instant::now(),
                                );
                                self.actual_quality.clear();
                                self.ended = true;
                                self.waiting_context =
                                    !advanced && self.queue.continuation().is_some();
                            }
                        }
                        Ok(End::Renew) => self.cast_start(self.position_time(), true),
                        Ok(End::Stopped) => self.paused = true,
                        Err(error) => {
                            self.paused = true;
                            self.error = Some(error.into());
                        }
                    }
                    break;
                }
                _ => {}
            }
        }
    }
}
