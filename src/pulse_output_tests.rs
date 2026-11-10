use super::*;
use rodio::Source;
use std::sync::atomic::AtomicUsize;

#[test]
fn output_names_are_bounded() {
    assert!(!valid_name(""));
    assert!(!valid_name("fixture\0other"));
    assert!(!valid_name(&"a".repeat(513)));
    assert!(valid_name("fixture_a"));
}

fn wait_for_sink(connection: &mut Connection, name: &str) {
    let expected = Rc::new(RefCell::new(None));
    let reply = expected.clone();
    let mut operation = connection
        .context
        .introspect()
        .get_sink_info_by_name(name, move |info| {
            if let ListResult::Item(info) = info {
                *reply.borrow_mut() = Some(info.index);
            }
        });
    connection.wait(&mut operation).unwrap();
    let expected = expected.borrow().expect("Private fixture must exist");
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        let sinks = Rc::new(RefCell::new(Vec::new()));
        let reply = sinks.clone();
        let mut operation = connection
            .context
            .introspect()
            .get_sink_input_info_list(move |info| {
                if let ListResult::Item(info) = info {
                    reply.borrow_mut().push(info.sink);
                }
            });
        connection.wait(&mut operation).unwrap();
        if sinks.borrow().as_slice() == [expected] {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "Application stream did not move to the requested private sink"
        );
        thread::sleep(Duration::from_millis(5));
    }
}

fn verify_transfer_release(output: &mut crate::audio_output::Output, connection: &mut Connection) {
    output.reset(0.65).unwrap();
    let consumed = Arc::new(AtomicUsize::new(0));
    let counter = consumed.clone();
    output.append(rodio::source::Zero::new(2, 48000).periodic_access(
        Duration::from_millis(10),
        move |_| {
            counter.fetch_add(1, Ordering::AcqRel);
        },
    ));
    output.pause(false);
    let deadline = Instant::now() + Duration::from_secs(3);
    while consumed.load(Ordering::Acquire) < 5 {
        assert!(Instant::now() < deadline, "Private output did not start");
        thread::sleep(Duration::from_millis(5));
    }
    assert!(
        output.quiesce_for_transfer(),
        "Native flush must be acknowledged before transfer"
    );
    let stopped = consumed.load(Ordering::Acquire);
    thread::sleep(Duration::from_millis(100));
    assert_eq!(
        consumed.load(Ordering::Acquire),
        stopped,
        "No PCM may be produced after release"
    );
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        let count = Rc::new(RefCell::new(0));
        let reply = count.clone();
        let mut operation = connection
            .context
            .introspect()
            .get_sink_input_info_list(move |info| {
                if matches!(info, ListResult::Item(_)) {
                    *reply.borrow_mut() += 1;
                }
            });
        connection.wait(&mut operation).unwrap();
        if *count.borrow() == 0 {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "Released stream remained on private server"
        );
        thread::sleep(Duration::from_millis(5));
    }
}

