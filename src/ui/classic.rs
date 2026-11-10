use super::*;
use crate::{
    actions::Action,
    skins::{Asset, Skin},
};
use std::{
    collections::BTreeMap,
    sync::{Arc, mpsc},
};

#[cfg(test)]
#[path = "classic_tests.rs"]
mod tests;

#[derive(Default)]
pub(super) struct Classic {
    pub skin: Option<Arc<Textures>>,
    path: String,
    notice: Option<String>,
    pending: Option<(mpsc::Receiver<anyhow::Result<Skin>>, bool)>,
    attempted: bool,
}
pub(super) struct Textures(BTreeMap<Asset, egui::TextureHandle>);
impl Textures {
    pub(super) fn upload(ctx: &egui::Context, skin: Skin) -> Self {
        Self(
            skin.images
                .into_iter()
                .map(|(kind, image)| {
                    let size = [image.width() as usize, image.height() as usize];
                    let texture = ctx.load_texture(
                        format!("classic-{kind:?}"),
                        egui::ColorImage::from_rgba_unmultiplied(size, image.as_raw()),
                        egui::TextureOptions::NEAREST,
                    );
                    (kind, texture)
                })
                .collect(),
        )
    }
}
impl App {
    fn start_skin_job(&mut self, import: bool, activate: bool, ctx: &egui::Context) {
        if self.classic.pending.is_some() {
            return;
        }
        self.classic.attempted = true;
        if self.state_store.is_none() || self.screenshot.is_some() {
            self.classic.notice = Some("Local skin storage is unavailable".into());
            return;
        }
        let Ok(root) = crate::store::config_dir() else {
            self.classic.notice = Some("Local skin storage is unavailable".into());
            return;
        };
        let path = if import {
            std::path::PathBuf::from(self.classic.path.trim())
        } else {
            root.join(crate::skins::STORED_NAME)
        };
        if !path.is_absolute() {
            self.classic.notice = Some("Enter an absolute local .wsz path".into());
            return;
        }
        let (tx, rx) = mpsc::sync_channel(1);
        let ctx = ctx.clone();
        let result = std::thread::Builder::new()
            .name("classic-skin".into())
            .spawn(move || {
                let result = crate::skins::load(&path, import.then_some(root.as_path()));
                let _ = tx.send(result);
                ctx.request_repaint();
            });
        match result {
            Ok(_) => {
                self.classic.notice = None;
                self.classic.pending = Some((rx, activate));
            }
            Err(_) => self.classic.notice = Some("Could not start the skin loader".into()),
        }
    }
    pub(super) fn classic_tick(&mut self, ctx: &egui::Context) {
        if self.appearance.classic_skin && !self.classic.attempted && self.classic.skin.is_none() {
            self.start_skin_job(false, false, ctx);
        }
        let result = self
            .classic
            .pending
            .as_ref()
            .and_then(|(rx, activate)| match rx.try_recv() {
                Ok(result) => Some((result, *activate)),
                Err(mpsc::TryRecvError::Disconnected) => {
                    Some((Err(anyhow::anyhow!("Skin loader stopped")), *activate))
                }
                Err(mpsc::TryRecvError::Empty) => None,
            });
        if let Some((result, activate)) = result {
            self.classic.pending = None;
            match result {
                Ok(skin) => {
                    self.classic.skin = Some(Arc::new(Textures::upload(ctx, skin)));
                    if activate {
                        self.appearance.classic_skin = true;
                        self.save_appearance();
                    }
                    self.classic.notice = Some("Classic skin ready. Playback is unchanged.".into());
                }
                Err(error) => {
                    if self.classic.skin.is_none() {
                        self.appearance.classic_skin = false;
                    }
                    self.classic.notice = Some(format!(
                        "Skin unavailable; the previous appearance is unchanged. {error}"
                    ));
                }
            }
        }
    }
    pub(super) fn skin_controls(&mut self, ui: &mut egui::Ui) {
        ui.collapsing(tr("Optional classic mini-player skin"), |ui| {
            ui.label(tr("Import your own classic .wsz main-panel artwork. Native text and shared playback controls; no scripts, plugins, modern .wal skins or separate EQ/playlist windows."));
            ui.label(tr("Use artwork you are entitled to use. Nothing is uploaded. The branded mini player remains the default and fallback."));
            ui.add(egui::TextEdit::singleline(&mut self.classic.path).hint_text(tr("Absolute path to a local .wsz file")).char_limit(4096).desired_width(f32::INFINITY));
            let busy = self.classic.pending.is_some();
            ui.horizontal_wrapped(|ui| {
                if ui.add_enabled(!busy && !self.classic.path.trim().is_empty(), egui::Button::new(tr("Import .wsz"))).clicked() { self.start_skin_job(true, true, ui.ctx()); }
                if ui.add_enabled(!busy, egui::Button::new(tr("Load saved skin"))).clicked() { self.start_skin_job(false, true, ui.ctx()); }
                if busy { ui.spinner(); ui.label(tr("Loading skin…")); }
                if ui.add_enabled(self.classic.skin.is_some() && !busy, egui::Checkbox::new(&mut self.appearance.classic_skin, tr("Use classic skin in mini player"))).changed() { self.save_appearance(); }
                if ui.button(tr("Open mini player")).clicked() { self.dispatch(Action::MiniPlayer, ui.ctx()); }
            });
            if let Some(notice) = &self.classic.notice { ui.label(notice); }
            ui.label(tr("8 MiB archive / 128 entries maximum. Local copy only. Classic pixel art has small hit targets; use keyboard focus or the branded mode when needed."));
        });
    }
    pub(super) fn classic_contents(&mut self, ui: &mut egui::Ui) {
        let Some(skin) = self.classic.skin.clone() else {
            return;
        };
        let ctx = ui.ctx().clone();
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(tr("Tidal Forces · Classic"))
                    .strong()
                    .size(11.),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if icon_button(
                    ui,
                    Icon::Close,
                    tr("Close mini player; keep playback unchanged"),
                    false,
                    24.,
                )
                .clicked()
                {
                    self.mini_open = false;
                    ctx.request_repaint_of(egui::ViewportId::ROOT);
                }
                self.cast_button(ui, true);
                if ui.button(tr("Player")).clicked() {
                    self.dispatch(Action::Raise, &ctx);
                }
                if ui.button(tr("Branded")).clicked() {
                    if let Some((_, activate)) = &mut self.classic.pending {
                        *activate = false;
                    }
                    self.appearance.classic_skin = false;
                    self.save_appearance();
                    ctx.request_repaint_of(egui::ViewportId::ROOT);
                }
            });
        });
        ui.add_space(4.);
        let scale = (ui.available_width() / 275.)
            .min((ui.available_height() - 38.) / 116.)
            .clamp(0.5, 4.);
        let (area, _) =
            ui.allocate_exact_size(vec2(275. * scale, 116. * scale), egui::Sense::hover());
        let canvas = Canvas {
            skin: &skin,
            area,
            scale,
        };
        canvas.draw(ui, Asset::Main, [0., 0., 275., 116.], [0., 0., 275., 116.]);
        canvas.draw(ui, Asset::Title, [27., 0., 275., 14.], [0., 0., 275., 14.]);
        let track = self.queue.current().cloned();
        let title = track
            .as_ref()
            .map_or("Nothing playing", |t| t.title.as_str());
        canvas.text(ui, [111., 25., 154., 12.], title, 9.);
        canvas.text(ui, [36., 25., 60., 15.], &time(self.position), 12.);
        canvas.text(ui, [111., 41., 154., 13.], "SOURCE QUALITY BELOW", 7.);
        let status = if track.is_none() || self.ended || self.audio_buffering() {
            18.
        } else if self.paused {
            9.
        } else {
            0.
        };
        canvas.draw(ui, Asset::Status, [status, 0., 9., 9.], [24., 28., 9., 9.]);
        let enabled = self.audio_available && (track.is_some() || self.queue.upcoming_len() > 0);
        for (index, action, label) in [
            (0, Action::Previous, "Previous"),
            (1, Action::Play, "Play"),
            (2, Action::Pause, "Pause"),
            (3, Action::Stop, "Stop and clear queue"),
            (4, Action::Next, "Next"),
        ] {
            let x = index as f32 * 23.;
            let spec = ButtonSpec {
                label,
                asset: Asset::Buttons,
                at: [16. + x, 88.],
                idle: [x, 0., 23., 18.],
                down: [x, 18., if index == 4 { 22. } else { 23. }, 18.],
            };
            if canvas.button(ui, &spec, enabled).clicked() {
                self.dispatch(action, &ctx);
            }
        }
        let eject = ButtonSpec {
            label: "Player",
            asset: Asset::Buttons,
            at: [136., 89.],
            idle: [114., 0., 22., 16.],
            down: [114., 16., 22., 16.],
        };
        if canvas.button(ui, &eject, true).clicked() {
            self.dispatch(Action::Raise, &ctx);
        }
        let close = ButtonSpec {
            label: "Close mini",
            asset: Asset::Title,
            at: [264., 3.],
            idle: [18., 0., 9., 9.],
            down: [18., 9., 9., 9.],
        };
        if skin.0.contains_key(&Asset::Title) && canvas.button(ui, &close, true).clicked() {
            self.mini_open = false;
            ctx.request_repaint_of(egui::ViewportId::ROOT);
        }
        let shuffled = self.queue.shuffled();
        let row = if shuffled { 30. } else { 0. };
        let shuffle = ButtonSpec {
            label: "Shuffle",
            asset: Asset::Shuffle,
            at: [164., 89.],
            idle: [28., row, 47., 15.],
            down: [28., row + 15., 47., 15.],
        };
        if canvas.button(ui, &shuffle, true).clicked() {
            self.queue_mut().toggle_shuffle();
        }
        let repeat_on = self.queue.repeat() != crate::queue::Repeat::Off;
        let row = if repeat_on { 30. } else { 0. };
        let repeat = ButtonSpec {
            label: "Repeat",
            asset: Asset::Shuffle,
            at: [210., 89.],
            idle: [0., row, 28., 15.],
            down: [0., row + 15., 28., 15.],
        };
        if canvas
            .button(ui, &repeat, true)
            .on_hover_text(format!("Repeat: {}", self.queue.repeat().label()))
            .clicked()
        {
            let next = self.queue.repeat().next();
            self.queue_mut().set_repeat(next);
        }
        for (label, x, source) in [("EQ", 219., 0.), ("Queue", 242., 23.)] {
            let spec = ButtonSpec {
                label,
                asset: Asset::Shuffle,
                at: [x, 58.],
                idle: [source, 61., 23., 12.],
                down: [source, 61., 23., 12.],
            };
            if canvas.button(ui, &spec, true).clicked() {
                if label == "EQ" {
                    self.settings_section = SettingsSection::Playback;
                    self.navigate(Page::Settings);
                } else {
                    self.close_now_playing(&ctx);
                    self.queue_open = true;
                    self.listening.show_panel = false;
                }
                self.dispatch(Action::Raise, &ctx);
            }
        }
        // Classic balance has no independent DSP owner. The native settings remain authoritative.
        canvas.text(ui, [177., 57., 38., 13.], "DSP: EQ", 6.);
        let mut volume = self.volume_target();
        let response = canvas.slider(ui, Bar::Volume, &mut volume, true);
        if response.changed() {
            self.dispatch(Action::Volume(volume), &ctx);
        }
        let duration = track.as_ref().map_or(0, |t| t.duration);
        let mut position =
            (self.position_time().as_secs_f64() / duration.max(1) as f64).clamp(0., 1.) as f32;
        let response = canvas.slider(
            ui,
            Bar::Position(self.play_generation),
            &mut position,
            duration > 0 && !self.buffering && !self.ended,
        );
        if duration > 0
            && !self.buffering
            && !self.ended
            && (response.drag_stopped() || (response.changed() && !response.dragged()))
        {
            self.dispatch(
                Action::Seek(std::time::Duration::from_secs_f64(
                    position as f64 * duration as f64,
                )),
                &ctx,
            );
        }
        let state = if self.output_fault.is_some() || !self.audio_available {
            "Audio unavailable"
        } else if self.audio_buffering() {
            "Buffering"
        } else if track.is_none() || self.ended {
            "Stopped"
        } else if self.paused {
            "Paused"
        } else {
            "Playing"
        };
        let quality = if self.actual_quality.is_empty() {
            "Source quality unavailable"
        } else {
            &self.actual_quality
        };
        ui.add(
            egui::Label::new(
                RichText::new(format!("{state} · {quality}"))
                    .size(10.)
                    .color(theme_muted()),
            )
            .truncate(),
        )
        .on_hover_text(tr(
            "Source quality; not a claim about the output device format",
        ));
        if self.output_fault.is_some() && ui.link("Choose an output in the main player").clicked() {
            self.settings_section = SettingsSection::Playback;
            self.navigate(Page::Settings);
            self.dispatch(Action::Raise, &ctx);
        }
        self.cast_picker(ui.ctx());
    }
}

