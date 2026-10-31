use crate::api::{Stream, StreamSource};
use crate::audio_handoff::{Boundary, Chain, Frame, Slot};
use crate::backend::{Event, Events};
use anyhow::Result;
use rodio::{Decoder, OutputStreamBuilder, Sink, Source};
use std::{
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
    time::Duration,
};
use stream_download::{Settings, StreamDownload};

type AudioStorage = crate::audio_buffer::CappedStorage;
type AudioDecoder = Decoder<StreamDownload<AudioStorage>>;
pub(crate) fn audio_storage() -> AudioStorage {
    crate::audio_buffer::CappedStorage
}

#[derive(Default)]
struct DownloadGuard(Option<Box<dyn Fn() + Send + Sync>>);
impl Drop for DownloadGuard {
    fn drop(&mut self) {
        if let Some(cancel) = &self.0 {
            cancel();
        }
    }
}

struct Loaded {
    generation: u64,
    flac: bool,
    normalization: crate::dsp::Metadata,
    decoder: Arc<crate::audio_buffer::Reusable>,
    quality: String,
    seeker: Option<crate::dash::SeekHandle>,
    _download: DownloadGuard,
}
impl Drop for Loaded {
    fn drop(&mut self) {
        if let Some(seeker) = &self.seeker {
            seeker.interrupt();
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NextKey {
    pub user: u64,
    pub generation: u64,
    pub revision: u64,
    pub current: u64,
    pub occurrence: u64,
    pub track: u64,
    pub quality: String,
    pub duration: u64,
}
#[derive(Clone)]
pub struct NextRequest {
    pub key: NextKey,
    boundary: Arc<Boundary>,
}
struct NextState {
    request: NextRequest,
    ready: Option<Arc<Prepared>>,
}
struct Prepared {
    request: NextRequest,
    loaded: Mutex<Option<Loaded>>,
    previous: Arc<Frame>,
    frame: Arc<Frame>,
    quality: String,
}
pub struct Transition {
    pub key: NextKey,
    pub generation: u64,
    pub quality: String,
    pub rendered: Duration,
    pub elapsed: Duration,
    pub position: u64,
}
impl Prepared {
    fn promote(&self, loaded: &Mutex<Option<Loaded>>, current: &AtomicU64) -> Option<Transition> {
        let mut loaded = loaded.lock().unwrap();
        let boundary = &self.request.boundary;
        if !boundary.claimed() {
            return None;
        }
        let _ = current.compare_exchange(
            boundary.from,
            boundary.to,
            Ordering::SeqCst,
            Ordering::SeqCst,
        );
        if current.load(Ordering::SeqCst) != boundary.to {
            return None;
        }
        if let Some(next) = self.loaded.lock().unwrap().take() {
            *loaded = Some(next);
        }
        Some(Transition {
            key: self.request.key.clone(),
            generation: boundary.to,
            quality: self.quality.clone(),
            rendered: self.previous.rendered(),
            elapsed: boundary.elapsed(),
            position: self.frame.position(),
        })
    }
    fn cancel(&self) {
        self.request.boundary.force_cancel();
        self.frame.feedback.cancel();
        self.loaded.lock().unwrap().take();
    }
}

struct NextOutput<'a> {
    staged: &'a Mutex<Option<NextState>>,
    sessions: &'a Mutex<Option<Loaded>>,
    generation: &'a Arc<AtomicU64>,
    capture: &'a Arc<crate::visualizer::Capture>,
    slot: &'a Slot,
}
impl NextOutput<'_> {
    fn arm(
        &self,
        previous: Option<&Arc<Frame>>,
        request: NextRequest,
        pcm: crate::pcm::PcmSource,
        session: Loaded,
        feedback: crate::pcm::Feedback,
    ) -> Option<Arc<Prepared>> {
        let mut state = self.staged.lock().unwrap();
        let matching = state
            .as_mut()
            .filter(|s| s.request.boundary.to == request.boundary.to);
        let previous = previous.filter(|f| f.id == request.key.generation);
        let compatible = self
            .sessions
            .lock()
            .unwrap()
            .as_ref()
            .is_some_and(|s| s.generation == request.key.generation && s.flac)
            && session.flac
            && previous.is_some_and(|f| f.format == (pcm.channels(), pcm.sample_rate()));
        if let (Some(state), Some(previous)) = (matching, previous)
            && compatible
            && request.boundary.preparing()
            && self.generation.load(Ordering::Acquire) == request.key.generation
        {
            let frame = Frame::new(request.boundary.to, &pcm, feedback, 0);
            let prepared = Arc::new(Prepared {
                quality: session.quality.clone(),
                loaded: Mutex::new(Some(session)),
                previous: previous.clone(),
                frame: frame.clone(),
                request: request.clone(),
            });
            state.ready = Some(prepared.clone());
            if self.slot.arm(
                pcm,
                frame,
                request.boundary.clone(),
                self.capture.clone(),
                self.generation.clone(),
            ) {
                return Some(prepared);
            }
            prepared.cancel();
        } else {
            request.boundary.force_cancel();
        }
        None
    }
}

enum Command {
    Load(
        u64,
        crate::pcm::PcmSource,
        String,
        crate::pcm::Feedback,
        u64,
    ),
    Prepare(
        NextRequest,
        crate::pcm::PcmSource,
        Box<Loaded>,
        crate::pcm::Feedback,
    ),
    CancelNext(u64),
    Pause(bool),
    Volume(f32),
    RetryOutput(u64),
    SelectOutput(crate::output_devices::Target),
    #[cfg(test)]
    FailOutput,
    Stop,
}

#[derive(Clone)]
pub struct Player {
    tx: mpsc::Sender<Command>,
    generation: Arc<AtomicU64>,
    serial: Arc<AtomicU64>,
    next: Arc<Mutex<Option<NextState>>>,
    pcm: crate::pcm::Pool,
    pub dsp: Arc<crate::dsp::Control>,
    pub outputs: Arc<crate::output_devices::Catalog>,
    loaded: Arc<Mutex<Option<Loaded>>>,
    preparing: Arc<tokio::sync::Semaphore>,
    seek_runtime: Arc<OnceLock<(tokio::runtime::Handle, Events)>>,
    pub visualizer: Arc<crate::visualizer::Capture>,
}

impl Player {
    #[cfg(any(test, debug_assertions))]
    pub fn inert() -> Self {
        let (tx, _) = mpsc::channel();
        Self {
            tx,
            generation: Arc::new(AtomicU64::new(0)),
            serial: Arc::new(AtomicU64::new(0)),
            next: Arc::new(Mutex::new(None)),
            seek_runtime: Arc::new(OnceLock::new()),
            preparing: Arc::new(tokio::sync::Semaphore::new(2)),
            loaded: Arc::new(Mutex::new(None)),
            pcm: crate::pcm::Pool::default(),
            dsp: Arc::new(crate::dsp::Control::default()),
            outputs: Arc::new(crate::output_devices::Catalog::default()),
            visualizer: Arc::new(crate::visualizer::Capture::default()),
        }
    }