#[test]
#[ignore = "Only tests/output-pipewire.py may supply the private null graph"]
fn song_changes_on_private_pipewire() {
    use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};
    let root = std::path::PathBuf::from(
        std::env::var("TIDAL_TEST_ISOLATED_PIPEWIRE").expect("Use tests/output-pipewire.py"),
    );
    assert_eq!(root.canonicalize().unwrap(), root);
    assert!(
        root.starts_with("/tmp")
            && root
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("tidal-pipewire-test-")
    );
    assert_eq!(root.metadata().unwrap().permissions().mode() & 0o777, 0o700);
    assert_eq!(
        root.metadata().unwrap().uid(),
        std::fs::metadata("/proc/self").unwrap().uid()
    );
    assert!(
        std::fs::symlink_metadata(root.join("native"))
            .unwrap()
            .file_type()
            .is_socket()
    );
    assert_eq!(
        std::env::var("PULSE_SERVER").unwrap(),
        format!("unix:{}/native", root.display())
    );
    assert_eq!(
        std::env::var("PIPEWIRE_RUNTIME_DIR").unwrap(),
        root.to_str().unwrap()
    );
    for (file, expected) in [
        (
            "pipewire.conf",
            include_str!("../tests/fixtures/pipewire-null.conf"),
        ),
        (
            "pipewire-pulse.conf",
            include_str!("../tests/fixtures/pipewire-pulse-null.conf"),
        ),
        (
            "wp/wireplumber.conf.d/99-policy.conf",
            include_str!("../tests/fixtures/wireplumber-policy-only.conf"),
        ),
    ] {
        assert_eq!(std::fs::read_to_string(root.join(file)).unwrap(), expected);
    }
    let targets = devices().unwrap();
    assert_eq!(targets.len(), 2);
    assert!(targets.iter().all(|device| matches!(&device.target,Target::Pulse(name,_) if name=="fixture_a" || name=="fixture_b")));
    crate::audio::Player::verify_private_native_continuity();
    let mut admin = Connection::new().unwrap();
    assert_eq!(admin.default_sink().unwrap(), "fixture_a");
    let mut output = crate::audio_output::Output::default();
    output.select(Target::SystemDefault);
    // Sustained consumption matters: short reset tests can miss a stalled writer.
    output.reset(0.65).unwrap();
    let consumed = Arc::new(AtomicUsize::new(0));
    let counter = consumed.clone();
    output.append(
        rodio::buffer::SamplesBuffer::new(2, 48000, vec![0.25; 48000 * 2 * 6]).periodic_access(
            Duration::from_millis(10),
            move |_| {
                if counter.fetch_add(1, Ordering::AcqRel) == 50 {
                    // Test-only scheduling stall: the server must resume real samples afterward.
                    thread::sleep(Duration::from_millis(250));
                }
            },
        ),
    );
    output.pause(false);
    for second in 0..5 {
        let before = consumed.load(Ordering::Acquire);
        thread::sleep(Duration::from_secs(1));
        output.sync_pause();
        assert!(!output.failed());
        assert!(
            consumed.load(Ordering::Acquire) > before + 50,
            "Native writer stalled during second {second}"
        );
    }
    for change in 0..30 {
        output.reset(0.65).unwrap();
        output.append(rodio::source::Zero::new(2, 48000));
        output.pause(false);
        let target = &targets[change % targets.len()].target;
        output.select(target.clone());
        if let Target::Pulse(name, _) = target {
            wait_for_sink(&mut admin, name);
        }
        thread::sleep(Duration::from_millis(100));
        assert!(!output.selection_failed());
        assert_eq!(
            admin.default_sink().unwrap(),
            "fixture_a",
            "Changing this app must not change the server default"
        );
        assert!(
            !output.failed(),
            "Unchanged virtual PipeWire route failed after song change {change}"
        );
        output.stop();
        thread::sleep(Duration::from_millis(20));
    }
    verify_transfer_release(&mut output, &mut admin);
}

