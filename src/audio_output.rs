use rodio::{OutputStream, OutputStreamBuilder, Sink, Source};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;

pub const LOST_MESSAGE: &str = "Output lost or unavailable. Choose your system output, then retry here or use Play in the player bar. No automatic restart.";

#[derive(Default)]
pub struct Recovery {
    epoch: u64,
    blocked: bool,
}
impl Recovery {
    pub fn fail(&mut self) {
        self.epoch = self.epoch.saturating_add(1);
        self.blocked = true;
    }
    pub fn epoch(&self) -> u64 {
        self.epoch
    }
    pub fn blocked(&self) -> bool {
        self.blocked
    }
    pub fn retry(&mut self, epoch: u64) {
        if epoch == self.epoch {
            self.blocked = false;
        }
    }
}

pub struct Output {
    stream: Option<OutputStream>,
    pub sink: Sink,
    failed: Arc<AtomicBool>,
}
impl Default for Output {
    fn default() -> Self {
        Self {
            stream: None,
            sink: Sink::new().0,
            failed: Arc::new(AtomicBool::new(false)),
        }
    }
}
impl Output {
    pub fn reset(&mut self, volume: f32) -> Result<(), rodio::StreamError> {
        if self.stream.is_none() {
            let failed = Arc::new(AtomicBool::new(false));
            let signal = failed.clone();
            // Unlike open_default_stream, this never searches other devices.
            let mut stream = OutputStreamBuilder::from_default_device()?
                .with_error_callback(move |_| {
                    signal.store(true, Ordering::Release);
                })
                .open_stream()?;
            stream.log_on_drop(false);
            self.failed = failed;
            self.stream = Some(stream);
        }
        self.sink.stop();
        self.sink = Sink::connect_new(self.stream.as_ref().unwrap().mixer());
        self.sink.set_volume(volume);
        self.sink.pause();
        Ok(())
    }
    #[cfg(test)]
    pub fn simulate_failure(&self) {
        self.failed.store(true, Ordering::Release);
    }
    pub fn failed(&self) -> bool {
        self.stream.is_some() && self.failed.load(Ordering::Acquire)
    }
    pub fn close(&mut self) {
        self.sink.stop();
        self.sink = Sink::new().0;
        self.stream.take();
    }
    pub fn append(&self, source: impl Source<Item = f32> + Send + 'static) {
        self.sink.append(Gate {
            source,
            failed: self.failed.clone(),
            channel: 0,
        });
    }
}

struct Gate<S> {
    source: S,
    failed: Arc<AtomicBool>,
    channel: u16,
}
impl<S: Source<Item = f32>> Iterator for Gate<S> {
    type Item = f32;
    fn next(&mut self) -> Option<f32> {
        if self.channel == 0 && self.failed.load(Ordering::Acquire) {
            return None;
        }
        let sample = self.source.next()?;
        self.channel = (self.channel + 1) % self.source.channels();
        Some(sample)
    }
}
impl<S: Source<Item = f32>> Source for Gate<S> {
    fn current_span_len(&self) -> Option<usize> {
        self.source.current_span_len()
    }
    fn channels(&self) -> u16 {
        self.source.channels()
    }
    fn sample_rate(&self) -> u32 {
        self.source.sample_rate()
    }
    fn total_duration(&self) -> Option<Duration> {
        self.source.total_duration()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn recovery_requires_the_current_fault_epoch_not_a_new_playback_generation() {
        let mut recovery = Recovery::default();
        recovery.fail();
        let first = recovery.epoch();
        for _ in 0..20 {
            assert!(recovery.blocked());
        }
        recovery.retry(first - 1);
        assert!(recovery.blocked());
        recovery.retry(first);
        assert!(!recovery.blocked());
        recovery.fail();
        recovery.retry(first);
        assert!(
            recovery.blocked(),
            "A stale retry cannot reopen a newer failed output"
        );
        recovery.retry(recovery.epoch());
        assert!(!recovery.blocked());
    }
    #[test]
    fn fault_stops_before_another_frame_without_splitting_current_frame() {
        let failed = Arc::new(AtomicBool::new(false));
        let mut gate = Gate {
            source: rodio::buffer::SamplesBuffer::new(2, 48000, vec![0.25, 0.75, 1., 1.]),
            failed: failed.clone(),
            channel: 0,
        };
        assert_eq!(gate.next(), Some(0.25));
        failed.store(true, Ordering::Release);
        assert_eq!(gate.next(), Some(0.75));
        assert_eq!(gate.next(), None);
        assert_eq!(
            gate.source.next(),
            Some(1.),
            "Do not consume failed-output PCM"
        );
    }
    #[test]
    fn old_stream_fault_does_not_poison_a_new_stream_signal() {
        let old = Arc::new(AtomicBool::new(false));
        let new = Arc::new(AtomicBool::new(false));
        let gate = Gate {
            source: rodio::buffer::SamplesBuffer::new(1, 48000, vec![0.5; 8]),
            failed: new,
            channel: 0,
        };
        old.store(true, Ordering::Release);
        assert_eq!(gate.collect::<Vec<_>>(), vec![0.5; 8]);
    }
}
