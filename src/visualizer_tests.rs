use super::*;
use rodio::buffer::SamplesBuffer;

fn capture() -> Arc<Capture> {
    let capture = Arc::new(Capture::default());
    capture.enable(true);
    capture.playing(true);
    capture
}
fn tap(samples: Vec<f32>, channels: u16, rate: u32, capture: Arc<Capture>) -> Tap<SamplesBuffer> {
    Tap::new(
        SamplesBuffer::new(channels, rate, samples),
        capture,
        Arc::new(AtomicU64::new(1)),
        1,
    )
}

#[test]
fn busy_snapshot_is_distinct_from_an_invalidated_display_epoch() {
    let capture = capture();
    tap(vec![0.4; SAMPLES], 1, 48000, capture.clone()).for_each(drop);
    let frame = capture.snapshot().unwrap();
    capture.sequence.fetch_add(1, Ordering::AcqRel);
    assert!(capture.snapshot().is_none());
    assert!(capture.is_current(frame.epoch));
    capture.invalidate();
    assert!(!capture.is_current(frame.epoch));
    capture.sequence.fetch_add(1, Ordering::Release);
    assert!(capture.snapshot().is_none());
}

#[test]
fn pcm_is_bit_identical_including_nonfinite_samples_and_metadata() {
    let mut samples: Vec<_> = (0..SAMPLES * 4).map(|i| (i as f32 * 0.1).sin()).collect();
    samples[5] = f32::NAN;
    samples[100] = f32::INFINITY;
    samples[123] = -0.0;
    let capture = capture();
    let source = tap(samples.clone(), 2, 44100, capture.clone());
    assert_eq!(source.channels(), 2);
    assert_eq!(source.sample_rate(), 44100);
    assert_eq!(
        source.total_duration(),
        SamplesBuffer::new(2, 44100, samples.clone()).total_duration()
    );
    assert_eq!(
        source.map(f32::to_bits).collect::<Vec<_>>(),
        samples.into_iter().map(f32::to_bits).collect::<Vec<_>>()
    );
    assert!(
        capture
            .snapshot()
            .unwrap()
            .samples
            .iter()
            .all(|v| v.is_finite())
    );
}

#[test]
fn capture_is_mono_bounded_and_independent_of_output_volume() {
    let capture = capture();
    let source = tap([0.2, 0.8].repeat(SAMPLES * 3), 2, 48000, capture.clone());
    assert!(source.amplify(0.).all(|sample| sample == 0.));
    let frame = capture.snapshot().unwrap();
    assert_eq!(frame.rate, 48000);
    assert!(frame.samples.iter().all(|s| (*s - 0.5).abs() < 0.00001));
}

#[test]
fn disabled_capture_does_not_publish_and_mode_cycles() {
    let capture = Arc::new(Capture::default());
    capture.playing(true);
    let samples = vec![0.5; SAMPLES * 2];
    assert_eq!(
        tap(samples.clone(), 1, 48000, capture.clone()).collect::<Vec<_>>(),
        samples
    );
    assert_eq!(capture.sequence.load(Ordering::Relaxed), 0);
    assert!(capture.snapshot().is_none());
    assert_eq!(Mode::default(), Mode::Off);
    assert_eq!(Mode::Off.next(), Mode::Spectrum);
    assert_eq!(Mode::Spectrum.next(), Mode::Waveform);
    assert_eq!(Mode::Waveform.next(), Mode::Off);
}

#[test]
fn pause_seek_and_track_changes_invalidate_old_audio() {
    let capture = capture();
    let mut source = tap(vec![0.25; 48000], 1, 48000, capture.clone());
    for _ in 0..SAMPLES {
        source.next();
    }
    assert!(capture.snapshot().is_some());
    capture.playing(false);
    assert!(capture.snapshot().is_none());
    capture.playing(true);
    assert!(capture.snapshot().is_none());
    for _ in 0..PUBLISH_EVERY {
        source.next();
    }
    assert!(capture.snapshot().is_some());
    source.try_seek(Duration::from_millis(200)).unwrap();
    assert!(capture.snapshot().is_none());
    for _ in 0..PUBLISH_EVERY {
        source.next();
    }
    let frame = capture.snapshot().unwrap();
    assert!(
        frame.samples[..SAMPLES - PUBLISH_EVERY]
            .iter()
            .all(|v| *v == 0.)
    );
    assert!(
        frame.samples[SAMPLES - PUBLISH_EVERY..]
            .iter()
            .all(|v| *v == 0.25)
    );
    source.generation.store(2, Ordering::Release);
    capture.invalidate();
    for _ in 0..SAMPLES {
        source.next();
    }
    assert!(
        capture.snapshot().is_none(),
        "An obsolete decoder must not publish into a new track"
    );
}

#[test]
fn high_rate_audio_is_downsampled_only_for_display() {
    let capture = capture();
    let samples = [0., 0.25, 0.5, 0.75].repeat(SAMPLES);
    assert_eq!(
        tap(samples.clone(), 1, 192000, capture.clone()).collect::<Vec<_>>(),
        samples
    );
    let frame = capture.snapshot().unwrap();
    assert_eq!(frame.rate, 48000);
    assert!(frame.samples.iter().all(|v| *v == 0.375));
}

#[test]
fn spectrum_finds_tones_silence_is_flat_and_peaks_decay() {
    let mut analyzer = Analyzer::default();
    let mut frame = Frame {
        samples: std::array::from_fn(|i| {
            (std::f32::consts::TAU * 1000. * i as f32 / 48000.).sin() * 0.5
        }),
        rate: 48000,
        epoch: 1,
        serial: 2,
    };
    analyzer.update(&frame, 0.033, true);
    let peak = analyzer
        .bars
        .iter()
        .enumerate()
        .max_by(|a, b| a.1.total_cmp(b.1))
        .unwrap()
        .0;
    assert!(
        (24..=27).contains(&peak),
        "1 kHz belongs near band 26, got {peak}"
    );
    assert!(analyzer.bars[peak] > 0.5);
    let previous = analyzer.peaks[peak];
    assert!(analyzer.wave.iter().any(|v| v.abs() > 0.2));
    frame.samples.fill(0.);
    frame.serial += 2;
    for _ in 0..60 {
        analyzer.update(&frame, 0.033, true);
    }
    assert!(analyzer.bars.iter().all(|v| *v < 0.001));
    assert!(analyzer.peaks[peak] < previous * 0.1);
    assert!(analyzer.wave.iter().all(|v| *v == 0.));
    frame.epoch += 1;
    analyzer.update(&frame, 0.033, true);
    assert!(analyzer.peaks.iter().all(|v| *v == 0.));
}

#[test]
fn spectrum_rejects_dc_offset() {
    let mut analyzer = Analyzer::default();
    analyzer.update(
        &Frame {
            samples: [0.5; SAMPLES],
            rate: 44100,
            epoch: 1,
            serial: 2,
        },
        0.033,
        true,
    );
    assert!(analyzer.bars.iter().all(|v| *v == 0.));
}

#[test]
fn concurrent_snapshots_never_mix_publications() {
    let capture = capture();
    let writer = capture.clone();
    let thread = std::thread::spawn(move || {
        let epoch = writer.epoch.load(Ordering::Acquire);
        for i in 0..1000 {
            writer.publish(&[i as f32 / 1000.; SAMPLES], 0, 48000, epoch);
        }
    });
    while !thread.is_finished() {
        if let Some(frame) = capture.snapshot() {
            assert!(frame.samples.iter().all(|v| *v == frame.samples[0]));
        }
    }
    thread.join().unwrap();
    assert!(capture.snapshot().is_some());
}