    #[cfg(test)]
    pub(crate) fn seed_buffered_fixture(&self) {
        let (decoder, lease) = crate::audio_buffer::Reusable::new(Box::new(
            rodio::buffer::SamplesBuffer::new(1, 1000, vec![0.25; 2000]),
        ));
        drop(lease);
        *self.loaded.lock().unwrap() = Some(Loaded {
            generation: self.generation.load(Ordering::SeqCst),
            decoder,
            quality: "Fixture FLAC".into(),
            flac: true,
            normalization: crate::dsp::Metadata::default(),
            seeker: None,
            _download: DownloadGuard::default(),
        });
    }

    #[cfg(test)]
    pub(crate) fn fixture_next(&self, request: &NextRequest, claim: bool) {
        let (previous_pcm, feedback) = self
            .pcm
            .start(Box::new(rodio::buffer::SamplesBuffer::new(
                1,
                1000,
                vec![0.; 2],
            )))
            .unwrap();
        let previous = Frame::new(request.key.generation, &previous_pcm, feedback, 0);
        previous
            .rendered
            .store(request.key.duration * 1_000_000_000, Ordering::Release);
        let (decoder, lease) = crate::audio_buffer::Reusable::new(Box::new(
            rodio::buffer::SamplesBuffer::new(1, 1000, vec![0.75; 2000]),
        ));
        let (pcm, feedback) = self.pcm.start(Box::new(lease)).unwrap();
        let frame = Frame::new(request.boundary.to, &pcm, feedback, 0);
        let loaded = Loaded {
            generation: request.boundary.to,
            decoder,
            quality: "Fixture next FLAC".into(),
            flac: true,
            normalization: crate::dsp::Metadata::default(),
            seeker: None,
            _download: DownloadGuard::default(),
        };
        let prepared = Arc::new(Prepared {
            request: request.clone(),
            quality: loaded.quality.clone(),
            loaded: Mutex::new(Some(loaded)),
            previous,
            frame,
        });
        self.next.lock().unwrap().as_mut().unwrap().ready = Some(prepared);
        request.boundary.fixture_ready();
        if claim {
            assert!(self.fixture_claim_next(request));
        }
    }
    #[cfg(test)]
    pub(crate) fn fixture_claim_next(&self, request: &NextRequest) -> bool {
        request
            .boundary
            .fixture_claim(&self.generation, Duration::from_secs(request.key.duration))
    }

