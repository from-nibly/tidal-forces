//! Debug-only native screenshots: synthetic catalog, no backend, device, D-Bus or storage.
use super::*;
use anyhow::{Context, Result};

pub(crate) fn capture(args: &[String]) -> Result<()> {
    let path = std::path::PathBuf::from(args.first().context("Supply a screenshot path")?);
    let page = args.get(1).map_or("collection", String::as_str).to_owned();
    anyhow::ensure!(
        [
            "collection",
            "selection",
            "view",
            "home",
            "albums",
            "artists",
            "history",
            "settings"
        ]
        .contains(&page.as_str()),
        "Unknown fixture page"
    );
    let width: f32 = args.get(2).map_or(Ok(1240.), |v| v.parse())?;
    let height: f32 = args.get(3).map_or(Ok(820.), |v| v.parse())?;
    let zoom: f32 = args.get(4).map_or(Ok(1.), |v| v.parse())?;
    anyhow::ensure!(
        width.is_finite()
            && (600. ..=2560.).contains(&width)
            && height.is_finite()
            && (440. ..=1440.).contains(&height)
            && (0.75..=1.5).contains(&zoom),
        "Invalid fixture dimensions"
    );
    let destination = path.clone();
    eframe::run_native(
        "Tidal Forces · Synthetic visual fixture",
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_app_id("rocks.tidalforces.VisualFixture")
                .with_inner_size([width, height]),
            renderer: eframe::Renderer::Glow,
            ..Default::default()
        },
        Box::new(move |cc| {
            let ctx = cc.egui_ctx.clone();
            let mut app = fixture(&ctx, &page);
            ctx.set_zoom_factor(zoom);
            app.screenshot = Some(destination);
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_secs(2));
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
                std::thread::sleep(std::time::Duration::from_secs(5));
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            });
            Ok(Box::new(Capture(app)))
        }),
    )
    .map_err(|e| anyhow::anyhow!("Native fixture failed: {e}"))?;
    anyhow::ensure!(path.is_file(), "Fixture screenshot was not produced");
    Ok(())
}

struct Capture(App);
impl eframe::App for Capture {
    fn raw_input_hook(&mut self, _ctx: &egui::Context, input: &mut egui::RawInput) {
        // Strip input before egui computes clicks; this is never an interactive fake account.
        input
            .events
            .retain(|event| matches!(event, egui::Event::Screenshot { .. }));
        input.modifiers = egui::Modifiers::NONE;
        input.focused = false;
    }
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        self.0.update(ctx, frame);
    }
}

