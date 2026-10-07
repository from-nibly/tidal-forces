//! A bounded, non-blocking PCM tap. Analysis runs on the UI thread, never in the audio callback.
use rodio::{Source, source::SeekError};
use serde::{Deserialize, Serialize};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering, fence},
    },
    time::Duration,
};

pub const SAMPLES: usize = 2048;
pub const BANDS: usize = 48;
pub const WAVE_POINTS: usize = 256;
const MAX_RATE: u32 = 48000;
const PUBLISH_EVERY: usize = 256;

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    #[default]
    Off,
    Spectrum,
    Waveform,
}
impl Mode {
    pub fn next(self) -> Self {
        match self {
            Self::Off => Self::Spectrum,
            Self::Spectrum => Self::Waveform,
            Self::Waveform => Self::Off,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::Spectrum => "Spectrum",
            Self::Waveform => "Waveform",
        }
    }
}

pub struct Frame {
    pub samples: [f32; SAMPLES],
    pub rate: u32,
    pub epoch: u64,
    pub serial: u64,
}

pub struct Capture {
    enabled: AtomicBool,
    active: AtomicBool,
    epoch: AtomicU64,
    sequence: AtomicU64,
    frame_epoch: AtomicU64,
    rate: AtomicU32,
    samples: [AtomicU32; SAMPLES],
}
impl Default for Capture {
    fn default() -> Self {
        Self {
            enabled: AtomicBool::new(false),
            active: AtomicBool::new(false),
            epoch: AtomicU64::new(1),
            sequence: AtomicU64::new(0),
            frame_epoch: AtomicU64::new(0),
            rate: AtomicU32::new(0),
            samples: std::array::from_fn(|_| AtomicU32::new(0)),
        }
    }
}
impl Capture {
    pub fn enable(&self, enabled: bool) {
        if self.enabled.swap(enabled, Ordering::Relaxed) != enabled {
            self.invalidate();
        }
    }
    pub fn playing(&self, playing: bool) {
        if self.active.swap(playing, Ordering::Relaxed) != playing {
            self.invalidate();
        }
    }
    pub fn invalidate(&self) {
        self.epoch.fetch_add(1, Ordering::AcqRel);
    }
    pub fn snapshot(&self) -> Option<Frame> {
        if !self.enabled.load(Ordering::Relaxed) || !self.active.load(Ordering::Relaxed) {
            return None;
        }
        // Atomic samples make even an interrupted read data-race-free. The sequence
        // check rejects torn frames; a busy writer costs a visual frame, never audio.
        for _ in 0..2 {
            let serial = self.sequence.load(Ordering::Acquire);
            if serial & 1 != 0 {
                continue;
            }
            let epoch = self.frame_epoch.load(Ordering::Relaxed);
            if epoch != self.epoch.load(Ordering::Acquire) {
                return None;
            }
            let frame = Frame {
                samples: std::array::from_fn(|i| {
                    f32::from_bits(self.samples[i].load(Ordering::Relaxed))
                }),
                rate: self.rate.load(Ordering::Relaxed),
                epoch,
                serial,
            };
            fence(Ordering::Acquire);
            if serial == self.sequence.load(Ordering::Relaxed)
                && epoch == self.epoch.load(Ordering::Acquire)
            {
                return Some(frame);
            }
        }
        None
    }
    fn publish(&self, samples: &[f32; SAMPLES], cursor: usize, rate: u32, epoch: u64) {
        // There is exactly one writer: the output device's source iterator.
        self.sequence.fetch_add(1, Ordering::AcqRel);
        // Pair with the reader's acquire fence through the atomic sample loads,
        // so observing any new sample also orders the final sequence check.
        fence(Ordering::Release);
        for (i, target) in self.samples.iter().enumerate() {
            target.store(samples[(cursor + i) % SAMPLES].to_bits(), Ordering::Relaxed);
        }
        self.rate.store(rate, Ordering::Relaxed);
        self.frame_epoch.store(epoch, Ordering::Relaxed);
        self.sequence.fetch_add(1, Ordering::Release);
    }
}