    pub fn new(events: Events) -> Self {
        let (tx, rx) = mpsc::channel();
        let generation = Arc::new(AtomicU64::new(0));
        let loaded: Arc<Mutex<Option<Loaded>>> = Arc::new(Mutex::new(None));
        let next: Arc<Mutex<Option<NextState>>> = Arc::new(Mutex::new(None));
        let current = generation.clone();
        let sessions = loaded.clone();
        let staged = next.clone();
        let visualizer = Arc::new(crate::visualizer::Capture::default());
        let capture = visualizer.clone();
        let outputs = Arc::new(crate::output_devices::Catalog::new(events.clone()));
        std::thread::spawn(move || {
            let mut output = crate::audio_output::Output::default();
            let mut volume = 0.65;
            let mut recovery = crate::audio_output::Recovery::default();
            let mut active: Option<Arc<Frame>> = None;
            let mut pending: Option<Arc<Prepared>> = None;
            let mut slot = Slot::default();
            let mut reported_buffering = false;
            let mut reported = Duration::ZERO;
            loop {
                let command = rx.recv_timeout(Duration::from_millis(100));
                if pending
                    .as_ref()
                    .is_some_and(|p| p.request.boundary.claimed())
                {
                    let prepared = pending.take().unwrap();
                    if let Some(transition) = prepared.promote(&sessions, &current) {
                        active = Some(prepared.frame.clone());
                        reported = Duration::ZERO;
                        reported_buffering = false;
                        events.send(Event::Transition(transition.generation));
                    }
                }
                if output.failed() {
                    // Close/join the output before resolving its last possible claim.
                    output.close();
                    events.send(Event::OutputDetails(output.description()));
                    if let Some(prepared) = pending.take() {
                        if prepared.request.boundary.claimed() {
                            if let Some(transition) = prepared.promote(&sessions, &current) {
                                active = Some(prepared.frame.clone());
                                events.send(Event::Transition(transition.generation));
                            }
                            prepared.frame.feedback.cancel();
                        } else {
                            prepared.cancel();
                        }
                    }
                    slot.clear();
                    let mut state = staged.lock().unwrap();
                    if state
                        .as_ref()
                        .is_some_and(|s| !s.request.boundary.claimed())
                        && let Some(state) = state.take()
                    {
                        state.request.boundary.force_cancel();
                        if let Some(ready) = state.ready {
                            ready.cancel();
                        }
                    }
                    drop(state);
                    capture.playing(false);
                    capture.invalidate();
                    let id = current.load(Ordering::Acquire);
                    recovery.fail();
                    let position = active.take().and_then(|frame| {
                        frame.feedback.cancel();
                        if frame.id != id {
                            return None;
                        }
                        events.send(Event::Listening {
                            generation: id,
                            rendered: frame.rendered(),
                            elapsed: frame.elapsed(),
                        });
                        Some(frame.position())
                    });
                    events.send(Event::OutputLost {
                        generation: id,
                        position,
                        epoch: recovery.epoch(),
                    });
                }
                match command {
                    Ok(Command::Load(id, pcm, quality, feedback, start)) => {
                        if id != current.load(Ordering::SeqCst) {
                            continue;
                        }
                        if let Some(previous) = active.take() {
                            previous.feedback.cancel();
                        }
                        if let Some(previous) = pending.take() {
                            previous.cancel();
                        }
                        slot.clear();
                        slot = Slot::default();
                        if recovery.blocked() || output.reset(volume).is_err() {
                            if !recovery.blocked() {
                                recovery.fail();
                            }
                            feedback.cancel();
                            events.send(Event::OutputLost {
                                generation: id,
                                position: Some(start),
                                epoch: recovery.epoch(),
                            });
                            continue;
                        }
                        if id != current.load(Ordering::SeqCst) {
                            feedback.cancel();
                            continue;
                        }
                        events.send(Event::OutputDetails(output.description()));
                        capture.invalidate();
                        capture.playing(false);
                        let frame = Frame::new(id, &pcm, feedback, start);
                        output.append(Chain::new(
                            pcm,
                            frame.clone(),
                            slot.clone(),
                            capture.clone(),
                            current.clone(),
                        ));
                        active = Some(frame);
                        reported = Duration::ZERO;
                        reported_buffering = false;
                        events.send(Event::Playing {
                            generation: id,
                            quality,
                        });
                    }
                    Ok(Command::Prepare(request, pcm, session, feedback)) => {
                        if let Some(prepared) = (NextOutput {
                            staged: &staged,
                            sessions: &sessions,
                            generation: &current,
                            capture: &capture,
                            slot: &slot,
                        })
                        .arm(
                            active.as_ref(),
                            request,
                            pcm,
                            *session,
                            feedback,
                        ) {
                            pending = Some(prepared);
                        }
                    }
                    Ok(Command::CancelNext(id)) => {
                        if pending
                            .as_ref()
                            .is_some_and(|p| p.request.boundary.to == id)
                        {
                            pending.take().unwrap().cancel();
                            slot.clear();
                        }
                    }
                    Ok(Command::Pause(paused)) => {
                        capture.playing(!paused);
                        output.pause(paused);
                    }
                    #[cfg(test)]
                    Ok(Command::FailOutput) => output.simulate_failure(),
                    Ok(Command::RetryOutput(epoch)) => recovery.retry(epoch),
                    Ok(Command::SelectOutput(target)) => output.select(target),
                    Ok(Command::Volume(v)) => {
                        volume = v;
                        output.volume(v);
                    }
                    Ok(Command::Stop) => {
                        capture.playing(false);
                        if let Some(frame) = active.take() {
                            frame.feedback.cancel();
                        }
                        if let Some(prepared) = pending.take() {
                            prepared.cancel();
                        }
                        slot.clear();
                        output.stop();
                    }
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                }
                if output.failed() {
                    continue;
                }
                output.sync_pause();
                if let Some(frame) = &active {
                    let id = frame.id;
                    // A just-claimed callback boundary is reconciled on the next
                    // loop, not mistaken for a superseding manual play/seek.
                    if pending
                        .as_ref()
                        .is_some_and(|p| p.request.boundary.claimed())
                    {
                        continue;
                    }
                    if id != current.load(Ordering::SeqCst) {
                        if pending
                            .as_ref()
                            .is_some_and(|p| p.request.boundary.claimed())
                        {
                            continue;
                        }
                        capture.playing(false);
                        frame.feedback.cancel();
                        output.stop();
                        active = None;
                        continue;
                    }
                    if frame.feedback.failed() {
                        capture.playing(false);
                        frame.feedback.cancel();
                        output.stop();
                        active = None;
                        events.send(Event::PlaybackError { generation: id, message: "Audio decoding failed or changed format. Press play to retry from this position.".into() });
                        continue;
                    }
                    let buffering = frame.feedback.buffering();
                    capture.playing(!output.sink.is_paused() && !buffering);
                    if buffering != reported_buffering {
                        events.send(Event::StreamBuffering {
                            generation: id,
                            buffering,
                        });
                        reported_buffering = buffering;
                    }
                    let rendered = frame.rendered();
                    if rendered != reported {
                        events.send(Event::Listening {
                            generation: id,
                            rendered,
                            elapsed: frame.elapsed(),
                        });
                        reported = rendered;
                    }
                    if output.sink.empty() {
                        if output.failed() {
                            continue;
                        }
                        if pending
                            .as_ref()
                            .is_some_and(|p| p.request.boundary.claimed())
                        {
                            continue;
                        }
                        capture.playing(false);
                        events.send(Event::Position {
                            generation: id,
                            seconds: frame.position(),
                        });
                        active = None;
                        events.send(Event::Ended(id));
                    } else if !output.sink.is_paused() {
                        events.send(Event::Position {
                            generation: id,
                            seconds: frame.position(),
                        });
                    }
                }
            }
            if let Some(frame) = active {
                frame.feedback.cancel();
            }
            if let Some(prepared) = pending {
                prepared.cancel();
            }
            slot.clear();
        });
        Self {
            tx,
            generation,
            serial: Arc::new(AtomicU64::new(0)),
            next,
            loaded,
            seek_runtime: Arc::new(OnceLock::new()),
            preparing: Arc::new(tokio::sync::Semaphore::new(2)),
            pcm: crate::pcm::Pool::default(),
            dsp: Arc::new(crate::dsp::Control::default()),
            outputs,
            visualizer,
        }
    }

