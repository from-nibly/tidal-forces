//! Bounded single-producer PCM handoff. Only the worker owns the decoder.
use anyhow::{Result, ensure};
use rodio::Source;
use std::{
    sync::{
        Arc, OnceLock,
        atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering},
    },
    time::Duration,
};

const MAX_WORKERS: usize = 4;
const MAX_CHANNELS: usize = 8;
const MAX_RATE: u32 = 384_000;

#[derive(Clone, Default)]
pub struct Pool(Arc<AtomicUsize>);
struct Permit(Arc<AtomicUsize>);
impl Drop for Permit {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

struct Shared {
    samples: Box<[AtomicU32]>,
    mask: usize,
    read: AtomicUsize,
    write: AtomicUsize,
    done: AtomicBool,
    failed: AtomicBool,
    cancelled: AtomicBool,
    activity: Arc<AtomicBool>,
    wake: OnceLock<std::thread::Thread>,
}
impl Shared {
    fn wake_worker(&self) {
        if let Some(worker) = self.wake.get() {
            worker.unpark();
        }
    }
}
#[derive(Clone)]
pub struct Feedback(Arc<Shared>);
impl Feedback {
    pub fn cancel(&self) {
        self.0.cancelled.store(true, Ordering::Release);
        self.0.wake_worker();
    }
    pub fn buffering(&self) -> bool {
        self.0.read.load(Ordering::Acquire) == self.0.write.load(Ordering::Acquire)
            && !self.0.done.load(Ordering::Acquire)
            && !self.0.cancelled.load(Ordering::Acquire)
    }
    pub fn failed(&self) -> bool {
        self.0.failed.load(Ordering::Acquire)
    }
    pub fn activity(&self) -> Arc<AtomicBool> {
        self.0.activity.clone()
    }
}
impl Drop for Feedback {
    fn drop(&mut self) {
        self.0.wake_worker();
    }
}
pub struct PcmSource {
    shared: Arc<Shared>,
    channels: u16,
    rate: u32,
    duration: Option<Duration>,
    channel: usize,
    real_frame: bool,
}

impl Pool {
    pub fn start(
        &self,
        decoder: Box<dyn Source<Item = f32> + Send>,
    ) -> Result<(PcmSource, Feedback)> {
        let channels = decoder.channels();
        let rate = decoder.sample_rate();
        let duration = decoder.total_duration();
        ensure!(
            (1..=MAX_CHANNELS as u16).contains(&channels) && (1..=MAX_RATE).contains(&rate),
            "Unsupported decoded audio format"
        );
        let mut active = self.0.load(Ordering::Acquire);
        loop {
            ensure!(
                active < MAX_WORKERS,
                "Previous audio work is still stopping. Please retry shortly."
            );
            match self.0.compare_exchange_weak(
                active,
                active + 1,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => break,
                Err(observed) => active = observed,
            }
        }
        let permit = Permit(self.0.clone());
        // About 250–500 ms, at most 4 MiB per handoff. Power-of-two frame
        // capacity keeps wrapping counters correct even for multichannel data.
        let frames = (rate as usize / 4).max(2).next_power_of_two();
        let shared = Arc::new(Shared {
            samples: (0..frames * usize::from(channels))
                .map(|_| AtomicU32::new(0))
                .collect(),
            mask: frames - 1,
            read: AtomicUsize::new(0),
            write: AtomicUsize::new(0),
            done: AtomicBool::new(false),
            failed: AtomicBool::new(false),
            cancelled: AtomicBool::new(false),
            activity: Arc::new(AtomicBool::new(false)),
            wake: OnceLock::new(),
        });
        let worker = shared.clone();
        let thread = std::thread::Builder::new()
            .name("tidal-pcm".into())
            .spawn(move || {
                let _permit = permit;
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    produce(decoder, &worker, channels, rate)
                }));
                if result.is_err() {
                    worker.failed.store(true, Ordering::Release);
                }
                worker.done.store(true, Ordering::Release);
                // Decoder destruction and final buffer deallocation stay off the
                // output callback. Stop/drop never waits for a blocked decoder.
                while Arc::strong_count(&worker) > 1 {
                    std::thread::park_timeout(Duration::from_millis(100));
                }
            })?;
        let _ = shared.wake.set(thread.thread().clone());
        Ok((
            PcmSource {
                shared: shared.clone(),
                channels,
                rate,
                duration,
                channel: 0,
                real_frame: false,
            },
            Feedback(shared),
        ))
    }
}