pub struct Tap<S> {
    inner: S,
    capture: Arc<Capture>,
    generation: Arc<AtomicU64>,
    id: u64,
    epoch: u64,
    channel: u16,
    channels: u16,
    rate: u32,
    mono: f32,
    sum: f32,
    count: u32,
    phase: u64,
    samples: [f32; SAMPLES],
    cursor: usize,
    pending: usize,
}
impl<S: Source<Item = f32>> Tap<S> {
    pub fn new(inner: S, capture: Arc<Capture>, generation: Arc<AtomicU64>, id: u64) -> Self {
        Self {
            channels: inner.channels().max(1),
            rate: inner.sample_rate().max(1),
            inner,
            capture,
            generation,
            id,
            epoch: 0,
            channel: 0,
            mono: 0.,
            sum: 0.,
            count: 0,
            phase: 0,
            samples: [0.; SAMPLES],
            cursor: 0,
            pending: 0,
        }
    }
    fn reset(&mut self, epoch: u64) {
        self.epoch = epoch;
        self.mono = 0.;
        self.sum = 0.;
        self.count = 0;
        self.phase = 0;
        self.samples.fill(0.);
        self.cursor = 0;
        self.pending = 0;
    }
}
impl<S: Source<Item = f32>> Iterator for Tap<S> {
    type Item = f32;
    fn next(&mut self) -> Option<f32> {
        let sample = self.inner.next()?;
        let enabled = self.capture.enabled.load(Ordering::Relaxed)
            && self.capture.active.load(Ordering::Relaxed);
        if self.channel == 0 {
            let channels = self.inner.channels().max(1);
            let rate = self.inner.sample_rate().max(1);
            let epoch = self.capture.epoch.load(Ordering::Acquire);
            if epoch != self.epoch || channels != self.channels || rate != self.rate {
                self.reset(epoch);
                self.channels = channels;
                self.rate = rate;
            }
        }
        if enabled {
            self.mono += if sample.is_finite() {
                sample.clamp(-1., 1.)
            } else {
                0.
            };
        }
        self.channel += 1;
        if self.channel == self.channels {
            self.channel = 0;
            if enabled {
                // Only the visualization is downsampled. Playback samples are returned
                // unchanged, before Sink applies volume. Average high-rate input frames.
                self.sum += self.mono / f32::from(self.channels);
                self.count += 1;
                self.phase += u64::from(self.rate.min(MAX_RATE));
                if self.phase >= u64::from(self.rate) {
                    self.phase -= u64::from(self.rate);
                    self.samples[self.cursor] = self.sum / self.count as f32;
                    self.sum = 0.;
                    self.count = 0;
                    self.cursor = (self.cursor + 1) % SAMPLES;
                    self.pending += 1;
                    if self.pending == PUBLISH_EVERY {
                        if self.generation.load(Ordering::Acquire) == self.id {
                            self.capture.publish(
                                &self.samples,
                                self.cursor,
                                self.rate.min(MAX_RATE),
                                self.epoch,
                            );
                        }
                        self.pending = 0;
                    }
                }
            }
            self.mono = 0.;
        }
        Some(sample)
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }
}
impl<S: Source<Item = f32>> Source for Tap<S> {
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
        self.capture.invalidate();
        self.channel = 0;
        self.reset(self.capture.epoch.load(Ordering::Acquire));
        Ok(())
    }
}