    pub fn reserve(&self) -> u64 {
        self.discard_next();
        let mut loaded = self.loaded.lock().unwrap();
        let id = self.serial.fetch_add(1, Ordering::SeqCst) + 1;
        self.generation.store(id, Ordering::SeqCst);
        *loaded = None;
        self.visualizer.playing(false);
        self.visualizer.invalidate();
        let _ = self.tx.send(Command::Stop);
        id
    }
    fn discard_state(&self, state: NextState) {
        state.request.boundary.force_cancel();
        if let Some(ready) = state.ready {
            ready.cancel();
        }
        let _ = self.tx.send(Command::CancelNext(state.request.boundary.to));
    }
    fn discard_next(&self) {
        let state = self.next.lock().unwrap().take();
        if let Some(state) = state {
            self.discard_state(state);
        }
    }
    pub fn reconcile_next(&self, cancel: bool) -> Option<Transition> {
        let mut next = self.next.lock().unwrap();
        let state = next.as_ref()?;
        if state.request.boundary.claimed() || (cancel && !state.request.boundary.cancel()) {
            let state = next.take().unwrap();
            drop(next);
            return state.ready?.promote(&self.loaded, &self.generation);
        }
        if cancel {
            let state = next.take().unwrap();
            drop(next);
            self.discard_state(state);
        }
        None
    }
    pub fn begin_next(&self, key: NextKey) -> Option<NextRequest> {
        self.discard_next();
        if !self.current(key.generation)
            || !self
                .loaded
                .lock()
                .unwrap()
                .as_ref()
                .is_some_and(|s| s.generation == key.generation && s.flac)
        {
            return None;
        }
        let id = self.serial.fetch_add(1, Ordering::SeqCst) + 1;
        let request = NextRequest {
            boundary: Arc::new(Boundary::new(key.generation, id, key.duration)),
            key,
        };
        *self.next.lock().unwrap() = Some(NextState {
            request: request.clone(),
            ready: None,
        });
        Some(request)
    }
    pub fn has_prepared_next(&self) -> bool {
        self.next
            .lock()
            .unwrap()
            .as_ref()
            .is_some_and(|state| state.request.boundary.pending())
    }
    pub fn is_preparing(&self, request: &NextRequest) -> bool {
        request.boundary.preparing() && self.current(request.key.generation)
    }
    pub fn fail_next(&self, request: &NextRequest) {
        let mut next = self.next.lock().unwrap();
        if next
            .as_ref()
            .is_some_and(|s| s.request.boundary.to == request.boundary.to)
        {
            let state = next.take().unwrap();
            drop(next);
            self.discard_state(state);
        }
    }
    pub async fn prepare_next(&self, request: NextRequest, stream: Stream) -> Result<()> {
        let _preparing = self.preparing.acquire().await?;
        if !self.is_preparing(&request) {
            return Ok(());
        }
        anyhow::ensure!(
            stream.codec.eq_ignore_ascii_case("flac"),
            "Continuous AAC transitions await priming/padding validation"
        );
        let (loaded, lease) = open_stream(request.boundary.to, stream, 0).await?;
        let previous = self
            .loaded
            .lock()
            .unwrap()
            .as_ref()
            .map(|s| s.normalization);
        anyhow::ensure!(
            self.dsp
                .get()
                .continuous(previous.unwrap_or_default(), loaded.normalization),
            "Stateful EQ or changed normalization requires an ordinary transition"
        );
        if !self.is_preparing(&request) {
            return Ok(());
        }
        let (pcm, feedback) = self.pcm.start(Box::new(crate::dsp::Processed::new(
            lease,
            self.dsp.clone(),
            loaded.normalization,
        )?))?;
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while !feedback.ready() {
            if !self.is_preparing(&request) {
                return Ok(());
            }
            anyhow::ensure!(
                !feedback.finished() && !feedback.failed() && std::time::Instant::now() < deadline,
                "Next audio is not ready"
            );
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        if self.is_preparing(&request) {
            self.tx
                .send(Command::Prepare(request, pcm, Box::new(loaded), feedback))
                .map_err(|_| anyhow::anyhow!("Audio output is unavailable"))?;
        }
        Ok(())
    }

    pub fn set_seek_runtime(&self, events: Events) {
        let _ = self
            .seek_runtime
            .set((tokio::runtime::Handle::current(), events));
    }

    // Cached seeks must not queue behind slow catalog/account HTTP requests.
    pub fn dispatch_seek(&self, generation: u64, position: u64) -> bool {
        let Some((runtime, events)) = self.seek_runtime.get() else {
            return false;
        };
        let player = self.clone();
        let events = events.clone();
        runtime.spawn(async move {
            if let Err(error) = player.seek_buffered(generation, position).await {
                events.send(Event::PlaybackError {
                    generation,
                    message: error.to_string(),
                });
            }
        });
        true
    }

    pub fn reserve_seek(&self, expected: u64) -> Option<u64> {
        if !self.current(expected) {
            return None;
        }
        self.discard_next();
        let mut loaded = self.loaded.lock().unwrap();
        let loaded = loaded.as_mut()?;
        if loaded.generation != expected || !self.current(expected) {
            return None;
        }
        let id = self.serial.fetch_add(1, Ordering::SeqCst) + 1;
        self.generation.store(id, Ordering::SeqCst);
        loaded.generation = id;
        if let Some(seeker) = &loaded.seeker {
            seeker.interrupt();
        }
        self.visualizer.playing(false);
        self.visualizer.invalidate();
        let _ = self.tx.send(Command::Stop);
        Some(id)
    }

    pub async fn seek_buffered(&self, id: u64, position: u64) -> Result<()> {
        let _preparing = self.preparing.acquire().await?;
        if !self.current(id) {
            return Ok(());
        }
        let (decoder, quality, normalization) = {
            let loaded = self.loaded.lock().unwrap();
            let loaded = loaded
                .as_ref()
                .filter(|s| s.generation == id)
                .ok_or_else(|| {
                    anyhow::anyhow!("The buffered song changed. Press play to reload it.")
                })?;
            (
                loaded.decoder.clone(),
                loaded.quality.clone(),
                loaded.normalization,
            )
        };
        let player = self.clone();
        let lease = tokio::task::spawn_blocking(move || {
            let mut lease = decoder.acquire(|| player.current(id))?;
            lease.try_seek(Duration::from_secs(position)).map_err(|_| anyhow::anyhow!(
                "Could not seek using the temporary audio buffer. Press play to refresh the stream at this position."
            ))?;
            Ok::<_, anyhow::Error>(lease)
        }).await??;
        if self.current(id) {
            let (pcm, feedback) = self.pcm.start(Box::new(crate::dsp::Processed::new(
                lease,
                self.dsp.clone(),
                normalization,
            )?))?;
            if self
                .tx
                .send(Command::Load(id, pcm, quality, feedback, position))
                .is_err()
            {
                let mut loaded = self.loaded.lock().unwrap();
                if loaded.as_ref().is_some_and(|s| s.generation == id) {
                    *loaded = None;
                }
                anyhow::bail!("Audio output is unavailable");
            }
        }
        Ok(())
    }

    pub fn normalization(&self) -> crate::dsp::Metadata {
        self.loaded
            .lock()
            .unwrap()
            .as_ref()
            .filter(|s| self.current(s.generation))
            .map_or(crate::dsp::Metadata::default(), |s| s.normalization)
    }
    pub fn current(&self, id: u64) -> bool {
        self.generation.load(Ordering::SeqCst) == id
    }
    pub fn stop(&self) {
        self.reserve();
    }
    pub fn select_output(&self, target: crate::output_devices::Target) {
        let _ = self.tx.send(Command::SelectOutput(target));
    }
    pub fn retry_output(&self, epoch: u64) {
        let _ = self.tx.send(Command::RetryOutput(epoch));
    }
    pub fn pause(&self, paused: bool) {
        if paused {
            self.visualizer.playing(false);
        }
        let _ = self.tx.send(Command::Pause(paused));
    }
    pub fn volume(&self, volume: f32) {
        let _ = self.tx.send(Command::Volume(volume));
    }

    pub async fn load(&self, id: u64, stream: Stream, position: u64) -> Result<()> {
        let _preparing = self.preparing.acquire().await?;
        if !self.current(id) {
            return Ok(());
        }
        let (session, lease) = open_stream(id, stream, position).await?;
        if self.current(id) {
            let (pcm, feedback) = self.pcm.start(Box::new(crate::dsp::Processed::new(
                lease,
                self.dsp.clone(),
                session.normalization,
            )?))?;
            let mut loaded = self.loaded.lock().unwrap();
            if !self.current(id) {
                return Ok(());
            }
            let quality = session.quality.clone();
            *loaded = Some(session);
            if self
                .tx
                .send(Command::Load(id, pcm, quality, feedback, position))
                .is_err()
            {
                *loaded = None;
                anyhow::bail!("Audio output is unavailable");
            }
        }
        Ok(())
    }
}

async fn open_stream(
    id: u64,
    stream: Stream,
    position: u64,
) -> Result<(Loaded, crate::audio_buffer::Lease)> {
    let quality = stream.label();
    let flac = stream.codec.eq_ignore_ascii_case("flac");
    let normalization = stream.normalization;
    let mut seeker = None;
    let mut download = DownloadGuard::default();
    let decoder: Box<dyn Source<Item = f32> + Send> = match stream.source {
        StreamSource::Direct(url) => {
            let reader = StreamDownload::new_http(
                url.parse()?,
                audio_storage(),
                Settings::default().prefetch_bytes(128 * 1024),
            )
            .await
            .map_err(|_| {
                anyhow::anyhow!(
                    "Unable to buffer the TIDAL stream. Check your connection and try again."
                )
            })?;
            let cancellation = reader.cancellation_token();
            download = DownloadGuard(Some(Box::new(move || cancellation.cancel())));
            Box::new(decode(reader).await?)
        }
        StreamSource::Dash(manifest) => {
            let source = crate::dash::source(manifest).await?;
            seeker = Some(source.seek_handle());
            Box::new(source)
        }
    };
    let (decoder, seeker) = prepare_start(decoder, seeker, position).await?;
    let (decoder, lease) = crate::audio_buffer::Reusable::new(decoder);
    Ok((
        Loaded {
            generation: id,
            decoder,
            quality,
            flac,
            normalization,
            seeker,
            _download: download,
        },
        lease,
    ))
}

async fn prepare_start(
    mut decoder: Box<dyn Source<Item = f32> + Send>,
    seeker: Option<crate::dash::SeekHandle>,
    position: u64,
) -> Result<(
    Box<dyn Source<Item = f32> + Send>,
    Option<crate::dash::SeekHandle>,
)> {
    if position == 0 {
        return Ok((decoder, seeker));
    }
    // Resolve the saved position before the decoder ever reaches the mixer.
    tokio::task::spawn_blocking(move || {
        let position = Duration::from_secs(position);
        if let Some(seeker) = &seeker {
            seeker.prepare(position)?;
        }
        decoder.try_seek(position).map_err(|_| {
            anyhow::anyhow!(
                "Could not prepare the saved position. Seek to the beginning to restart this track."
            )
        })?;
        Ok((decoder, seeker))
    })
    .await?
}

pub(crate) async fn decode(reader: StreamDownload<AudioStorage>) -> Result<AudioDecoder> {
    Ok(tokio::task::spawn_blocking(move || {
        let length = reader.content_length();
        let mut builder = Decoder::builder()
            .with_data(reader)
            .with_seekable(length.is_some());
        if let Some(length) = length {
            builder = builder.with_byte_len(length);
        }
        builder.build()
    })
    .await??)
}

pub fn audio_test() -> Result<()> {
    use rodio::Source;
    let mut output = OutputStreamBuilder::from_default_device()?.open_stream()?;
    output.log_on_drop(false);
    let sink = Sink::connect_new(output.mixer());
    sink.append(
        rodio::source::SineWave::new(440.0)
            .take_duration(Duration::from_millis(150))
            .amplify(0.03),
    );
    sink.sleep_until_end();
    println!("Audio device opened and rendered 150 ms of test audio.");
    Ok(())
}

#[cfg(test)]
#[path = "audio_next_tests.rs"]
mod next_tests;

#[cfg(test)]
mod resume_tests {
    use super::*;

