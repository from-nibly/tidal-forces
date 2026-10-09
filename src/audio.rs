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
        u64,
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
    #[cfg(any(test, debug_assertions))]
    pub fn inert() -> Self {
        let (tx, _) = mpsc::channel();
        Self {
            tx,
            generation: Arc::new(AtomicU64::new(0)),
            visualizer: Arc::new(crate::visualizer::Capture::default()),
        }
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
            let mut seeker: Option<crate::dash::SeekHandle> = None;
            let mut rendered = Arc::new(AtomicU64::new(0));
            let mut clock_start = std::time::Instant::now();
            let mut reported = 0;
            loop {
                match rx.recv_timeout(Duration::from_millis(250)) {
                    Ok(Command::Load(id, decoder, quality, seek_handle, start)) => {
                        if id != current.load(Ordering::SeqCst) {
                            continue;
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
                            crate::audio_meter::Meter::new(decoder, rendered.clone()),
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
                        } else {
                            position_offset = 0;
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
                        seeker = None;
                        events.send(Event::Ended(id));
                    } else if !sink.is_paused() {
                        events.send(Event::Position {
                            generation: id,
                            seconds: sink.get_pos().as_secs().saturating_add(position_offset),
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

    pub async fn load(&self, id: u64, stream: Stream, position: u64) -> Result<()> {
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
        let (decoder, seeker) = prepare_start(decoder, seeker, position).await?;
        if self.current(id) {
            self.tx
                .send(Command::Load(id, decoder, quality, seeker, position))
                .map_err(|_| anyhow::anyhow!("Audio output is unavailable"))?;
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

#[cfg(test)]
mod resume_tests {
    use super::*;

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
