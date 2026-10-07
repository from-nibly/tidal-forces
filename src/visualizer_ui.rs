use crate::visualizer::{Analyzer, BANDS, Capture, Mode};
use eframe::egui::{self, Color32, Rect, Stroke, pos2, vec2};
use std::time::{Duration, Instant};

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
    last_frame: Option<u64>,
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
        let Some(frame) = capture.snapshot() else {
            self.analyzer.clear();
            return;
        };
        if self.last_frame != Some(frame.serial) {
            self.last_frame = Some(frame.serial);
            self.fresh_at = Instant::now();
        }
        if self.fresh_at.elapsed() > Duration::from_millis(180) {
            self.analyzer.clear();
            return;
        }
        self.analyzer.update(
            &frame,
            ui.input(|i| i.stable_dt),
            self.mode == Mode::Spectrum,
        );
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