    fn collect_pcm(mut pcm: crate::pcm::PcmSource, state: crate::pcm::Feedback) -> usize {
        let activity = state.activity();
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        let mut samples = 0;
        for sample in &mut pcm {
            assert!(std::time::Instant::now() < deadline);
            if activity.load(Ordering::Relaxed) {
                assert_eq!(sample, 0.25);
                samples += 1;
            } else {
                std::thread::sleep(Duration::from_millis(1));
            }
        }
        assert!(!state.failed());
        samples
    }

    #[tokio::test]
    async fn buffered_seek_reuses_decoder_and_stop_discards_its_scope() {
        let mut player = Player::inert();
        let (tx, rx) = mpsc::channel();
        player.tx = tx;
        player.seed_buffered_fixture();
        let original = player
            .loaded
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .decoder
            .clone();
        let weak = Arc::downgrade(&original);
        for (expected, position, samples) in [(0, 1, 1000), (1, 0, 2000)] {
            let id = player.reserve_seek(expected).unwrap();
            assert!(matches!(rx.recv().unwrap(), Command::Stop));
            assert!(
                player.reserve_seek(expected).is_none(),
                "Stale scope must not reserve a seek"
            );
            player.seek_buffered(id, position).await.unwrap();
            let Command::Load(generation, pcm, quality, state, offset) = rx.recv().unwrap() else {
                panic!("Expected prepared PCM");
            };
            assert_eq!((generation, offset), (id, position));
            assert_eq!(quality, "Fixture FLAC");
            assert_eq!(collect_pcm(pcm, state), samples);
            assert!(Arc::ptr_eq(
                &original,
                &player.loaded.lock().unwrap().as_ref().unwrap().decoder
            ));
        }
        drop(original);
        player.stop();
        assert!(matches!(rx.recv().unwrap(), Command::Stop));
        assert!(
            weak.upgrade().is_none(),
            "Stopped/account-replaced song must be discarded"
        );
        player.seek_buffered(2, 1).await.unwrap();
        assert!(rx.try_recv().is_err(), "Stale seek must not publish audio");
    }

