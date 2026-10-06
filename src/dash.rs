use anyhow::{Context, Result, bail, ensure};
use rodio::{Decoder, Source, source::SeekError};
use std::{
    collections::BTreeMap,
    io::Cursor,
    sync::{Arc, Condvar, Mutex},
    time::Duration,
};
use tokio::sync::watch;

#[derive(Clone, Debug)]
pub struct Segment {
    pub url: String,
    pub start: Duration,
    pub duration: Duration,
}

#[derive(Clone, Debug)]
pub struct Manifest {
    pub init: String,
    pub segments: Vec<Segment>,
    pub sample_rate: u32,
    pub duration: Duration,
}

pub fn parse(xml: &str) -> Result<Manifest> {
    ensure!(xml.len() < 2_000_000, "DASH manifest is too large");
    let document = roxmltree::Document::parse(xml)?;
    ensure!(
        !document
            .descendants()
            .any(|n| n.has_tag_name("ContentProtection")),
        "Encrypted DASH audio is not supported"
    );
    let root = document.root_element();
    ensure!(
        root.attribute("type").unwrap_or("static") == "static",
        "Live DASH is not supported"
    );
    let representation = document
        .descendants()
        .find(|n| {
            n.has_tag_name("Representation")
                && n.attribute("codecs")
                    .is_some_and(|c| c.eq_ignore_ascii_case("flac"))
        })
        .context("DASH stream does not contain clear FLAC audio")?;
    let template = representation
        .children()
        .find(|n| n.has_tag_name("SegmentTemplate"))
        .or_else(|| {
            representation
                .parent()
                .and_then(|n| n.children().find(|n| n.has_tag_name("SegmentTemplate")))
        })
        .context("Missing DASH segment template")?;
    let init = template
        .attribute("initialization")
        .context("Missing DASH initialization URL")?
        .to_owned();
    validate_url(&init)?;
    let media = template
        .attribute("media")
        .context("Missing DASH media URL")?;
    ensure!(
        media.contains("$Number$") || media.contains("$Time$"),
        "Unsupported DASH URL template"
    );
    let scale: u64 = template.attribute("timescale").unwrap_or("1").parse()?;
    ensure!(scale > 0, "Invalid DASH timescale");
    let sample_rate: u32 = representation
        .attribute("audioSamplingRate")
        .context("Missing FLAC sample rate")?
        .parse()?;
    ensure!(sample_rate > 0, "Invalid FLAC sample rate");
    let mut number: u64 = template.attribute("startNumber").unwrap_or("1").parse()?;
    let timeline = template
        .children()
        .find(|n| n.has_tag_name("SegmentTimeline"))
        .context("Missing DASH timeline")?;
    let mut time = 0u64;
    let mut segments = Vec::new();
    for node in timeline.children().filter(|n| n.has_tag_name("S")) {
        if let Some(t) = node.attribute("t") {
            let t: u64 = t.parse()?;
            ensure!(t == time, "Discontinuous DASH timeline is unsupported");
        }
        let duration: u64 = node
            .attribute("d")
            .context("Missing segment duration")?
            .parse()?;
        ensure!(duration > 0, "Zero-length DASH segment");
        let repeats: i64 = node.attribute("r").unwrap_or("0").parse()?;
        ensure!(
            (0..=20_000).contains(&repeats),
            "Unsupported DASH repeat count"
        );
        for _ in 0..=repeats {
            ensure!(segments.len() < 20_000, "Too many DASH segments");
            let url = media
                .replace("$Number$", &number.to_string())
                .replace("$Time$", &time.to_string());
            ensure!(!url.contains('$'), "Unsupported DASH template variable");
            validate_url(&url)?;
            segments.push(Segment {
                url,
                start: Duration::try_from_secs_f64(time as f64 / scale as f64)
                    .context("Invalid DASH duration")?,
                duration: Duration::try_from_secs_f64(duration as f64 / scale as f64)
                    .context("Invalid DASH duration")?,
            });
            time = time
                .checked_add(duration)
                .context("DASH timeline overflow")?;
            number = number
                .checked_add(1)
                .context("DASH segment number overflow")?;
        }
    }
    ensure!(!segments.is_empty(), "Empty DASH timeline");
    Ok(Manifest {
        init,
        segments,
        sample_rate,
        duration: Duration::try_from_secs_f64(time as f64 / scale as f64)
            .context("Invalid DASH duration")?,
    })
}

