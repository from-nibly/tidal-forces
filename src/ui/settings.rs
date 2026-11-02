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
                ui.label(RichText::new(tr("Settings")).size(24.).strong());
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
            surfaces::page_title(ui, tr("Settings"));
            ui.label(
                RichText::new(tr("One place for your listening preferences.")).color(theme_muted()),
            );
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
        egui::Frame::new().fill(theme_panel()).stroke(Stroke::new(1.0_f32,theme_border())).corner_radius(12).inner_margin(20).show(ui, |ui| {
        ui.set_width((width - 42.).max(0.));
        match self.settings_section {
            SettingsSection::General => {
                surfaces::section_title(ui, tr("Your account"));
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
                surfaces::section_title(ui, tr("Recommendation continuation"));
                self.radio_controls(ui);
                ui.add_space(16.);
                surfaces::section_title(ui, tr("Windows & background playback"));
                if ui.button(tr("Open mini player")).clicked() { self.dispatch(crate::actions::Action::MiniPlayer, ui.ctx()); }
                if ui.add_enabled(cfg!(target_os = "linux"), egui::Checkbox::new(&mut self.appearance.tray_enabled, "Enable desktop tray controls (close-to-tray on X11)")).changed() {
                    self.save_appearance(); ui.ctx().request_repaint();
                }
                if self.appearance.tray_enabled {
                    ui.label(match self.tray.as_ref().map(crate::tray::Tray::status) {
                        Some(crate::tray::Status::Ready) if super::mini::wayland() => "Tray registered. This native Wayland backend cannot reliably hide and restore windows, so Close still exits. Focus requests follow compositor policy.",
                        Some(crate::tray::Status::Ready) => "Tray registered. Close hides the main window; Quit still exits with credential safety checks.",
                        Some(crate::tray::Status::Starting) => "Connecting to the desktop tray. Until registered, closing still exits.",
                        _ => "No supported tray host is registered. Closing still exits; toggle this setting to retry a failed connection.",
                    });
                } else { ui.label(tr("Closing the main window exits the player. Tray/background playback is off by default.")); }
                ui.label(tr("Closing only the mini player leaves playback unchanged. If an active tray disappears, a hidden main window is restored."));
                ui.label(tr("Your queue, position, volume and last page restore paused on the next launch."));
                ui.label(tr("Saved playback is private to each TIDAL account. Press Play to resolve a fresh authorized stream; saved audio URLs are never reused."));
                ui.label(tr("TIDAL links open in this instance. System media keys use your existing MPRIS routing."));
            }
            SettingsSection::Appearance => {
                self.theme_controls(ui);
                ui.add_space(20.);
                surfaces::section_title(ui, tr("Player bar visualizer"));
                let mut mode = self.visualizer.mode;
                ui.horizontal_wrapped(|ui| {
                    for choice in [Mode::Off, Mode::Spectrum, Mode::Waveform] {
                        ui.selectable_value(&mut mode, choice, choice.label());
                    }
                });
                if mode != self.visualizer.mode {
                    self.set_visualizer(mode);
                }
                ui.label(RichText::new(tr("Real audio, colored by the artwork. Click empty space in the bottom bar to cycle modes. Volume does not affect the display.")).color(theme_muted()));
                ui.add_space(20.);
                surfaces::section_title(ui, tr("Track density"));
                let before = self.appearance.compact_rows;
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut self.appearance.compact_rows, false, tr("Comfortable"));
                    ui.selectable_value(&mut self.appearance.compact_rows, true, tr("Compact"));
                });
                if before != self.appearance.compact_rows {
                    self.save_appearance();
                }
                ui.add_space(20.);
                surfaces::section_title(ui, tr("Interface scale"));
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
                ui.label(RichText::new(tr("At narrow sizes the sidebar collapses to icons and the queue opens in a floating panel.")).color(theme_muted()));
            }
            SettingsSection::Playback => {
                surfaces::section_title(ui, tr("Audio output"));
                self.output_controls(ui);
                ui.label(RichText::new(tr("Google Cast and TIDAL Connect are separate integrations. Neither is enabled in this release.")).color(theme_muted()));
                ui.add_space(20.);
                surfaces::section_title(ui, tr("Listening quality"));
                if self.auth_pkce {
                    ui.colored_label(theme_accent(), tr("Lossless-capable sign-in connected"));
                } else {
                    ui.label(tr("Compatibility sign-in is AAC-only. Use General to authorize lossless playback."));
                }
                let mut quality = self.quality.clone();
                ui.radio_value(
                    &mut quality,
                    "LOSSLESS".into(),
                    tr("Prefer lossless · AAC fallback when unavailable"),
                );
                ui.radio_value(&mut quality, "HIGH".into(), tr("High · AAC"));
                ui.radio_value(&mut quality, "LOW".into(), tr("Low · AAC"));
                if quality != self.quality {
                    self.sync_next(true);
                    self.next_attempt = None;
                    self.quality = quality;
                }
                ui.label(RichText::new(tr("Applies to the next track. The player reports the actual source format, not unverified hi-res or bit-perfect output.")).color(theme_muted()));
                ui.add_space(20.);
                self.dsp_controls(ui);
            }
            SettingsSection::Privacy => {
                self.metadata_cache_controls(ui);
                ui.add_space(16.);
                surfaces::section_title(ui,tr("Artwork disk cache"));
                if let Some(cache)=self.artwork_cache.clone() {
                    if ui.checkbox(&mut self.appearance.artwork_cache,tr("Cache TIDAL artwork on this device")).changed() {
                        cache.set_enabled(self.appearance.artwork_cache);ui.ctx().forget_all_images();self.save_appearance();
                    }
                    ui.label(tr("Up to 64 MiB / 128 files of unsigned TIDAL artwork, shared across accounts on this device. No audio or signed URLs. Disabling retains files; Clear deletes cached artwork. Enabled caching can refill it as images are viewed."));
                    if ui.button(tr("Clear cached artwork")).clicked() {cache.clear(ui.ctx());ui.ctx().forget_all_images();}
                    ui.label(cache.status());
                } else {ui.label(tr("Artwork disk cache is unavailable in this session."));}
                ui.add_space(16.);
                self.favorite_cache_controls(ui);
                ui.add_space(16.);
                surfaces::section_title(ui, tr("Credentials"));
                self.credential_controls(ui);
                ui.label(tr("No passwords, telemetry or third-party music proxies. Favorite changes are sent only when you explicitly choose them."));
                ui.add_space(16.);
                surfaces::section_title(ui, tr("Saved playback"));
                ui.label(tr("Private account-scoped player-state.json files retain the queue, position, volume and last page. Signing out removes credentials, but leaves these playback snapshots for your next sign-in."));
                ui.label(tr("These files contain metadata, not audio, stream URLs or tokens. They are separate from credential storage."));
                if self.connected && ui.button(tr("Stop and clear playback queue")).clicked() {
                    self.stop_and_clear();
                }
                ui.label(RichText::new(tr("Clears current, manual, source and previous queue entries. Your saved navigation/preferences and TIDAL library are unchanged.")).size(12.).color(theme_muted()));
                ui.add_space(16.);
                surfaces::section_title(ui, tr("Listening history"));
                self.history_controls(ui, true);
                ui.label(RichText::new(tr("Recording is off by default. Disabling keeps existing records; Clear deletes them. Signing out retains this account's local history.")).size(12.).color(theme_muted()));
            }
            SettingsSection::Shortcuts => {
                surfaces::section_title(ui, tr("Keyboard & media controls"));
                for (key, action) in [
                    ("Space", "Play / pause when not editing"),
                    ("L / Esc", "Open Now Playing / return to browsing"),
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
                ui.label(tr("Independent native Rust client. Not affiliated with TIDAL."));
                ui.label(tr("Lossless is preferred; authorized AAC/HE-AAC remains supported. Encrypted audio and DRM circumvention are not supported."));
                ui.label(tr("Use --licenses for notices and --export-fdk-source PATH for the bundled AAC codec source."));
                ui.label(tr("Managed installations should be updated through Home Manager, not an in-app updater."));
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
                "Compatibility default · OS-managed routing".to_owned()
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
                    tr("Compatibility default · OS-managed routing"),
                );
            });
        if ui
            .add_enabled(
                !self.backend.player.outputs.busy(),
                egui::Button::new(tr("Refresh outputs")),
            )
            .clicked()
        {
            self.backend.player.outputs.refresh();
        }
        if catalog.unavailable {
            ui.label(RichText::new(tr("Native output server unavailable. No automatic switch to compatibility output.")).color(theme_muted()));
        }
        if self.backend.player.outputs.busy() {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(100));
        }
        ui.label(RichText::new(&self.output_details).color(theme_muted()));
        ui.label(RichText::new(tr("The audio server controls routing and fallback, including speakers when headphones disconnect. Native output changes move only this app’s stream, not the system default. Switching compatibility mode pauses playback.")).color(theme_muted()));
        if before != self.appearance.output {
            self.sync_next(true);
            self.next_attempt = None;
            if matches!(before, crate::output_devices::Target::CompatibilityDefault)
                || matches!(
                    self.appearance.output,
                    crate::output_devices::Target::CompatibilityDefault
                )
            {
                let position = self.position_time();
                self.play_generation = self.backend.player.reserve();
                self.position_clock.target(
                    self.play_generation,
                    position,
                    std::time::Instant::now(),
                );
                self.paused = true;
                self.buffering = false;
                self.ended = false;
                self.actual_quality.clear();
            }
            self.backend
                .player
                .select_output(self.appearance.output.clone());
            self.save_appearance();
        }
    }

    fn dsp_controls(&mut self, ui: &mut egui::Ui) {
        surfaces::section_title(ui, tr("Local audio processing"));
        let before = self.appearance.dsp;
        ui.checkbox(&mut self.appearance.dsp.enabled, tr("Enable processing"));
        ui.label(RichText::new(tr("Off is a sample-preserving bypass. Changes reach playback through the bounded PCM buffer (up to about half a second).")).color(theme_muted()));
        let metadata = self.backend.player.normalization();
        ui.add_enabled_ui(self.appearance.dsp.enabled, |ui| {
            ui.add(egui::Slider::new(&mut self.appearance.dsp.preamp_db, -24. ..=12.).text(tr("Preamp · dB")));
            ui.add(egui::Slider::new(&mut self.appearance.dsp.balance, -1. ..=1.).text(tr("Balance · left / right")));
            ui.checkbox(&mut self.appearance.dsp.mono, tr("Mono · stereo sources"));
            ui.label(RichText::new(tr("Balance/mono apply only to stereo. Positive EQ gains reserve automatic headroom; the processed path limits peaks. No limiter is inserted into bypass.")).color(theme_muted()));
            ui.add_space(8.);
            ui.label(tr("Equalizer · dB"));
            ui.label(RichText::new(tr("Stateful EQ or changing normalization gains use ordinary track transitions; no track crossfade.")).color(theme_muted()));
            for (band, frequency) in self.appearance.dsp.bands.iter_mut().zip(crate::dsp::FREQUENCIES) {
                ui.add(egui::Slider::new(band, -12. ..=12.).text(format!("{frequency} Hz")));
            }
            ui.add_space(8.);
            ui.label(tr("Normalization · TIDAL SDK attenuation policy"));
            ui.horizontal_wrapped(|ui| {
                for (mode, name) in [(crate::dsp::Normalization::Off, "Off"), (crate::dsp::Normalization::Track, "Track"), (crate::dsp::Normalization::Album, "Album")] {
                    ui.selectable_value(&mut self.appearance.dsp.normalization, mode, name);
                }
            });
            if !metadata.available(self.appearance.dsp.normalization) {
                ui.label(RichText::new(tr("Unavailable for this stream: verified gain metadata was not supplied. No normalization gain is applied.")).color(theme_muted()));
            }
        });
        if ui.button(tr("Reset processing")).clicked() {
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
