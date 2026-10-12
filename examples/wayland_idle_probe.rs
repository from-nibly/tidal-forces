//! Account-free rendering diagnostic. Run only with permission on a live desktop.
use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

struct Probe(Arc<AtomicUsize>);
impl eframe::App for Probe {
    fn update(&mut self, ctx: &eframe::egui::Context, _: &mut eframe::Frame) {
        self.0.fetch_add(1, Ordering::Relaxed);
        eframe::egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Tidal rendering diagnostic");
            ui.label("No account, network or audio access. This window closes automatically.");
        });
        ctx.request_repaint_after(Duration::from_millis(16));
    }
}
fn main() -> eframe::Result {
    let vsync = std::env::args().nth(1).as_deref() != Some("no-vsync");
    let frames = Arc::new(AtomicUsize::new(0));
    eframe::run_native(
        "Tidal rendering diagnostic",
        eframe::NativeOptions {
            viewport: eframe::egui::ViewportBuilder::default()
                .with_app_id("rocks.tidalforces.RenderProbe")
                .with_inner_size([440., 160.]),
            renderer: eframe::Renderer::Glow,
            vsync,
            ..Default::default()
        },
        Box::new(move |cc| {
            let ctx = cc.egui_ctx.clone();
            let count = frames.clone();
            std::thread::spawn(move || {
                let start = Instant::now();
                for _ in 0..25 {
                    std::thread::sleep(Duration::from_secs(1));
                    eprintln!(
                        "elapsed={} updates={}",
                        start.elapsed().as_secs(),
                        count.load(Ordering::Relaxed)
                    );
                }
                ctx.send_viewport_cmd(eframe::egui::ViewportCommand::Close);
            });
            Ok(Box::new(Probe(frames)))
        }),
    )
}
