use super::super::tests::{fixture, track};
use super::*;

fn install(ctx: &egui::Context, app: &mut App) {
    let mut skin = Skin::parse(&crate::skins::fixture::bytes()).unwrap();
    skin.images
        .insert(Asset::Title, image::RgbaImage::new(302, 29));
    app.classic.skin = Some(Arc::new(Textures::upload(ctx, skin)));
    app.appearance.classic_skin = true;
    app.mini_open = true;
    app.audio_available = true;
}
fn draw(
    ctx: &egui::Context,
    app: &mut App,
    size: egui::Vec2,
    events: Vec<egui::Event>,
) -> (egui::FullOutput, egui::Id) {
    let mut id = egui::Id::NULL;
    let output = ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
            events,
            ..Default::default()
        },
        |ctx| {
            egui::CentralPanel::default()
                .frame(egui::Frame::new().inner_margin(16))
                .show(ctx, |ui| {
                    id = ui.id();
                    app.mini_contents(ui);
                    assert!(ui.min_rect().bottom() <= ui.max_rect().bottom() + 1.);
                    assert!(ui.min_rect().right() <= ui.max_rect().right() + 1.);
                });
        },
    );
    (output, id)
}
fn click(ctx: &egui::Context, app: &mut App, id: egui::Id) {
    let pos = ctx.read_response(id).unwrap().rect.center();
    for pressed in [true, false] {
        draw(
            ctx,
            app,
            vec2(380., 250.),
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
    }
}
fn key(ctx: &egui::Context, app: &mut App, id: egui::Id, key: egui::Key) {
    ctx.memory_mut(|memory| memory.request_focus(id));
    draw(
        ctx,
        app,
        vec2(380., 250.),
        vec![egui::Event::Key {
            key,
            physical_key: Some(key),
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }],
    );
    draw(
        ctx,
        app,
        vec2(380., 250.),
        vec![egui::Event::Key {
            key,
            physical_key: Some(key),
            pressed: false,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }],
    );
}
#[test]
fn skin_mode_keeps_quality_visible_and_does_not_create_a_playback_owner() {
    let (ctx, mut app, _, mut requests) = fixture();
    app.queue_mut()
        .start(vec![track(1)], 0, "Fixture".into(), None)
        .unwrap();
    app.actual_quality = "FLAC · fixture source".into();
    let before = (
        app.queue.revision(),
        app.play_generation,
        app.paused,
        app.volume,
    );
    install(&ctx, &mut app);
    for size in [vec2(380., 250.), vec2(460., 260.), vec2(582., 340.)] {
        for _ in 0..2 {
            let (output, _) = draw(&ctx, &mut app, size, vec![]);
            assert!(
                output.shapes.iter().any(
                    |s| matches!(&s.shape,egui::Shape::Rect(r) if r.rect.size()==vec2(21.,15.))
                ),
                "Classic player must expose the Cast control"
            );
            let mut quality = false;
            for shape in output.shapes {
                if let egui::Shape::Text(text) = shape.shape
                    && text.galley.job.text.contains("fixture source")
                {
                    quality = true;
                    assert!(text.pos.y + text.galley.size().y <= shape.clip_rect.bottom());
                    assert!(text.pos.y + text.galley.size().y <= size.y);
                }
            }
            assert!(quality);
        }
    }
    assert_eq!(
        (
            app.queue.revision(),
            app.play_generation,
            app.paused,
            app.volume
        ),
        before
    );
    assert!(requests.try_recv().is_err());
}
#[test]
fn classic_transport_keyboard_sliders_and_close_share_authoritative_state() {
    let (ctx, mut app, _, mut requests) = fixture();
    app.queue_mut()
        .start(vec![track(1), track(2)], 0, "Fixture".into(), None)
        .unwrap();
    install(&ctx, &mut app);
    app.paused = true;
    draw(&ctx, &mut app, vec2(380., 250.), vec![]);
    let (_, id) = draw(&ctx, &mut app, vec2(380., 250.), vec![]);
    click(&ctx, &mut app, id.with(("classic", "Play")));
    assert!(!app.paused);
    assert!(matches!(requests.try_recv().unwrap(), Request::Play { .. }));
    click(&ctx, &mut app, id.with(("classic", "Pause")));
    assert!(app.paused);
    let occurrence = app.queue.current_entry().unwrap().occurrence;
    let generation = app.play_generation;
    let volume = app.volume;
    key(
        &ctx,
        &mut app,
        id.with(("classic-slider", "Volume", 0u64)),
        egui::Key::ArrowLeft,
    );
    assert!(app.volume < volume);
    assert_eq!(app.play_generation, generation);
    // The fake backend does not decode; acknowledge load readiness before seeking.
    app.buffering = false;
    app.actual_quality = "FLAC fixture".into();
    draw(&ctx, &mut app, vec2(380., 250.), vec![]);
    key(
        &ctx,
        &mut app,
        id.with(("classic-slider", "Seek", generation)),
        egui::Key::ArrowRight,
    );
    assert!(app.position_time() > std::time::Duration::ZERO);
    assert!(app.paused);
    assert_eq!(app.queue.current_entry().unwrap().occurrence, occurrence);
    assert!(matches!(
        requests.try_recv().unwrap(),
        Request::Play { .. } | Request::SeekBuffered { .. }
    ));
    let before = (app.queue.revision(), app.play_generation, app.paused);
    click(&ctx, &mut app, id.with(("classic", "Close mini")));
    assert!(!app.mini_open && !app.quitting);
    assert_eq!(
        (app.queue.revision(), app.play_generation, app.paused),
        before
    );
    assert!(requests.try_recv().is_err());
}
#[test]
fn seek_drag_cannot_cross_playback_generations_or_seek_while_buffering() {
    let (ctx, mut app, _, mut requests) = fixture();
    app.queue_mut()
        .start(vec![track(1)], 0, "Fixture".into(), None)
        .unwrap();
    install(&ctx, &mut app);
    app.paused = true;
    app.actual_quality = "FLAC fixture".into();
    draw(&ctx, &mut app, vec2(380., 250.), vec![]);
    let (_, id) = draw(&ctx, &mut app, vec2(380., 250.), vec![]);
    let epoch = app.play_generation;
    app.buffering = true;
    key(
        &ctx,
        &mut app,
        id.with(("classic-slider", "Seek", epoch)),
        egui::Key::ArrowRight,
    );
    assert_eq!(app.position_time(), std::time::Duration::ZERO);
    assert!(requests.try_recv().is_err());
    app.buffering = false;
    draw(&ctx, &mut app, vec2(380., 250.), vec![]);
    let rect = ctx
        .read_response(id.with(("classic-slider", "Seek", epoch)))
        .unwrap()
        .rect;
    let start = pos2(rect.left() + rect.width() * 0.2, rect.center().y);
    let end = pos2(rect.left() + rect.width() * 0.8, rect.center().y);
    draw(
        &ctx,
        &mut app,
        vec2(380., 250.),
        vec![
            egui::Event::PointerMoved(start),
            egui::Event::PointerButton {
                pos: start,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            },
        ],
    );
    draw(
        &ctx,
        &mut app,
        vec2(380., 250.),
        vec![egui::Event::PointerMoved(end)],
    );
    assert!(requests.try_recv().is_err());
    app.play_generation += 1;
    draw(
        &ctx,
        &mut app,
        vec2(380., 250.),
        vec![egui::Event::PointerButton {
            pos: end,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        }],
    );
    assert_eq!(app.position_time(), std::time::Duration::ZERO);
    assert!(requests.try_recv().is_err());
}

#[test]
fn failed_import_and_cancelled_activation_preserve_current_mode_and_playback() {
    let (ctx, mut app, _, mut requests) = fixture();
    install(&ctx, &mut app);
    let original = app.classic.skin.clone().unwrap();
    let before = (
        app.queue.revision(),
        app.play_generation,
        app.paused,
        app.volume,
    );
    let (tx, rx) = mpsc::channel();
    app.classic.pending = Some((rx, true));
    tx.send(Err(anyhow::anyhow!("invalid fixture"))).unwrap();
    app.classic_tick(&ctx);
    assert!(Arc::ptr_eq(&original, app.classic.skin.as_ref().unwrap()));
    assert!(app.appearance.classic_skin);
    let (tx, rx) = mpsc::channel();
    app.classic.pending = Some((rx, false));
    app.appearance.classic_skin = false;
    tx.send(Ok(Skin::parse(&crate::skins::fixture::bytes()).unwrap()))
        .unwrap();
    app.classic_tick(&ctx);
    assert!(!app.appearance.classic_skin);
    assert_eq!(
        (
            app.queue.revision(),
            app.play_generation,
            app.paused,
            app.volume
        ),
        before
    );
    assert!(requests.try_recv().is_err());
}

#[test]
#[ignore = "private profile only: python3 tests/skin-import.py"]
fn classic_skin_import_on_private_profile() {
    let root = std::path::PathBuf::from(
        std::env::var_os("TIDAL_TEST_SKINS_ROOT").expect("Use tests/skin-import.py"),
    );
    assert_eq!(root.canonicalize().unwrap(), root);
    assert_eq!(root.parent().unwrap(), std::path::Path::new("/tmp"));
    assert!(
        root.file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .starts_with("tidal-skin-test-")
    );
    for (key, part) in [
        ("HOME", "home"),
        ("XDG_CONFIG_HOME", "config"),
        ("XDG_DATA_HOME", "data"),
        ("XDG_STATE_HOME", "state"),
        ("XDG_CACHE_HOME", "cache"),
    ] {
        assert_eq!(std::env::var_os(key).unwrap(), root.join(part).as_os_str());
        assert_eq!(root.join(part).canonicalize().unwrap(), root.join(part));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let metadata = std::fs::metadata(&root).unwrap();
        assert_eq!(metadata.permissions().mode() & 0o777, 0o700);
        assert_eq!(
            metadata.uid(),
            std::fs::metadata("/proc/self").unwrap().uid()
        );
    }
    assert_eq!(std::fs::read_dir(root.join("config")).unwrap().count(), 0);
    let (ctx, mut app, _, mut requests) = fixture();
    app.state_store =
        Some(crate::player_state::StateStore::new(root.join("state"), ctx.clone()).unwrap());
    let input = root.join("original.wsz");
    std::fs::write(&input, crate::skins::fixture::bytes()).unwrap();
    app.classic.path = input.to_string_lossy().into_owned();
    let before = (
        app.queue.revision(),
        app.play_generation,
        app.paused,
        app.volume,
    );
    app.start_skin_job(true, true, &ctx);
    assert!(app.classic.pending.is_some());
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while app.classic.pending.is_some() {
        assert!(std::time::Instant::now() < deadline);
        app.classic_tick(&ctx);
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(app.classic.skin.is_some() && app.appearance.classic_skin);
    assert!(crate::store::Appearance::load().unwrap().classic_skin);
    assert!(
        crate::store::config_dir()
            .unwrap()
            .join(crate::skins::STORED_NAME)
            .is_file()
    );
    assert_eq!(
        (
            app.queue.revision(),
            app.play_generation,
            app.paused,
            app.volume
        ),
        before
    );
    assert!(requests.try_recv().is_err());
    app.appearance.classic_skin = false;
    app.save_appearance();
    app.classic.skin = None;
    app.start_skin_job(false, true, &ctx);
    while app.classic.pending.is_some() {
        assert!(std::time::Instant::now() < deadline);
        app.classic_tick(&ctx);
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(app.classic.skin.is_some() && app.appearance.classic_skin);
}