fn produce(
    mut decoder: Box<dyn Source<Item = f32> + Send>,
    shared: &Shared,
    channels: u16,
    rate: u32,
) {
    let mut frame = [0.; MAX_CHANNELS];
    let channels = usize::from(channels);
    loop {
        if shared.cancelled.load(Ordering::Acquire) {
            return;
        }
        let write = shared.write.load(Ordering::Relaxed);
        if write.wrapping_sub(shared.read.load(Ordering::Acquire)) > shared.mask {
            std::thread::park_timeout(Duration::from_secs(1));
            continue;
        }
        for (channel, sample) in frame[..channels].iter_mut().enumerate() {
            if decoder.channels() as usize != channels || decoder.sample_rate() != rate {
                shared.failed.store(true, Ordering::Release);
                return;
            }
            match decoder.next() {
                Some(value) if value.is_finite() => *sample = value,
                None if channel == 0 => return,
                _ => {
                    shared.failed.store(true, Ordering::Release);
                    return;
                }
            }
        }
        if shared.cancelled.load(Ordering::Acquire) {
            return;
        }
        if decoder.channels() as usize != channels || decoder.sample_rate() != rate {
            shared.failed.store(true, Ordering::Release);
            return;
        }
        let base = (write & shared.mask) * channels;
        for (i, value) in frame[..channels].iter().enumerate() {
            shared.samples[base + i].store(value.to_bits(), Ordering::Relaxed);
        }
        shared.write.store(write.wrapping_add(1), Ordering::Release);
    }
}

impl Iterator for PcmSource {
    type Item = f32;
    fn next(&mut self) -> Option<f32> {
        let read = self.shared.read.load(Ordering::Relaxed);
        if self.channel == 0 {
            if self.shared.cancelled.load(Ordering::Acquire) {
                self.shared.activity.store(false, Ordering::Relaxed);
                return None;
            }
            let mut write = self.shared.write.load(Ordering::Acquire);
            if write == read && self.shared.done.load(Ordering::Acquire) {
                // Re-read after acquiring EOF: the final frame may have been
                // published between the first cursor read and the EOF load.
                write = self.shared.write.load(Ordering::Acquire);
                if write == read {
                    self.shared.activity.store(false, Ordering::Relaxed);
                    return None;
                }
            }
            self.real_frame = write != read;
            self.shared
                .activity
                .store(self.real_frame, Ordering::Relaxed);
        }
        let value = if self.real_frame {
            f32::from_bits(
                self.shared.samples
                    [(read & self.shared.mask) * usize::from(self.channels) + self.channel]
                    .load(Ordering::Relaxed),
            )
        } else {
            0.
        };
        self.channel += 1;
        if self.channel == usize::from(self.channels) {
            self.channel = 0;
            if self.real_frame {
                let next = read.wrapping_add(1);
                self.shared.read.store(next, Ordering::Release);
                let capacity = self.shared.mask + 1;
                let quantum = (capacity / 2).min(256);
                if next & (quantum - 1) == 0
                    && self.shared.write.load(Ordering::Acquire).wrapping_sub(next) <= capacity / 2
                {
                    self.shared.wake_worker();
                }
            }
        }
        Some(value)
    }
}
impl Source for PcmSource {
    fn current_span_len(&self) -> Option<usize> {
        Some(usize::from(self.channels) - self.channel)
    }
    fn channels(&self) -> u16 {
        self.channels
    }
    fn sample_rate(&self) -> u32 {
        self.rate
    }
    fn total_duration(&self) -> Option<Duration> {
        self.duration
    }
}
impl Drop for PcmSource {
    fn drop(&mut self) {
        self.shared.cancelled.store(true, Ordering::Release);
        self.shared.wake_worker();
    }
}

#[cfg(test)]
#[path = "pcm_tests.rs"]
mod tests;
