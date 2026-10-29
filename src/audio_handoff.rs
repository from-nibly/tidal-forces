//! A bounded, non-waiting PCM boundary. Decoder/file ownership stays in audio.rs.
#[cfg(test)]
#[path = "audio_handoff_tests.rs"]
mod tests;
use crate::{audio_meter::Meter, pcm, visualizer};
use rodio::Source;
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicU8, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

const PREPARING: u8 = 0;
const READY: u8 = 1;
const CLAIMED: u8 = 2;
const CANCELLED: u8 = 3;

pub struct Boundary {
    pub from: u64,
    pub to: u64,
    expected_end: u64,
    phase: AtomicU8,
    elapsed: AtomicU64,
}
impl Boundary {
    pub fn new(from: u64, to: u64, expected_end: u64) -> Self {
        Self {
            from,
            to,
            expected_end,
            phase: AtomicU8::new(PREPARING),
            elapsed: AtomicU64::new(0),
        }
    }
    #[cfg(test)]
    pub fn fixture_ready(&self) {
        self.phase.store(READY, Ordering::Release);
    }
    #[cfg(test)]
    pub fn fixture_claim(&self, current: &AtomicU64, elapsed: Duration) -> bool {
        self.elapsed.store(nanos(elapsed), Ordering::Relaxed);
        if self
            .phase
            .compare_exchange(READY, CLAIMED, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return false;
        }
        current
            .compare_exchange(self.from, self.to, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
    }
    pub fn pending(&self) -> bool {
        matches!(self.phase.load(Ordering::Acquire), PREPARING | READY)
    }
    pub fn preparing(&self) -> bool {
        self.phase.load(Ordering::Acquire) == PREPARING
    }
    pub fn claimed(&self) -> bool {
        self.phase.load(Ordering::Acquire) == CLAIMED
    }
    pub fn elapsed(&self) -> Duration {
        Duration::from_nanos(self.elapsed.load(Ordering::Acquire))
    }
    /// False means the boundary won: reconcile it before modifying the queue.
    pub fn cancel(&self) -> bool {
        let mut phase = self.phase.load(Ordering::Acquire);
        loop {
            match phase {
                CLAIMED => return false,
                CANCELLED => return true,
                _ => match self.phase.compare_exchange_weak(
                    phase,
                    CANCELLED,
                    Ordering::AcqRel,
                    Ordering::Acquire,
                ) {
                    Ok(_) => return true,
                    Err(observed) => phase = observed,
                },
            }
        }
    }
    pub fn force_cancel(&self) {
        self.phase.store(CANCELLED, Ordering::Release);
    }
    fn claim(&self, previous: &Frame, next: &Frame, generation: &AtomicU64) -> bool {
        if generation.load(Ordering::Acquire) != self.from
            || previous.feedback.failed()
            || previous.position().saturating_add(3) < self.expected_end
            || previous.format != next.format
            || !next.feedback.ready()
        {
            return false;
        }
        self.elapsed
            .store(nanos(previous.elapsed()), Ordering::Relaxed);
        next.started
            .store(nanos(next.origin.elapsed()), Ordering::Relaxed);
        if self
            .phase
            .compare_exchange(READY, CLAIMED, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return false;
        }
        // UI reconciliation can finish this CAS first. A newer manual generation
        // must never be overwritten, even if it superseded a claimed boundary.
        let _ =
            generation.compare_exchange(self.from, self.to, Ordering::AcqRel, Ordering::Acquire);
        generation.load(Ordering::Acquire) == self.to && self.claimed()
    }
}
fn nanos(duration: Duration) -> u64 {
    duration.as_nanos().min(u128::from(u64::MAX)) as u64
}

pub struct Frame {
    pub id: u64,
    pub feedback: pcm::Feedback,
    pub rendered: Arc<AtomicU64>,
    pub format: (u16, u32),
    offset: u64,
    origin: Instant,
    started: AtomicU64,
}
impl Frame {
    pub fn new(id: u64, pcm: &pcm::PcmSource, feedback: pcm::Feedback, offset: u64) -> Arc<Self> {
        Arc::new(Self {
            id,
            feedback,
            rendered: Arc::new(AtomicU64::new(0)),
            format: (pcm.channels(), pcm.sample_rate()),
            offset,
            origin: Instant::now(),
            started: AtomicU64::new(0),
        })
    }
    pub fn rendered(&self) -> Duration {
        Duration::from_nanos(self.rendered.load(Ordering::Acquire))
    }
    pub fn elapsed(&self) -> Duration {
        self.origin
            .elapsed()
            .saturating_sub(Duration::from_nanos(self.started.load(Ordering::Acquire)))
    }
    pub fn position(&self) -> u64 {
        self.rendered().as_secs().saturating_add(self.offset)
    }
}

type Samples = visualizer::Tap<Meter<pcm::PcmSource>>;
struct Track {
    samples: Samples,
    frame: Arc<Frame>,
}
impl Track {
    fn new(
        pcm: pcm::PcmSource,
        frame: Arc<Frame>,
        capture: Arc<visualizer::Capture>,
        generation: Arc<AtomicU64>,
    ) -> Self {
        let samples = visualizer::Tap::new(
            Meter::with_activity(pcm, frame.rendered.clone(), frame.feedback.activity()),
            capture,
            generation,
            frame.id,
        );
        Self { samples, frame }
    }
}
struct Next {
    track: Track,
    boundary: Arc<Boundary>,
}
#[derive(Clone, Default)]
pub struct Slot(Arc<Mutex<Option<Next>>>);
impl Slot {
    pub fn arm(
        &self,
        pcm: pcm::PcmSource,
        frame: Arc<Frame>,
        boundary: Arc<Boundary>,
        capture: Arc<visualizer::Capture>,
        generation: Arc<AtomicU64>,
    ) -> bool {
        let mut slot = self.0.lock().unwrap();
        *slot = Some(Next {
            track: Track::new(pcm, frame, capture, generation),
            boundary: boundary.clone(),
        });
        if boundary
            .phase
            .compare_exchange(PREPARING, READY, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            *slot = None;
            return false;
        }
        true
    }
    pub fn clear(&self) {
        self.0.lock().unwrap().take();
    }
}
pub struct Chain {
    track: Track,
    slot: Slot,
    generation: Arc<AtomicU64>,
    capture: Arc<visualizer::Capture>,
    channel: u16,
}
impl Chain {
    pub fn new(
        pcm: pcm::PcmSource,
        frame: Arc<Frame>,
        slot: Slot,
        capture: Arc<visualizer::Capture>,
        generation: Arc<AtomicU64>,
    ) -> Self {
        Self {
            track: Track::new(pcm, frame, capture.clone(), generation.clone()),
            slot,
            capture,
            generation,
            channel: 0,
        }
    }
}
impl Iterator for Chain {
    type Item = f32;
    fn next(&mut self) -> Option<f32> {
        if self.channel == 0 && self.generation.load(Ordering::Acquire) != self.track.frame.id {
            return None;
        }
        let sample = if let Some(sample) = self.track.samples.next() {
            sample
        } else {
            let next = self.slot.0.try_lock().ok()?.take()?;
            if !next
                .boundary
                .claim(&self.track.frame, &next.track.frame, &self.generation)
            {
                return None;
            }
            self.capture.invalidate();
            self.track = next.track;
            self.track.samples.next()?
        };
        self.channel = (self.channel + 1) % self.track.frame.format.0;
        Some(sample)
    }
}
impl Source for Chain {
    fn current_span_len(&self) -> Option<usize> {
        None
    }
    fn channels(&self) -> u16 {
        self.track.frame.format.0
    }
    fn sample_rate(&self) -> u32 {
        self.track.frame.format.1
    }
    fn total_duration(&self) -> Option<Duration> {
        None
    }
}
