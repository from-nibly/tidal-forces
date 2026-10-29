use super::*;
#[cfg(test)]
#[path = "playback_next_tests.rs"]
mod tests;
use crate::audio::{NextKey, Transition};
use std::ops::Deref;

// No DerefMut: queue changes must first reconcile/cancel a callback boundary.
pub(super) struct PlaybackQueue(Queue);
impl From<Queue> for PlaybackQueue {
    fn from(queue: Queue) -> Self {
        Self(queue)
    }
}
impl Deref for PlaybackQueue {
    type Target = Queue;
    fn deref(&self) -> &Queue {
        &self.0
    }
}
impl App {
    pub(super) fn queue_mut(&mut self) -> &mut Queue {
        self.sync_next(true);
        self.next_attempt = None;
        &mut self.queue.0
    }
    pub(super) fn replace_queue(&mut self, queue: Queue) {
        self.sync_next(true);
        self.next_attempt = None;
        self.queue = queue.into();
    }
    pub(super) fn sync_next(&mut self, cancel: bool) {
        if let Some(transition) = self.backend.player.reconcile_next(cancel) {
            self.accept_transition(transition);
        }
    }
    fn accept_transition(&mut self, transition: Transition) {
        let key = &transition.key;
        if self.play_generation != key.generation
            || self.account != Some(key.user)
            || self.queue.revision() != key.revision
            || self.queue.current_entry().map(|e| e.occurrence) != Some(key.current)
            || self
                .queue
                .next_natural()
                .map(|e| (e.occurrence, e.track.id))
                != Some((key.occurrence, key.track))
            || self.quality != key.quality
            || !self.backend.player.current(transition.generation)
        {
            if self.backend.player.current(transition.generation) {
                self.play_generation = self.backend.player.reserve();
                self.paused = true;
                self.buffering = false;
                self.actual_quality.clear();
                self.error = Some(
                    "The playback queue changed. Press play to resume the selected song.".into(),
                );
            }
            return;
        }
        self.observe_listening(key.generation, transition.rendered, transition.elapsed);
        let advanced = self.queue.0.advance(true);
        debug_assert!(advanced);
        self.play_generation = transition.generation;
        self.position = transition.position;
        self.actual_quality = transition.quality;
        self.buffering = false;
        self.starved_generation = None;
        self.ended = false;
        self.waiting_context = false;
        self.next_attempt = None;
        self.error = None;
        self.backend.player.pause(self.paused);
    }
    pub(super) fn prefetch_audio(&mut self) {
        self.sync_next(false);
        if self.paused || self.ended || self.audio_buffering() || self.actual_quality.is_empty() {
            if self.audio_buffering() || self.actual_quality.is_empty() {
                self.sync_next(true);
            }
            return;
        }
        let (Some(user), Some(current), Some(next)) = (
            self.account,
            self.queue.current_entry(),
            self.queue.next_natural(),
        ) else {
            return;
        };
        if current.track.duration == 0 || current.track.duration.saturating_sub(self.position) > 30
        {
            return;
        }
        let key = NextKey {
            user,
            generation: self.play_generation,
            revision: self.queue.revision(),
            current: current.occurrence,
            occurrence: next.occurrence,
            track: next.track.id,
            quality: self.quality.clone(),
            duration: current.track.duration,
        };
        if self.next_budget.0 != self.play_generation {
            self.next_budget = (self.play_generation, 0);
        }
        if self.next_attempt.as_ref() == Some(&key) || self.next_budget.1 >= 2 {
            return;
        }
        self.next_attempt = Some(key.clone());
        if let Some(request) = self.backend.player.begin_next(key) {
            self.next_budget.1 += 1;
            if self
                .backend
                .tx
                .try_send(Request::PrepareNext(request.clone()))
                .is_err()
            {
                self.backend.player.fail_next(&request);
            }
        }
    }
}
