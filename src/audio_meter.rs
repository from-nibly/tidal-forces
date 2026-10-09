//! Output-consumption accounting, independent of seek position and visualizer mode.
use rodio::{Source, source::SeekError};
use std::{
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

pub struct Meter<S> {
    inner: S,
    total: Arc<AtomicU64>,
    nanos: u64,
    remainder: u64,
    rate: u32,
    channel: u16,
    frames: u16,
}
impl<S: Source<Item = f32>> Meter<S> {
    pub fn new(inner: S, total: Arc<AtomicU64>) -> Self {
        Self {
            rate: inner.sample_rate().max(1),
            inner,
            total,
            nanos: 0,
            remainder: 0,
            channel: 0,
            frames: 0,
        }
    }
}
impl<S: Source<Item = f32>> Iterator for Meter<S> {
    type Item = f32;
    fn next(&mut self) -> Option<f32> {
        let rate = self.inner.sample_rate().max(1);
        let channels = self.inner.channels().max(1);
        let Some(sample) = self.inner.next() else {
            self.total.store(self.nanos, Ordering::Relaxed);
            return None;
        };
        if rate != self.rate {
            self.rate = rate;
            self.remainder = 0;
        }
        self.channel += 1;
        if self.channel >= channels {
            self.channel = 0;
            self.remainder += 1_000_000_000;
            self.nanos = self.nanos.saturating_add(self.remainder / u64::from(rate));
            self.remainder %= u64::from(rate);
            self.frames += 1;
            if self.frames == 256 {
                self.total.store(self.nanos, Ordering::Relaxed);
                self.frames = 0;
            }
        }
        Some(sample)
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }
}
impl<S: Source<Item = f32>> Source for Meter<S> {
    fn current_span_len(&self) -> Option<usize> {
        self.inner.current_span_len()
    }
    fn channels(&self) -> u16 {
        self.inner.channels()
    }
    fn sample_rate(&self) -> u32 {
        self.inner.sample_rate()
    }
    fn total_duration(&self) -> Option<Duration> {
        self.inner.total_duration()
    }
    fn try_seek(&mut self, position: Duration) -> Result<(), SeekError> {
        self.inner.try_seek(position)?;
        self.channel = 0;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn samples_are_unchanged_and_seeks_do_not_count_as_listening() {
        for channels in [1, 2] {
            for rate in [44100, 48000, 192000] {
                let samples: Vec<f32> = (0..rate * u32::from(channels) * 3)
                    .map(|n| (n % 31) as f32 / 31.)
                    .collect();
                let total = Arc::new(AtomicU64::new(0));
                let mut meter = Meter::new(
                    rodio::buffer::SamplesBuffer::new(channels, rate, samples.clone()),
                    total.clone(),
                );
                let first = meter
                    .by_ref()
                    .take(rate as usize * usize::from(channels))
                    .collect::<Vec<_>>();
                assert_eq!(first, samples[..first.len()]);
                meter.try_seek(Duration::from_secs(2)).unwrap();
                meter.by_ref().for_each(drop);
                assert_eq!(total.load(Ordering::Relaxed), 2_000_000_000);
            }
        }
    }
    #[test]
    fn sink_pause_produces_no_listening_credit() {
        let total = Arc::new(AtomicU64::new(0));
        let source = rodio::buffer::SamplesBuffer::new(1, 1000, vec![0.5; 10000]);
        let (sink, mut output) = rodio::Sink::new();
        sink.pause();
        sink.append(Meter::new(source, total.clone()));
        for _ in 0..1000 {
            assert_eq!(output.next(), Some(0.));
        }
        assert_eq!(total.load(Ordering::Relaxed), 0);
        sink.play();
        for _ in 0..1000 {
            output.next();
        }
        assert!(total.load(Ordering::Relaxed) > 0);
    }
}
