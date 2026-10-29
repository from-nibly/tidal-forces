use super::*;
use rodio::{buffer::SamplesBuffer, source::UniformSourceIterator};

fn buffered(
    pool: &pcm::Pool,
    id: u64,
    channels: u16,
    rate: u32,
    samples: Vec<f32>,
) -> (pcm::PcmSource, Arc<Frame>) {
    let (source, feedback) = pool
        .start(Box::new(SamplesBuffer::new(channels, rate, samples)))
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    while !feedback.finished() {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    let frame = Frame::new(id, &source, feedback, 0);
    (source, frame)
}
fn pair(
    rate: u32,
    next_rate: u32,
    next_channels: u16,
    duration: u64,
) -> (Chain, Arc<Boundary>, Arc<Frame>, Arc<Frame>) {
    let pool = pcm::Pool::default();
    let generation = Arc::new(AtomicU64::new(7));
    let capture = Arc::new(visualizer::Capture::default());
    let slot = Slot::default();
    let (pcm, first) = buffered(&pool, 7, 2, rate, vec![0.25; 256]);
    let (next, second) = buffered(
        &pool,
        11,
        next_channels,
        next_rate,
        vec![0.75; usize::from(next_channels) * 128],
    );
    let boundary = Arc::new(Boundary::new(7, 11, duration));
    assert!(slot.arm(
        next,
        second.clone(),
        boundary.clone(),
        capture.clone(),
        generation.clone()
    ));
    (
        Chain::new(pcm, first.clone(), slot, capture, generation),
        boundary,
        first,
        second,
    )
}

#[test]
fn ready_same_format_boundary_is_exact_and_keeps_resampler_state() {
    for rate in [44100, 48000, 96000, 192000] {
        let (chain, boundary, first, second) = pair(rate, rate, 2, 0);
        let expected: Vec<_> = UniformSourceIterator::new(
            SamplesBuffer::new(2, rate, [vec![0.25; 256], vec![0.75; 256]].concat()),
            2,
            48000,
        )
        .collect();
        let actual: Vec<_> = UniformSourceIterator::new(chain, 2, 48000).collect();
        assert_eq!(actual, expected, "{rate} Hz");
        assert!(boundary.claimed());
        assert_eq!(first.rendered(), second.rendered());
        assert_eq!(
            first.rendered().as_nanos(),
            128_000_000_000u128 / u128::from(rate)
        );
    }
}

#[test]
fn queue_cancel_early_eof_and_format_changes_never_consume_next() {
    for (rate, channels, duration, cancel) in [
        (48000, 2, 0, true),
        (48000, 2, 10, false),
        (44100, 2, 0, false),
        (48000, 1, 0, false),
    ] {
        let (chain, boundary, _, next) = pair(48000, rate, channels, duration);
        if cancel {
            assert!(boundary.cancel());
        }
        assert_eq!(chain.collect::<Vec<_>>(), vec![0.25; 256]);
        assert!(!boundary.claimed());
        assert_eq!(next.rendered(), Duration::ZERO);
    }
}

#[test]
fn slot_contention_falls_back_without_waiting() {
    let (mut chain, boundary, _, next) = pair(48000, 48000, 2, 0);
    assert_eq!(chain.by_ref().take(256).count(), 256);
    let slot = chain.slot.clone();
    let _locked = slot.0.lock().unwrap();
    assert_eq!(chain.next(), None);
    assert!(!boundary.claimed());
    assert_eq!(next.rendered(), Duration::ZERO);
}

#[test]
fn cancel_or_claim_has_one_linearized_winner() {
    for _ in 0..32 {
        let (mut chain, boundary, _, _) = pair(48000, 48000, 2, 0);
        assert_eq!(chain.by_ref().take(256).count(), 256);
        let barrier = Arc::new(std::sync::Barrier::new(2));
        let gate = barrier.clone();
        let candidate = boundary.clone();
        let cancel = std::thread::spawn(move || {
            gate.wait();
            candidate.cancel()
        });
        barrier.wait();
        let sample = chain.next();
        if cancel.join().unwrap() {
            assert_eq!(sample, None);
            assert!(!boundary.claimed());
            assert_eq!(chain.generation.load(Ordering::Acquire), 7);
        } else {
            assert_eq!(sample, Some(0.75));
            assert!(boundary.claimed());
            assert_eq!(chain.generation.load(Ordering::Acquire), 11);
        }
    }
}

#[test]
fn newer_manual_generation_wins_and_cannot_split_a_frame() {
    let (mut chain, boundary, _, _) = pair(48000, 48000, 2, 0);
    assert_eq!(chain.next(), Some(0.25));
    chain.generation.store(12, Ordering::Release);
    assert_eq!(chain.next(), Some(0.25));
    assert_eq!(chain.next(), None);
    assert!(!boundary.claimed());
    assert_eq!(chain.generation.load(Ordering::Acquire), 12);
}

#[test]
fn next_track_underflow_is_silence_without_next_listening_credit() {
    struct Held {
        count: usize,
        gate: std::sync::mpsc::Receiver<()>,
        entered: std::sync::mpsc::Sender<()>,
    }
    impl Iterator for Held {
        type Item = f32;
        fn next(&mut self) -> Option<f32> {
            if self.count < 2 {
                self.count += 1;
                return Some(0.75);
            }
            self.entered.send(()).ok()?;
            self.gate.recv().ok()?;
            None
        }
    }
    impl Source for Held {
        fn current_span_len(&self) -> Option<usize> {
            None
        }
        fn channels(&self) -> u16 {
            2
        }
        fn sample_rate(&self) -> u32 {
            1000
        }
        fn total_duration(&self) -> Option<Duration> {
            None
        }
    }
    let pool = pcm::Pool::default();
    let generation = Arc::new(AtomicU64::new(7));
    let capture = Arc::new(visualizer::Capture::default());
    let slot = Slot::default();
    let (pcm, first) = buffered(&pool, 7, 2, 1000, vec![0.25; 4]);
    let (release, gate) = std::sync::mpsc::channel();
    let (entered, waiting) = std::sync::mpsc::channel();
    let (next, feedback) = pool
        .start(Box::new(Held {
            count: 0,
            gate,
            entered,
        }))
        .unwrap();
    waiting.recv_timeout(Duration::from_secs(3)).unwrap();
    let second = Frame::new(11, &next, feedback, 0);
    let boundary = Arc::new(Boundary::new(7, 11, 0));
    slot.arm(
        next,
        second.clone(),
        boundary.clone(),
        capture.clone(),
        generation.clone(),
    );
    let mut chain = Chain::new(pcm, first.clone(), slot, capture, generation);
    assert_eq!(
        chain.by_ref().take(6).collect::<Vec<_>>(),
        vec![0.25, 0.25, 0.25, 0.25, 0.75, 0.75]
    );
    assert!(boundary.claimed());
    assert!(chain.by_ref().take(1000).all(|sample| sample == 0.));
    assert_eq!(first.rendered(), Duration::from_millis(2));
    assert_eq!(second.rendered(), Duration::from_millis(1));
    assert!(second.feedback.buffering());
    release.send(()).unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    while !second.feedback.finished() {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(chain.next(), None);
    assert_eq!(second.rendered(), Duration::from_millis(1));
}

#[test]
fn pause_does_not_claim_or_count_prepared_audio() {
    let (chain, boundary, first, next) = pair(48000, 48000, 2, 0);
    let (sink, mut output) = rodio::Sink::new();
    sink.pause();
    sink.append(chain);
    for _ in 0..1024 {
        assert_eq!(output.next(), Some(0.));
    }
    assert!(!boundary.claimed());
    assert_eq!(first.rendered(), Duration::ZERO);
    assert_eq!(next.rendered(), Duration::ZERO);
}
