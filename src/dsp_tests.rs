use super::*;
use rodio::buffer::SamplesBuffer;

fn process(samples: Vec<f32>, channels: u16, settings: Settings, metadata: Metadata) -> Vec<f32> {
    let control = Arc::new(Control::default());
    control.set(settings);
    Processed::new(
        SamplesBuffer::new(channels, 48000, samples),
        control,
        metadata,
    )
    .unwrap()
    .collect()
}
fn enabled() -> Settings {
    Settings {
        enabled: true,
        ..Default::default()
    }
}
#[test]
fn disabled_and_flat_processing_preserve_every_finite_sample_bit() {
    let samples = vec![-0., 0., -1.25, 1.25, f32::MIN_POSITIVE, -0.1234567];
    for settings in [Settings::default(), enabled()] {
        let actual = process(samples.clone(), 2, settings, Metadata::default());
        assert_eq!(
            actual.iter().map(|x| x.to_bits()).collect::<Vec<_>>(),
            samples.iter().map(|x| x.to_bits()).collect::<Vec<_>>()
        );
    }
}
#[test]
fn stereo_mono_balance_and_preamp_have_explicit_bounded_effects() {
    let mono = Settings {
        mono: true,
        balance: 1.,
        ..enabled()
    };
    assert_eq!(
        process(vec![0.25, 0.75, -0.25, -0.75], 2, mono, Metadata::default()),
        vec![0., 0.5, 0., -0.5]
    );
    let preamp = Settings {
        preamp_db: -6.,
        ..enabled()
    };
    let actual = process(vec![0.5; 8], 2, preamp, Metadata::default());
    assert!(
        actual
            .iter()
            .all(|x| (*x - 0.5 * 10f32.powf(-6. / 20.)).abs() < 1e-6)
    );
    assert_eq!(
        process(vec![0.25; 8], 1, mono, Metadata::default()),
        vec![0.25; 8]
    );
    assert_eq!(
        process(vec![0.25; 8], 4, mono, Metadata::default()),
        vec![0.25; 8]
    );
}
#[test]
fn missing_or_invalid_normalization_is_unavailable_not_an_invented_gain() {
    for (db, peak) in [
        (f32::NAN, 1.),
        (f32::INFINITY, 1.),
        (0., 0.),
        (0., -1.),
        (0., f32::NAN),
        (100., 1.),
    ] {
        assert!(Gain::from_db_and_linear_peak(db, peak).is_none());
    }
    let settings = Settings {
        normalization: Normalization::Track,
        ..enabled()
    };
    let metadata = Metadata::default();
    assert!(!metadata.available(Normalization::Track));
    assert_eq!(process(vec![1.25; 8], 2, settings, metadata), vec![1.25; 8]);
    let metadata = Metadata {
        track: Gain::from_db_and_linear_peak(12., 0.75),
        album: Gain::from_db_and_linear_peak(-6., 1.),
    };
    assert_eq!(process(vec![0.75; 8], 2, settings, metadata), vec![1.; 8]);
    let settings = Settings {
        normalization: Normalization::Album,
        ..settings
    };
    assert!(
        process(vec![1.; 8], 2, settings, metadata)
            .iter()
            .all(|x| (*x - 10f32.powf(-6. / 20.)).abs() < 1e-6)
    );
}
#[test]
fn sdk_normalization_is_attenuation_only_and_stateful_boundaries_are_gated() {
    let settings = Settings {
        normalization: Normalization::Track,
        ..enabled()
    };
    for db in [-60., -8., -4., 0., 12.] {
        let metadata = Metadata {
            track: Gain::from_replay_gain_db(db),
            album: None,
        };
        let actual = process(vec![0.5; 2], 2, settings, metadata);
        let expected = 0.5 * 10f32.powf((db + 4.) / 20.).min(1.);
        assert!((actual[0] - expected).abs() < 1e-6);
        assert!(settings.continuous(metadata, metadata));
    }
    let quiet = Metadata {
        track: Gain::from_replay_gain_db(-8.),
        album: None,
    };
    assert!(!settings.continuous(quiet, Metadata::default()));
    let mut eq = enabled();
    eq.bands[5] = 3.;
    assert!(!eq.continuous(Metadata::default(), Metadata::default()));
    eq.enabled = false;
    assert!(eq.continuous(quiet, Metadata::default()));
}

