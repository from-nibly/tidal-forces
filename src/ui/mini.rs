use super::*;
use crate::actions::Action;

#[cfg(test)]
#[path = "mini_tests.rs"]
mod tests;

pub(super) fn viewport_id() -> egui::ViewportId {
    egui::ViewportId::from_hash_of("tidal-forces-mini")
}
pub(super) fn wayland() -> bool {
    std::env::var_os("WAYLAND_DISPLAY").is_some_and(|value| !value.is_empty())
        || std::env::var_os("WAYLAND_SOCKET").is_some_and(|value| !value.is_empty())
}

impl App {
    pub(super) fn tray_tick(&mut self, ctx: &egui::Context) {
        let enabled = cfg!(target_os = "linux")
            && self.appearance.tray_enabled
            && self.instance.is_some()
            && self.screenshot.is_none();
        if enabled && self.tray.is_none() {
            self.tray = Some(crate::tray::Tray::new(ctx.clone()));
        }
        if !enabled {
            self.tray.take();
        }
        let actions: Vec<_> = self
            .tray
            .as_ref()
            .map(|tray| tray.events.try_iter().take(32).collect())
            .unwrap_or_default();
        for action in actions {
            self.dispatch(action, ctx);
        }
    }

    pub(super) fn background_close(&mut self, ctx: &egui::Context) -> bool {
        if !ctx.input(|input| input.viewport().close_requested())
            || self.quitting
            || wayland()
            || !self.appearance.tray_enabled
            || self.credentials.dirty
            || self.credentials.busy
        {
            return false;
        }
        if self.tray.as_ref().is_some_and(crate::tray::Tray::hide) {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            true
        } else {
            false
        }
    }

    pub(super) fn mini_window(&mut self, ctx: &egui::Context) {
        if !self.mini_open {
            return;
        }
        let level = if self.appearance.mini_on_top && !wayland() {
            egui::WindowLevel::AlwaysOnTop
        } else {
            egui::WindowLevel::Normal
        };
        let classic = self.appearance.classic_skin && self.classic.skin.is_some();
        let builder = egui::ViewportBuilder::default()
            .with_title("Tidal Forces · Mini")
            .with_app_id("rocks.tidalforces.Player")
            .with_inner_size(if classic { [582., 340.] } else { [460., 260.] })
            .with_min_inner_size([380., 250.])
            .with_window_level(level);
        let mut open = true;
        ctx.show_viewport_immediate(viewport_id(), builder, |ctx, class| {
            if class != egui::ViewportClass::Embedded
                && ctx.input(|input| input.viewport().close_requested())
            {
                self.mini_open = false;
                self.cast_mini_closed();
                ctx.request_repaint_of(egui::ViewportId::ROOT);
                return;
            }
            if class == egui::ViewportClass::Embedded {
                egui::Window::new(tr("Tidal Forces · Mini"))
                    .id(egui::Id::new("mini_fallback"))
                    .open(&mut open)
                    .default_width(420.)
                    .resizable(true)
                    .show(ctx, |ui| self.mini_contents(ui));
            } else {
                egui::CentralPanel::default()
                    .frame(egui::Frame::new().fill(theme_panel()).inner_margin(16))
                    .show(ctx, |ui| self.mini_contents(ui));
            }
        });
        if !open {
            self.mini_open = false;
        }
    }

