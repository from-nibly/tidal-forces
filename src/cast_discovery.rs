//! User-initiated DNS-SD browse. Advertisements never establish receiver trust.
use mdns_sd::{ResolvedService, ServiceDaemon, ServiceEvent};
use std::{
    collections::BTreeMap,
    net::{Ipv4Addr, SocketAddrV4},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::Duration,
};
const SERVICE: &str = "_googlecast._tcp.local.";
const MAX_SERVICES: usize = 128;
const MAX_DEVICES: usize = 64;

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct Device {
    pub id: String,
    pub name: String,
    pub model: String,
    pub endpoint: SocketAddrV4,
}
#[derive(Clone, Copy, Default)]
pub(crate) struct Stats {
    pub found: u64,
    pub resolved: u64,
    pub rejected: u64,
    pub removed: u64,
}
pub(crate) struct Update {
    pub devices: Vec<Device>,
    pub stats: Stats,
    pub error: Option<&'static str>,
    pub finished: bool,
}
type Mailbox = Arc<Mutex<Option<Update>>>;
pub(crate) struct Browser {
    cancel: Arc<AtomicBool>,
    mailbox: Mailbox,
    // Completion is separate from the coalescing progress slot.
    done: mpsc::Receiver<()>,
    ended: bool,
}
impl Browser {
    #[cfg(any(test, debug_assertions))]
    pub(crate) fn fixture() -> (Self, mpsc::SyncSender<()>) {
        let (done, completion) = mpsc::sync_channel(1);
        (
            Self {
                cancel: Arc::new(AtomicBool::new(false)),
                mailbox: Arc::new(Mutex::new(None)),
                done: completion,
                ended: false,
            },
            done,
        )
    }
    pub fn next(&mut self) -> Option<Update> {
        if self.ended {
            return None;
        }
        let update = self
            .mailbox
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take();
        if update.as_ref().is_some_and(|u| u.finished) {
            self.ended = true;
        }
        if update.is_none() && !matches!(self.done.try_recv(), Err(mpsc::TryRecvError::Empty)) {
            self.ended = true;
            return Some(Update {
                devices: Vec::new(),
                stats: Stats::default(),
                error: Some("Device search ended without a final update"),
                finished: true,
            });
        }
        update
    }
    pub fn stop(&self) {
        self.cancel.store(true, Ordering::Release);
    }
    #[cfg(test)]
    fn wait_stopped(&self) {
        self.stop();
        self.done
            .recv_timeout(Duration::from_secs(5))
            .expect("Discovery worker did not stop");
    }
}
impl Drop for Browser {
    fn drop(&mut self) {
        self.stop();
    }
}
pub(crate) fn permitted(ip: Ipv4Addr) -> bool {
    ip.is_private() && !ip.is_loopback() && !ip.is_link_local() && !ip.is_broadcast()
}
fn label(value: &str) -> String {
    value.chars().filter(|c| !c.is_control()).take(80).collect()
}
fn cast_id(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    if bytes.len() != 32 && bytes.len() != 36 {
        return None;
    }
    let mut id = String::with_capacity(32);
    for (i, &b) in bytes.iter().enumerate() {
        if bytes.len() == 36 && [8, 13, 18, 23].contains(&i) {
            if b != b'-' {
                return None;
            }
        } else if b.is_ascii_hexdigit() {
            id.push((b as char).to_ascii_lowercase());
        } else {
            return None;
        }
    }
    (id.len() == 32 && id.bytes().any(|b| b != b'0')).then_some(id)
}
fn device(info: &ResolvedService) -> Option<Device> {
    if !info.ty_domain.eq_ignore_ascii_case(SERVICE)
        || info.get_fullname().len() > 256
        || !info.get_fullname().to_ascii_lowercase().ends_with(SERVICE)
        || info.get_fullname().to_ascii_lowercase().contains("._sub.")
        || info.get_port() == 0
    {
        return None;
    }
    let id = cast_id(info.get_property_val_str("id")?)?;
    let name = label(info.get_property_val_str("fn")?);
    if name.trim().is_empty() {
        return None;
    }
    let address = info
        .get_addresses_v4()
        .into_iter()
        .filter(|ip| permitted(*ip))
        .min()?;
    Some(Device {
        id,
        name,
        model: label(info.get_property_val_str("md").unwrap_or("Chromecast")),
        endpoint: SocketAddrV4::new(address, info.get_port()),
    })
}
#[derive(Default)]
struct Inventory {
    services: BTreeMap<String, Device>,
    stats: Stats,
}
impl Inventory {
    fn observe(&mut self, event: ServiceEvent) -> bool {
        match event {
            ServiceEvent::ServiceFound(..) => {
                self.stats.found = self.stats.found.saturating_add(1);
            }
            ServiceEvent::ServiceResolved(info) => {
                self.stats.resolved = self.stats.resolved.saturating_add(1);
                let key = info.get_fullname().to_ascii_lowercase();
                self.services.remove(&key);
                if let Some(device) = device(&info) {
                    if self.services.len() < MAX_SERVICES {
                        self.services.insert(key, device);
                    } else {
                        self.stats.rejected = self.stats.rejected.saturating_add(1);
                    }
                } else {
                    self.stats.rejected = self.stats.rejected.saturating_add(1);
                }
            }
            ServiceEvent::ServiceRemoved(_, name) => {
                self.stats.removed = self.stats.removed.saturating_add(1);
                self.services.remove(&name.to_ascii_lowercase());
            }
            _ => return false,
        }
        true
    }
    fn update(&self, error: Option<&'static str>, finished: bool) -> Update {
        let mut unique = BTreeMap::new();
        for device in self.services.values() {
            unique.entry(&device.id).or_insert(device);
        }
        let mut devices: Vec<Device> = unique.into_values().take(MAX_DEVICES).cloned().collect();
        devices.sort_by(|a, b| {
            a.name
                .cmp(&b.name)
                .then(a.model.cmp(&b.model))
                .then(a.id.cmp(&b.id))
        });
        Update {
            devices,
            stats: self.stats,
            error,
            finished,
        }
    }
}
fn publish(mailbox: &Mailbox, update: Update, wake: &impl Fn()) {
    let notify = mailbox
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .replace(update)
        .is_none();
    if notify {
        wake();
    }
}
pub(crate) fn browse(wake: impl Fn() + Send + 'static) -> Result<Browser, &'static str> {
    let mailbox = Arc::new(Mutex::new(None));
    let cancel = Arc::new(AtomicBool::new(false));
    let (done, completion) = mpsc::sync_channel(1);
    let browser = Browser {
        cancel: cancel.clone(),
        mailbox: mailbox.clone(),
        done: completion,
        ended: false,
    };
    std::thread::Builder::new()
        .name("cast-discovery".into())
        .spawn(move || {
            let mut inventory = Inventory::default();
            let result = (|| {
                let daemon =
                    ServiceDaemon::new().map_err(|_| "Could not start device discovery")?;
                let result = (|| {
                    let monitor = daemon
                        .monitor()
                        .map_err(|_| "Could not monitor device discovery")?;
                    let events = daemon
                        .browse(SERVICE)
                        .map_err(|_| "Could not search for Cast devices")?;
                    publish(&mailbox, inventory.update(None, false), &wake);
                    while !cancel.load(Ordering::Acquire) {
                        while let Ok(event) = monitor.try_recv() {
                            if matches!(event, mdns_sd::DaemonEvent::Error(_)) {
                                return Err("Local multicast discovery failed");
                            }
                        }
                        match events.recv_timeout(Duration::from_millis(100)) {
                            Ok(event) => {
                                if inventory.observe(event) {
                                    publish(&mailbox, inventory.update(None, false), &wake);
                                }
                            }
                            Err(mdns_sd::RecvTimeoutError::Timeout) => {}
                            Err(_) => return Err("Device discovery channel closed"),
                        }
                    }
                    Ok(())
                })();
                let _ = daemon.stop_browse(SERVICE);
                match daemon.shutdown() {
                    Ok(stopped) => {
                        stopped
                            .recv_timeout(Duration::from_secs(2))
                            .map_err(|_| "Device discovery shutdown was not confirmed")?;
                    }
                    Err(_) => return Err("Could not stop device discovery"),
                }
                result
            })();
            publish(&mailbox, inventory.update(result.err(), true), &wake);
            let _ = done.try_send(());
        })
        .map_err(|_| "Could not start device discovery worker")?;
    Ok(browser)
}

#[cfg(test)]
#[path = "cast_discovery_tests.rs"]
mod tests;
