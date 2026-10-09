use crate::visualizer::{Analyzer, BANDS, Capture, Frame, Mode};
use eframe::egui::{self, Color32, Rect, Stroke, pos2, vec2};
use std::time::{Duration, Instant};

const STALE_AFTER: Duration = Duration::from_millis(180);

const FALLBACK: [Color32; 2] = [
    Color32::from_rgb(81, 225, 219),
    Color32::from_rgb(139, 157, 237),
];

pub struct Visualizer {
    pub mode: Mode,
    analyzer: Analyzer,
    cover: Option<String>,
    palette: [Color32; 2],
    palette_loaded: bool,
    last_frame: Option<(u64, u64)>,
    fresh_at: Instant,
}
impl Visualizer {
    pub fn new(mode: Mode) -> Self {
        Self {
            mode,
            analyzer: Analyzer::default(),
            cover: None,
            palette: FALLBACK,
            palette_loaded: false,
            last_frame: None,
            fresh_at: Instant::now(),
        }
    }
    pub fn set_mode(&mut self, mode: Mode) {
        self.mode = mode;
        self.analyzer.clear();
        self.last_frame = None;
    }
    fn update_capture(
        &mut self,
        capture: &Capture,
        frame: Option<Frame>,
        now: Instant,
        dt: f32,
    ) -> bool {
        if let Some(frame) = &frame {
            let identity = (frame.epoch, frame.serial);
            if self.last_frame != Some(identity) {
                self.last_frame = Some(identity);
                self.fresh_at = now;
            }
        }
        let fresh = self
            .last_frame
            .is_some_and(|(epoch, _)| capture.is_current(epoch))
            && now.saturating_duration_since(self.fresh_at) <= STALE_AFTER;
        if !fresh {
            self.analyzer.clear();
            // Keep the identity: rereading the same stalled frame must not revive it.
            return false;
        }
        if let Some(frame) = frame {
            self.analyzer
                .update(&frame, dt, self.mode == Mode::Spectrum);
        }
        // A busy publisher may prevent a coherent read. Retain the last analyzed
        // display without advancing it or extending its freshness deadline.
        true
    }
    pub fn paint(
        &mut self,
        ui: &egui::Ui,
        rect: Rect,
        capture: &Capture,
        active: bool,
        cover: Option<String>,
    ) {
        if self.mode == Mode::Off || !active {
            self.analyzer.clear();
            self.last_frame = None;
            return;
        }
        if self.cover != cover {
            self.cover = cover;
            self.palette = FALLBACK;
            self.palette_loaded = false;
        }
        if !self.palette_loaded
            && let Some(url) = &self.cover
        {
            match ui
                .ctx()
                .try_load_image(url, egui::load::SizeHint::Width(80))
            {
                Ok(egui::load::ImagePoll::Ready { image }) => {
                    self.palette = palette(&image.pixels);
                    self.palette_loaded = true;
                }
                Err(_) => self.palette_loaded = true,
                _ => {}
            }
        }
        // No animation timer while paused/off/minimized. A stalled stream goes dark
        // instead of animating stale samples indefinitely.
        ui.ctx().request_repaint_after(Duration::from_millis(33));
        if !self.update_capture(
            capture,
            capture.snapshot(),
            Instant::now(),
            ui.input(|input| input.stable_dt),
        ) {
            return;
        }
        let painter = ui.painter().with_clip_rect(rect.intersect(ui.clip_rect()));
        match self.mode {
            Mode::Spectrum => {
                let step = rect.width() / BANDS as f32;
                for i in 0..BANDS {
                    let value = self.analyzer.bars[i];
                    let height = value * rect.height() * 0.83;
                    let color = blend(
                        self.palette[0],
                        self.palette[1],
                        i as f32 / (BANDS - 1) as f32,
                    );
                    let x = rect.left() + (i as f32 + 0.5) * step;
                    if height > 0.5 {
                        let bar = Rect::from_center_size(
                            pos2(x, rect.bottom() - height / 2.),
                            vec2((step - 4.).max(1.), height),
                        );
                        painter.rect_filled(bar.expand(3.), 4., alpha(color, 9));
                        painter.rect_filled(bar, 2., alpha(color, 38));
                        painter.line_segment(
                            [bar.left_top(), bar.right_top()],
                            Stroke::new(1_f32, alpha(color, 95)),
                        );
                    }
                    let peak = self.analyzer.peaks[i];
                    if peak > 0.02 {
                        let y = rect.bottom() - peak * rect.height() * 0.83;
                        painter.line_segment(
                            [pos2(x - step * 0.24, y), pos2(x + step * 0.24, y)],
                            Stroke::new(1.5_f32, alpha(color, 120)),
                        );
                    }
                }
            }
            Mode::Waveform => {
                let points: Vec<_> = self
                    .analyzer
                    .wave
                    .iter()
                    .enumerate()
                    .map(|(i, sample)| {
                        pos2(
                            rect.left()
                                + i as f32 / (self.analyzer.wave.len() - 1) as f32 * rect.width(),
                            rect.center().y - (sample * 1.8).clamp(-1., 1.) * rect.height() * 0.40,
                        )
                    })
                    .collect();
                if self.analyzer.wave.iter().any(|v| v.abs() > 0.0001) {
                    for (width, color) in [
                        (10_f32, alpha(self.palette[0], 10)),
                        (4., alpha(self.palette[0], 25)),
                        (1.5, alpha(self.palette[1], 100)),
                    ] {
                        painter.add(egui::Shape::line(points.clone(), Stroke::new(width, color)));
                    }
                }
            }
            Mode::Off => {}
        }
    }
}