#[test]
#[ignore = "Only tests/output-pulse.py may supply the private null server"]
fn native_routing_on_private_null_server() {
    use std::os::unix::fs::{FileTypeExt, PermissionsExt};
    let root = std::path::PathBuf::from(
        std::env::var("TIDAL_TEST_ISOLATED_PULSE").expect("Use tests/output-pulse.py"),
    );
    assert!(
        root.starts_with("/tmp")
            && root
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("tidal-v031-pulse-")
    );
    assert_eq!(root.canonicalize().unwrap(), root);
    assert_eq!(
        std::fs::metadata(&root).unwrap().permissions().mode() & 0o777,
        0o700
    );
    assert!(
        std::fs::symlink_metadata(root.join("native"))
            .unwrap()
            .file_type()
            .is_socket()
    );
    assert_eq!(
        std::env::var("PULSE_SERVER").unwrap(),
        format!("unix:{}/native", root.display())
    );
    assert_eq!(
        std::fs::read_to_string(root.join("null.pa")).unwrap(),
        format!(
            "load-module module-native-protocol-unix socket={}/native auth-anonymous=1\nload-module module-null-sink sink_name=fixture_a\nload-module module-null-sink sink_name=fixture_b\nset-default-sink fixture_a\n",
            root.display()
        )
    );
    let mut connection = Connection::new().unwrap();
    let witness = Arc::new(());
    let captured = witness.clone();
    let mut cancelled = connection.context.introspect().get_server_info(move |_| {
        let _ = &captured;
    });
    assert_eq!(Arc::strong_count(&witness), 2);
    cancelled.cancel();
    assert_eq!(
        Arc::strong_count(&witness),
        1,
        "Cancellation must release captured callback data"
    );
    cancelled.cancel();
    let captured = witness.clone();
    let mut completed = connection.context.introspect().get_server_info(move |_| {
        let _ = &captured;
    });
    connection.wait(&mut completed).unwrap();
    assert_eq!(Arc::strong_count(&witness), 1);
    completed.cancel();
    assert_eq!(
        Arc::strong_count(&witness),
        1,
        "Completed callbacks must never be freed twice"
    );
    let captured = witness.clone();
    let pending = Pending(connection.context.introspect().get_server_info(move |_| {
        let _ = &captured;
    }));
    drop(pending);
    assert_eq!(Arc::strong_count(&witness), 1);
    let targets = devices().unwrap();
    assert_eq!(targets.len(), 2);
    assert!(targets.iter().all(
        |d| matches!(&d.target,Target::Pulse(name, _) if name=="fixture_a" || name=="fixture_b")
    ));
    let target_a = targets
        .iter()
        .find(|d| matches!(&d.target, Target::Pulse(name, _) if name == "fixture_a"))
        .unwrap()
        .target
        .clone();
    let target_b = targets
        .iter()
        .find(|d| matches!(&d.target, Target::Pulse(name, _) if name == "fixture_b"))
        .unwrap()
        .target
        .clone();
    if let Target::Pulse(name, _) = &target_a {
        assert!(
            Driver::open(
                &Target::Pulse(name.clone(), "wrong-identity".into()),
                Arc::new(AtomicBool::new(false))
            )
            .is_ok(),
            "Legacy fingerprints must not pin or reject an OS output"
        );
    }
    {
        let mut output = crate::audio_output::Output::default();
        output.select(target_a.clone());
        for _ in 0..40 {
            output.reset(0.65).unwrap();
            output.append(rodio::source::Zero::new(2, 48000));
            output.pause(false);
            thread::sleep(Duration::from_millis(30));
            assert!(
                !output.failed(),
                "Changing sources must not fault an unchanged private output"
            );
        }
        verify_transfer_release(&mut output, &mut connection);
    }
    let failed = Arc::new(AtomicBool::new(false));
    let driver = Driver::open(&target_a, failed.clone()).unwrap();
    let rendered = Arc::new(AtomicUsize::new(0));
    let counter = rendered.clone();
    let sink = rodio::Sink::connect_new(&driver.mixer);
    sink.append(rodio::source::Zero::new(2, 48000).periodic_access(
        Duration::from_millis(10),
        move |_| {
            counter.fetch_add(1, Ordering::AcqRel);
        },
    ));
    driver.pause(false);
    let deadline = Instant::now() + Duration::from_secs(3);
    while rendered.load(Ordering::Acquire) < 5 {
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(5));
    }
    driver.pause(true);
    thread::sleep(Duration::from_millis(50));
    let before = rendered.load(Ordering::Acquire);
    thread::sleep(Duration::from_millis(60));
    assert_eq!(rendered.load(Ordering::Acquire), before);
    driver.reset();
    driver.pause(false);
    let deadline = Instant::now() + Duration::from_secs(3);
    while rendered.load(Ordering::Acquire) == before {
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(5));
    }
    let mut admin = Connection::new().unwrap();
    let input = Rc::new(RefCell::new(None));
    let shared = input.clone();
    let mut operation = admin
        .context
        .introspect()
        .get_sink_input_info_list(move |info| {
            if let ListResult::Item(info) = info {
                *shared.borrow_mut() = Some(info.index);
            }
        });
    admin.wait(&mut operation).unwrap();
    driver.select(target_b.clone());
    wait_for_sink(&mut admin, "fixture_b");
    driver.select(Target::SystemDefault);
    wait_for_sink(&mut admin, "fixture_a");
    assert_eq!(admin.default_sink().unwrap(), "fixture_a");
    driver.select(Target::Pulse("missing_fixture".into(), String::new()));
    let deadline = Instant::now() + Duration::from_secs(3);
    while !driver.selection_failed.load(Ordering::Acquire) {
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(5));
    }
    wait_for_sink(&mut admin, "fixture_a");
    assert!(
        !failed.load(Ordering::Acquire),
        "A rejected selection must not terminate the existing stream"
    );
    let moved = Rc::new(RefCell::new(None));
    let result = moved.clone();
    let mut operation = admin.context.introspect().move_sink_input_by_name(
        input.borrow().unwrap(),
        "fixture_b",
        Some(Box::new(move |ok| *result.borrow_mut() = Some(ok))),
    );
    admin.wait(&mut operation).unwrap();
    assert_eq!(
        *moved.borrow(),
        Some(true),
        "The OS must be allowed to move the application stream"
    );
    assert!(!failed.load(Ordering::Acquire));
    let module = Rc::new(RefCell::new(None));
    let shared = module.clone();
    let mut operation =
        admin
            .context
            .introspect()
            .get_sink_info_by_name("fixture_a", move |info| {
                if let ListResult::Item(info) = info {
                    *shared.borrow_mut() = info.owner_module;
                }
            });
    admin.wait(&mut operation).unwrap();
    let removed = Rc::new(RefCell::new(false));
    let result = removed.clone();
    let mut operation = admin
        .context
        .introspect()
        .unload_module(module.borrow().unwrap(), move |ok| {
            *result.borrow_mut() = ok
        });
    admin.wait(&mut operation).unwrap();
    assert!(*removed.borrow());
    thread::sleep(Duration::from_millis(150));
    assert!(
        !failed.load(Ordering::Acquire),
        "Removing the old sink must not fault the OS-moved stream"
    );
    drop(driver);
    let stopped = rendered.load(Ordering::Acquire);
    thread::sleep(Duration::from_millis(50));
    assert_eq!(rendered.load(Ordering::Acquire), stopped);
    assert!(
        Driver::open(&target_a, Arc::new(AtomicBool::new(false))).is_err(),
        "The server may reject an unavailable explicitly requested output"
    );
    let retry = Driver::open(&target_b, Arc::new(AtomicBool::new(false))).unwrap();
    drop(retry);
}
