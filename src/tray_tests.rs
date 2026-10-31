use super::*;

#[test]
fn menu_is_static_bounded_and_dispatches_only_shared_actions() {
    let tray = Tray::fixture(egui::Context::default(), true);
    let menu = Menu(tray.shared.clone());
    let (revision, layout) = menu.get_layout(0, -1, Vec::new()).unwrap();
    assert_eq!(revision, 1);
    assert_eq!(layout.2.len(), 7);
    assert!(menu.get_layout(0, 0, Vec::new()).unwrap().1.2.is_empty());
    assert!(menu.get_layout(-1, -1, Vec::new()).is_err());
    assert!(menu.get_layout(0, -2, Vec::new()).is_err());
    assert!(menu.get_group_properties(vec![1; 65], Vec::new()).is_err());
    let (_, layout) = menu.get_layout(1, 1, vec!["label".into()]).unwrap();
    assert_eq!(layout.1.len(), 1);
    assert!(layout.2.is_empty());
    menu.event(4, "hovered", 0i32.into(), 0).unwrap();
    assert!(tray.events.try_recv().is_err());
    for id in 1..=7 {
        menu.event(id, "clicked", 0i32.into(), 0).unwrap();
        assert_eq!(tray.events.try_recv().unwrap(), menu_action(id).unwrap());
    }
    assert!(menu.event(0, "clicked", 0i32.into(), 0).is_err());
    for _ in 0..32 {
        menu.event(4, "clicked", 0i32.into(), 0).unwrap();
    }
    assert!(menu.event(4, "clicked", 0i32.into(), 0).is_err());
}

#[test]
fn tray_host_loss_restores_hidden_state_and_icon_is_bounded_argb() {
    let tray = Tray::fixture(egui::Context::default(), true);
    assert!(tray.hide());
    assert!(tray.shared.hidden.load(Ordering::Acquire));
    tray.shared.status(2);
    assert!(!tray.shared.hidden.load(Ordering::Acquire));
    assert!(!tray.hide());
    let image = icon();
    assert_eq!(
        (image[0].0, image[0].1, image[0].2.len()),
        (64, 64, 64 * 64 * 4)
    );
}

struct Watcher {
    items: Arc<std::sync::Mutex<Vec<String>>>,
    host: Arc<AtomicBool>,
}
#[zbus::dbus_interface(name = "org.kde.StatusNotifierWatcher")]
impl Watcher {
    fn register_status_notifier_item(
        &self,
        service: &str,
        #[zbus(header)] header: zbus::MessageHeader<'_>,
    ) -> zbus::fdo::Result<()> {
        if service != ITEM_PATH {
            return Err(zbus::fdo::Error::InvalidArgs(
                "Unexpected object path".into(),
            ));
        }
        let sender = header
            .sender()?
            .ok_or_else(|| zbus::fdo::Error::Failed("Missing sender".into()))?;
        *self.items.lock().unwrap() = vec![format!("{sender}{service}")];
        Ok(())
    }
    #[dbus_interface(property)]
    fn registered_status_notifier_items(&self) -> Vec<String> {
        self.items.lock().unwrap().clone()
    }
    #[dbus_interface(property)]
    fn is_status_notifier_host_registered(&self) -> bool {
        self.host.load(Ordering::Acquire)
    }
    #[dbus_interface(property)]
    fn protocol_version(&self) -> i32 {
        0
    }
}

fn wait(mut condition: impl FnMut() -> bool) {
    let deadline = std::time::Instant::now() + Duration::from_secs(6);
    while !condition() {
        assert!(
            std::time::Instant::now() < deadline,
            "Private tray contract timed out"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
#[ignore = "Run only through tests/tray.py on its isolated private D-Bus"]
fn native_tray_on_isolated_bus() {
    use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};
    let root = std::path::PathBuf::from(
        std::env::var("TIDAL_TEST_PRIVATE_TRAY").expect("Use tests/tray.py"),
    );
    assert_eq!(root.canonicalize().unwrap(), root);
    assert!(root.starts_with(std::env::temp_dir()));
    assert!(
        root.file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("tidal-tray-test-")
    );
    let metadata = root.metadata().unwrap();
    assert_eq!(metadata.permissions().mode() & 0o777, 0o700);
    let uid = std::fs::metadata("/proc/self").unwrap().uid();
    assert_eq!(metadata.uid(), uid);
    let socket = std::fs::symlink_metadata(root.join("bus")).unwrap();
    assert!(socket.file_type().is_socket());
    assert_eq!(socket.uid(), uid);
    assert_eq!(
        std::env::var("DBUS_SESSION_BUS_ADDRESS").unwrap(),
        format!("unix:path={}/bus", root.display())
    );

    let items = Arc::new(std::sync::Mutex::new(Vec::new()));
    let host = Arc::new(AtomicBool::new(true));
    let watcher = || {
        zbus::blocking::ConnectionBuilder::session()
            .unwrap()
            .name(WATCHER)
            .unwrap()
            .serve_at(
                WATCHER_PATH,
                Watcher {
                    items: items.clone(),
                    host: host.clone(),
                },
            )
            .unwrap()
            .build()
            .unwrap()
    };
    let first = watcher();
    let tray = Tray::new(egui::Context::default());
    wait(|| tray.status() == Status::Ready);
    let identity = items.lock().unwrap()[0].clone();
    let owner = identity.strip_suffix(ITEM_PATH).unwrap();
    let client = zbus::blocking::Connection::session().unwrap();
    let item = zbus::blocking::Proxy::new(&client, owner, ITEM_PATH, "org.kde.StatusNotifierItem")
        .unwrap();
    assert_eq!(item.get_property::<i32>("WindowId").unwrap(), 0);
    let pixels: Pixmaps = item.get_property("IconPixmap").unwrap();
    assert_eq!(pixels[0].2.len(), 64 * 64 * 4);
    let path: OwnedObjectPath = item.get_property("Menu").unwrap();
    assert_eq!(path.as_str(), MENU_PATH);
    item.call::<_, _, ()>("Activate", &(0i32, 0i32)).unwrap();
    assert_eq!(
        tray.events.recv_timeout(Duration::from_secs(1)).unwrap(),
        Action::Raise
    );
    let menu =
        zbus::blocking::Proxy::new(&client, owner, MENU_PATH, "com.canonical.dbusmenu").unwrap();
    let (_, layout): (u32, Layout) = menu
        .call("GetLayout", &(0i32, -1i32, Vec::<String>::new()))
        .unwrap();
    assert_eq!(layout.2.len(), 7);
    menu.call::<_, _, ()>("Event", &(7i32, "clicked", OwnedValue::from(0i32), 0u32))
        .unwrap();
    assert_eq!(
        tray.events.recv_timeout(Duration::from_secs(1)).unwrap(),
        Action::Quit
    );
    assert!(tray.hide());
    host.store(false, Ordering::Release);
    wait(|| tray.status() == Status::Unavailable);
    assert!(!tray.shared.hidden.load(Ordering::Acquire));
    host.store(true, Ordering::Release);
    wait(|| tray.status() == Status::Ready);
    drop(first);
    wait(|| tray.status() == Status::Unavailable);
    items.lock().unwrap().clear();
    let _second = watcher();
    wait(|| tray.status() == Status::Ready);
    assert_eq!(
        items.lock().unwrap().as_slice(),
        std::slice::from_ref(&identity)
    );
    drop(tray);
    let bus = zbus::blocking::fdo::DBusProxy::new(&client).unwrap();
    wait(|| !bus.name_has_owner(owner.try_into().unwrap()).unwrap());
}
