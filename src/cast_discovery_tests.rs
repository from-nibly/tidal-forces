use super::*;
use mdns_sd::ServiceInfo;
use std::time::{Instant, SystemTime, UNIX_EPOCH};
const ID: &str = "12345678123412341234123456789abc";
fn resolved(service: &str, id: &str, name: &str, port: u16) -> ServiceEvent {
    let info = ServiceInfo::new(
        SERVICE,
        service,
        "fixture.local.",
        "192.168.99.1",
        port,
        &[("id", id), ("fn", name), ("md", "Synthetic receiver")][..],
    )
    .unwrap();
    ServiceEvent::ServiceResolved(Box::new(info.as_resolved_service()))
}
#[test]
fn identity_is_cast_uuid_not_instance_name_or_friendly_name() {
    let mut inventory = Inventory::default();
    inventory.observe(resolved("first", ID, "Room", 8009));
    inventory.observe(resolved(
        "alias",
        "12345678-1234-1234-1234-123456789ABC",
        "Room",
        32001,
    ));
    let update = inventory.update(None, false);
    assert_eq!(update.devices.len(), 1);
    assert_eq!(update.devices[0].id, ID);
    assert_eq!(update.devices[0].endpoint.port(), 32001);
    inventory.observe(resolved(
        "different",
        "abcdefabcdefabcdefabcdefabcdefab",
        "Room",
        8009,
    ));
    assert_eq!(inventory.update(None, false).devices.len(), 2);
}
#[test]
fn updates_and_alias_removals_keep_live_inventory_consistent() {
    let mut inventory = Inventory::default();
    inventory.observe(resolved("first", ID, "Old room", 8009));
    inventory.observe(resolved("first", ID, "Renamed room", 32100));
    let update = inventory.update(None, false);
    assert_eq!(update.devices.len(), 1);
    assert_eq!(update.devices[0].name, "Renamed room");
    assert_eq!(update.devices[0].endpoint.port(), 32100);
    inventory.observe(resolved("alias", ID, "Renamed room", 32100));
    inventory.observe(ServiceEvent::ServiceRemoved(
        SERVICE.into(),
        format!("first.{SERVICE}").to_uppercase(),
    ));
    assert_eq!(inventory.update(None, false).devices.len(), 1);
    inventory.observe(ServiceEvent::ServiceRemoved(
        SERVICE.into(),
        format!("alias.{SERVICE}"),
    ));
    assert!(inventory.update(None, false).devices.is_empty());
}
#[test]
fn malformed_updates_cannot_leave_a_stale_selectable_endpoint() {
    let mut inventory = Inventory::default();
    inventory.observe(resolved("first", ID, "Room", 8009));
    inventory.observe(resolved("first", "not-a-uuid", "Room", 8009));
    let update = inventory.update(None, false);
    assert!(update.devices.is_empty());
    assert_eq!(update.stats.rejected, 1);
    for id in [
        "",
        "0",
        "00000000000000000000000000000000",
        "12345678123412341234123456789abz",
        "12345678_1234-1234-1234-123456789abc",
    ] {
        assert!(cast_id(id).is_none());
    }
}
#[test]
fn untrusted_addresses_labels_and_zero_ports_are_restricted() {
    assert!(permitted(Ipv4Addr::new(192, 168, 2, 1)));
    for ip in [
        Ipv4Addr::LOCALHOST,
        Ipv4Addr::new(8, 8, 8, 8),
        Ipv4Addr::new(169, 254, 1, 1),
        Ipv4Addr::BROADCAST,
    ] {
        assert!(!permitted(ip));
    }
    assert_eq!(label("TV\n\0Room"), "TVRoom");
    assert_eq!(label(&"x".repeat(200)).len(), 80);
    let mut inventory = Inventory::default();
    inventory.observe(resolved("zero", ID, "Room", 0));
    inventory.observe(resolved("blank", ID, "\n\0", 8009));
    assert!(inventory.update(None, false).devices.is_empty());
}
#[test]
fn service_associations_and_presented_devices_are_bounded() {
    let mut inventory = Inventory::default();
    for i in 1..=MAX_SERVICES + 20 {
        inventory.observe(resolved(
            &format!("fixture-{i}"),
            &format!("{i:032x}"),
            "Room",
            8009,
        ));
    }
    assert_eq!(inventory.services.len(), MAX_SERVICES);
    assert_eq!(inventory.update(None, false).devices.len(), MAX_DEVICES);
    assert_eq!(inventory.stats.rejected, 20);
}
fn controlled_browser() -> (Browser, Mailbox, Arc<AtomicBool>, mpsc::SyncSender<()>) {
    let mailbox = Arc::new(Mutex::new(None));
    let cancel = Arc::new(AtomicBool::new(false));
    let (done, completion) = mpsc::sync_channel(1);
    (
        Browser {
            cancel: cancel.clone(),
            mailbox: mailbox.clone(),
            done: completion,
            ended: false,
        },
        mailbox,
        cancel,
        done,
    )
}
#[test]
fn streaming_updates_coalesce_and_terminal_is_delivered_once() {
    let (mut browser, mailbox, _, done) = controlled_browser();
    let mut inventory = Inventory::default();
    inventory.observe(resolved("one", ID, "Old", 8009));
    publish(&mailbox, inventory.update(None, false), &|| {});
    assert_eq!(browser.next().unwrap().devices[0].name, "Old");
    for i in 0..100 {
        inventory.observe(resolved("one", ID, &format!("Name {i}"), 8009));
        publish(&mailbox, inventory.update(None, false), &|| {});
    }
    assert_eq!(browser.next().unwrap().devices[0].name, "Name 99");
    publish(
        &mailbox,
        inventory.update(Some("Synthetic shutdown error"), true),
        &|| {},
    );
    done.send(()).unwrap();
    drop(done);
    assert!(browser.next().unwrap().finished);
    assert!(browser.next().is_none());
}
#[test]
fn closing_browser_cancels_worker_and_lost_worker_is_not_empty_success() {
    let (browser, _, cancel, _) = controlled_browser();
    drop(browser);
    assert!(cancel.load(Ordering::Acquire));
    let (mut browser, _, _, done) = controlled_browser();
    drop(done);
    let update = browser.next().unwrap();
    assert!(update.finished && update.error.is_some());
    assert!(browser.next().is_none());
}
#[test]
#[ignore = "real multicast Cast discovery only; explicit current permission/deadline required"]
fn authorized_device_discovery() {
    assert_eq!(
        std::env::var("TIDAL_CAST_DISCOVERY_TEST").as_deref(),
        Ok("1")
    );
    let now = || {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
    };
    let until = std::env::var("TIDAL_CAST_DISCOVERY_UNTIL")
        .unwrap()
        .parse::<u64>()
        .unwrap();
    let seconds = std::env::var("TIDAL_CAST_DISCOVERY_SECONDS")
        .unwrap()
        .parse::<u64>()
        .unwrap();
    assert!((1..=60).contains(&seconds));
    assert!(until > now() + seconds + 5 && until <= now() + 600);
    let mut browser = browse(|| {}).unwrap();
    let deadline = Instant::now() + Duration::from_secs(seconds);
    let mut latest = Inventory::default().update(None, false);
    let mut updates = 0;
    while Instant::now() < deadline && now() < until {
        if let Some(update) = browser.next() {
            assert!(
                update.error.is_none(),
                "Discovery reported a local worker/socket error"
            );
            assert!(
                !update.finished,
                "Discovery stopped before the requested search ended"
            );
            latest = update;
            updates += 1;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    browser.wait_stopped();
    if let Some(update) = browser.next() {
        assert!(update.error.is_none(), "Discovery shutdown error");
        latest = update;
    }
    println!(
        "Discovery: devices={}, service_found={}, resolved={}, rejected={}, removed={}, streamed_updates={updates}, window={seconds}s",
        latest.devices.len(),
        latest.stats.found,
        latest.stats.resolved,
        latest.stats.rejected,
        latest.stats.removed
    );
    println!(
        "Browser shutdown confirmed; no device names/addresses logged; no receiver sessions opened"
    );
    assert!(
        !latest.devices.is_empty(),
        "No supported Cast devices discovered"
    );
}