fn validate_url(value: &str) -> Result<()> {
    let url = reqwest::Url::parse(value)?;
    ensure!(
        url.scheme() == "https"
            && url.host_str().is_some()
            && url.username().is_empty()
            && url.password().is_none(),
        "Insecure DASH URL rejected"
    );
    Ok(())
}

#[derive(Default)]
struct CacheState {
    chunks: BTreeMap<usize, Arc<Vec<u8>>>,
    error: Option<String>,
}
#[derive(Default)]
struct Cache {
    state: Mutex<CacheState>,
    ready: Condvar,
}

type ChunkDecoder = Decoder<Cursor<Vec<u8>>>;
struct PreparedSeek {
    position: Duration,
    index: usize,
    data: Arc<Vec<u8>>,
    decoder: ChunkDecoder,
}

#[derive(Clone)]
pub struct SeekHandle {
    manifest: Manifest,
    init: Arc<Vec<u8>>,
    cache: Arc<Cache>,
    client: reqwest::Client,
    runtime: tokio::runtime::Handle,
    prepared: Arc<Mutex<Option<PreparedSeek>>>,
    channels: u16,
}
impl SeekHandle {
    // Called on the audio-control thread, never the real-time output callback.
    pub fn prepare(&self, position: Duration) -> Result<()> {
        let position = position.min(
            self.manifest
                .duration
                .saturating_sub(Duration::from_millis(1)),
        );
        let index = self
            .manifest
            .segments
            .partition_point(|s| s.start <= position)
            .saturating_sub(1);
        let cached = self.cache.state.lock().unwrap().chunks.get(&index).cloned();
        let data = match cached {
            Some(data) => data,
            None => Arc::new(
                self.runtime
                    .block_on(download(&self.client, &self.manifest.segments[index].url))?,
            ),
        };
        let mut decoder = decode_chunk(&self.init, &data)?;
        ensure!(
            decoder.channels() == self.channels
                && decoder.sample_rate() == self.manifest.sample_rate,
            "FLAC format changed during seek"
        );
        let offset = position.saturating_sub(self.manifest.segments[index].start);
        let skip = (offset.as_secs_f64() * self.manifest.sample_rate as f64).round() as usize
            * self.channels as usize;
        if skip > 0 {
            ensure!(
                decoder.nth(skip - 1).is_some(),
                "Seek exceeds audio fragment"
            );
        }
        *self.prepared.lock().unwrap() = Some(PreparedSeek {
            position,
            index,
            data,
            decoder,
        });
        Ok(())
    }
}

pub struct DashSource {
    manifest: Manifest,
    init: Arc<Vec<u8>>,
    cache: Arc<Cache>,
    request: watch::Sender<Option<usize>>,
    decoder: ChunkDecoder,
    index: usize,
    channels: u16,
    sample_rate: u32,
    exhausted: bool,
    seeker: SeekHandle,
}