    #[test]
    fn seek_retains_download_but_stop_cancels_it() {
        let player = Player::inert();
        player.seed_buffered_fixture();
        let cancelled = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let signal = cancelled.clone();
        player.loaded.lock().unwrap().as_mut().unwrap()._download =
            DownloadGuard(Some(Box::new(move || {
                signal.store(true, Ordering::Release)
            })));
        assert!(player.reserve_seek(0).is_some());
        assert!(!cancelled.load(Ordering::Acquire));
        player.stop();
        assert!(cancelled.load(Ordering::Acquire));
    }

    #[tokio::test]
    async fn saved_position_is_prepared_before_append_and_sink_waits_for_explicit_play() {
        let samples: Vec<f32> = (0..1000).map(|index| index as f32 / 1000.).collect();
        let source = rodio::buffer::SamplesBuffer::new(1, 100, samples);
        let (source, _) = prepare_start(Box::new(source), None, 3).await.unwrap();
        let (sink, mut output) = Sink::new();
        sink.pause();
        sink.append(source);
        for _ in 0..100 {
            assert_eq!(output.next(), Some(0.));
        }
        assert_eq!(sink.get_pos(), Duration::ZERO);
        sink.play();
        let first = output
            .by_ref()
            .take(100)
            .find(|sample| *sample != 0.)
            .unwrap();
        assert!(
            (first - 0.3).abs() < 0.0001,
            "Resumed at the wrong sample: {first}"
        );
    }

    #[tokio::test]
    async fn unseekable_resume_fails_instead_of_playing_the_beginning() {
        struct Unseekable;
        impl Iterator for Unseekable {
            type Item = f32;
            fn next(&mut self) -> Option<f32> {
                Some(0.)
            }
        }
        impl Source for Unseekable {
            fn current_span_len(&self) -> Option<usize> {
                None
            }
            fn channels(&self) -> u16 {
                1
            }
            fn sample_rate(&self) -> u32 {
                100
            }
            fn total_duration(&self) -> Option<Duration> {
                None
            }
        }
        assert!(prepare_start(Box::new(Unseekable), None, 5).await.is_err());
    }
}