fn alpha(color: Color32, opacity: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), opacity)
}
fn blend(a: Color32, b: Color32, t: f32) -> Color32 {
    let mix = |a: u8, b: u8| (a as f32 * (1. - t) + b as f32 * t).round() as u8;
    Color32::from_rgb(mix(a.r(), b.r()), mix(a.g(), b.g()), mix(a.b(), b.b()))
}

fn palette(pixels: &[Color32]) -> [Color32; 2] {
    #[derive(Clone, Copy, Default)]
    struct Bucket {
        weight: f32,
        sum: [f32; 3],
    }
    let mut buckets = [Bucket::default(); 64];
    for pixel in pixels.iter().step_by((pixels.len() / 4096).max(1)) {
        if pixel.a() < 128 {
            continue;
        }
        let rgb = [pixel.r(), pixel.g(), pixel.b()];
        let max = *rgb.iter().max().unwrap() as f32;
        let min = *rgb.iter().min().unwrap() as f32;
        if !(35. ..=245.).contains(&max) {
            continue;
        }
        let weight = 0.2 + (max - min) / max;
        let bucket = &mut buckets
            [((rgb[0] >> 6) as usize * 16) + (rgb[1] >> 6) as usize * 4 + (rgb[2] >> 6) as usize];
        bucket.weight += weight;
        for (sum, channel) in bucket.sum.iter_mut().zip(rgb) {
            *sum += channel as f32 * weight;
        }
    }
    let colors: Vec<_> = buckets
        .iter()
        .filter(|b| b.weight > 0.)
        .map(|b| {
            let rgb = b.sum.map(|v| v / b.weight);
            let gain = 225. / rgb.into_iter().fold(0., f32::max).max(1.);
            (
                b.weight,
                Color32::from_rgb(
                    (rgb[0] * gain) as u8,
                    (rgb[1] * gain) as u8,
                    (rgb[2] * gain) as u8,
                ),
            )
        })
        .collect();
    let Some(&(weight, primary)) = colors.iter().max_by(|a, b| a.0.total_cmp(&b.0)) else {
        return FALLBACK;
    };
    let secondary = colors
        .iter()
        .filter(|(w, _)| *w >= weight * 0.05)
        .max_by(|a, b| {
            let score = |(weight, c): &(f32, Color32)| {
                let distance = (c.r() as f32 - primary.r() as f32).powi(2)
                    + (c.g() as f32 - primary.g() as f32).powi(2)
                    + (c.b() as f32 - primary.b() as f32).powi(2);
                distance * weight.sqrt()
            };
            score(a).total_cmp(&score(b))
        })
        .map_or(primary, |(_, c)| *c);
    [primary, secondary]
}