pub struct Analyzer {
    real: [f32; SAMPLES],
    imaginary: [f32; SAMPLES],
    window: [f32; SAMPLES],
    twiddles: [(f32, f32); SAMPLES / 2],
    targets: [f32; BANDS],
    pub bars: [f32; BANDS],
    pub peaks: [f32; BANDS],
    holds: [f32; BANDS],
    pub wave: [f32; WAVE_POINTS],
    last: Option<(u64, u64)>,
}
impl Default for Analyzer {
    fn default() -> Self {
        Self {
            real: [0.; SAMPLES],
            imaginary: [0.; SAMPLES],
            window: std::array::from_fn(|i| {
                0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / (SAMPLES - 1) as f32).cos()
            }),
            twiddles: std::array::from_fn(|i| {
                let (sin, cos) = (-std::f32::consts::TAU * i as f32 / SAMPLES as f32).sin_cos();
                (cos, sin)
            }),
            targets: [0.; BANDS],
            bars: [0.; BANDS],
            peaks: [0.; BANDS],
            holds: [0.; BANDS],
            wave: [0.; WAVE_POINTS],
            last: None,
        }
    }
}
impl Analyzer {
    pub fn clear(&mut self) {
        self.targets.fill(0.);
        self.bars.fill(0.);
        self.peaks.fill(0.);
        self.holds.fill(0.);
        self.wave.fill(0.);
        self.last = None;
    }
    pub fn update(&mut self, frame: &Frame, dt: f32, spectrum: bool) {
        if self.last.is_some_and(|(epoch, _)| epoch != frame.epoch) {
            self.clear();
        }
        if self.last != Some((frame.epoch, frame.serial)) {
            if spectrum {
                self.analyze(frame);
            }
            for (i, sample) in self.wave.iter_mut().enumerate() {
                let start = SAMPLES / 2 + i * (SAMPLES / 2 / WAVE_POINTS);
                *sample = frame.samples[start..start + (SAMPLES / 2 / WAVE_POINTS)]
                    .iter()
                    .sum::<f32>()
                    / (SAMPLES / 2 / WAVE_POINTS) as f32;
            }
            self.last = Some((frame.epoch, frame.serial));
        }
        let dt = dt.clamp(0., 0.1);
        for i in 0..BANDS {
            let tau = if self.targets[i] > self.bars[i] {
                0.025
            } else {
                0.18
            };
            self.bars[i] += (self.targets[i] - self.bars[i]) * (1. - (-dt / tau).exp());
            if self.bars[i] >= self.peaks[i] {
                self.peaks[i] = self.bars[i];
                self.holds[i] = 0.18;
            } else if self.holds[i] > 0. {
                self.holds[i] -= dt;
            } else {
                self.peaks[i] = (self.peaks[i] - dt * 0.6).max(self.bars[i]);
            }
        }
    }
    fn analyze(&mut self, frame: &Frame) {
        let mean = frame.samples.iter().sum::<f32>() / SAMPLES as f32;
        for i in 0..SAMPLES {
            let reversed = i.reverse_bits() >> (usize::BITS - SAMPLES.ilog2());
            self.real[reversed] = (frame.samples[i] - mean) * self.window[i];
        }
        self.imaginary.fill(0.);
        let mut length = 2;
        while length <= SAMPLES {
            let half = length / 2;
            for start in (0..SAMPLES).step_by(length) {
                for j in 0..half {
                    let (cos, sin) = self.twiddles[j * SAMPLES / length];
                    let a = start + j;
                    let b = a + half;
                    let re = self.real[b] * cos - self.imaginary[b] * sin;
                    let im = self.real[b] * sin + self.imaginary[b] * cos;
                    self.real[b] = self.real[a] - re;
                    self.imaginary[b] = self.imaginary[a] - im;
                    self.real[a] += re;
                    self.imaginary[a] += im;
                }
            }
            length *= 2;
        }
        let upper = 20000_f32.min(frame.rate as f32 * 0.48).max(31.);
        for (i, target) in self.targets.iter_mut().enumerate() {
            let low = 30. * (upper / 30.).powf(i as f32 / BANDS as f32);
            let high = 30. * (upper / 30.).powf((i + 1) as f32 / BANDS as f32);
            let first = ((low * SAMPLES as f32 / frame.rate as f32).floor() as usize)
                .clamp(1, SAMPLES / 2 - 1);
            let end = ((high * SAMPLES as f32 / frame.rate as f32).ceil() as usize)
                .clamp(first + 1, SAMPLES / 2);
            let amplitude = (first..end)
                .map(|bin| self.real[bin].hypot(self.imaginary[bin]))
                .fold(0., f32::max)
                * 4.
                / SAMPLES as f32;
            *target = ((20. * amplitude.max(0.00001).log10() + 65.) / 65.).clamp(0., 1.);
        }
    }
}

#[cfg(test)]
#[path = "visualizer_tests.rs"]
mod tests;