async fn download(client: &reqwest::Client, url: &str) -> Result<Vec<u8>> {
    let mut response = client
        .get(url)
        .send()
        .await
        .map_err(|_| anyhow::anyhow!("DASH connection failed"))?;
    ensure!(
        response.status().is_success(),
        "TIDAL audio segment returned HTTP {}",
        response.status()
    );
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| anyhow::anyhow!("DASH download interrupted"))?
    {
        ensure!(
            bytes.len() + chunk.len() <= 32 * 1024 * 1024,
            "DASH segment exceeds buffer limit"
        );
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

fn decode_chunk(init: &[u8], media: &[u8]) -> Result<ChunkDecoder> {
    let mut data = Vec::with_capacity(init.len() + media.len());
    data.extend_from_slice(init);
    data.extend_from_slice(media);
    let len = data.len() as u64;
    Ok(Decoder::builder()
        .with_data(Cursor::new(data))
        .with_byte_len(len)
        .with_seekable(true)
        .build()?)
}

pub async fn source(manifest: Manifest) -> Result<DashSource> {
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(25))
        .build()?;
    let init = Arc::new(download(&client, &manifest.init).await?);
    let first = Arc::new(download(&client, &manifest.segments[0].url).await?);
    let cache = Arc::new(Cache::default());
    cache.state.lock().unwrap().chunks.insert(0, first.clone());
    let (tx, mut rx) = watch::channel(Some(0));
    let worker_cache = cache.clone();
    let segments = manifest.segments.clone();
    let seek_client = client.clone();
    let runtime = tokio::runtime::Handle::current();
    tokio::spawn(async move {
        loop {
            let Some(index) = *rx.borrow_and_update() else {
                break;
            };
            // Retain only the current fragment and two ahead, even after a seek.
            let next = {
                let mut state = worker_cache.state.lock().unwrap();
                state.chunks.retain(|i, _| *i >= index && *i <= index + 2);
                (index..=(index + 2).min(segments.len() - 1))
                    .find(|i| !state.chunks.contains_key(i))
            };
            if let Some(next) = next {
                tokio::select! {
                    changed = rx.changed() => { if changed.is_err() { break; } }
                    data = download(&client, &segments[next].url) => {
                        let mut state = worker_cache.state.lock().unwrap();
                        match data {
                            Ok(data) => { state.chunks.insert(next, Arc::new(data)); }
                            Err(e) => { state.error = Some(e.to_string()); worker_cache.ready.notify_all(); break; }
                        }
                        worker_cache.ready.notify_all();
                    }
                }
            } else if rx.changed().await.is_err() {
                break;
            }
        }
    });
    tokio::task::spawn_blocking(move || {
        let decoder = decode_chunk(&init, &first)?;
        let channels = decoder.channels();
        let sample_rate = decoder.sample_rate();
        ensure!(
            sample_rate == manifest.sample_rate,
            "Decoded FLAC rate does not match manifest"
        );
        let seeker = SeekHandle {
            manifest: manifest.clone(),
            init: init.clone(),
            cache: cache.clone(),
            client: seek_client,
            runtime,
            prepared: Arc::new(Mutex::new(None)),
            channels,
        };
        Ok(DashSource {
            manifest,
            init,
            cache,
            request: tx,
            decoder,
            index: 0,
            channels,
            sample_rate,
            exhausted: false,
            seeker,
        })
    })
    .await?
}

impl DashSource {
    pub fn seek_handle(&self) -> SeekHandle {
        self.seeker.clone()
    }

    fn load(&mut self, index: usize, offset: Duration) -> Result<()> {
        ensure!(
            offset < self.manifest.segments[index].duration,
            "Seek exceeds fragment duration"
        );
        self.request.send_if_modified(|current| {
            if *current == Some(index) {
                false
            } else {
                *current = Some(index);
                true
            }
        });
        let state = self.cache.state.lock().unwrap();
        let (state, wait) = self
            .cache
            .ready
            .wait_timeout_while(state, Duration::from_secs(30), |s| {
                !s.chunks.contains_key(&index) && s.error.is_none()
            })
            .unwrap();
        if let Some(error) = &state.error {
            bail!("{error}");
        }
        ensure!(!wait.timed_out(), "DASH buffering timed out");
        let data = state
            .chunks
            .get(&index)
            .context("Missing audio fragment")?
            .clone();
        drop(state);
        let mut decoder = decode_chunk(&self.init, &data)?;
        ensure!(
            decoder.sample_rate() == self.sample_rate && decoder.channels() == self.channels,
            "FLAC format changed between segments"
        );
        let skip = (offset.as_secs_f64() * self.sample_rate as f64).round() as usize
            * self.channels as usize;
        if skip > 0 {
            ensure!(
                decoder.nth(skip - 1).is_some(),
                "Seek exceeds audio fragment"
            );
        }
        self.decoder = decoder;
        self.index = index;
        self.exhausted = false;
        Ok(())
    }
}