    pub(super) fn mini_contents(&mut self, ui: &mut egui::Ui) {
        if self.appearance.classic_skin && self.classic.skin.is_some() {
            self.classic_contents(ui);
            return;
        }
        ui.push_id("mini_controls", |ui| {
            let ctx = ui.ctx().clone();
            ui.horizontal(|ui| {
                mark(ui, 16., theme_accent());
                ui.label(RichText::new(if ui.available_width() > 280. { "TIDAL FORCES" } else { "MINI" }).size(11.).strong().color(theme_accent()));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if icon_button(ui, Icon::Close, tr("Close mini player; keep playback unchanged"), false, 24.).clicked() { self.mini_open = false; ctx.request_repaint_of(egui::ViewportId::ROOT); }
                    let more = icon_button(ui, Icon::More, tr("Mini player options"), false, 24.);
                    egui::Popup::menu(&more).show(|ui| {
                        if let Some(track)=self.queue.current() {copy_link_menu(ui,Link::Track(track.id));ui.separator();}
                        if ui.button(tr("Now Playing · lyrics")).clicked() { self.dispatch(Action::NowPlaying, &ctx); ui.close(); }
                        let available = !wayland();
                        if ui.add_enabled(available, egui::Checkbox::new(&mut self.appearance.mini_on_top, "Request always on top"))
                            .on_hover_text(if available { "A window-level request; your desktop decides whether to honor it." } else { "Native Wayland does not provide this window-level control. Use your compositor's window rules instead." }).changed() {
                            self.save_appearance(); ctx.request_repaint_of(egui::ViewportId::ROOT);
                        }
                        ui.separator();
                        let mut shuffle = self.queue.shuffled();
                        if ui.checkbox(&mut shuffle, tr("Shuffle source context")).changed() { self.queue_mut().toggle_shuffle(); }
                        if ui.button(format!("Repeat: {}", self.queue.repeat().label())).clicked() {
                            let repeat = self.queue.repeat().next(); self.queue_mut().set_repeat(repeat);
                        }
                    });
                    self.cast_button(ui,true);
                    if ui.button(tr("Player")).on_hover_text(tr("Show the main player")).clicked() { self.dispatch(Action::Raise, &ctx); }
                });
            });
            ui.add_space(10.);
            let track = self.queue.current().cloned();
            ui.horizontal(|ui| {
                let size = if ui.available_width() < 300. { 48. } else { 64. };
                let cover = artwork(ui, track.as_ref().and_then(|track| track.cover_url(80)), size).on_hover_text(tr("Open Now Playing"));
                if cover.clicked() { self.dispatch(Action::NowPlaying, &ctx); }
                ui.allocate_ui_with_layout(vec2(ui.available_width(), size), egui::Layout::top_down(egui::Align::Min), |ui| {
                    if let Some(track) = &track {
                        ui.add(egui::Label::new(RichText::new(&track.title).strong().size(16.)).truncate()).on_hover_text(&track.title);
                        ui.add(egui::Label::new(RichText::new(&track.artist.name).color(theme_muted()).size(12.)).truncate()).on_hover_text(&track.artist.name);
                        if !self.actual_quality.is_empty() { ui.add(egui::Label::new(RichText::new(&self.actual_quality).color(theme_accent()).size(10.)).truncate()).on_hover_text(tr("Source quality; not a claim about the output device format")); }
                    } else {
                        ui.label(RichText::new(tr("Nothing playing")).strong());
                        ui.label(RichText::new(tr("Choose music in the main player")).size(12.).color(theme_muted()));
                    }
                });
            });
            ui.add_space(8.);
            let enabled = self.audio_available && (track.is_some() || self.queue.upcoming_len() > 0);
            ui.horizontal(|ui| {
                ui.add_enabled_ui(enabled, |ui| {
                    if icon_button(ui, Icon::Previous, tr("Previous"), false, 28.).clicked() { self.dispatch(Action::Previous, &ctx); }
                    if icon_button(ui, if self.paused { Icon::Play } else { Icon::Pause }, if self.paused { "Play" } else { "Pause" }, true, 38.).clicked() { self.dispatch(Action::Toggle, &ctx); }
                    if icon_button(ui, Icon::Next, tr("Next"), false, 28.).clicked() { self.dispatch(Action::Next, &ctx); }
                });
                if self.audio_buffering() { ui.add(egui::Spinner::new().size(14.)); }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.spacing_mut().slider_width = (ui.available_width() - 24.).clamp(35., 85.);
                    let mut volume = self.volume_target();
                    if ui.add(egui::Slider::new(&mut volume, 0. ..=1.).show_value(false)).on_hover_text(tr("Volume")).changed() { self.dispatch(Action::Volume(volume), &ctx); }
                    ui.label(RichText::new(tr("Vol")).size(10.).color(theme_muted()));
                });
            });
            ui.horizontal(|ui| {
                let duration = track.as_ref().map_or(0, |track| track.duration);
                ui.label(RichText::new(time(self.position)).size(10.).color(theme_muted()));
                ui.spacing_mut().slider_width = (ui.available_width() - 45.).max(60.);
                let mut position = self.position_time().as_secs_f64();
                let slider = ui.add_enabled(duration > 0 && !self.buffering && !self.ended, egui::Slider::new(&mut position, 0. ..=duration.max(1) as f64).show_value(false));
                if slider.drag_stopped() || (slider.changed() && !slider.dragged()) { self.dispatch(Action::Seek(std::time::Duration::from_secs_f64(position)), &ctx); }
                ui.label(RichText::new(time(duration)).size(10.).color(theme_muted()));
            });
            if (self.output_fault.is_some() || (self.queue.current().is_some() && self.appearance.output == crate::output_devices::Target::RequireSelection))
                && ui.link("Choose an output in the main player").clicked() {
                self.settings_section = SettingsSection::Playback;
                self.navigate(Page::Settings);
                self.dispatch(Action::Raise, &ctx);
            }
        });
        self.cast_picker(ui.ctx());
    }
}
