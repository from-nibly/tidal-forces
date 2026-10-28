use crate::api::{Stream, StreamSource};
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

enum Command {
    Load(
        u64,
        crate::pcm::PcmSource,
        String,
        crate::pcm::Feedback,
        u64,
    ),
    Pause(bool),
    Volume(f32),
    Stop,
}

#[derive(Clone)]
pub struct Player {
    tx: mpsc::Sender<Command>,
    generation: Arc<AtomicU64>,
    pcm: crate::pcm::Pool,
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
            seek_runtime: Arc::new(OnceLock::new()),
            preparing: Arc::new(tokio::sync::Semaphore::new(2)),
            loaded: Arc::new(Mutex::new(None)),
            pcm: crate::pcm::Pool::default(),
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
            seeker: None,
            _download: DownloadGuard::default(),
        });
    }

    pub fn new(events: Events) -> Self {
        let (tx, rx) = mpsc::channel();
        let generation = Arc::new(AtomicU64::new(0));
        let current = generation.clone();
        let visualizer = Arc::new(crate::visualizer::Capture::default());
        let capture = visualizer.clone();
        std::thread::spawn(move || {
            // Keep the output device alive on its owner thread, not the render thread.
            let mut output = match OutputStreamBuilder::open_default_stream() {
                Ok(s) => s,
                Err(_) => {
                    events.send(Event::AudioError(
                        "No audio output device. Connect one and restart Tidal Forces.".into(),
                    ));
                    return;
                }
            };
            output.log_on_drop(false);
            let mut sink = Sink::connect_new(output.mixer());
            let mut volume = 0.65;
            let mut active = None;
            let mut position_offset = 0;
            let mut feedback: Option<crate::pcm::Feedback> = None;
            let mut reported_buffering = false;
            let mut rendered = Arc::new(AtomicU64::new(0));
            let mut clock_start = std::time::Instant::now();
            let mut reported = 0;
            loop {
                match rx.recv_timeout(Duration::from_millis(250)) {
                    Ok(Command::Load(id, pcm, quality, state, start)) => {
                        if id != current.load(Ordering::SeqCst) {
                            continue;
                        }
                        if let Some(previous) = feedback.take() {
                            previous.cancel();
                        }
                        sink.stop();
                        sink = Sink::connect_new(output.mixer());
                        sink.set_volume(volume);
                        sink.pause();
                        position_offset = start;
                        capture.invalidate();
                        capture.playing(false);
                        rendered = Arc::new(AtomicU64::new(0));
                        clock_start = std::time::Instant::now();
                        reported = 0;
                        sink.append(crate::visualizer::Tap::new(
                            crate::audio_meter::Meter::with_activity(
                                pcm,
                                rendered.clone(),
                                state.activity(),
                            ),
                            capture.clone(),
                            current.clone(),
                            id,
                        ));
                        active = Some(id);
                        feedback = Some(state);
                        reported_buffering = false;
                        events.send(Event::Playing {
                            generation: id,
                            quality,
                        });
                    }
                    Ok(Command::Pause(paused)) => {
                        capture.playing(!paused);
                        if paused {
                            sink.pause();
                        } else {
                            sink.play();
                        }
                    }
                    Ok(Command::Volume(v)) => {
                        volume = v;
                        sink.set_volume(v);
                    }
                    Ok(Command::Stop) => {
                        capture.playing(false);
                        if let Some(state) = feedback.take() {
                            state.cancel();
                        }
                        sink.stop();
                        active = None;
                    }
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                }
                if let Some(id) = active {
                    if id != current.load(Ordering::SeqCst) {
                        capture.playing(false);
                        if let Some(state) = feedback.take() {
                            state.cancel();
                        }
                        sink.stop();
                        active = None;
                        continue;
                    }
                    if feedback.as_ref().is_some_and(|state| state.failed()) {
                        capture.playing(false);
                        if let Some(state) = feedback.take() {
                            state.cancel();
                        }
                        sink.stop();
                        active = None;
                        events.send(Event::PlaybackError { generation: id, message: "Audio decoding failed or changed format. Press play to retry from this position.".into() });
                        continue;
                    }
                    let buffering = feedback.as_ref().is_some_and(|state| state.buffering());
                    capture.playing(!sink.is_paused() && !buffering);
                    if buffering != reported_buffering {
                        events.send(Event::StreamBuffering {
                            generation: id,
                            buffering,
                        });
                        reported_buffering = buffering;
                    }
                    let nanos = rendered.load(Ordering::Relaxed);
                    if nanos != reported {
                        events.send(Event::Listening {
                            generation: id,
                            rendered: Duration::from_nanos(nanos),
                            elapsed: clock_start.elapsed(),
                        });
                        reported = nanos;
                    }
                    if sink.empty() {
                        capture.playing(false);
                        active = None;
                        feedback = None;
                        events.send(Event::Ended(id));
                    } else if !sink.is_paused() {
                        events.send(Event::Position {
                            generation: id,
                            seconds: Duration::from_nanos(nanos)
                                .as_secs()
                                .saturating_add(position_offset),
                        });
                    }
                }
            }
            if let Some(state) = feedback {
                state.cancel();
            }
        });
        Self {
            tx,
            generation,
            seek_runtime: Arc::new(OnceLock::new()),
            preparing: Arc::new(tokio::sync::Semaphore::new(2)),
            loaded: Arc::new(Mutex::new(None)),
            pcm: crate::pcm::Pool::default(),
            visualizer,
        }
    }

    pub fn reserve(&self) -> u64 {
        let mut loaded = self.loaded.lock().unwrap();
        let id = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        *loaded = None;
        self.visualizer.playing(false);
        self.visualizer.invalidate();
        let _ = self.tx.send(Command::Stop);
        id
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
        let mut loaded = self.loaded.lock().unwrap();
        let loaded = loaded.as_mut()?;
        if loaded.generation != expected || !self.current(expected) {
            return None;
        }
        let id = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
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
        let (decoder, quality) = {
            let loaded = self.loaded.lock().unwrap();
            let loaded = loaded
                .as_ref()
                .filter(|s| s.generation == id)
                .ok_or_else(|| {
                    anyhow::anyhow!("The buffered song changed. Press play to reload it.")
                })?;
            (loaded.decoder.clone(), loaded.quality.clone())
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
            let (pcm, feedback) = self.pcm.start(Box::new(lease))?;
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

    pub fn current(&self, id: u64) -> bool {
        self.generation.load(Ordering::SeqCst) == id
    }
    pub fn stop(&self) {
        self.reserve();
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
        let quality = stream.label();
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
        if self.current(id) {
            let (decoder, lease) = crate::audio_buffer::Reusable::new(decoder);
            let (pcm, feedback) = self.pcm.start(Box::new(lease))?;
            let mut loaded = self.loaded.lock().unwrap();
            if !self.current(id) {
                return Ok(());
            }
            *loaded = Some(Loaded {
                generation: id,
                decoder,
                quality: quality.clone(),
                seeker,
                _download: download,
            });
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
    let mut output = OutputStreamBuilder::open_default_stream()?;
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