#[cfg(test)]
mod tests {
    use super::*;
    fn feed(capture: &std::sync::Arc<Capture>) {
        let samples = (0..4096)
            .map(|i| (std::f32::consts::TAU * 440. * i as f32 / 48000.).sin() * 0.7)
            .collect::<Vec<_>>();
        let source = rodio::buffer::SamplesBuffer::new(1, 48000, samples);
        let generation = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(1));
        crate::visualizer::Tap::new(source, capture.clone(), generation, 1).for_each(drop);
    }
    fn signal() -> std::sync::Arc<Capture> {
        let capture = std::sync::Arc::new(Capture::default());
        capture.enable(true);
        capture.playing(true);
        feed(&capture);
        capture
    }
    #[test]
    fn transient_snapshot_misses_preserve_both_displays_but_never_extend_freshness() {
        for mode in [Mode::Spectrum, Mode::Waveform] {
            let capture = signal();
            let mut visualizer = Visualizer::new(mode);
            let now = Instant::now();
            assert!(visualizer.update_capture(&capture, capture.snapshot(), now, 0.033));
            let bars = visualizer.analyzer.bars;
            let wave = visualizer.analyzer.wave;
            assert!(wave.iter().any(|value| value.abs() > 0.01));
            if mode == Mode::Spectrum {
                assert!(bars.iter().any(|value| *value > 0.01));
            }
            for millis in [33, 66, 99, 132, 165, 180] {
                assert!(visualizer.update_capture(
                    &capture,
                    None,
                    now + Duration::from_millis(millis),
                    0.033
                ));
                assert_eq!(visualizer.analyzer.bars, bars);
                assert_eq!(visualizer.analyzer.wave, wave);
            }
            assert!(!visualizer.update_capture(
                &capture,
                None,
                now + Duration::from_millis(181),
                0.033
            ));
            // Reading the same old publication successfully cannot revive a stalled display.
            for millis in [200, 250, 400] {
                assert!(!visualizer.update_capture(
                    &capture,
                    capture.snapshot(),
                    now + Duration::from_millis(millis),
                    0.033
                ));
                assert!(visualizer.analyzer.wave.iter().all(|value| *value == 0.));
            }
            feed(&capture);
            assert!(visualizer.update_capture(
                &capture,
                capture.snapshot(),
                now + Duration::from_millis(401),
                0.033
            ));
            assert!(
                visualizer
                    .analyzer
                    .wave
                    .iter()
                    .any(|value| value.abs() > 0.01)
            );
        }
    }
    #[test]
    fn snapshot_misses_never_reuse_invalidated_paused_or_disabled_data() {
        for transition in 0..3 {
            let capture = signal();
            let mut visualizer = Visualizer::new(Mode::Spectrum);
            let now = Instant::now();
            assert!(visualizer.update_capture(&capture, capture.snapshot(), now, 0.033));
            match transition {
                0 => capture.invalidate(),
                1 => capture.playing(false),
                _ => capture.enable(false),
            }
            assert!(capture.snapshot().is_none());
            assert!(!visualizer.update_capture(
                &capture,
                None,
                now + Duration::from_millis(1),
                0.033
            ));
            assert!(visualizer.analyzer.bars.iter().all(|value| *value == 0.));
            assert!(visualizer.analyzer.wave.iter().all(|value| *value == 0.));
        }
    }
    #[test]
    fn inactive_or_off_paint_clears_even_a_fresh_cached_display() {
        for mode in [Mode::Spectrum, Mode::Off] {
            let capture = signal();
            let mut visualizer = Visualizer::new(Mode::Spectrum);
            assert!(visualizer.update_capture(&capture, capture.snapshot(), Instant::now(), 0.033));
            visualizer.mode = mode;
            let ctx = egui::Context::default();
            let _ = ctx.run(egui::RawInput::default(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    visualizer.paint(ui, ui.max_rect(), &capture, mode == Mode::Off, None)
                });
            });
            assert!(visualizer.last_frame.is_none());
            assert!(visualizer.analyzer.wave.iter().all(|value| *value == 0.));
        }
    }

    #[test]
    fn artwork_colors_ignore_transparency_and_do_not_invent_random_colors() {
        assert_eq!(palette(&[]), FALLBACK);
        assert_eq!(palette(&[Color32::TRANSPARENT, Color32::BLACK]), FALLBACK);
        let colors = palette(&[
            Color32::from_rgb(200, 30, 20),
            Color32::from_rgb(180, 20, 10),
            Color32::from_rgb(20, 40, 180),
        ]);
        assert!(colors[0].r() > colors[0].b());
        assert!(colors[1].b() > colors[1].r());
        assert_eq!(
            colors,
            palette(&[
                Color32::from_rgb(200, 30, 20),
                Color32::from_rgb(180, 20, 10),
                Color32::from_rgb(20, 40, 180)
            ])
        );
    }
}
