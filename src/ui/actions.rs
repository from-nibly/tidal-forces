use super::*;
use crate::actions::Action;

impl App {
    pub(super) fn dispatch(&mut self, action: Action, ctx: &egui::Context) {
        let starts_playback = matches!(
            action,
            Action::Play | Action::Toggle | Action::Next | Action::Previous
        );
        match action {
            Action::Play if self.paused => self.toggle(),
            Action::Pause if !self.paused => {
                self.sync_next(true);
                self.next_attempt = None;
                self.paused = true;
                self.paused_since = Some(std::time::SystemTime::now());
                self.backend.player.pause(true);
            }
            Action::Toggle => self.toggle(),
            Action::Next => self.next(),
            Action::Previous => self.previous(),
            Action::Stop => self.stop_and_clear(),
            Action::Seek(position) => self.seek_time(position),
            Action::SeekBy(millis) => {
                let current = self.position_time();
                let delta = std::time::Duration::from_millis(millis.unsigned_abs());
                self.seek_time(if millis < 0 {
                    current.saturating_sub(delta)
                } else {
                    current.saturating_add(delta)
                });
            }
            Action::Volume(value) if value.is_finite() => {
                self.volume = value.clamp(0., 1.);
                self.backend.player.volume(self.volume);
            }
            Action::VolumeBy(delta) if delta.is_finite() => {
                self.dispatch(Action::Volume(self.volume + delta), ctx)
            }
            Action::NowPlaying => {
                self.open_now_playing(ctx);
                self.dispatch(Action::Raise, ctx);
            }
            Action::MiniPlayer => {
                self.mini_open = true;
                ctx.request_repaint();
                ctx.send_viewport_cmd_to(super::mini::viewport_id(), egui::ViewportCommand::Focus);
            }
            Action::Open(link) => self.open_link(link),
            Action::Raise => {
                if let Some(tray) = &self.tray {
                    tray.show();
                }
                ctx.send_viewport_cmd_to(
                    egui::ViewportId::ROOT,
                    egui::ViewportCommand::Visible(true),
                );
                ctx.send_viewport_cmd_to(
                    egui::ViewportId::ROOT,
                    egui::ViewportCommand::Minimized(false),
                );
                ctx.send_viewport_cmd_to(egui::ViewportId::ROOT, egui::ViewportCommand::Focus);
            }
            Action::Quit => {
                self.quitting = true;
                self.dispatch(Action::Raise, ctx);
                ctx.send_viewport_cmd_to(egui::ViewportId::ROOT, egui::ViewportCommand::Close);
            }
            _ => {}
        }
        if starts_playback
            && self.paused
            && self.appearance.output == crate::output_devices::Target::RequireSelection
            && self.page == Page::Settings
            && self.settings_section == SettingsSection::Playback
        {
            self.dispatch(Action::Raise, ctx);
        }
    }

    pub(super) fn desktop_events(&mut self, ctx: &egui::Context) {
        let events: Vec<_> = self
            .media
            .as_ref()
            .map(|m| m.events.try_iter().take(64).collect())
            .unwrap_or_default();
        for event in events {
            match Action::media(event) {
                Ok(Some(action)) => self.dispatch(action, ctx),
                Err(error) => self.error = Some(error.to_string()),
                Ok(None) => {}
            }
        }
        let commands: Vec<_> = self
            .instance
            .as_ref()
            .map(|i| i.commands.try_iter().take(32).collect())
            .unwrap_or_default();
        for command in commands {
            self.dispatch(command, ctx);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn fractional_seek_preserves_occurrence_and_pause_for_cached_and_fresh_loads() {
        for cached in [false, true] {
            let (ctx, mut app, events, mut requests) = super::super::tests::fixture();
            app.queue_mut()
                .start(
                    vec![super::super::tests::track(1)],
                    0,
                    "Fixture".into(),
                    None,
                )
                .unwrap();
            if cached {
                app.backend.player.seed_buffered_fixture();
            }
            app.paused = true;
            app.actual_quality = "FLAC".into();
            let occurrence = app.queue.current_entry().unwrap().occurrence;
            let old = app.play_generation;
            let position = Duration::from_millis(42_125);
            app.dispatch(Action::Seek(position), &ctx);
            match requests.try_recv().unwrap() {
                Request::SeekBuffered {
                    position: actual, ..
                } if cached => assert_eq!(actual, position),
                Request::Play {
                    position: actual, ..
                } if !cached => assert_eq!(actual, position),
                _ => panic!("Expected seek or fresh load"),
            }
            assert!(app.paused);
            assert_eq!(app.queue.current_entry().unwrap().occurrence, occurrence);
            assert!(!app.position_clock.is_confirmed(app.play_generation));
            events
                .send(Event::Position {
                    generation: old,
                    position: Duration::from_secs(99),
                })
                .unwrap();
            app.events();
            assert_eq!(app.position_time(), position);
            events
                .send(Event::Position {
                    generation: app.play_generation,
                    position: Duration::from_millis(42_375),
                })
                .unwrap();
            app.events();
            assert_eq!(app.position_time(), Duration::from_millis(42_375));
        }
    }

    #[test]
    fn deferred_seek_retains_fraction_and_pause_never_starts_a_browsed_track() {
        let (ctx, mut app, _, mut requests) = super::super::tests::fixture();
        app.tracks = vec![super::super::tests::track(1)];
        app.paused = false;
        app.dispatch(Action::Pause, &ctx);
        assert!(app.paused && app.queue.current().is_none());
        assert!(requests.try_recv().is_err());
        let tracks = app.tracks.clone();
        app.queue_mut()
            .start(tracks, 0, "Fixture".into(), None)
            .unwrap();
        app.dispatch(Action::Seek(Duration::from_millis(12_345)), &ctx);
        assert!(requests.try_recv().is_err());
        app.dispatch(Action::Play, &ctx);
        assert!(
            matches!(requests.try_recv(), Ok(Request::Play { position, .. }) if position == Duration::from_millis(12_345))
        );
    }
}