#[test]
fn eq_response_and_automatic_headroom_are_measured() {
    fn rms(frequency: f64, settings: Settings) -> f64 {
        let input: Vec<_> = (0..48000)
            .map(|n| {
                (0.1 * (std::f64::consts::TAU * frequency * f64::from(n) / 48000.).sin()) as f32
            })
            .collect();
        let actual = process(input, 1, settings, Metadata::default());
        (actual[4800..]
            .iter()
            .map(|x| f64::from(*x).powi(2))
            .sum::<f64>()
            / 43200.)
            .sqrt()
    }
    let mut settings = enabled();
    settings.bands[5] = 6.;
    let center = rms(1000., settings);
    let low = rms(31.25, settings);
    assert!((center - 0.1 / std::f64::consts::SQRT_2).abs() < 0.001);
    assert!(center / low > 1.9 && center / low < 2.1);
}
#[test]
fn boost_path_limits_peaks_but_disabled_settings_do_not() {
    let settings = Settings {
        preamp_db: 12.,
        ..enabled()
    };
    assert_eq!(
        process(vec![0.9, -0.9], 2, settings, Metadata::default()),
        vec![1., -1.]
    );
    let mut settings = settings;
    settings.bands = [12.; 10];
    let actual = process(
        (0..10000)
            .map(|n| if n % 2 == 0 { 4. } else { -4. })
            .collect(),
        2,
        settings,
        Metadata::default(),
    );
    assert!(
        actual
            .iter()
            .all(|x| x.is_finite() && (-1. ..=1.).contains(x))
    );
}
#[test]
fn worker_updates_are_complete_frame_aligned() {
    let control = Arc::new(Control::default());
    let mut source = Processed::new(
        SamplesBuffer::new(2, 48000, vec![0.5; 1024]),
        control.clone(),
        Metadata::default(),
    )
    .unwrap();
    assert!(source.by_ref().take(256).all(|x| x == 0.5));
    control.set(Settings {
        balance: 1.,
        ..enabled()
    });
    assert!(source.by_ref().take(256).all(|x| x == 0.5));
    let ramp: Vec<_> = source.collect();
    assert!(ramp.as_chunks::<2>().0.iter().all(|frame| frame[1] == 0.5));
    let left: Vec<_> = ramp
        .as_chunks::<2>()
        .0
        .iter()
        .map(|frame| frame[0])
        .collect();
    assert!(left.windows(2).all(|pair| pair[1] <= pair[0]));
    assert!(left[0] > 0. && left[0] < 0.5);
    assert!(left[239..].iter().all(|v| *v == 0.));
}
#[test]
fn malformed_frames_are_not_silently_treated_as_successful_eof() {
    let control = Arc::new(Control::default());
    let actual: Vec<_> = Processed::new(
        SamplesBuffer::new(2, 48000, vec![0.25; 3]),
        control,
        Metadata::default(),
    )
    .unwrap()
    .collect();
    assert_eq!(&actual[..2], &[0.25, 0.25]);
    assert!(actual[2].is_nan());
}
#[test]
fn a_decoder_format_change_is_rejected_before_publishing_its_first_frame() {
    struct Changing {
        read: bool,
    }
    impl Iterator for Changing {
        type Item = f32;
        fn next(&mut self) -> Option<f32> {
            self.read = true;
            Some(0.25)
        }
    }
    impl Source for Changing {
        fn channels(&self) -> u16 {
            2
        }
        fn sample_rate(&self) -> u32 {
            if self.read { 48000 } else { 44100 }
        }
        fn current_span_len(&self) -> Option<usize> {
            None
        }
        fn total_duration(&self) -> Option<Duration> {
            None
        }
    }
    let mut source = Processed::new(
        Changing { read: false },
        Arc::new(Control::default()),
        Metadata::default(),
    )
    .unwrap();
    assert!(source.next().unwrap().is_nan());
    assert_eq!(source.next(), None);
}

#[test]
fn persisted_settings_are_sanitized_and_default_to_bypass() {
    let old: Settings = serde_json::from_str("{}").unwrap();
    assert_eq!(old, Settings::default());
    let settings = Settings {
        bands: [100.; 10],
        preamp_db: f32::NAN,
        balance: -10.,
        ..enabled()
    }
    .validated();
    assert_eq!(settings.bands, [12.; 10]);
    assert_eq!(settings.preamp_db, 0.);
    assert_eq!(settings.balance, -1.);
}