struct Canvas<'a> {
    skin: &'a Textures,
    area: egui::Rect,
    scale: f32,
}
struct ButtonSpec {
    label: &'static str,
    asset: Asset,
    at: [f32; 2],
    idle: [f32; 4],
    down: [f32; 4],
}
#[derive(Clone, Copy)]
enum Bar {
    Volume,
    Position(u64),
}
impl Canvas<'_> {
    fn rect(&self, [x, y, w, h]: [f32; 4]) -> egui::Rect {
        egui::Rect::from_min_size(
            self.area.min + vec2(x, y) * self.scale,
            vec2(w, h) * self.scale,
        )
    }
    fn draw(&self, ui: &egui::Ui, asset: Asset, source: [f32; 4], target: [f32; 4]) -> bool {
        let Some(texture) = self.skin.0.get(&asset) else {
            return false;
        };
        let [x, y, w, h] = source;
        let size = texture.size_vec2();
        let uv = egui::Rect::from_min_max(
            pos2(x / size.x, y / size.y),
            pos2((x + w) / size.x, (y + h) / size.y),
        );
        ui.painter()
            .image(texture.id(), self.rect(target), uv, Color32::WHITE);
        true
    }
    fn text(&self, ui: &egui::Ui, target: [f32; 4], text: &str, size: f32) {
        let rect = self.rect(target);
        let painter = ui.painter().with_clip_rect(rect.intersect(ui.clip_rect()));
        painter.rect_filled(rect, 0., Color32::BLACK);
        painter.text(
            rect.left_center(),
            egui::Align2::LEFT_CENTER,
            text,
            egui::FontId::monospace(size * self.scale),
            Color32::WHITE,
        );
    }
    fn button(&self, ui: &mut egui::Ui, spec: &ButtonSpec, enabled: bool) -> egui::Response {
        let rect = self.rect([spec.at[0], spec.at[1], spec.idle[2], spec.idle[3]]);
        let response = ui.interact(
            rect,
            ui.id().with(("classic", spec.label)),
            if enabled {
                egui::Sense::click()
            } else {
                egui::Sense::hover()
            },
        );
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, tr(spec.label))
        });
        let sprite = if response.is_pointer_button_down_on() {
            spec.down
        } else {
            spec.idle
        };
        if !self.draw(
            ui,
            spec.asset,
            sprite,
            [spec.at[0], spec.at[1], spec.idle[2], spec.idle[3]],
        ) {
            self.text(
                ui,
                [spec.at[0], spec.at[1], spec.idle[2], spec.idle[3]],
                tr(spec.label),
                7.,
            );
        }
        if !enabled {
            ui.painter()
                .rect_filled(rect, 0., Color32::from_black_alpha(150));
        }
        if response.has_focus() {
            focus(ui, rect);
        }
        response.on_hover_text(tr(spec.label))
    }
    fn slider(
        &self,
        ui: &mut egui::Ui,
        kind: Bar,
        value: &mut f32,
        enabled: bool,
    ) -> egui::Response {
        let (name, target, thumb) = match kind {
            Bar::Volume => ("Volume", [107., 57., 68., 13.], 14.),
            Bar::Position(_) => ("Seek", [16., 72., 248., 10.], 29.),
        };
        let rect = self.rect(target);
        let epoch = match kind {
            Bar::Volume => 0,
            Bar::Position(epoch) => epoch,
        };
        let id = ui.id().with(("classic-slider", name, epoch));
        let mut response = ui.interact(
            rect,
            id,
            if enabled {
                egui::Sense::click_and_drag()
            } else {
                egui::Sense::hover()
            },
        );
        let before = *value;
        if (response.dragged() || response.clicked())
            && let Some(pos) = response.interact_pointer_pos()
        {
            *value = ((pos.x - rect.left() - thumb * self.scale / 2.)
                / ((target[2] - thumb) * self.scale))
                .clamp(0., 1.);
            ui.ctx().data_mut(|data| data.insert_temp(id, *value));
        } else if response.drag_stopped() {
            *value = ui
                .ctx()
                .data(|data| data.get_temp::<f32>(id))
                .unwrap_or(*value);
        }
        if enabled && response.has_focus() {
            ui.input_mut(|input| {
                if input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowRight) {
                    *value = (*value + 0.02).min(1.);
                }
                if input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowLeft) {
                    *value = (*value - 0.02).max(0.);
                }
                if input.consume_key(egui::Modifiers::NONE, egui::Key::Home) {
                    *value = 0.;
                }
                if input.consume_key(egui::Modifiers::NONE, egui::Key::End) {
                    *value = 1.;
                }
            });
        }
        if *value != before {
            response.mark_changed();
        }
        response.widget_info(|| egui::WidgetInfo::slider(enabled, *value as f64, tr(name)));
        let (asset, source, knob) = match kind {
            Bar::Volume => (
                Asset::Volume,
                [0., (*value * 27.).round() * 15., 68., 13.],
                [if response.dragged() { 0. } else { 15. }, 422., 14., 11.],
            ),
            Bar::Position(_) => (
                Asset::Position,
                [0., 0., 248., 10.],
                [if response.dragged() { 278. } else { 248. }, 0., 29., 10.],
            ),
        };
        let knob_target = [
            target[0] + *value * (target[2] - thumb),
            target[1],
            thumb,
            knob[3],
        ];
        if self.draw(ui, asset, source, target) {
            self.draw(ui, asset, knob, knob_target);
        } else {
            ui.painter().rect_filled(rect, 0., Color32::from_gray(40));
            ui.painter()
                .rect_filled(self.rect(knob_target), 0., Color32::WHITE);
        }
        if !enabled {
            ui.painter()
                .rect_filled(rect, 0., Color32::from_black_alpha(150));
        }
        if response.has_focus() {
            focus(ui, rect);
        }
        response.on_hover_text(tr(name))
    }
}
fn focus(ui: &egui::Ui, rect: egui::Rect) {
    ui.painter().rect_stroke(
        rect.expand(2.),
        0.,
        Stroke::new(3.0_f32, Color32::BLACK),
        egui::StrokeKind::Outside,
    );
    ui.painter().rect_stroke(
        rect.expand(2.),
        0.,
        Stroke::new(1.0_f32, Color32::WHITE),
        egui::StrokeKind::Outside,
    );
}
