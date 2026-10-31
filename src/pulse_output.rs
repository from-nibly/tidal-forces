use crate::output_devices::{Device, Target};
use anyhow::{Result, anyhow, bail, ensure};
use libpulse_binding as pulse;
use pulse::{
    callbacks::ListResult,
    context::Context,
    mainloop::standard::{IterateResult, Mainloop},
    operation::Operation,
    stream::Stream,
};
use std::{
    cell::RefCell,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};

const RATE: u32 = 48000;
const CHANNELS: u16 = 2;
const TIMEOUT: Duration = Duration::from_secs(5);
const ERROR: &str = "Native audio output is unavailable. Select an output and retry.";
type SinkOperation = Pending<dyn FnMut(ListResult<&pulse::context::introspect::SinkInfo<'_>>)>;
struct Pending<C: ?Sized>(Operation<C>);
impl<C: ?Sized> Pending<C> {
    fn get_state(&self) -> pulse::operation::State {
        self.0.get_state()
    }
    fn cancel(&mut self) {
        self.0.cancel();
    }
}
impl<C: ?Sized> Drop for Pending<C> {
    fn drop(&mut self) {
        if self.0.get_state() != pulse::operation::State::Done {
            self.0.cancel();
        }
    }
}

struct Connection {
    context: Context,
    mainloop: Mainloop,
    deadline: Instant,
}
impl Connection {
    fn new() -> Result<Self> {
        let mainloop = Mainloop::new().ok_or_else(|| anyhow!(ERROR))?;
        let mut properties = pulse::proplist::Proplist::new().ok_or_else(|| anyhow!(ERROR))?;
        properties
            .set_str(
                pulse::proplist::properties::APPLICATION_NAME,
                "Tidal Forces",
            )
            .map_err(|_| anyhow!(ERROR))?;
        properties
            .set_str(pulse::proplist::properties::APPLICATION_ID, "tidal-forces")
            .map_err(|_| anyhow!(ERROR))?;
        properties
            .set_str(pulse::proplist::properties::MEDIA_ROLE, "music")
            .map_err(|_| anyhow!(ERROR))?;
        let mut context = Context::new_with_proplist(&mainloop, "Tidal Forces", &properties)
            .ok_or_else(|| anyhow!(ERROR))?;
        context
            .connect(None, pulse::context::FlagSet::NOAUTOSPAWN, None)
            .map_err(|_| anyhow!(ERROR))?;
        let deadline = Instant::now() + TIMEOUT;
        let mut connection = Self {
            context,
            mainloop,
            deadline,
        };
        while connection.context.get_state() != pulse::context::State::Ready {
            connection.poll()?;
            ensure!(Instant::now() < deadline, "{ERROR}");
            thread::sleep(Duration::from_millis(2));
        }
        Ok(connection)
    }
    fn poll(&mut self) -> Result<()> {
        match self.mainloop.iterate(false) {
            IterateResult::Success(_) => {}
            _ => bail!(ERROR),
        }
        ensure!(
            !matches!(
                self.context.get_state(),
                pulse::context::State::Failed | pulse::context::State::Terminated
            ),
            "{ERROR}"
        );
        Ok(())
    }
    fn wait<C: ?Sized>(&mut self, operation: &mut Operation<C>) -> Result<()> {
        let deadline = self.deadline;
        while operation.get_state() == pulse::operation::State::Running {
            if self.poll().is_err() || Instant::now() >= deadline {
                operation.cancel();
                bail!(ERROR);
            }
            thread::sleep(Duration::from_millis(2));
        }
        if operation.get_state() != pulse::operation::State::Done {
            operation.cancel();
            bail!(ERROR);
        }
        Ok(())
    }
    fn default_sink(&mut self) -> Result<String> {
        let result = Rc::new(RefCell::new(None));
        let shared = result.clone();
        let mut operation = self.context.introspect().get_server_info(move |info| {
            *shared.borrow_mut() = info.default_sink_name.as_ref().map(|s| s.to_string());
        });
        self.wait(&mut operation)?;
        let name = result.borrow_mut().take().ok_or_else(|| anyhow!(ERROR))?;
        ensure!(valid_name(&name), "{ERROR}");
        Ok(name)
    }
    fn sink(&mut self, name: &str) -> Result<Route> {
        ensure!(valid_name(name), "{ERROR}");
        let result = Rc::new(RefCell::new(None));
        let shared = result.clone();
        let mut operation = self
            .context
            .introspect()
            .get_sink_info_by_name(name, move |info| {
                if let ListResult::Item(info) = info {
                    *shared.borrow_mut() = Route::from_info(info);
                }
            });
        self.wait(&mut operation)?;
        let route = result.borrow_mut().take().ok_or_else(|| anyhow!(ERROR))?;
        ensure!(route.available, "{ERROR}");
        Ok(route)
    }
}
impl Drop for Connection {
    fn drop(&mut self) {
        self.context.set_subscribe_callback(None);
        self.context.disconnect();
    }
}
fn valid_name(name: &str) -> bool {
    !name.is_empty() && name.len() <= 512 && !name.contains('\0')
}
#[derive(Clone, PartialEq, Eq)]
struct Route {
    name: String,
    index: u32,
    port: Option<String>,
    available: bool,
    identity: String,
    label: String,
}
impl Route {
    fn from_info(info: &pulse::context::introspect::SinkInfo<'_>) -> Option<Self> {
        let name = info.name.as_ref()?.to_string();
        if !valid_name(&name) {
            return None;
        }
        use sha2::{Digest, Sha256};
        static SESSION: std::sync::OnceLock<u128> = std::sync::OnceLock::new();
        let stable = [
            "device.serial",
            "device.bus_path",
            "api.bluez5.address",
            "bluez.path",
        ]
        .iter()
        .find_map(|key| {
            info.proplist
                .get_str(key)
                .filter(|value| !value.is_empty() && value.len() <= 512)
                .map(|value| (*key, value))
        });
        let mut fingerprint = Sha256::new();
        fingerprint.update(b"Tidal Forces output identity v1\0");
        fingerprint.update((name.len() as u64).to_le_bytes());
        fingerprint.update(name.as_bytes());
        let (identity_key, identity_source) = stable
            .clone()
            .unwrap_or_else(|| ("session", format!("{}", SESSION.get_or_init(rand::random))));
        fingerprint.update((identity_key.len() as u64).to_le_bytes());
        fingerprint.update(identity_key.as_bytes());
        fingerprint.update((identity_source.len() as u64).to_le_bytes());
        fingerprint.update(identity_source.as_bytes());
        for key in [
            "device.vendor.id",
            "device.product.id",
            "device.bus",
            "device.api",
        ] {
            if let Some(value) = info.proplist.get_str(key) {
                if value.len() > 512 {
                    return None;
                }
                fingerprint.update((key.len() as u64).to_le_bytes());
                fingerprint.update(key.as_bytes());
                fingerprint.update((value.len() as u64).to_le_bytes());
                fingerprint.update(value.as_bytes());
            }
        }
        if let Some(port) = &info.active_port
            && let Some(name) = &port.name
        {
            if !valid_name(name) {
                return None;
            }
            fingerprint.update(b"port\0");
            fingerprint.update(name.as_bytes());
        }
        let identity = format!("{:x}", fingerprint.finalize());
        let mut label: String = info
            .description
            .as_deref()
            .unwrap_or("Native audio output")
            .chars()
            .filter(|c| !c.is_control())
            .take(160)
            .collect();
        if let Some(port) = &info.active_port
            && let Some(description) = &port.description
        {
            label.push_str(" · ");
            label.extend(description.chars().filter(|c| !c.is_control()).take(80));
        }
        if stable.is_none() {
            label.push_str(" · select again next launch");
        }
        Some(Self {
            name,
            identity,
            index: info.index,
            port: info
                .active_port
                .as_ref()
                .and_then(|p| p.name.as_ref())
                .map(|p| p.to_string()),
            available: info
                .active_port
                .as_ref()
                .is_none_or(|p| p.available != pulse::def::PortAvailable::No),
            label,
        })
    }
    fn same_route(&self, next: &Self) -> bool {
        self.name == next.name
            && self.index == next.index
            && self.port == next.port
            && self.identity == next.identity
            && next.available
    }
}

pub fn devices() -> Result<Vec<Device>> {
    let mut connection = Connection::new()?;
    let items = Rc::new(RefCell::new(Vec::new()));
    let shared = items.clone();
    let overflow = Rc::new(RefCell::new(false));
    let too_many = overflow.clone();
    let failed = Rc::new(RefCell::new(false));
    let failure = failed.clone();
    let mut seen = 0usize;
    let mut operation =
        connection
            .context
            .introspect()
            .get_sink_info_list(move |info| match info {
                ListResult::Item(info) => {
                    seen = seen.saturating_add(1);
                    if seen > 64 {
                        *too_many.borrow_mut() = true;
                        return;
                    }
                    if let Some(route) = Route::from_info(info)
                        && route.available
                    {
                        shared.borrow_mut().push(Device {
                            target: Target::Pulse(route.name, route.identity),
                            label: route.label,
                        });
                    }
                }
                ListResult::Error => *failure.borrow_mut() = true,
                ListResult::End => {}
            });
    connection.wait(&mut operation)?;
    ensure!(!*failed.borrow(), "{ERROR}");
    ensure!(
        !*overflow.borrow(),
        "Too many native outputs to list safely"
    );
    Ok(std::mem::take(&mut *items.borrow_mut()))
}

pub struct Driver {
    pub mixer: rodio::mixer::Mixer,
    pub label: String,
    pub port_observed: bool,
    stop: Arc<AtomicBool>,
    paused: Arc<AtomicBool>,
    flush: Arc<AtomicU64>,
    worker: Option<thread::JoinHandle<()>>,
}
impl Driver {
    pub fn open(target: &Target, failed: Arc<AtomicBool>) -> Result<Self> {
        let target = target.clone();
        let stop = Arc::new(AtomicBool::new(false));
        let paused = Arc::new(AtomicBool::new(true));
        let flush = Arc::new(AtomicU64::new(0));
        let worker_flush = flush.clone();
        let (mixer, source) = rodio::mixer::mixer(CHANNELS, RATE);
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        let worker_stop = stop.clone();
        let worker_paused = paused.clone();
        let worker_failed = failed.clone();
        let worker =
            thread::Builder::new().name("native-audio".into()).spawn(
                move || match Session::open(&target, worker_failed.clone(), &worker_stop) {
                    Ok(mut session) => {
                        let _ = ready_tx.send(Ok((
                            session.route.label.clone(),
                            session.route.port.is_some(),
                        )));
                        if session
                            .run(
                                source,
                                &worker_stop,
                                &worker_paused,
                                &worker_failed,
                                &worker_flush,
                            )
                            .is_err()
                        {
                            worker_failed.store(true, Ordering::Release);
                        }
                    }
                    Err(_) => {
                        worker_failed.store(true, Ordering::Release);
                        let _ = ready_tx.send(Err(()));
                    }
                },
            )?;
        match ready_rx.recv_timeout(TIMEOUT + Duration::from_secs(1)) {
            Ok(Ok((label, port_observed))) => Ok(Self {
                mixer,
                label,
                port_observed,
                stop,
                paused,
                flush,
                worker: Some(worker),
            }),
            _ => {
                stop.store(true, Ordering::Release);
                let _ = worker.join();
                bail!(ERROR)
            }
        }
    }
    pub fn reset(&self) {
        self.paused.store(true, Ordering::Release);
        self.flush.fetch_add(1, Ordering::Release);
    }
    pub fn pause(&self, paused: bool) {
        self.paused.store(paused, Ordering::Release);
    }
}
impl Drop for Driver {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

struct Session {
    stream: Stream,
    connection: Connection,
    route: Route,
    dirty: Arc<AtomicBool>,
    demand: Arc<AtomicBool>,
}
impl Session {
    fn open(target: &Target, failed: Arc<AtomicBool>, stop: &AtomicBool) -> Result<Self> {
        let mut connection = Connection::new()?;
        let name = match target {
            Target::SystemDefault => connection.default_sink()?,
            Target::Pulse(name, _) => name.clone(),
            Target::CompatibilityDefault | Target::RequireSelection => bail!(ERROR),
        };
        let route = connection.sink(&name)?;
        if let Target::Pulse(_, identity) = target {
            ensure!(
                route.identity == *identity,
                "Saved output identity or port changed. Select the output again before retrying."
            );
        }
        let dirty = Arc::new(AtomicBool::new(false));
        let notify = dirty.clone();
        let index = route.index;
        let removed = failed.clone();
        connection.context.set_subscribe_callback(Some(Box::new(
            move |facility, operation, changed| {
                if facility == Some(pulse::context::subscribe::Facility::Sink) && changed == index {
                    if operation == Some(pulse::context::subscribe::Operation::Removed) {
                        removed.store(true, Ordering::Release);
                    } else {
                        notify.store(true, Ordering::Release);
                    }
                } else if facility == Some(pulse::context::subscribe::Facility::Card) {
                    notify.store(true, Ordering::Release);
                }
            },
        )));
        let subscribed = Rc::new(RefCell::new(false));
        let confirmation = subscribed.clone();
        let mut operation = connection.context.subscribe(
            pulse::context::subscribe::InterestMaskSet::SINK
                | pulse::context::subscribe::InterestMaskSet::CARD,
            move |ok| *confirmation.borrow_mut() = ok,
        );
        connection.wait(&mut operation)?;
        ensure!(*subscribed.borrow(), "{ERROR}");
        let spec = pulse::sample::Spec {
            format: pulse::sample::Format::FLOAT32NE,
            channels: CHANNELS as u8,
            rate: RATE,
        };
        let mut stream = Stream::new(&mut connection.context, "Music", &spec, None)
            .ok_or_else(|| anyhow!(ERROR))?;
        let moved = failed;
        stream.set_moved_callback(Some(Box::new(move || {
            moved.store(true, Ordering::Release);
        })));
        let demand = Arc::new(AtomicBool::new(true));
        let needed = demand.clone();
        stream.set_write_callback(Some(Box::new(move |_| {
            needed.store(true, Ordering::Release);
        })));
        let attr = pulse::def::BufferAttr {
            maxlength: 4096 * 8,
            tlength: 1024 * 8,
            prebuf: 0,
            minreq: 256 * 8,
            fragsize: u32::MAX,
        };
        let flags = pulse::stream::FlagSet::START_CORKED
            | pulse::stream::FlagSet::DONT_MOVE
            | pulse::stream::FlagSet::ADJUST_LATENCY;
        stream
            .connect_playback(Some(&name), Some(&attr), flags, None, None)
            .map_err(|_| anyhow!(ERROR))?;
        let mut session = Self {
            stream,
            connection,
            route,
            dirty,
            demand,
        };
        let deadline = session.connection.deadline;
        while session.stream.get_state() != pulse::stream::State::Ready {
            session.connection.poll()?;
            ensure!(
                !stop.load(Ordering::Acquire)
                    && Instant::now() < deadline
                    && !matches!(
                        session.stream.get_state(),
                        pulse::stream::State::Failed | pulse::stream::State::Terminated
                    ),
                "{ERROR}"
            );
            thread::sleep(Duration::from_millis(2));
        }
        ensure!(
            session.stream.get_device_index() == Some(session.route.index),
            "{ERROR}"
        );
        ensure!(
            session.route.same_route(&session.connection.sink(&name)?),
            "{ERROR}"
        );
        let actual = session
            .stream
            .get_sample_spec()
            .ok_or_else(|| anyhow!(ERROR))?;
        ensure!(
            actual.rate == RATE
                && actual.channels == CHANNELS as u8
                && actual.format == pulse::sample::Format::FLOAT32NE,
            "{ERROR}"
        );
        Ok(session)
    }
    fn run(
        &mut self,
        mut source: rodio::mixer::MixerSource,
        stop: &AtomicBool,
        paused: &AtomicBool,
        failed: &AtomicBool,
        flush: &AtomicU64,
    ) -> Result<()> {
        let mut corked = true;
        let mut transport_operation: Option<Pending<dyn FnMut(bool)>> = None;
        let transport_result = Rc::new(RefCell::new(None));
        let mut transport_deadline = Instant::now();
        let mut flush_seen = 0;
        let mut flush_operation: Option<Pending<dyn FnMut(bool)>> = None;
        let flush_result = Rc::new(RefCell::new(None));
        let mut flush_deadline = Instant::now();
        let mut buffer = [0u8; 8192];
        let route_result = Rc::new(RefCell::new(None));
        let mut route_operation: Option<SinkOperation> = None;
        let mut route_deadline = Instant::now();
        while !stop.load(Ordering::Acquire) && !failed.load(Ordering::Acquire) {
            self.connection.poll()?;
            ensure!(
                self.stream.get_state() == pulse::stream::State::Ready
                    && self.stream.get_device_index() == Some(self.route.index),
                "{ERROR}"
            );
            if let Some(operation) = &mut transport_operation {
                if operation.get_state() == pulse::operation::State::Done {
                    ensure!(
                        transport_result.borrow_mut().take() == Some(true),
                        "{ERROR}"
                    );
                    transport_operation = None;
                } else {
                    ensure!(
                        operation.get_state() == pulse::operation::State::Running
                            && Instant::now() < transport_deadline,
                        "{ERROR}"
                    );
                }
            }
            if let Some(operation) = &mut flush_operation {
                if operation.get_state() == pulse::operation::State::Done {
                    ensure!(flush_result.borrow_mut().take() == Some(true), "{ERROR}");
                    flush_operation = None;
                } else {
                    ensure!(Instant::now() < flush_deadline, "{ERROR}");
                }
            }
            let serial = flush.load(Ordering::Acquire);
            if flush_operation.is_none() && serial != flush_seen {
                self.stream.cork(None);
                corked = true;
                let result = flush_result.clone();
                flush_operation =
                    Some(Pending(self.stream.flush(Some(Box::new(move |ok| {
                        *result.borrow_mut() = Some(ok)
                    })))));
                flush_seen = serial;
                flush_deadline = Instant::now() + Duration::from_secs(1);
            }
            if flush_operation.is_some() {
                thread::sleep(Duration::from_millis(2));
                continue;
            }
            if let Some(operation) = &mut route_operation {
                if operation.get_state() == pulse::operation::State::Done {
                    let next = route_result
                        .borrow_mut()
                        .take()
                        .ok_or_else(|| anyhow!(ERROR))?;
                    ensure!(self.route.same_route(&next), "{ERROR}");
                    route_operation = None;
                } else if Instant::now() >= route_deadline {
                    operation.cancel();
                    bail!(ERROR);
                }
            }
            if route_operation.is_none() && self.dirty.swap(false, Ordering::AcqRel) {
                let result = route_result.clone();
                route_operation = Some(Pending(
                    self.connection.context.introspect().get_sink_info_by_index(
                        self.route.index,
                        move |info| {
                            if let ListResult::Item(info) = info {
                                *result.borrow_mut() = Route::from_info(info);
                            }
                        },
                    ),
                ));
                route_deadline = Instant::now() + Duration::from_secs(1);
            }
            // Fail closed while a route change is unresolved: don't render more frames.
            // The server can already have buffered audio; this is not a zero-tail hardware guarantee.
            if route_operation.is_some() {
                thread::sleep(Duration::from_millis(2));
                continue;
            }
            let requested_pause = paused.load(Ordering::Acquire);
            if requested_pause != corked && transport_operation.is_none() {
                let result = transport_result.clone();
                let callback: Box<dyn FnMut(bool)> =
                    Box::new(move |ok| *result.borrow_mut() = Some(ok));
                transport_operation = Some(Pending(if requested_pause {
                    self.stream.cork(Some(callback))
                } else {
                    self.demand.store(true, Ordering::Release);
                    self.stream.uncork(Some(callback))
                }));
                transport_deadline = Instant::now() + Duration::from_secs(1);
                corked = requested_pause;
            }
            if transport_operation.is_some() {
                thread::sleep(Duration::from_millis(2));
                continue;
            }
            if !corked && self.demand.swap(false, Ordering::AcqRel) {
                let bytes = self
                    .stream
                    .writable_size()
                    .ok_or_else(|| anyhow!(ERROR))?
                    .min(buffer.len())
                    / 8
                    * 8;
                for frame in buffer[..bytes].as_chunks_mut::<8>().0 {
                    if failed.load(Ordering::Acquire) || stop.load(Ordering::Acquire) {
                        return Ok(());
                    }
                    for sample in frame.as_chunks_mut::<4>().0 {
                        sample.copy_from_slice(&source.next().unwrap_or(0.).to_ne_bytes());
                    }
                }
                if bytes > 0 {
                    self.stream
                        .write_copy(&buffer[..bytes], 0, pulse::stream::SeekMode::Relative)
                        .map_err(|_| anyhow!(ERROR))?;
                }
                if bytes == buffer.len() {
                    self.demand.store(true, Ordering::Release);
                }
            }
            thread::sleep(Duration::from_millis(if corked { 10 } else { 2 }));
        }
        Ok(())
    }
}
#[cfg(test)]
#[path = "pulse_output_tests.rs"]
mod tests;

impl Drop for Session {
    fn drop(&mut self) {
        self.stream.set_write_callback(None);
        self.stream.set_moved_callback(None);
        let _ = self.stream.disconnect();
    }
}
