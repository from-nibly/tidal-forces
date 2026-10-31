use rodio::{OutputStream, OutputStreamBuilder, Sink, Source};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;

pub const LOST_MESSAGE: &str = "Output lost or changed. Choose an output in Playback settings, then retry here or use Play in the player bar. No automatic restart.";

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

enum Stream {
    Compatibility(OutputStream),
    #[cfg(target_os = "linux")]
    Native(crate::pulse_output::Driver),
}
impl Stream {
    fn mixer(&self) -> &rodio::mixer::Mixer {
        match self {
            Self::Compatibility(stream) => stream.mixer(),
            #[cfg(target_os = "linux")]
            Self::Native(stream) => &stream.mixer,
        }
    }
}
pub struct Output {
    stream: Option<Stream>,
    target: crate::output_devices::Target,
    envelope: Arc<crate::audio_envelope::Control>,
    pub sink: Sink,
    failed: Arc<AtomicBool>,
}
impl Default for Output {
    fn default() -> Self {
        Self {
            stream: None,
            target: crate::output_devices::Target::default(),
            envelope: Arc::new(crate::audio_envelope::Control::default()),
            sink: Sink::new().0,
            failed: Arc::new(AtomicBool::new(false)),
        }
    }
}
impl Output {
    pub fn select(&mut self, target: crate::output_devices::Target) {
        if self.target != target {
            self.target = target;
            if self.stream.is_some() {
                self.failed.store(true, Ordering::Release);
            }
        }
    }
    pub fn description(&self) -> String {
        match &self.stream {
            Some(Stream::Compatibility(stream)) => format!(
                "Compatibility output · {} Hz · {} channels · {:?}. Physical route protection unavailable.",
                stream.config().sample_rate(),
                stream.config().channel_count(),
                stream.config().sample_format()
            ),
            #[cfg(target_os = "linux")]
            Some(Stream::Native(stream)) => format!(
                "{} · 48 kHz / 32-bit float / stereo engine · sink lock requested · {}. DAC format and zero-tail unplug behavior are not verified.",
                stream.label,
                if stream.port_observed {
                    "physical port monitored"
                } else {
                    "physical port not exposed"
                }
            ),
            None => "Output closed · choose an output, then use Play to open it.".into(),
        }
    }
    pub fn pause(&self, paused: bool) {
        self.envelope.pause(paused);
        if !paused {
            self.sink.play();
            #[cfg(target_os = "linux")]
            if let Some(Stream::Native(stream)) = &self.stream {
                stream.pause(false);
            }
        }
    }
    pub fn sync_pause(&self) {
        if self.envelope.stopped() {
            self.sink.pause();
            #[cfg(target_os = "linux")]
            if let Some(Stream::Native(stream)) = &self.stream {
                stream.pause(true);
            }
        }
    }
    pub fn stop(&self) {
        self.envelope.pause(true);
        self.sink.stop();
        #[cfg(target_os = "linux")]
        if let Some(Stream::Native(stream)) = &self.stream {
            stream.pause(true);
        }
    }
    pub fn volume(&self, volume: f32) {
        self.envelope.volume(volume);
    }
    pub fn reset(&mut self, volume: f32) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.target != crate::output_devices::Target::RequireSelection,
            "Select an output in Playback settings before retrying."
        );
        #[cfg(not(target_os = "linux"))]
        anyhow::ensure!(
            !matches!(self.target, crate::output_devices::Target::Pulse(_, _)),
            "The saved native output is unavailable on this platform. Select an output explicitly."
        );
        if self.stream.is_none() {
            let failed = Arc::new(AtomicBool::new(false));
            #[cfg(target_os = "linux")]
            let native = self.target != crate::output_devices::Target::CompatibilityDefault;
            #[cfg(not(target_os = "linux"))]
            let native = false;
            let stream;
            if native {
                #[cfg(target_os = "linux")]
                {
                    stream = Stream::Native(crate::pulse_output::Driver::open(
                        &self.target,
                        failed.clone(),
                    )?);
                }
                #[cfg(not(target_os = "linux"))]
                {
                    anyhow::bail!("Native routing is unavailable on this platform");
                }
            } else {
                let signal = failed.clone();
                // Compatibility is explicitly selected; never search alternate devices.
                let mut output = OutputStreamBuilder::from_default_device()?
                    .with_error_callback(move |_| {
                        signal.store(true, Ordering::Release);
                    })
                    .open_stream()?;
                output.log_on_drop(false);
                stream = Stream::Compatibility(output);
            }
            self.failed = failed;
            self.stream = Some(stream);
        }
        self.sink.stop();
        self.sink = Sink::connect_new(self.stream.as_ref().unwrap().mixer());
        self.envelope = Arc::new(crate::audio_envelope::Control::default());
        self.envelope.volume(volume);
        self.sync_pause();
        #[cfg(target_os = "linux")]
        if let Some(Stream::Native(stream)) = &self.stream {
            stream.reset();
        }
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
            source: crate::audio_envelope::Envelope::new(source, self.envelope.clone()),
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
