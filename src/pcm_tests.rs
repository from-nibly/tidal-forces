use super::*;
use std::sync::{atomic::AtomicU64, mpsc};
use std::time::Instant;

fn wait_for(mut ready: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(3);
    while !ready() {
        assert!(Instant::now() < deadline, "Synthetic worker did not finish");
        std::thread::sleep(Duration::from_millis(1));
    }
}

struct HeldSource {
    samples: std::vec::IntoIter<f32>,
    gate: Option<mpsc::Receiver<()>>,
    entered: mpsc::Sender<()>,
    dropped: mpsc::Sender<std::thread::ThreadId>,
    channels: u16,
}
impl Iterator for HeldSource {
    type Item = f32;
    fn next(&mut self) -> Option<f32> {
        if let Some(gate) = self.gate.take() {
            self.entered.send(()).ok()?;
            gate.recv().ok()?;
        }
        self.samples.next()
    }
}
impl Source for HeldSource {
    fn current_span_len(&self) -> Option<usize> {
        Some(self.samples.len())
    }
    fn channels(&self) -> u16 {
        self.channels
    }
    fn sample_rate(&self) -> u32 {
        1000
    }
    fn total_duration(&self) -> Option<Duration> {
        None
    }
}
impl Drop for HeldSource {
    fn drop(&mut self) {
        let _ = self.dropped.send(std::thread::current().id());
    }
}
fn held(
    channels: u16,
    samples: Vec<f32>,
) -> (
    Box<HeldSource>,
    mpsc::Sender<()>,
    mpsc::Receiver<()>,
    mpsc::Receiver<std::thread::ThreadId>,
) {
    let (release, gate) = mpsc::channel();
    let (entered, seen) = mpsc::channel();
    let (dropped, done) = mpsc::channel();
    (
        Box::new(HeldSource {
            samples: samples.into_iter(),
            gate: Some(gate),
            entered,
            dropped,
            channels,
        }),
        release,
        seen,
        done,
    )
}

#[test]
fn blocked_decoder_never_blocks_pcm_and_silence_has_no_listening_credit() {
    let pool = Pool::default();
    let (decoder, release, entered, dropped) = held(2, vec![0.25, -0.25, 0.5, -0.5]);
    let (source, feedback) = pool.start(decoder).unwrap();
    entered.recv_timeout(Duration::from_secs(3)).unwrap();
    let total = Arc::new(AtomicU64::new(0));
    let mut meter =
        crate::audio_meter::Meter::with_activity(source, total.clone(), feedback.activity());
    for _ in 0..2000 {
        assert_eq!(meter.next(), Some(0.));
    }
    assert_eq!(total.load(Ordering::Relaxed), 0);
    assert!(feedback.buffering());
    release.send(()).unwrap();
    wait_for(|| feedback.0.done.load(Ordering::Acquire));
    assert_eq!(
        meter.by_ref().collect::<Vec<_>>(),
        vec![0.25, -0.25, 0.5, -0.5]
    );
    assert_eq!(total.load(Ordering::Relaxed), 2_000_000);
    assert!(!feedback.buffering());
    assert_ne!(
        dropped.recv_timeout(Duration::from_secs(3)).unwrap(),
        std::thread::current().id()
    );
    drop(meter);
    drop(feedback);
    wait_for(|| pool.0.load(Ordering::Acquire) == 0);
}

#[test]
fn underflow_cannot_split_a_stereo_frame() {
    let pool = Pool::default();
    let (decoder, release, entered, _) = held(2, vec![0.25, -0.25]);
    let (mut source, feedback) = pool.start(decoder).unwrap();
    entered.recv_timeout(Duration::from_secs(3)).unwrap();
    assert_eq!(source.next(), Some(0.));
    release.send(()).unwrap();
    wait_for(|| feedback.0.done.load(Ordering::Acquire));
    assert_eq!(source.next(), Some(0.));
    assert_eq!(source.next(), Some(0.25));
    assert_eq!(source.next(), Some(-0.25));
    assert_eq!(source.next(), None);
}

#[test]
fn pcm_preserves_finite_sample_bits_channels_and_rates() {
    for channels in [1, 2, 6, 8] {
        for rate in [44100, 48000, 192000] {
            let samples: Vec<f32> = (0..usize::from(channels) * 64)
                .map(|i| [-0., 0.25, -0.5, 1., -1., f32::MIN_POSITIVE][i % 6])
                .collect();
            let pool = Pool::default();
            let (mut source, feedback) = pool
                .start(Box::new(rodio::buffer::SamplesBuffer::new(
                    channels,
                    rate,
                    samples.clone(),
                )))
                .unwrap();
            wait_for(|| feedback.0.done.load(Ordering::Acquire));
            assert_eq!(source.channels(), channels);
            assert_eq!(source.sample_rate(), rate);
            let actual: Vec<u32> = source.by_ref().map(f32::to_bits).collect();
            assert_eq!(
                actual,
                samples.into_iter().map(f32::to_bits).collect::<Vec<_>>()
            );
            assert!(!feedback.failed());
        }
    }
}

#[test]
fn pcm_backpressure_limits_decode_ahead_and_drop_cancels() {
    let pool = Pool::default();
    let (source, feedback) = pool
        .start(Box::new(rodio::source::SineWave::new(440.)))
        .unwrap();
    let capacity = feedback.0.mask + 1;
    wait_for(|| feedback.0.write.load(Ordering::Acquire) == capacity);
    assert_eq!(feedback.0.read.load(Ordering::Acquire), 0);
    assert!(feedback.0.samples.len() * std::mem::size_of::<AtomicU32>() <= 4 * 1024 * 1024);
    drop(source);
    assert!(feedback.0.cancelled.load(Ordering::Acquire));
    drop(feedback);
    wait_for(|| pool.0.load(Ordering::Acquire) == 0);
}

