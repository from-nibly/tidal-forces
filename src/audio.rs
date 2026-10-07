use crate::api::{Stream, StreamSource};
use crate::backend::{Event, Events};
use anyhow::Result;
use rodio::{Decoder, OutputStreamBuilder, Sink, Source};
use std::{
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
    time::Duration,
};
use stream_download::{Settings, StreamDownload, storage::temp::TempStorageProvider};

type AudioDecoder = Decoder<StreamDownload<TempStorageProvider>>;

enum Command {
    Load(
        u64,
        Box<dyn Source<Item = f32> + Send>,
        String,
        Option<crate::dash::SeekHandle>,
    ),
    Pause(bool),
    Seek(u64),
    Volume(f32),
    Stop,
}

#[derive(Clone)]
pub struct Player {
    tx: mpsc::Sender<Command>,
    generation: Arc<AtomicU64>,
    pub visualizer: Arc<crate::visualizer::Capture>,
}

impl Player {
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
            let mut seeker: Option<crate::dash::SeekHandle> = None;
            loop {
                match rx.recv_timeout(Duration::from_millis(250)) {
                    Ok(Command::Load(id, decoder, quality, seek_handle)) => {
                        if id != current.load(Ordering::SeqCst) {
                            continue;
                        }
                        sink.stop();
                        sink = Sink::connect_new(output.mixer());
                        sink.set_volume(volume);
                        capture.invalidate();
                        capture.playing(true);
                        sink.append(crate::visualizer::Tap::new(
                            decoder,
                            capture.clone(),
                            current.clone(),
                            id,
                        ));
                        active = Some(id);
                        seeker = seek_handle;
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
                    Ok(Command::Seek(s)) => {
                        capture.playing(false);
                        let paused = sink.is_paused();
                        sink.pause();
                        let position = Duration::from_secs(s);
                        let prepared = if let Some(seeker) = &seeker {
                            seeker.prepare(position)
                        } else {
                            Ok(())
                        };
                        let result = prepared.and_then(|()| {
                            sink.try_seek(position)
                                .map_err(|e| anyhow::anyhow!(e.to_string()))
                        });
                        if !paused {
                            sink.play();
                        }
                        capture.playing(!paused);
                        if let Err(e) = result {
                            events.send(Event::Error(format!("Cannot seek this stream: {e}")));
                        }
                    }
                    Ok(Command::Stop) => {
                        capture.playing(false);
                        sink.stop();
                        active = None;
                        seeker = None;
                    }
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                }
                if let Some(id) = active {
                    if id != current.load(Ordering::SeqCst) {
                        capture.playing(false);
                        sink.stop();
                        active = None;
                        seeker = None;
                        continue;
                    }
                    if sink.empty() {
                        capture.playing(false);
                        active = None;
                        seeker = None;
                        events.send(Event::Ended(id));
                    } else if !sink.is_paused() {
                        events.send(Event::Position {
                            generation: id,
                            seconds: sink.get_pos().as_secs(),
                        });
                    }
                }
            }
        });
        Self {
            tx,
            generation,
            visualizer,
        }
    }

    pub fn reserve(&self) -> u64 {
        let id = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        self.visualizer.playing(false);
        self.visualizer.invalidate();
        let _ = self.tx.send(Command::Stop);
        id
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
    pub fn seek(&self, seconds: u64) {
        self.visualizer.playing(false);
        self.visualizer.invalidate();
        let _ = self.tx.send(Command::Seek(seconds));
    }

    pub async fn load(&self, id: u64, stream: Stream) -> Result<()> {
        if !self.current(id) {
            return Ok(());
        }
        let quality = stream.label();
        let mut seeker = None;
        let decoder: Box<dyn Source<Item = f32> + Send> = match stream.source {
            StreamSource::Direct(url) => {
                let reader = StreamDownload::new_http(
                    url.parse()?,
                    TempStorageProvider::new(),
                    Settings::default().prefetch_bytes(128 * 1024),
                )
                .await
                .map_err(|_| {
                    anyhow::anyhow!(
                        "Unable to buffer the TIDAL stream. Check your connection and try again."
                    )
                })?;
                Box::new(decode(reader).await?)
            }
            StreamSource::Dash(manifest) => {
                let source = crate::dash::source(manifest).await?;
                seeker = Some(source.seek_handle());
                Box::new(source)
            }
        };
        if self.current(id) {
            self.tx
                .send(Command::Load(id, decoder, quality, seeker))
                .map_err(|_| anyhow::anyhow!("Audio output is unavailable"))?;
        }
        Ok(())
    }
}

pub(crate) async fn decode(reader: StreamDownload<TempStorageProvider>) -> Result<AudioDecoder> {
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
