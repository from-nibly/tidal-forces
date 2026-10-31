use super::*;
use rodio::Source;
use std::sync::atomic::AtomicUsize;

#[test]
fn route_changes_are_fail_closed_without_inventing_port_visibility() {
    let route = Route {
        name: "fixture_a".into(),
        index: 7,
        port: Some("headphones".into()),
        available: true,
        identity: "fixture-identity".into(),
        label: "Fixture".into(),
    };
    assert!(route.same_route(&route));
    let mut next = route.clone();
    next.label = "Renamed".into();
    assert!(route.same_route(&next));
    let mut next = route.clone();
    next.port = Some("speaker".into());
    assert!(!route.same_route(&next));
    let mut next = route.clone();
    next.port = None;
    assert!(!route.same_route(&next));
    let mut next = route.clone();
    next.available = false;
    assert!(!route.same_route(&next));
    let mut next = route.clone();
    next.index = 8;
    assert!(!route.same_route(&next));
    assert!(!valid_name(""));
    assert!(!valid_name("fixture\0other"));
    assert!(!valid_name(&"a".repeat(513)));
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
            .is_err(),
            "An existing sink with a different saved identity must not open"
        );
    }
    let failed = Arc::new(AtomicBool::new(false));
    let driver = Driver::open(&target_a, failed.clone()).unwrap();
    assert!(
        !driver.port_observed,
        "Null sinks are not physical headphone evidence"
    );
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
        Some(false),
        "The server must honor DONT_MOVE"
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
    let deadline = Instant::now() + Duration::from_secs(3);
    while !failed.load(Ordering::Acquire) {
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(5));
    }
    drop(driver);
    let stopped = rendered.load(Ordering::Acquire);
    thread::sleep(Duration::from_millis(50));
    assert_eq!(rendered.load(Ordering::Acquire), stopped);
    assert!(
        Driver::open(&target_a, Arc::new(AtomicBool::new(false))).is_err(),
        "A missing selected sink must not fall back to fixture_b"
    );
    let retry = Driver::open(&target_b, Arc::new(AtomicBool::new(false))).unwrap();
    drop(retry);
}
