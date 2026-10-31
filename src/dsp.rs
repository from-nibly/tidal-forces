use rodio::Source;
use serde::{Deserialize, Serialize};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU64, Ordering},
};
use std::time::Duration;

pub const FREQUENCIES: [f64; 10] = [
    31.25, 62.5, 125., 250., 500., 1000., 2000., 4000., 8000., 16000.,
];

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub enum Normalization {
    #[default]
    Off,
    Track,
    Album,
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub enabled: bool,
    pub bands: [f32; 10],
    pub preamp_db: f32,
    pub balance: f32,
    pub mono: bool,
    pub normalization: Normalization,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            enabled: false,
            bands: [0.; 10],
            preamp_db: 0.,
            balance: 0.,
            mono: false,
            normalization: Normalization::Off,
        }
    }
}
impl Settings {
    pub fn continuous(self, previous: Metadata, next: Metadata) -> bool {
        !self.enabled
            || (self.bands.iter().all(|gain| *gain == 0.)
                && match self.normalization {
                    Normalization::Off => true,
                    Normalization::Track => previous.track == next.track,
                    Normalization::Album => previous.album == next.album,
                })
    }
    pub fn validated(mut self) -> Self {
        fn bounded(x: f32, min: f32, max: f32) -> f32 {
            if x.is_finite() { x.clamp(min, max) } else { 0. }
        }
        for band in &mut self.bands {
            *band = bounded(*band, -12., 12.);
        }
        self.preamp_db = bounded(self.preamp_db, -24., 12.);
        self.balance = bounded(self.balance, -1., 1.);
        self
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Gain {
    db: f32,
    peak: f32,
}
impl Gain {
    pub fn from_replay_gain_db(db: f32) -> Option<Self> {
        // TIDAL's SDK uses +4 dB preamp and caps the multiplier at unity.
        // This is an attenuation policy, not an invented provider peak value.
        if !db.is_finite() || !(-60. ..=24.).contains(&db) {
            return None;
        }
        Self::from_db_and_linear_peak((db + 4.).min(0.), 1.)
    }
    // Callers must establish provider units before constructing this value.
    pub fn from_db_and_linear_peak(db: f32, peak: f32) -> Option<Self> {
        (db.is_finite()
            && (-60. ..=24.).contains(&db)
            && peak.is_finite()
            && peak > 0.
            && peak <= 16.)
            .then_some(Self { db, peak })
    }
    fn multiplier(self) -> f64 {
        10f64
            .powf(f64::from(self.db) / 20.)
            .min(1. / f64::from(self.peak))
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Metadata {
    pub track: Option<Gain>,
    pub album: Option<Gain>,
}
impl Metadata {
    pub fn available(self, mode: Normalization) -> bool {
        match mode {
            Normalization::Off => true,
            Normalization::Track => self.track.is_some(),
            Normalization::Album => self.album.is_some(),
        }
    }
    fn gain(self, mode: Normalization) -> f64 {
        match mode {
            Normalization::Off => None,
            Normalization::Track => self.track,
            Normalization::Album => self.album,
        }
        .map_or(1., Gain::multiplier)
    }
}

#[derive(Default)]
pub struct Control {
    settings: Mutex<Settings>,
    revision: AtomicU64,
}
impl Control {
    pub fn set(&self, settings: Settings) {
        *self.settings.lock().unwrap() = settings.validated();
        self.revision.fetch_add(1, Ordering::Release);
    }
    pub fn get(&self) -> Settings {
        *self.settings.lock().unwrap()
    }
}

#[derive(Clone, Copy)]
struct Coefficients {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
}
impl Coefficients {
    const IDENTITY: Self = Self {
        b0: 1.,
        b1: 0.,
        b2: 0.,
        a1: 0.,
        a2: 0.,
    };
    fn peak(rate: u32, frequency: f64, gain: f32) -> Self {
        if gain == 0. || frequency >= f64::from(rate) / 2. {
            return Self::IDENTITY;
        }
        let a = 10f64.powf(f64::from(gain) / 40.);
        let w = std::f64::consts::TAU * frequency / f64::from(rate);
        let alpha = w.sin() / (2. * std::f64::consts::SQRT_2);
        let a0 = 1. + alpha / a;
        Self {
            b0: (1. + alpha * a) / a0,
            b1: -2. * w.cos() / a0,
            b2: (1. - alpha * a) / a0,
            a1: -2. * w.cos() / a0,
            a2: (1. - alpha / a) / a0,
        }
    }
}
#[derive(Clone, Copy, Default)]
struct State {
    z1: f64,
    z2: f64,
}
impl State {
    fn process(&mut self, c: Coefficients, x: f64) -> f64 {
        let y = c.b0 * x + self.z1;
        self.z1 = c.b1 * x - c.a1 * y + self.z2;
        self.z2 = c.b2 * x - c.a2 * y;
        y
    }
}
struct Processor {
    active: bool,
    coefficients: [Coefficients; 10],
    states: [[State; 10]; 8],
    gain: f64,
    balance: f32,
    mono: bool,
}
impl Processor {
    fn new(settings: Settings, rate: u32, channels: u16, metadata: Metadata) -> Self {
        let settings = settings.validated();
        let bands = std::array::from_fn::<_, 10, _>(|i| {
            if settings.enabled && FREQUENCIES[i] < f64::from(rate) / 2. {
                settings.bands[i]
            } else {
                0.
            }
        });
        let headroom: f64 = bands.iter().map(|v| f64::from(v.max(0.))).sum();
        let gain = if settings.enabled {
            metadata.gain(settings.normalization)
                * 10f64.powf((f64::from(settings.preamp_db) - headroom) / 20.)
        } else {
            1.
        };
        let balance = if settings.enabled && channels == 2 {
            settings.balance
        } else {
            0.
        };
        let mono = settings.enabled && channels == 2 && settings.mono;
        Self {
            active: gain != 1.
                || balance != 0.
                || mono
                || bands.iter().any(|x| *x != 0.)
                || (settings.enabled
                    && settings.normalization != Normalization::Off
                    && metadata.available(settings.normalization)),
            coefficients: std::array::from_fn(|i| {
                Coefficients::peak(rate, FREQUENCIES[i], bands[i])
            }),
            states: [[State::default(); 10]; 8],
            gain,
            balance,
            mono,
        }
    }
    fn frame(&mut self, samples: &mut [f32]) {
        if !self.active {
            return;
        }
        if self.mono {
            let mono = samples[0] * 0.5 + samples[1] * 0.5;
            samples[0] = mono;
            samples[1] = mono;
        }
        for (channel, sample) in samples.iter_mut().enumerate() {
            let mut x = f64::from(*sample) * self.gain;
            for (state, coefficient) in self.states[channel].iter_mut().zip(self.coefficients) {
                x = state.process(coefficient, x);
            }
            if channel == 0 && self.balance > 0. {
                x *= f64::from(1. - self.balance);
            }
            if channel == 1 && self.balance < 0. {
                x *= f64::from(1. + self.balance);
            }
            // Only the explicitly processed path limits peaks; bypass stays bit-identical.
            *sample = x.clamp(-1., 1.) as f32;
        }
    }
}

/// Runs on the existing decoder worker, before the bounded PCM ring.
/// Control reads/coefficient calculation never enter the audio callback.
pub struct Processed<S> {
    source: S,
    control: Arc<Control>,
    revision: u64,
    processor: Processor,
    pending: Option<Processor>,
    last_settings: Settings,
    ramp_frame: u32,
    metadata: Metadata,
    channels: u16,
    rate: u32,
    frame: [f32; 8],
    index: usize,
    frames: u64,
    ended: bool,
}
impl<S: Source<Item = f32>> Processed<S> {
    pub fn new(source: S, control: Arc<Control>, metadata: Metadata) -> anyhow::Result<Self> {
        let channels = source.channels();
        let rate = source.sample_rate();
        anyhow::ensure!(
            (1..=8).contains(&channels) && rate > 0 && rate <= 384000,
            "Unsupported processing format"
        );
        let revision = control.revision.load(Ordering::Acquire);
        let last_settings = control.get();
        let processor = Processor::new(last_settings, rate, channels, metadata);
        Ok(Self {
            source,
            control,
            revision,
            processor,
            pending: None,
            last_settings,
            ramp_frame: 0,
            metadata,
            channels,
            rate,
            frame: [0.; 8],
            index: usize::from(channels),
            frames: 0,
            ended: false,
        })
    }
    fn failed(&mut self) -> Option<f32> {
        self.ended = true;
        Some(f32::NAN)
    }
}
impl<S: Source<Item = f32>> Iterator for Processed<S> {
    type Item = f32;
    fn next(&mut self) -> Option<f32> {
        if self.ended {
            return None;
        }
        if self.index == usize::from(self.channels) {
            if self.source.channels() != self.channels || self.source.sample_rate() != self.rate {
                return self.failed();
            }
            for i in 0..usize::from(self.channels) {
                match self.source.next() {
                    Some(sample) if sample.is_finite() => self.frame[i] = sample,
                    None if i == 0 => {
                        self.ended = true;
                        return None;
                    }
                    _ => return self.failed(),
                }
            }
            if self.source.channels() != self.channels || self.source.sample_rate() != self.rate {
                return self.failed();
            }
            if self.frames.is_multiple_of(256) && self.pending.is_none() {
                let revision = self.control.revision.load(Ordering::Acquire);
                if revision != self.revision {
                    let settings = self.control.get();
                    if settings != self.last_settings {
                        self.pending = Some(Processor::new(
                            settings,
                            self.rate,
                            self.channels,
                            self.metadata,
                        ));
                        self.last_settings = settings;
                        self.ramp_frame = 0;
                    }
                    self.revision = revision;
                }
            }
            let mut changed = self.frame;
            let channels = usize::from(self.channels);
            self.processor.frame(&mut self.frame[..channels]);
            if let Some(pending) = &mut self.pending {
                pending.frame(&mut changed[..channels]);
                self.ramp_frame += 1;
                let duration = (self.rate / 200).max(1);
                let weight = f64::from(self.ramp_frame) / f64::from(duration);
                for (old, new) in self.frame[..channels].iter_mut().zip(&changed[..channels]) {
                    if old.to_bits() != new.to_bits() {
                        *old =
                            (f64::from(*old) + (f64::from(*new) - f64::from(*old)) * weight) as f32;
                    }
                }
                if self.ramp_frame == duration {
                    self.processor = self.pending.take().unwrap();
                }
            }
            self.frames += 1;
            self.index = 0;
        }
        let sample = self.frame[self.index];
        self.index += 1;
        Some(sample)
    }
}
impl<S: Source<Item = f32>> Source for Processed<S> {
    fn channels(&self) -> u16 {
        self.channels
    }
    fn sample_rate(&self) -> u32 {
        self.rate
    }
    fn current_span_len(&self) -> Option<usize> {
        None
    }
    fn total_duration(&self) -> Option<Duration> {
        self.source.total_duration()
    }
}

#[cfg(test)]
#[path = "dsp_tests.rs"]
mod tests;
