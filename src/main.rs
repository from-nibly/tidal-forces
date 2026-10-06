mod aac;
mod api;
mod audio;
#[cfg(test)]
mod audio_tests;
mod auth;
mod backend;
mod dash;
mod desktop;
mod install;
mod licenses;
mod links;
mod model;
mod queue;
mod store;
mod ui;

use anyhow::{Context, Result};

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let smoke_dir = tempfile::tempdir()?;
    let mut link_uri = None;
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
        Some("--licenses") => {
            licenses::print();
            return Ok(());
        }
        Some("--export-fdk-source") => {
            return licenses::export(args.get(1).context("Supply an output path")?);
        }
        Some("--audio-test") => return audio::audio_test(),
        Some("--check-account") => return check_account(args.iter().any(|a| a == "--refresh")),
        Some("--verify-playback") => {
            return verify_playback(
                args.get(1).context("Supply a TIDAL track ID")?.parse()?,
                args.iter().any(|a| a == "--seek"),
            );
        }
        Some("--help") => {
            println!(
                "Tidal Forces — native TIDAL player\n\n  --install           Install this binary and a desktop launcher for this user\n  --audio-test        Play a quiet 150 ms test tone\n  --check-account     Verify saved account, search, favorites and playlists\n  --verify-playback ID  Verify 9s of preferred-quality audio; --seek also tests pause and seek\n  --version           Show version\n  --licenses          Show bundled codec and font license notices\n  --export-fdk-source PATH  Export the complete bundled AAC codec source\n  --open URI          Open a tidal:// or official TIDAL web link\n\nStart without arguments to open the player, or pass a TIDAL URI directly."
            );
            return Ok(());
        }
        Some("--open") => {
            link_uri = Some(args.get(1).context("Supply a TIDAL link")?.as_str());
        }
        Some(arg) if arg.starts_with("tidal:") || arg.starts_with("https:") => {
            link_uri = Some(arg);
        }
        Some("--smoke-ui" | "--screenshot") => {}
        Some(arg) => anyhow::bail!("Unknown argument: {arg}. Use --help."),
        None => {}
    }
    let initial_link = link_uri.map(links::Link::parse).transpose()?;
    let instance = if screenshot.is_none() {
        match links::Instance::start(link_uri)? {
            Some(instance) => Some(instance),
            None => return Ok(()),
        }
    } else {
        None
    };
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
            Ok(Box::new(ui::App::new(cc, capture, instance, initial_link)))
        }),
    )
    .map_err(|e| anyhow::anyhow!("Could not start the desktop player: {e}"))?;
    if let Some(path) = screenshot {
        anyhow::ensure!(path.is_file(), "Native screenshot smoke test failed");
        println!("Rendered native window: {}", path.display());
    }
    Ok(())
}

fn check_account(refresh: bool) -> Result<()> {
    tokio::runtime::Runtime::new()?.block_on(async {
        let mut api = api::Api::new()?;
        api.session = store::load()?;
        if refresh {
            api.session.as_mut().context("Sign in first")?.expires_at = 0;
        }
        api.identify().await?;
        println!(
            "Authenticated account in {}",
            api.session.as_ref().unwrap().country
        );
        println!(
            "Favorite tracks (first page): {}",
            api.favorites(0).await?.len()
        );
        let folders = api.folder("root").await?;
        println!("Root folder entries: {}", folders.len());
        if let Some(model::LibraryEntry::Folder { id, .. }) = folders
            .iter()
            .find(|e| matches!(e, model::LibraryEntry::Folder { .. }))
        {
            println!("First folder contents: {}", api.folder(id).await?.len());
        }
        let home = api.home().await?;
        println!(
            "Personal mixes: {}; Daily Discovery tracks: {}",
            home.mixes.len(),
            home.tracks.len()
        );
        let results = api.search("Massive Attack Teardrop").await?;
        println!(
            "Search returned {} tracks and {} albums",
            results.tracks.len(),
            results.albums.len()
        );
        if let Some(track) = results.tracks.first() {
            println!(
                "Track radio: {} tracks",
                api.radio(&model::RadioSeed::Track {
                    id: track.id,
                    title: track.title.clone()
                })
                .await?
                .len()
            );
            if track.artist.id != 0 {
                println!(
                    "Artist radio: {} tracks",
                    api.radio(&model::RadioSeed::Artist {
                        id: track.artist.id,
                        name: track.artist.name.clone()
                    })
                    .await?
                    .len()
                );
            }
        }
        Ok(())
    })
}

fn verify_playback(id: u64, seek: bool) -> Result<()> {
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
    let mut seeking = false;
    while start.elapsed() < Duration::from_secs(90) {
        if let Ok(event) = backend.rx.recv_timeout(Duration::from_secs(1)) {
            match event {
                Event::Playing { quality, .. } => println!("Decoder started: {quality}"),
                Event::Position { seconds, .. } if !seeking && seconds >= 9 => {
                    if seek {
                        backend.player.pause(true);
                        std::thread::sleep(Duration::from_millis(500));
                        backend.player.seek(45);
                        backend.player.pause(false);
                        seeking = true;
                        println!(
                            "Nine seconds across segment boundaries passed; testing seek to 45s."
                        );
                    } else {
                        backend.player.stop();
                        println!(
                            "Verified authenticated playback across segment boundaries for {seconds}s."
                        );
                        return Ok(());
                    }
                }
                Event::Position { seconds, .. } if seeking && seconds >= 50 => {
                    backend.player.stop();
                    println!(
                        "Verified continuous playback, pause/resume, seek, and playback after seeking."
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
