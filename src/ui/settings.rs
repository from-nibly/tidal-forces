use super::*;

#[derive(Clone, Copy, Default, Eq, PartialEq)]
pub(super) enum SettingsSection {
    #[default]
    General,
    Appearance,
    Playback,
    Privacy,
    Shortcuts,
    About,
}

impl App {
    pub(super) fn settings_page(&mut self, ui: &mut egui::Ui) {
        surfaces::page_title(ui, "Settings");
        ui.label(RichText::new("One place for your listening preferences.").color(MUTED));
        ui.add_space(16.);
        ui.horizontal_wrapped(|ui| {
            for (section, name) in [
                (SettingsSection::General, "General"),
                (SettingsSection::Appearance, "Appearance"),
                (SettingsSection::Playback, "Playback"),
                (SettingsSection::Privacy, "Library & Privacy"),
                (SettingsSection::Shortcuts, "Shortcuts"),
                (SettingsSection::About, "About"),
            ] {
                ui.selectable_value(&mut self.settings_section, section, name);
            }
        });
        ui.separator();
        ui.add_space(16.);
        let width = ui.available_width().min(880.);
        egui::Frame::new().fill(PANEL).stroke(Stroke::new(1.0_f32,BORDER)).corner_radius(12).inner_margin(20).show(ui, |ui| {
        ui.set_width((width - 42.).max(0.));
        match self.settings_section {
            SettingsSection::General => {
                surfaces::section_title(ui, "Your account");
                ui.label(if self.connected {
                    format!("Connected to TIDAL · {}", self.country)
                } else {
                    "Not signed in".into()
                });
                if ui
                    .button(if self.auth_pkce {
                        "Reconnect lossless account"
                    } else {
                        "Connect with TIDAL"
                    })
                    .clicked()
                {
                    self.send(Request::BeginPkce);
                }
                ui.add_space(16.);
                ui.label("Closing the window exits the player. Your queue, position, volume and last page restore paused on the next launch.");
                ui.label("Saved playback is private to each TIDAL account. Press Play to resolve a fresh authorized stream; saved audio URLs are never reused.");
                ui.label("TIDAL links open in this instance. System media keys use your existing MPRIS routing.");
            }
            SettingsSection::Appearance => {
                surfaces::section_title(ui, "Player bar visualizer");
                let mut mode = self.visualizer.mode;
                ui.horizontal_wrapped(|ui| {
                    for choice in [Mode::Off, Mode::Spectrum, Mode::Waveform] {
                        ui.selectable_value(&mut mode, choice, choice.label());
                    }
                });
                if mode != self.visualizer.mode {
                    self.set_visualizer(mode);
                }
                ui.label(RichText::new("Real audio, colored by the artwork. Click empty space in the bottom bar to cycle modes. Volume does not affect the display.").color(MUTED));
                ui.add_space(20.);
                surfaces::section_title(ui, "Track density");
                let before = self.appearance.compact_rows;
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut self.appearance.compact_rows, false, "Comfortable");
                    ui.selectable_value(&mut self.appearance.compact_rows, true, "Compact");
                });
                if before != self.appearance.compact_rows {
                    self.save_appearance();
                }
                ui.add_space(20.);
                surfaces::section_title(ui, "Interface scale");
                let mut zoom = self.appearance.zoom;
                ui.horizontal_wrapped(|ui| {
                    for (factor, label) in [
                        (0.75, "75%"),
                        (1., "100%"),
                        (1.1, "110%"),
                        (1.25, "125%"),
                        (1.5, "150%"),
                    ] {
                        ui.selectable_value(&mut zoom, factor, label);
                    }
                });
                if zoom != self.appearance.zoom {
                    self.appearance.zoom = zoom;
                    ui.ctx().set_zoom_factor(zoom);
                    self.save_appearance();
                }
                ui.label(RichText::new("At narrow sizes the sidebar collapses to icons and the queue opens in a floating panel.").color(MUTED));
            }
            SettingsSection::Playback => {
                surfaces::section_title(ui, "Listening quality");
                if self.auth_pkce {
                    ui.colored_label(ACCENT, "Lossless-capable sign-in connected");
                } else {
                    ui.label("Compatibility sign-in is AAC-only. Use General to authorize lossless playback.");
                }
                ui.radio_value(
                    &mut self.quality,
                    "LOSSLESS".into(),
                    "Prefer lossless · AAC fallback when unavailable",
                );
                ui.radio_value(&mut self.quality, "HIGH".into(), "High · AAC");
                ui.radio_value(&mut self.quality, "LOW".into(), "Low · AAC");
                ui.label(RichText::new("Applies to the next track. The player reports the actual source format, not unverified hi-res or bit-perfect output.").color(MUTED));
                ui.add_space(20.);
                surfaces::section_title(ui, "Audio output");
                ui.label("This computer · system default audio output");
                ui.label(RichText::new("Google Cast and TIDAL Connect are separate integrations. Neither is enabled in this release.").color(MUTED));
            }
            SettingsSection::Privacy => {
                surfaces::section_title(ui, "Credentials");
                self.credential_controls(ui);
                ui.label("No passwords, telemetry or third-party music proxies. Favorite changes are sent only when you explicitly choose them.");
                ui.add_space(16.);
                surfaces::section_title(ui, "Saved playback");
                ui.label("Private account-scoped player-state.json files retain the queue, position, volume and last page. Signing out removes credentials, but leaves these playback snapshots for your next sign-in.");
                ui.label("These files contain metadata, not audio, stream URLs or tokens. They are separate from credential storage.");
                if self.connected && ui.button("Stop and clear playback queue").clicked() {
                    self.stop_and_clear();
                }
                ui.label(RichText::new("Clears current, manual, source and previous queue entries. Your saved navigation/preferences and TIDAL library are unchanged.").size(12.).color(MUTED));
                ui.add_space(16.);
                surfaces::section_title(ui, "Listening history");
                self.history_controls(ui, true);
                ui.label(RichText::new("Recording is off by default. Disabling keeps existing records; Clear deletes them. Signing out retains this account's local history.").size(12.).color(MUTED));
            }
            SettingsSection::Shortcuts => {
                surfaces::section_title(ui, "Keyboard & media controls");
                for (key, action) in [
                    ("Space", "Play / pause when not editing"),
                    ("Ctrl+K", "Search music or open a TIDAL link"),
                    (
                        "Alt+Left / Right",
                        "Back / forward without restarting playback",
                    ),
                    ("Ctrl+Left / Right", "Previous / next track"),
                    ("Tab / Shift+Tab", "Move keyboard focus"),
                    ("System media keys", "MPRIS, with existing desktop routing"),
                ] {
                    ui.horizontal_wrapped(|ui| {
                        ui.monospace(key);
                        ui.label(action);
                    });
                }
            }
            SettingsSection::About => {
                surfaces::section_title(ui, &format!("Tidal Forces {}", env!("CARGO_PKG_VERSION")));
                ui.label("Independent native Rust client. Not affiliated with TIDAL.");
                ui.label("Lossless is preferred; authorized AAC/HE-AAC remains supported. Encrypted audio and DRM circumvention are not supported.");
                ui.label("Use --licenses for notices and --export-fdk-source PATH for the bundled AAC codec source.");
                ui.label("Managed installations should be updated through Home Manager, not an in-app updater.");
            }
        }
        });
    }
}
