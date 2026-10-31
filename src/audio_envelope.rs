use rodio::Source;
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU32, Ordering},
};
use std::time::Duration;

pub struct Control {
    volume: AtomicU32,
    paused: AtomicBool,
    silent: AtomicBool,
}
impl Default for Control {
    fn default() -> Self {
        Self {
            volume: AtomicU32::new(1f32.to_bits()),
            paused: AtomicBool::new(true),
            silent: AtomicBool::new(true),
        }
    }
}
impl Control {
    pub fn volume(&self, value: f32) {
        self.volume.store(
            if value.is_finite() {
                value.clamp(0., 1.).to_bits()
            } else {
                0f32.to_bits()
            },
            Ordering::Release,
        );
    }
    pub fn pause(&self, paused: bool) {
        if !paused {
            self.silent.store(false, Ordering::Release);
        }
        self.paused.store(paused, Ordering::Release);
    }
    pub fn stopped(&self) -> bool {
        self.paused.load(Ordering::Acquire) && self.silent.load(Ordering::Acquire)
    }
}
/// Complete-frame gain ramp. It never reads the source once a pause reaches silence.
/// A compatible prepared transition stays inside this same envelope, with no fade at its boundary.
pub struct Envelope<S> {
    source: S,
    control: Arc<Control>,
    channel: u16,
    gain: f32,
    target: f32,
    step: f32,
    remaining: u32,
    ramp: u32,
    paused_frame: bool,
}
impl<S: Source<Item = f32>> Envelope<S> {
    pub fn new(source: S, control: Arc<Control>) -> Self {
        let ramp = (source.sample_rate() / 200).max(1);
        Self {
            source,
            control,
            channel: 0,
            gain: 0.,
            target: 0.,
            step: 0.,
            remaining: 0,
            ramp,
            paused_frame: true,
        }
    }
}
impl<S: Source<Item = f32>> Iterator for Envelope<S> {
    type Item = f32;
    fn next(&mut self) -> Option<f32> {
        if self.channel == 0 {
            self.paused_frame = self.control.paused.load(Ordering::Acquire);
            let target = if self.paused_frame {
                0.
            } else {
                f32::from_bits(self.control.volume.load(Ordering::Acquire))
            };
            if target != self.target {
                self.target = target;
                self.remaining = self.ramp;
                self.step = (target - self.gain) / self.ramp as f32;
            }
            if self.remaining > 0 {
                self.remaining -= 1;
                self.gain = if self.remaining == 0 {
                    self.target
                } else {
                    self.gain + self.step
                };
            }
        }
        let silent = self.paused_frame && self.gain == 0.;
        let sample = if silent { 0. } else { self.source.next()? };
        self.channel = (self.channel + 1) % self.source.channels();
        if self.channel == 0 {
            self.control.silent.store(silent, Ordering::Release);
        }
        Some(if self.gain == 1. {
            sample
        } else {
            sample * self.gain
        })
    }
}
impl<S: Source<Item = f32>> Source for Envelope<S> {
    fn channels(&self) -> u16 {
        self.source.channels()
    }
    fn sample_rate(&self) -> u32 {
        self.source.sample_rate()
    }
    fn current_span_len(&self) -> Option<usize> {
        self.source.current_span_len()
    }
    fn total_duration(&self) -> Option<Duration> {
        self.source.total_duration()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ramps_are_frame_aligned_and_paused_frames_do_not_consume_audio() {
        let control = Arc::new(Control::default());
        let source = rodio::buffer::SamplesBuffer::new(2, 48000, vec![1.; 10000]);
        let mut envelope = Envelope::new(source, control.clone());
        assert!(envelope.by_ref().take(100).all(|v| v == 0.));
        control.pause(false);
        let start: Vec<_> = envelope.by_ref().take(480).collect();
        assert!(start.as_chunks::<2>().0.iter().all(|v| v[0] == v[1]));
        assert!(start.windows(2).all(|v| v[1] >= v[0]));
        assert_eq!(*start.last().unwrap(), 1.);
        control.volume(0.5);
        let volume: Vec<_> = envelope.by_ref().take(480).collect();
        assert!(volume.windows(2).all(|v| v[1] <= v[0]));
        assert_eq!(*volume.last().unwrap(), 0.5);
        control.pause(true);
        let pause: Vec<_> = envelope.by_ref().take(480).collect();
        assert!(pause.windows(2).all(|v| v[1] <= v[0]));
        assert_eq!(*pause.last().unwrap(), 0.);
        assert!(control.stopped());
        let left = envelope.source.clone().count();
        assert!(envelope.by_ref().take(2000).all(|v| v == 0.));
        assert_eq!(envelope.source.clone().count(), left);
        control.pause(false);
        assert!(envelope.next().unwrap() > 0.);
        assert!(!control.stopped());
    }
    #[test]
    fn pause_and_resume_at_zero_volume_never_split_a_source_frame() {
        let control = Arc::new(Control::default());
        control.volume(0.);
        control.pause(false);
        let source = rodio::buffer::SamplesBuffer::new(2, 48000, vec![0.25, 0.75, 0.5, 1.]);
        let mut envelope = Envelope::new(source, control.clone());
        assert_eq!(envelope.next(), Some(0.));
        control.pause(true);
        assert_eq!(envelope.next(), Some(0.));
        assert_eq!(envelope.source.clone().count(), 2);
        assert_eq!(envelope.next(), Some(0.));
        control.pause(false);
        assert_eq!(envelope.next(), Some(0.));
        assert_eq!(envelope.source.clone().count(), 2);
        assert_eq!(envelope.next(), Some(0.));
        assert_eq!(envelope.next(), Some(0.));
        assert_eq!(envelope.source.count(), 0);
    }

    #[test]
    fn steady_unity_preserves_bits_without_limiting() {
        let control = Arc::new(Control::default());
        control.pause(false);
        let mut samples = vec![0.; 480];
        let tail = vec![-0., 0., 1.25, -1.25];
        samples.extend(&tail);
        let actual: Vec<_> = Envelope::new(
            rodio::buffer::SamplesBuffer::new(2, 48000, samples),
            control,
        )
        .skip(480)
        .collect();
        assert_eq!(
            actual.iter().map(|v| v.to_bits()).collect::<Vec<_>>(),
            tail.iter().map(|v| v.to_bits()).collect::<Vec<_>>()
        );
    }
}