impl Iterator for DashSource {
    type Item = f32;
    fn next(&mut self) -> Option<f32> {
        if self.exhausted {
            return None;
        }
        if let Some(sample) = self.decoder.next() {
            return Some(sample);
        }
        if self.index + 1 >= self.manifest.segments.len()
            || self.load(self.index + 1, Duration::ZERO).is_err()
        {
            self.exhausted = true;
            return None;
        }
        self.decoder.next()
    }
}
impl Source for DashSource {
    fn current_span_len(&self) -> Option<usize> {
        None
    }
    fn channels(&self) -> u16 {
        self.channels
    }
    fn sample_rate(&self) -> u32 {
        self.sample_rate
    }
    fn total_duration(&self) -> Option<Duration> {
        Some(self.manifest.duration)
    }
    fn try_seek(&mut self, pos: Duration) -> std::result::Result<(), SeekError> {
        let pos = pos.min(
            self.manifest
                .duration
                .saturating_sub(Duration::from_millis(1)),
        );
        if let Some(prepared) = self.seeker.prepared.lock().unwrap().take()
            && prepared.position == pos
        {
            self.decoder = prepared.decoder;
            self.index = prepared.index;
            self.exhausted = false;
            let mut state = self.cache.state.lock().unwrap();
            state
                .chunks
                .retain(|i, _| *i >= self.index && *i <= self.index + 2);
            state.chunks.insert(self.index, prepared.data);
            let _ = self.request.send(Some(self.index));
            return Ok(());
        }
        let index = self
            .manifest
            .segments
            .partition_point(|s| s.start <= pos)
            .saturating_sub(1);
        self.load(
            index,
            pos.saturating_sub(self.manifest.segments[index].start),
        )
        .map_err(|e| SeekError::Other(Box::new(std::io::Error::other(e.to_string()))))
    }
}
impl Drop for DashSource {
    fn drop(&mut self) {
        let _ = self.request.send(None);
    }
}

#[cfg(test)]
#[path = "dash_tests.rs"]
mod streaming_tests;

#[cfg(test)]
mod tests {
    use super::*;
    fn xml() -> &'static str {
        r#"<MPD type="static"><Period><AdaptationSet><Representation codecs="flac" audioSamplingRate="44100"><SegmentTemplate timescale="44100" initialization="https://audio.tidal.com/0.mp4" media="https://audio.tidal.com/$Number$.mp4" startNumber="1"><SegmentTimeline><S d="176400" r="1"/><S d="88200"/></SegmentTimeline></SegmentTemplate></Representation></AdaptationSet></Period></MPD>"#
    }
    #[test]
    fn expands_bounded_timeline_and_rejects_drm() {
        let m = parse(xml()).unwrap();
        assert_eq!(m.segments.len(), 3);
        assert_eq!(m.duration, Duration::from_secs(10));
        assert_eq!(m.segments[2].start, Duration::from_secs(8));
        assert_eq!(m.segments[2].duration, Duration::from_secs(2));
        assert!(m.segments[2].url.ends_with("/3.mp4"));
        assert!(parse(&xml().replace("<Period>", "<Period><ContentProtection/>")).is_err());
        assert!(parse(&xml().replace("https://", "http://")).is_err());
        assert!(parse(&xml().replace("r=\"1\"", "r=\"-1\"")).is_err());
        assert!(parse(&xml().replace("d=\"176400\"", "d=\"0\"")).is_err());
    }
}