#[test]
fn cancel_rejects_queued_frames_even_before_output_drops_the_source() {
    let pool = Pool::default();
    let (mut source, feedback) = pool
        .start(Box::new(rodio::source::SineWave::new(440.)))
        .unwrap();
    wait_for(|| feedback.0.write.load(Ordering::Acquire) == feedback.0.mask + 1);
    feedback.cancel();
    assert_eq!(source.next(), None);
    assert!(!feedback.buffering());
    wait_for(|| feedback.0.done.load(Ordering::Acquire));
    assert!(!feedback.failed());
    drop(source);
    drop(feedback);
    wait_for(|| pool.0.load(Ordering::Acquire) == 0);
}

#[test]
fn blocked_retired_workers_are_bounded_and_release_capacity() {
    let pool = Pool::default();
    let mut gates = Vec::new();
    for _ in 0..MAX_WORKERS {
        let (decoder, release, entered, _) = held(1, vec![0.5]);
        let (source, feedback) = pool.start(decoder).unwrap();
        entered.recv_timeout(Duration::from_secs(3)).unwrap();
        drop(source);
        drop(feedback);
        gates.push(release);
    }
    assert_eq!(pool.0.load(Ordering::Acquire), MAX_WORKERS);
    assert!(
        pool.start(Box::new(rodio::source::SineWave::new(440.)))
            .is_err()
    );
    for gate in gates {
        gate.send(()).unwrap();
    }
    wait_for(|| pool.0.load(Ordering::Acquire) == 0);
}

#[test]
fn malformed_frames_and_nonfinite_samples_fail_closed() {
    for samples in [vec![0.5], vec![f32::NAN, 0.], vec![0., f32::INFINITY]] {
        let pool = Pool::default();
        let (mut source, feedback) = pool
            .start(Box::new(rodio::buffer::SamplesBuffer::new(
                2, 1000, samples,
            )))
            .unwrap();
        wait_for(|| feedback.0.done.load(Ordering::Acquire));
        assert!(feedback.failed());
        assert_eq!(source.next(), None);
    }
}

#[test]
fn direct_flac_and_aac_decode_identically_through_pcm() {
    for bytes in [
        include_bytes!("../tests/fixtures/tone.flac").as_slice(),
        include_bytes!("../tests/fixtures/tone.m4a").as_slice(),
    ] {
        let make = || {
            rodio::Decoder::builder()
                .with_data(std::io::Cursor::new(bytes))
                .with_byte_len(bytes.len() as u64)
                .with_seekable(true)
                .build()
                .unwrap()
        };
        let expected: Vec<_> = make().map(f32::to_bits).collect();
        let (mut source, feedback) = Pool::default().start(Box::new(make())).unwrap();
        let activity = feedback.activity();
        let deadline = Instant::now() + Duration::from_secs(3);
        let mut actual = Vec::new();
        for sample in &mut source {
            assert!(Instant::now() < deadline);
            if activity.load(Ordering::Relaxed) {
                actual.push(sample.to_bits());
            } else {
                std::thread::sleep(Duration::from_millis(1));
            }
        }
        assert!(!feedback.failed());
        assert_eq!(actual, expected);
    }
}

#[test]
fn paused_sink_does_not_consume_predecoded_audio() {
    let pool = Pool::default();
    let (source, feedback) = pool
        .start(Box::new(rodio::buffer::SamplesBuffer::new(
            1,
            1000,
            vec![0.5; 32],
        )))
        .unwrap();
    wait_for(|| feedback.0.done.load(Ordering::Acquire));
    let total = Arc::new(AtomicU64::new(0));
    let (sink, mut output) = rodio::Sink::new();
    sink.pause();
    sink.append(crate::audio_meter::Meter::with_activity(
        source,
        total.clone(),
        feedback.activity(),
    ));
    for _ in 0..1000 {
        assert_eq!(output.next(), Some(0.));
    }
    assert_eq!(total.load(Ordering::Relaxed), 0);
    assert_eq!(feedback.0.read.load(Ordering::Acquire), 0);
    sink.play();
    for _ in 0..1000 {
        output.next();
    }
    assert_eq!(total.load(Ordering::Relaxed), 32_000_000);
}

#[test]
fn changed_format_is_rejected_before_publishing_the_changed_frame() {
    struct Changes(bool);
    impl Iterator for Changes {
        type Item = f32;
        fn next(&mut self) -> Option<f32> {
            self.0 = true;
            Some(0.5)
        }
    }
    impl Source for Changes {
        fn current_span_len(&self) -> Option<usize> {
            Some(1)
        }
        fn channels(&self) -> u16 {
            1
        }
        fn sample_rate(&self) -> u32 {
            if self.0 { 2000 } else { 1000 }
        }
        fn total_duration(&self) -> Option<Duration> {
            None
        }
    }
    let (mut source, feedback) = Pool::default().start(Box::new(Changes(false))).unwrap();
    wait_for(|| feedback.0.done.load(Ordering::Acquire));
    assert!(feedback.failed());
    assert_eq!(source.next(), None);
}

#[test]
fn unsupported_pcm_formats_do_not_allocate_workers() {
    let pool = Pool::default();
    for (channels, rate) in [(9, 48000), (2, 384001)] {
        assert!(
            pool.start(Box::new(rodio::buffer::SamplesBuffer::new(
                channels,
                rate,
                vec![0.; 18]
            )))
            .is_err()
        );
    }
    assert_eq!(pool.0.load(Ordering::Acquire), 0);
}
