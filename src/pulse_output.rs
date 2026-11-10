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
        Arc, Mutex,
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
struct Pending<C: ?Sized>(Operation<C>);
impl<C: ?Sized> Pending<C> {
    fn get_state(&self) -> pulse::operation::State {
        self.0.get_state()
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
        let deadline = Instant::now() + TIMEOUT;
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
}
impl Drop for Connection {
    fn drop(&mut self) {
        self.context.disconnect();
    }
}
fn valid_name(name: &str) -> bool {
    !name.is_empty() && name.len() <= 512 && !name.contains('\0')
}
fn device(info: &pulse::context::introspect::SinkInfo<'_>) -> Option<Device> {
    let name = info.name.as_ref()?.to_string();
    if !valid_name(&name) {
        return None;
    }
    let label = info
        .description
        .as_deref()
        .unwrap_or("Native audio output")
        .chars()
        .filter(|c| !c.is_control())
        .take(160)
        .collect();
    Some(Device {
        target: Target::Pulse(name, String::new()),
        label,
    })
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
                    if let Some(device) = device(info) {
                        shared.borrow_mut().push(device);
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
    selection: Arc<Mutex<Option<Target>>>,
    pub selection_failed: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    paused: Arc<AtomicBool>,
    flush: Arc<AtomicU64>,
    flush_completed: Arc<AtomicU64>,
    worker: Option<thread::JoinHandle<()>>,
}
impl Driver {
    pub fn open(target: &Target, failed: Arc<AtomicBool>) -> Result<Self> {
        let target = target.clone();
        let selection = Arc::new(Mutex::new(None));
        let worker_selection = selection.clone();
        let selection_failed = Arc::new(AtomicBool::new(false));
        let worker_selection_failed = selection_failed.clone();
        let stop = Arc::new(AtomicBool::new(false));
        let paused = Arc::new(AtomicBool::new(true));
        let flush = Arc::new(AtomicU64::new(0));
        let worker_flush = flush.clone();
        let flush_completed = Arc::new(AtomicU64::new(0));
        let worker_flush_completed = flush_completed.clone();
        let (mixer, source) = rodio::mixer::mixer(CHANNELS, RATE);
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        let worker_stop = stop.clone();
        let worker_paused = paused.clone();
        let worker_failed = failed.clone();
        let worker = thread::Builder::new()
            .name("native-audio".into())
            .spawn(move || match Session::open(&target, &worker_stop) {
                Ok(mut session) => {
                    let _ = ready_tx.send(Ok(()));
                    if session
                        .run(
                            source,
                            &worker_stop,
                            &worker_paused,
                            &worker_failed,
                            &worker_flush,
                            &worker_flush_completed,
                            &worker_selection,
                            &worker_selection_failed,
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
            })?;
        match ready_rx.recv_timeout(TIMEOUT + Duration::from_secs(1)) {
            Ok(Ok(())) => Ok(Self {
                mixer,
                selection,
                selection_failed,
                stop,
                paused,
                flush,
                flush_completed,
                worker: Some(worker),
            }),
            _ => {
                stop.store(true, Ordering::Release);
                let _ = worker.join();
                bail!(ERROR)
            }
        }
    }
    pub fn select(&self, target: Target) {
        *self.selection.lock().unwrap() = Some(target);
    }
    pub fn reset(&self) {
        self.paused.store(true, Ordering::Release);
        self.flush.fetch_add(1, Ordering::Release);
    }
    pub fn pause(&self, paused: bool) {
        self.paused.store(paused, Ordering::Release);
    }
    pub fn quiesce_for_transfer(&self) -> Result<()> {
        self.paused.store(true, Ordering::Release);
        let serial = self.flush.fetch_add(1, Ordering::AcqRel).wrapping_add(1);
        ensure!(serial != 0, "Output flush serial exhausted");
        let deadline = Instant::now() + TIMEOUT + Duration::from_secs(2);
        while self.flush_completed.load(Ordering::Acquire) != serial {
            ensure!(
                Instant::now() < deadline,
                "Local output flush was not confirmed"
            );
            thread::sleep(Duration::from_millis(2));
        }
        Ok(())
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
    demand: Arc<AtomicBool>,
}
impl Session {
    fn open(target: &Target, stop: &AtomicBool) -> Result<Self> {
        let mut connection = Connection::new()?;
        let name = match target {
            Target::SystemDefault => None,
            Target::Pulse(name, _) => {
                ensure!(valid_name(name), "{ERROR}");
                Some(name.as_str())
            }
            Target::CompatibilityDefault | Target::RequireSelection => bail!(ERROR),
        };
        let spec = pulse::sample::Spec {
            format: pulse::sample::Format::FLOAT32NE,
            channels: CHANNELS as u8,
            rate: RATE,
        };
        let mut stream = Stream::new(&mut connection.context, "Music", &spec, None)
            .ok_or_else(|| anyhow!(ERROR))?;
        let demand = Arc::new(AtomicBool::new(true));
        let needed = demand.clone();
        stream.set_write_callback(Some(Box::new(move |_| {
            needed.store(true, Ordering::Release);
        })));
        let attr = pulse::def::BufferAttr {
            // A music stream needs buffer headroom, not a device-latency target.
            maxlength: 19200 * 8,
            tlength: 4800 * 8,
            // Let the server rebuffer after an underrun.
            prebuf: u32::MAX,
            minreq: 256 * 8,
            fragsize: u32::MAX,
        };
        // ADJUST_LATENCY subtracts device latency from tlength, potentially
        // leaving less client audio than a graph quantum can consume.
        let flags = pulse::stream::FlagSet::START_CORKED;
        stream
            .connect_playback(name, Some(&attr), flags, None, None)
            .map_err(|_| anyhow!(ERROR))?;
        let mut session = Self {
            stream,
            connection,
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
    fn move_to(&mut self, target: &Target) -> Result<()> {
        let name = match target {
            Target::SystemDefault => self.connection.default_sink()?,
            Target::Pulse(name, _) => name.clone(),
            _ => bail!(ERROR),
        };
        ensure!(valid_name(&name), "{ERROR}");
        let index = self.stream.get_index().ok_or_else(|| anyhow!(ERROR))?;
        let result = Rc::new(RefCell::new(false));
        let reply = result.clone();
        let mut operation = self
            .connection
            .context
            .introspect()
            .move_sink_input_by_name(
                index,
                &name,
                Some(Box::new(move |ok| *reply.borrow_mut() = ok)),
            );
        self.connection.wait(&mut operation)?;
        ensure!(
            *result.borrow(),
            "The audio server rejected the output change"
        );
        Ok(())
    }
    #[allow(clippy::too_many_arguments)]
    fn run(
        &mut self,
        mut source: rodio::mixer::MixerSource,
        stop: &AtomicBool,
        paused: &AtomicBool,
        failed: &AtomicBool,
        flush: &AtomicU64,
        flush_completed: &AtomicU64,
        selection: &Mutex<Option<Target>>,
        selection_failed: &AtomicBool,
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
        while !stop.load(Ordering::Acquire) && !failed.load(Ordering::Acquire) {
            self.connection.poll()?;
            ensure!(
                self.stream.get_state() == pulse::stream::State::Ready,
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
                    flush_completed.store(flush_seen, Ordering::Release);
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
            let requested_target = selection.lock().unwrap().take();
            if let Some(target) = requested_target {
                selection_failed.store(self.move_to(&target).is_err(), Ordering::Release);
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
        let _ = self.stream.disconnect();
    }
}