fn fixture(ctx: &egui::Context, page: &str) -> App {
    theme::configure(ctx);
    // Deliberately install only the decoder, not HTTP or file byte loaders.
    ctx.add_image_loader(std::sync::Arc::new(
        egui_extras::loaders::image_loader::ImageCrateLoader::default(),
    ));
    let (tx, _requests) = tokio::sync::mpsc::channel(1);
    let (_events, rx) = std::sync::mpsc::channel();
    let mut app = App::with_backend(
        Backend {
            tx,
            rx,
            player: crate::audio::Player::inert(),
        },
        crate::store::Appearance::default(),
    );
    app.connected = true;
    app.account = Some(7);
    app.country = "DEMO".into();
    let titles = [
        "Afterimage",
        "Paper Atlas",
        "Glass Cities",
        "Between Stations",
        "Daybreak, Slowly",
        "Blue Frequency",
        "A Place to Return",
        "The Quiet Hours",
    ];
    let artists = [
        "Signal Park",
        "Aster Fields",
        "June Meridian",
        "North Arcade",
    ];
    let albums = [
        "Night Transit",
        "Small Hours",
        "Soft Geometry",
        "Field Notes",
    ];
    let palette = [
        [62, 128, 135],
        [171, 101, 75],
        [122, 103, 157],
        [105, 138, 107],
    ];
    for (index, color) in palette.into_iter().enumerate() {
        let image = image::RgbaImage::from_fn(160, 160, |x, y| {
            let x = x as f32 / 160.;
            let y = y as f32 / 160.;
            let curve = ((x * 2.4 + y * 1.2 + index as f32).sin() * 0.5 + 0.5) * 0.75 + 0.25;
            let halo = (1. - ((x - 0.55).powi(2) + (y - 0.35).powi(2)).sqrt()).max(0.);
            image::Rgba(
                [
                    color[0] as f32 * curve + 35. * halo,
                    color[1] as f32 * curve + 35. * halo,
                    color[2] as f32 * curve + 35. * halo,
                    255.,
                ]
                .map(|v| v as u8),
            )
        });
        let mut bytes = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(image)
            .write_to(&mut bytes, image::ImageFormat::Png)
            .unwrap();
        for size in [80, 320] {
            ctx.include_bytes(
                cover_url(Some(&format!("fixture-{index}")), size).unwrap(),
                bytes.get_ref().clone(),
            );
        }
    }
    app.tracks = titles
        .iter()
        .enumerate()
        .map(|(index, title)| {
            let i = index % 4;
            Track {
                id: index as u64 + 1,
                title: (*title).into(),
                duration: 220 + index as u64 * 7,
                artist: Artist {
                    id: i as u64 + 1,
                    name: artists[i].into(),
                    ..Default::default()
                },
                album: Album {
                    id: i as u64 + 1,
                    title: albums[i].into(),
                    cover: Some(format!("fixture-{i}")),
                    ..Default::default()
                },
                explicit: index == 2,
            }
        })
        .collect();
    app.queue
        .start(app.tracks.clone(), 0, "Night drives".into(), None)
        .unwrap();
    for track in app.tracks[1..3].iter().cloned() {
        app.queue.add(track, false).unwrap();
    }
    let mut history = crate::history::History::new(7);
    history.set_enabled(true);
    let mut listener = crate::history::Listener::default();
    for (index, track) in app.tracks[3..6].iter().enumerate() {
        let now = crate::history::now() - 300 * (3 - index as u64);
        for seconds in [0, 30] {
            listener.observe(
                Some(&mut history),
                index as u64,
                std::sync::Arc::new(track.clone()),
                std::time::Duration::from_secs(seconds),
                std::time::Duration::from_secs(seconds),
                now + seconds,
            );
        }
    }
    app.history_loaded(7, 0, Ok(history));
    for id in 1..=4 {
        app.favorites
            .observe(Favorite::new(FavoriteKind::Albums, id));
        app.favorites
            .observe(Favorite::new(FavoriteKind::Artists, id));
    }
    app.position = 84;
    app.actual_quality = "FIXTURE · FLAC 16/44.1".into();
    app.queue_open = matches!(page, "collection" | "selection" | "view" | "history");
    app.folders.insert(
        "root".into(),
        ["Night drives", "Slow mornings"]
            .iter()
            .enumerate()
            .map(|(id, title)| {
                LibraryEntry::Playlist(Playlist {
                    uuid: format!("fixture-{id}"),
                    title: (*title).into(),
                    number_of_tracks: 8,
                    square_image: Some(format!("fixture-{id}")),
                    ..Default::default()
                })
            })
            .collect(),
    );
    for (id, name) in [("rotation", "In rotation"), ("later", "For later")] {
        app.folders
            .get_mut("root")
            .unwrap()
            .push(LibraryEntry::Folder {
                id: id.into(),
                name: name.into(),
                count: 0,
            });
        app.folders.insert(id.into(), Vec::new());
    }
    if matches!(page, "collection" | "selection" | "view") {
        app.playlist_page = Some(PlaylistPage {
            playlist: Playlist {
                uuid: "fixture-0".into(),
                title: "Night drives".into(),
                square_image: Some("fixture-0".into()),
                number_of_tracks: 8,
                ..Default::default()
            },
            etag: "fixture-revision".into(),
            editable: true,
            rows: app.tracks.iter().cloned().enumerate().collect(),
            next_offset: 8,
            more: false,
        });
    }
    for id in [1, 3, 5] {
        app.favorites
            .observe(Favorite::new(FavoriteKind::Tracks, id));
    }
    app.page = match page {
        "settings" => {
            app.settings_section = SettingsSection::Appearance;
            Page::Settings
        }
        "albums" => {
            app.albums = app.tracks[..4]
                .iter()
                .map(|t| Album {
                    artist: t.artist.clone(),
                    ..t.album.clone()
                })
                .collect();
            Page::Library(FavoriteKind::Albums)
        }
        "artists" => {
            app.artists = app.tracks[..4]
                .iter()
                .map(|t| Artist {
                    picture: t.album.cover.clone(),
                    ..t.artist.clone()
                })
                .collect();
            Page::Library(FavoriteKind::Artists)
        }
        "home" => {
            app.mixes = (0..4).map(|i| Mix { id: format!("fixture-{i}"), title: ["Daily Discovery", "Late-night signals", "A softer start", "On repeat"][i].into(), sub_title: "A mix for your day".into(), images: serde_json::json!({"MEDIUM":{"url":cover_url(Some(&format!("fixture-{i}")),320)}}), ..Default::default() }).collect();
            app.daily = app.mixes.first().cloned();
            Page::Home
        }
        "history" => {
            app.listening.show_panel = true;
            Page::Home
        }
        _ => Page::Collection {
            kind: "playlists".into(),
            id: "fixture-0".into(),
            title: "Night drives".into(),
        },
    };
    if page == "view" {
        app.track_view.query = "Signal".into();
        app.track_view.sort = track_view::Sort::Title;
    }
    if page == "selection" {
        app.track_selection
            .sync(app.account, app.generation, app.tracks.len());
        for row in [0, 1, 3] {
            app.track_selection.select(row, true, false);
        }
    }
    app
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::App as _;
    #[test]
    fn fixture_has_no_live_services_and_strips_input_before_widget_interaction() {
        let ctx = egui::Context::default();
        let app = fixture(&ctx, "collection");
        assert!(app.backend.tx.is_closed());
        assert!(app.state_store.is_none() && app.media.is_none() && app.instance.is_none());
        assert!(
            ctx.try_load_bytes("https://example.invalid/unregistered.png")
                .is_err()
        );
        assert!(ctx.try_load_bytes("file:///not-a-fixture.png").is_err());
        let mut capture = Capture(app);
        let mut raw = egui::RawInput {
            events: vec![
                egui::Event::PointerButton {
                    pos: pos2(20., 20.),
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
                egui::Event::Text("must not edit".into()),
            ],
            ..Default::default()
        };
        capture.raw_input_hook(&ctx, &mut raw);
        assert!(raw.events.is_empty() && !raw.focused);
    }
}
