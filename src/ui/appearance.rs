use super::*;
use crate::palette::{Palette, Theme};

impl App {
    pub(super) fn theme_controls(&mut self, ui: &mut egui::Ui) {
        ui.label(tr("Interface language: English"));
        self.skin_controls(ui);
        surfaces::section_title(ui, tr("Theme"));
        let before = self.appearance.theme;
        ui.horizontal_wrapped(|ui| {
            for (mode, label) in [
                (Theme::Dark, "Dark"),
                (Theme::Light, "Light"),
                (Theme::System, "System"),
                (Theme::Custom, "Custom"),
            ] {
                ui.selectable_value(&mut self.appearance.theme, mode, tr(label));
            }
        });
        if before != self.appearance.theme {
            self.save_appearance();
            ui.ctx().request_repaint();
        }
        ui.label(tr("System follows the desktop’s light/dark preference when reported; otherwise it uses Dark. Playback is unchanged."));
        ui.collapsing(tr("Custom palette"), |ui| {
            let current = Palette::resolve(self.appearance.theme, ui.ctx().system_theme() != Some(egui::Theme::Light), self.appearance.palette);
            if self.palette_editor.is_empty() {
                self.palette_editor = serde_json::to_string_pretty(&current).unwrap();
            }
            ui.label(tr("RGB colors, 0–255. All text colors must meet 4.5:1 contrast on every surface. Invalid palettes leave the current appearance unchanged."));
            ui.horizontal_wrapped(|ui| {
                if ui.button(tr("Start from Dark")).clicked() { self.palette_editor = serde_json::to_string_pretty(&Palette::DARK).unwrap(); }
                if ui.button(tr("Start from Light")).clicked() { self.palette_editor = serde_json::to_string_pretty(&Palette::LIGHT).unwrap(); }
                if ui.button(tr("Copy current palette")).clicked() { ui.ctx().copy_text(serde_json::to_string_pretty(&current).unwrap()); }
            });
            ui.add(egui::TextEdit::multiline(&mut self.palette_editor).code_editor().desired_rows(8).desired_width(f32::INFINITY).char_limit(4096));
            if ui.button(tr("Apply custom palette")).clicked() {
                match Palette::parse(self.palette_editor.as_bytes()) {
                    Ok(palette) => {
                        self.appearance.palette = palette;
                        self.appearance.theme = Theme::Custom;
                        self.palette_error = None;
                        self.save_appearance();
                        ui.ctx().request_repaint();
                    }
                    Err(error) => self.palette_error = Some(error.to_string()),
                }
            }
            if let Some(error) = &self.palette_error { ui.colored_label(theme_error(), error); }
        });
    }
}
