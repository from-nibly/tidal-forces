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
        let compact = ui.available_height() < 400.;
        let sections = [
            (SettingsSection::General, "General"),
            (SettingsSection::Appearance, "Appearance"),
            (SettingsSection::Playback, "Playback"),
            (SettingsSection::Privacy, "Library & Privacy"),
            (SettingsSection::Shortcuts, "Shortcuts"),
            (SettingsSection::About, "About"),
        ];
        if compact {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Settings").size(24.).strong());
                let label = sections
                    .iter()
                    .find_map(|(section, name)| {
                        (*section == self.settings_section).then_some(*name)
                    })
                    .unwrap_or("Settings");
                egui::ComboBox::from_id_salt("settings_section")
                    .selected_text(label)
                    .show_ui(ui, |ui| {
                        for (section, name) in sections {
                            ui.selectable_value(&mut self.settings_section, section, name);
                        }
                    });
            });
        } else {
            surfaces::page_title(ui, "Settings");
            ui.label(RichText::new("One place for your listening preferences.").color(MUTED));
            ui.add_space(16.);
            ui.horizontal_wrapped(|ui| {
                for (section, name) in sections {
                    ui.selectable_value(&mut self.settings_section, section, name);
                }
            });
        }
        ui.separator();
        ui.add_space(if compact { 8. } else { 16. });
        egui::ScrollArea::vertical().id_salt("settings_content").auto_shrink([false, false]).show(ui, |ui| {
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
                surfaces::section_title(ui, "Audio output");
                self.output_controls(ui);
                ui.label(RichText::new("Google Cast and TIDAL Connect are separate integrations. Neither is enabled in this release.").color(MUTED));
                ui.add_space(20.);
                surfaces::section_title(ui, "Listening quality");
                if self.auth_pkce {
                    ui.colored_label(ACCENT, "Lossless-capable sign-in connected");
                } else {
                    ui.label("Compatibility sign-in is AAC-only. Use General to authorize lossless playback.");
                }
                let mut quality = self.quality.clone();
                ui.radio_value(
                    &mut quality,
                    "LOSSLESS".into(),
                    "Prefer lossless · AAC fallback when unavailable",
                );
                ui.radio_value(&mut quality, "HIGH".into(), "High · AAC");
                ui.radio_value(&mut quality, "LOW".into(), "Low · AAC");
                if quality != self.quality {
                    self.sync_next(true);
                    self.next_attempt = None;
                    self.quality = quality;
                }
                ui.label(RichText::new("Applies to the next track. The player reports the actual source format, not unverified hi-res or bit-perfect output.").color(MUTED));
                ui.add_space(20.);
                self.dsp_controls(ui);
            }
            SettingsSection::Privacy => {
                self.metadata_cache_controls(ui);
                ui.add_space(16.);
                surfaces::section_title(ui,"Artwork disk cache");
                if let Some(cache)=self.artwork_cache.clone() {
                    if ui.checkbox(&mut self.appearance.artwork_cache,"Cache TIDAL artwork on this device").changed() {
                        cache.set_enabled(self.appearance.artwork_cache);ui.ctx().forget_all_images();self.save_appearance();
                    }
                    ui.label("Up to 64 MiB / 128 files of unsigned TIDAL artwork, shared across accounts on this device. No audio or signed URLs. Disabling retains files; Clear deletes cached artwork. Enabled caching can refill it as images are viewed.");
                    if ui.button("Clear cached artwork").clicked() {cache.clear(ui.ctx());ui.ctx().forget_all_images();}
                    ui.label(cache.status());
                } else {ui.label("Artwork disk cache is unavailable in this session.");}
                ui.add_space(16.);
                self.favorite_cache_controls(ui);
                ui.add_space(16.);
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
                    ("Shift+F10", "Organization menu on a focused library folder/playlist row"),
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
        });
    }

    fn output_controls(&mut self, ui: &mut egui::Ui) {
        let catalog = self.backend.player.outputs.snapshot();
        if !catalog.requested {
            self.backend.player.outputs.refresh();
        }
        let before = self.appearance.output.clone();
        let selected = match &before {
            crate::output_devices::Target::RequireSelection => {
                "Choose an output before playback".into()
            }
            crate::output_devices::Target::SystemDefault => {
                crate::output_devices::DEFAULT_LABEL.to_owned()
            }
            crate::output_devices::Target::CompatibilityDefault => {
                "Compatibility default · no physical route guard".to_owned()
            }
            _ => catalog
                .devices
                .iter()
                .find(|d| d.target == before)
                .map_or_else(
                    || "Saved output · unavailable or not refreshed".into(),
                    |d| d.label.clone(),
                ),
        };
        egui::ComboBox::from_id_salt("audio_output_picker")
            .selected_text(selected)
            .width(ui.available_width().min(420.))
            .show_ui(ui, |ui| {
                ui.selectable_value(
                    &mut self.appearance.output,
                    crate::output_devices::Target::SystemDefault,
                    crate::output_devices::DEFAULT_LABEL,
                );
                for device in &catalog.devices {
                    ui.selectable_value(
                        &mut self.appearance.output,
                        device.target.clone(),
                        &device.label,
                    );
                }
                ui.selectable_value(
                    &mut self.appearance.output,
                    crate::output_devices::Target::CompatibilityDefault,
                    "Compatibility default · no physical route guard",
                );
            });
        if ui
            .add_enabled(
                !self.backend.player.outputs.busy(),
                egui::Button::new("Refresh outputs"),
            )
            .clicked()
        {
            self.backend.player.outputs.refresh();
        }
        if catalog.unavailable {
            ui.label(RichText::new("Native output server unavailable. No automatic switch to compatibility output.").color(MUTED));
        }
        if self.backend.player.outputs.busy() {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(100));
        }
        ui.label(RichText::new(&self.output_details).color(MUTED));
        ui.label(RichText::new("Changing output pauses playback; press Play to continue. A missing saved device never selects another device. Physical route notifications cannot promise zero buffered sound after unplugging.").color(MUTED));
        if before != self.appearance.output {
            self.sync_next(true);
            self.next_attempt = None;
            if !self.paused {
                self.toggle();
            }
            self.backend
                .player
                .select_output(self.appearance.output.clone());
            self.save_appearance();
        }
    }

    fn dsp_controls(&mut self, ui: &mut egui::Ui) {
        surfaces::section_title(ui, "Local audio processing");
        let before = self.appearance.dsp;
        ui.checkbox(&mut self.appearance.dsp.enabled, "Enable processing");
        ui.label(RichText::new("Off is a sample-preserving bypass. Changes reach playback through the bounded PCM buffer (up to about half a second).").color(MUTED));
        let metadata = self.backend.player.normalization();
        ui.add_enabled_ui(self.appearance.dsp.enabled, |ui| {
            ui.add(egui::Slider::new(&mut self.appearance.dsp.preamp_db, -24. ..=12.).text("Preamp · dB"));
            ui.add(egui::Slider::new(&mut self.appearance.dsp.balance, -1. ..=1.).text("Balance · left / right"));
            ui.checkbox(&mut self.appearance.dsp.mono, "Mono · stereo sources");
            ui.label(RichText::new("Balance/mono apply only to stereo. Positive EQ gains reserve automatic headroom; the processed path limits peaks. No limiter is inserted into bypass.").color(MUTED));
            ui.add_space(8.);
            ui.label("Equalizer · dB");
            ui.label(RichText::new("Stateful EQ or changing normalization gains use ordinary track transitions; no track crossfade.").color(MUTED));
            for (band, frequency) in self.appearance.dsp.bands.iter_mut().zip(crate::dsp::FREQUENCIES) {
                ui.add(egui::Slider::new(band, -12. ..=12.).text(format!("{frequency} Hz")));
            }
            ui.add_space(8.);
            ui.label("Normalization · TIDAL SDK attenuation policy");
            ui.horizontal_wrapped(|ui| {
                for (mode, name) in [(crate::dsp::Normalization::Off, "Off"), (crate::dsp::Normalization::Track, "Track"), (crate::dsp::Normalization::Album, "Album")] {
                    ui.selectable_value(&mut self.appearance.dsp.normalization, mode, name);
                }
            });
            if !metadata.available(self.appearance.dsp.normalization) {
                ui.label(RichText::new("Unavailable for this stream: verified gain metadata was not supplied. No normalization gain is applied.").color(MUTED));
            }
        });
        if ui.button("Reset processing").clicked() {
            self.appearance.dsp = Default::default();
        }
        if before != self.appearance.dsp {
            self.sync_next(true);
            self.next_attempt = None;
            self.backend.player.dsp.set(self.appearance.dsp);
            self.appearance_save =
                Some(std::time::Instant::now() + std::time::Duration::from_millis(250));
        }
    }
}
