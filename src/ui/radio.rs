use super::*;

#[cfg(test)]
#[path = "radio_tests.rs"]
mod tests;

const MAX_REQUESTS: u8 = 3;
const MAX_ADDED: usize = 150;

struct Pending {
    serial: u64,
    user: u64,
    revision: u64,
    generation: u64,
    occurrence: u64,
    _cancel: tokio::sync::oneshot::Sender<()>,
}
#[derive(Default)]
pub(super) struct Radio {
    serial: u64,
    pending: Option<Pending>,
    attempts: u8,
    added: usize,
    pub message: Option<String>,
    blocked: bool,
}
impl Radio {
    pub fn reset(&mut self) {
        *self = Self {
            serial: self.serial.wrapping_add(1),
            ..Self::default()
        };
    }
    pub fn stop(&mut self, message: impl Into<String>) {
        self.pending = None;
        self.blocked = true;
        self.message = Some(message.into());
    }
}
impl App {
    fn radio_allowed(&self) -> bool {
        self.appearance.radio_continuation
            && self.connected
            && self.account.is_some()
            && !self.paused
            && !self.ended
            && !self.buffering
            && !self.audio_buffering()
            && self.output_fault.is_none()
            && self.appearance.output != crate::output_devices::Target::RequireSelection
            && !self.credentials.busy
            && self.queue.repeat() == crate::queue::Repeat::Off
            && self.queue.continuation().is_none()
    }
    fn radio_scope(&self, pending: &Pending) -> bool {
        self.radio_allowed()
            && self.account == Some(pending.user)
            && self.queue.revision() == pending.revision
            && self.play_generation == pending.generation
            && self.queue.current_entry().map(|entry| entry.occurrence) == Some(pending.occurrence)
    }
    pub(super) fn radio_tick(&mut self) {
        self.sync_next(false);
        if self
            .radio
            .pending
            .as_ref()
            .is_some_and(|pending| !self.radio_scope(pending))
        {
            self.radio.pending = None;
            self.radio.message =
                Some("Recommendation request cancelled after playback or queue changed.".into());
        }
        if !self.radio_allowed()
            || self.radio.blocked
            || self.radio.pending.is_some()
            || self.radio.attempts >= MAX_REQUESTS
            || self.radio.added >= MAX_ADDED
            || self.queue.upcoming_len() > 2
            || self.context_loading
            || self.backend.player.has_prepared_next()
        {
            return;
        }
        let (Some(user), Some(entry)) = (self.account, self.queue.current_entry()) else {
            return;
        };
        self.radio.serial = self.radio.serial.wrapping_add(1);
        self.radio.attempts += 1;
        let (cancel, receiver) = tokio::sync::oneshot::channel();
        let (serial, id) = (self.radio.serial, entry.track.id);
        self.radio.pending = Some(Pending {
            serial,
            user,
            revision: self.queue.revision(),
            generation: self.play_generation,
            occurrence: entry.occurrence,
            _cancel: cancel,
        });
        self.radio.message = Some(format!(
            "Loading track recommendations ({} of {MAX_REQUESTS} requests)…",
            self.radio.attempts
        ));
        self.send(Request::Recommendations {
            user,
            serial,
            id,
            cancel: receiver,
        });
    }
    pub(super) fn radio_result(
        &mut self,
        user: u64,
        serial: u64,
        result: Result<Vec<Track>, String>,
    ) {
        if !self
            .radio
            .pending
            .as_ref()
            .is_some_and(|pending| pending.user == user && pending.serial == serial)
        {
            return;
        }
        let pending = self.radio.pending.take().unwrap();
        self.sync_next(false);
        if !self.radio_scope(&pending) {
            self.radio.message =
                Some("Stale recommendations discarded; queue left unchanged.".into());
            return;
        }
        let tracks = match result {
            Ok(tracks) => tracks
                .into_iter()
                .filter(|track| !self.unavailable.contains(&track.id))
                .collect(),
            Err(message) => {
                self.radio.stop(message);
                return;
            }
        };
        let limit = (MAX_ADDED - self.radio.added).min(50);
        let tracks = match self.queue.recommendation_candidates(tracks, limit) {
            Ok(tracks) if !tracks.is_empty() => tracks,
            Ok(_) => {
                self.radio
                    .stop("No new tracks were returned. Recommendation continuation stopped.");
                return;
            }
            Err(_) => {
                self.radio
                    .stop("The source cannot accept recommendations. Continuation stopped.");
                return;
            }
        };
        // Reconcile/revoke the callback boundary before validating the final append scope.
        self.sync_next(true);
        if !self.radio_scope(&pending) {
            return;
        }
        match self.queue_mut().append_recommendations(tracks, limit) {
            Ok(added) => {
                self.radio.added += added;
                self.radio.message = Some(format!(
                    "Added {added} track recommendations · {} of {MAX_ADDED} maximum · {} of {MAX_REQUESTS} requests used",
                    self.radio.added, self.radio.attempts
                ));
            }
            Err(_) => self
                .radio
                .stop("Queue capacity reached. Recommendation continuation stopped."),
        }
    }
    pub(super) fn radio_controls(&mut self, ui: &mut egui::Ui) {
        if ui
            .checkbox(
                &mut self.appearance.radio_continuation,
                "Continue with TIDAL track recommendations",
            )
            .changed()
        {
            self.radio.reset();
            self.save_appearance();
        }
        ui.label("Opt-in: up to 3 requests / 150 new tracks per source or explicit re-enable. Manual Up next keeps priority; repeat disables fetching. Late or failed results never restart stopped playback.");
        if self.appearance.radio_continuation
            && let Some(message) = &self.radio.message
        {
            ui.label(RichText::new(message).size(12.).color(MUTED));
        }
    }
}
