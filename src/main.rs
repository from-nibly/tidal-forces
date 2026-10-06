mod api;
mod audio;
#[cfg(test)]
mod audio_tests;
mod backend;
mod install;
mod model;
mod queue;
mod store;
mod ui;

use anyhow::{Context, Result};

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let smoke_dir = tempfile::tempdir()?;
    let screenshot = match args.first().map(String::as_str) {
        Some("--smoke-ui") => Some(smoke_dir.path().join("smoke.png")),
        Some("--screenshot") => Some(std::path::PathBuf::from(
            args.get(1).context("Supply a screenshot path")?,
        )),
        _ => None,
    };
    match args.first().map(String::as_str) {
        Some("--version") => {
            println!("Tidal Forces {}", env!("CARGO_PKG_VERSION"));
            return Ok(());
        }
        Some("--install") => {
            install::install()?;
            return Ok(());
        }
        Some("--audio-test") => return audio::audio_test(),
        Some("--check-account") => return check_account(),
        Some("--verify-playback") => {
            return verify_playback(args.get(1).context("Supply a TIDAL track ID")?.parse()?);
        }
        Some("--help") => {
            println!(
                "Tidal Forces — native TIDAL player\n\n  --install           Install this binary and a desktop launcher for this user\n  --audio-test        Play a quiet 150 ms test tone\n  --check-account     Verify saved account, search, favorites and playlists\n  --verify-playback ID  Play a TIDAL track for 3 seconds to verify streaming\n  --version           Show version\n\nStart without arguments to open the player."
            );
            return Ok(());
        }
        Some("--smoke-ui" | "--screenshot") => {}
        Some(arg) => anyhow::bail!("Unknown argument: {arg}. Use --help."),
        None => {}
    }
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_app_id("rocks.tidalforces.Player")
            .with_icon(eframe::icon_data::from_png_bytes(include_bytes!(
                "../assets/icon.png"
            ))?)
            .with_inner_size([1240., 820.])
            .with_min_inner_size([1000., 660.]),
        renderer: eframe::Renderer::Glow,
        vsync: true,
        ..Default::default()
    };
    let capture = screenshot.clone();
    eframe::run_native(
        "Tidal Forces",
        options,
        Box::new(move |cc| {
            if capture.is_some() {
                let ctx = cc.egui_ctx.clone();
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_secs(3));
                    ctx.send_viewport_cmd(eframe::egui::ViewportCommand::Screenshot(
                        Default::default(),
                    ));
                    std::thread::sleep(std::time::Duration::from_secs(7));
                    ctx.send_viewport_cmd(eframe::egui::ViewportCommand::Close);
                });
            }
            Ok(Box::new(ui::App::new(cc, capture)))
        }),
    )
    .map_err(|e| anyhow::anyhow!("Could not start the desktop player: {e}"))?;
    if let Some(path) = screenshot {
        anyhow::ensure!(path.is_file(), "Native screenshot smoke test failed");
        println!("Rendered native window: {}", path.display());
    }
    Ok(())
}

fn check_account() -> Result<()> {
    tokio::runtime::Runtime::new()?.block_on(async {
        let mut api = api::Api::new()?;
        api.session = store::load()?;
        api.identify().await?;
        println!(
            "Authenticated account in {}",
            api.session.as_ref().unwrap().country
        );
        println!(
            "Favorite tracks (first page): {}",
            api.favorites(0).await?.len()
        );
        println!("Playlists (first page): {}", api.playlists().await?.len());
        let results = api.search("Massive Attack Teardrop").await?;
        println!(
            "Search returned {} tracks and {} albums",
            results.tracks.len(),
            results.albums.len()
        );
        Ok(())
    })
}

fn verify_playback(id: u64) -> Result<()> {
    use backend::{Backend, Event, Request};
    use std::time::{Duration, Instant};
    let backend = Backend::new(eframe::egui::Context::default());
    let generation = backend.player.reserve();
    backend.tx.blocking_send(Request::Play {
        generation,
        id,
        quality: "LOSSLESS".into(),
    })?;
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(90) {
        if let Ok(event) = backend.rx.recv_timeout(Duration::from_secs(1)) {
            match event {
                Event::Playing { quality, .. } => println!("Decoder started: {quality}"),
                Event::Position { seconds, .. } if seconds >= 3 => {
                    backend.player.stop();
                    println!(
                        "Verified authenticated TIDAL streaming, decoding and audio output for {seconds}s."
                    );
                    return Ok(());
                }
                Event::Error(e)
                | Event::AudioError(e)
                | Event::PlaybackError { message: e, .. } => anyhow::bail!(e),
                _ => {}
            }
        }
    }
    backend.player.stop();
    anyhow::bail!("Playback verification timed out")
}
