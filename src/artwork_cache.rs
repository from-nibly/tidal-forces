use anyhow::{Result, ensure};
use eframe::egui::{
    self,
    load::{Bytes, BytesLoadResult, BytesLoader, BytesPoll, LoadError},
};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    fs,
    io::Read,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, mpsc},
    time::Duration,
};

const MAX_IMAGE: usize = 2 * 1024 * 1024;
const MAX_FILES: usize = 128;
const MAX_DISK: u64 = 64 * 1024 * 1024;
const MAX_MEMORY: usize = 32;
type ImageResult = std::result::Result<Arc<[u8]>, String>;
struct Entry {
    value: Option<ImageResult>,
    used: u64,
}
#[derive(Default)]
struct State {
    enabled: bool,
    epoch: u64,
    clock: u64,
    entries: HashMap<String, Entry>,
    status: String,
    clear_pending: bool,
}
enum Job {
    Image {
        uri: String,
        key: String,
        epoch: u64,
        ctx: egui::Context,
    },
    Clear(egui::Context),
}
pub struct ArtworkCache {
    state: Arc<Mutex<State>>,
    tx: mpsc::SyncSender<Job>,
    root: Option<PathBuf>,
    disk: Arc<Mutex<()>>,
}

fn key(uri: &str) -> Option<String> {
    let path = uri
        .strip_prefix("https://resources.tidal.com/images/")?
        .strip_suffix(".jpg")?;
    let (asset, size) = path.rsplit_once('/')?;
    let parts: Vec<_> = asset.split('/').collect();
    if parts.len() != 5
        || !parts
            .iter()
            .zip([8, 4, 4, 4, 12])
            .all(|(p, n)| p.len() == n && p.bytes().all(|b| b.is_ascii_hexdigit()))
    {
        return None;
    }
    let (w, h) = size.split_once('x')?;
    let w = w.parse::<u32>().ok()?;
    let h = h.parse::<u32>().ok()?;
    if w == 0 || w > 1280 || h == 0 || h > 1280 {
        return None;
    }
    Some(format!("{:x}.jpg", Sha256::digest(uri.as_bytes())))
}
fn owned(name: &str) -> bool {
    if let Some(suffix) = name.strip_prefix(".tidal-art-") {
        return (6..=32).contains(&suffix.len())
            && suffix.bytes().all(|b| b.is_ascii_alphanumeric());
    }
    name.strip_suffix(".jpg")
        .is_some_and(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit()))
}
fn directory(root: &Path) -> Result<()> {
    if let Ok(meta) = fs::symlink_metadata(root) {
        ensure!(
            meta.is_dir() && !meta.file_type().is_symlink(),
            "Invalid artwork cache directory"
        );
    }
    crate::store::private_dir(root)
}
fn valid_image(bytes: &[u8]) -> Result<()> {
    ensure!(bytes.len() <= MAX_IMAGE, "Artwork size limit exceeded");
    let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format()?;
    ensure!(
        matches!(
            reader.format(),
            Some(image::ImageFormat::Jpeg | image::ImageFormat::Png)
        ),
        "Unexpected artwork format"
    );
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(2048);
    limits.max_image_height = Some(2048);
    limits.max_alloc = Some(16 * 1024 * 1024);
    reader.limits(limits);
    let (w, h) = reader.into_dimensions()?;
    ensure!(
        w > 0 && h > 0 && w <= 2048 && h <= 2048,
        "Artwork dimensions exceed limit"
    );
    Ok(())
}
fn read(root: &Path, key: &str) -> Result<Vec<u8>> {
    directory(root)?;
    let path = root.join(key);
    let meta = fs::symlink_metadata(&path)?;
    ensure!(
        meta.is_file() && !meta.file_type().is_symlink() && meta.len() <= MAX_IMAGE as u64,
        "Invalid cached artwork"
    );
    let mut bytes = Vec::new();
    fs::File::open(path)?
        .take(MAX_IMAGE as u64 + 1)
        .read_to_end(&mut bytes)?;
    valid_image(&bytes)?;
    Ok(bytes)
}
fn files(root: &Path) -> Result<Vec<(std::time::SystemTime, PathBuf, u64)>> {
    directory(root)?;
    let mut files = Vec::new();
    for (index, entry) in fs::read_dir(root)?.enumerate() {
        ensure!(index < 4096, "Artwork cache directory limit exceeded");
        let entry = entry?;
        if !entry.file_name().to_str().is_some_and(owned) {
            continue;
        }
        if entry.file_type()?.is_file() {
            let meta = entry.metadata()?;
            files.push((meta.modified()?, entry.path(), meta.len()));
        }
    }
    files.sort_by_key(|e| e.0);
    Ok(files)
}
fn save(root: &Path, key: &str, bytes: &[u8]) -> Result<()> {
    valid_image(bytes)?;
    let mut files = files(root)?;
    let mut total: u64 = files.iter().map(|f| f.2).sum();
    while !files.is_empty() && (files.len() >= MAX_FILES || total + bytes.len() as u64 > MAX_DISK) {
        let (_, path, size) = files.remove(0);
        fs::remove_file(path)?;
        total = total.saturating_sub(size);
    }
    use std::io::Write;
    // An identifiable temporary-file prefix lets quota/clear also cover files
    // left by a crash, without touching unrelated files in this directory.
    let mut file = tempfile::Builder::new()
        .prefix(".tidal-art-")
        .tempfile_in(root)?;
    file.write_all(bytes)?;
    file.as_file().sync_all()?;
    file.persist(root.join(key))?;
    #[cfg(unix)]
    fs::File::open(root)?.sync_all()?;
    Ok(())
}
fn clear_files(root: &Path) -> Result<()> {
    for (_, path, _) in files(root)? {
        fs::remove_file(path)?;
    }
    #[cfg(unix)]
    fs::File::open(root)?.sync_all()?;
    Ok(())
}
async fn fetch(client: &reqwest::Client, uri: &str) -> Result<Vec<u8>> {
    let mut response = client.get(uri).send().await?.error_for_status()?;
    ensure!(
        response
            .content_length()
            .is_none_or(|n| n <= MAX_IMAGE as u64),
        "Artwork size limit exceeded"
    );
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        ensure!(
            bytes.len() + chunk.len() <= MAX_IMAGE,
            "Artwork size limit exceeded"
        );
        bytes.extend_from_slice(&chunk);
    }
    valid_image(&bytes)?;
    Ok(bytes)
}
impl ArtworkCache {
    #[cfg(debug_assertions)]
    pub fn fixture() -> Arc<Self> {
        let (tx, _rx) = mpsc::sync_channel(1);
        Arc::new(Self {
            state: Arc::new(Mutex::new(State::default())),
            tx,
            root: None,
            disk: Arc::new(Mutex::new(())),
        })
    }
    pub fn new(root: PathBuf, enabled: bool) -> Result<Arc<Self>> {
        let state = Arc::new(Mutex::new(State {
            enabled,
            ..Default::default()
        }));
        let shared = state.clone();
        let disk = Arc::new(Mutex::new(()));
        let worker_disk = disk.clone();
        let saved_root = root.clone();
        let (tx, rx) = mpsc::sync_channel::<Job>(MAX_MEMORY);
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(8))
            .build()?;
        std::thread::Builder::new()
            .name("tidal-artwork".into())
            .spawn(move || {
                while let Ok(job) = rx.recv() {
                    match job {
                        Job::Clear(ctx) => {
                            let _disk = worker_disk.lock().unwrap();
                            let result = clear_files(&root);
                            let mut state = shared.lock().unwrap();
                            state.clear_pending = false;
                            state.status = if result.is_ok() {
                                "Artwork cache cleared."
                            } else {
                                "Could not fully clear artwork cache; cached files may remain."
                            }
                            .into();
                            drop(state);
                            ctx.request_repaint();
                        }
                        Job::Image {
                            uri,
                            key,
                            epoch,
                            ctx,
                        } => {
                            {
                                let state = shared.lock().unwrap();
                                if !state.enabled || state.epoch != epoch {
                                    continue;
                                }
                            }
                            let cached = {
                                let _disk = worker_disk.lock().unwrap();
                                read(&root, &key)
                            };
                            let (result, downloaded) = match cached {
                                Ok(bytes) => (Ok(bytes), false),
                                Err(_) => (runtime.block_on(fetch(&client, &uri)), true),
                            };
                            let current = {
                                let state = shared.lock().unwrap();
                                state.enabled && state.epoch == epoch
                            };
                            if !current {
                                continue;
                            }
                            if let Ok(bytes) = &result
                                && downloaded
                            {
                                let _disk = worker_disk.lock().unwrap();
                                let current = {
                                    let state = shared.lock().unwrap();
                                    state.enabled && state.epoch == epoch
                                };
                                if current && save(&root, &key, bytes).is_err() {
                                    shared.lock().unwrap().status =
                                        "Artwork is visible but could not be saved to disk.".into();
                                }
                            }
                            let mut state = shared.lock().unwrap();
                            if state.enabled
                                && state.epoch == epoch
                                && let Some(entry) = state.entries.get_mut(&uri)
                            {
                                entry.value = Some(
                                    result
                                        .map(Arc::from)
                                        .map_err(|_| "Artwork could not be loaded.".into()),
                                );
                            }
                            drop(state);
                            ctx.request_repaint();
                        }
                    }
                }
            })?;
        Ok(Arc::new(Self {
            state,
            tx,
            root: Some(saved_root),
            disk,
        }))
    }
    pub fn set_enabled(&self, enabled: bool) {
        let mut s = self.state.lock().unwrap();
        s.enabled = enabled;
        s.epoch = s.epoch.wrapping_add(1);
        s.entries.clear();
    }
    pub fn status(&self) -> String {
        self.state.lock().unwrap().status.clone()
    }
    pub fn clear(&self, ctx: &egui::Context) {
        let mut state = self.state.lock().unwrap();
        if state.clear_pending {
            return;
        }
        state.epoch = state.epoch.wrapping_add(1);
        state.entries.clear();
        state.clear_pending = true;
        state.status = "Clearing artwork cache…".into();
        if self.tx.try_send(Job::Clear(ctx.clone())).is_err() {
            state.clear_pending = false;
            state.status = "Artwork cache is busy; try Clear again.".into();
        }
    }
    pub fn finish_on_exit(&self) -> Result<()> {
        let pending = {
            let mut state = self.state.lock().unwrap();
            state.enabled = false;
            state.epoch = state.epoch.wrapping_add(1);
            state.entries.clear();
            state.clear_pending
        };
        if pending && let Some(root) = &self.root {
            // Complete a requested clear without waiting for unrelated network work.
            // The disk lock plus epoch check prevents late downloads undoing it.
            let _disk = self.disk.lock().unwrap();
            let result = clear_files(root);
            self.state.lock().unwrap().clear_pending = false;
            result?;
        }
        Ok(())
    }
}
impl BytesLoader for ArtworkCache {
    fn id(&self) -> &str {
        egui::generate_loader_id!(ArtworkCache)
    }
    fn load(&self, ctx: &egui::Context, uri: &str) -> BytesLoadResult {
        let Some(key) = key(uri) else {
            return Err(LoadError::NotSupported);
        };
        let mut state = self.state.lock().unwrap();
        if !state.enabled {
            return Err(LoadError::NotSupported);
        }
        state.clock = state.clock.wrapping_add(1);
        let clock = state.clock;
        if let Some(entry) = state.entries.get_mut(uri) {
            entry.used = clock;
            return match &entry.value {
                None => Ok(BytesPoll::Pending { size: None }),
                Some(Ok(bytes)) => Ok(BytesPoll::Ready {
                    size: None,
                    bytes: Bytes::Shared(bytes.clone()),
                    mime: None,
                }),
                Some(Err(error)) => Err(LoadError::Loading(error.clone())),
            };
        }
        if state.entries.len() >= MAX_MEMORY {
            if let Some(oldest) = state
                .entries
                .iter()
                .filter(|(_, e)| e.value.is_some())
                .min_by_key(|(_, e)| e.used)
                .map(|(k, _)| k.clone())
            {
                state.entries.remove(&oldest);
            } else {
                ctx.request_repaint_after(Duration::from_millis(100));
                return Ok(BytesPoll::Pending { size: None });
            }
        }
        if self
            .tx
            .try_send(Job::Image {
                uri: uri.into(),
                key,
                epoch: state.epoch,
                ctx: ctx.clone(),
            })
            .is_ok()
        {
            state.entries.insert(
                uri.into(),
                Entry {
                    value: None,
                    used: clock,
                },
            );
        } else {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
        Ok(BytesPoll::Pending { size: None })
    }
    fn forget(&self, uri: &str) {
        self.state.lock().unwrap().entries.remove(uri);
    }
    fn forget_all(&self) {
        let mut s = self.state.lock().unwrap();
        s.epoch = s.epoch.wrapping_add(1);
        s.entries.clear();
    }
    fn byte_size(&self) -> usize {
        self.state
            .lock()
            .unwrap()
            .entries
            .values()
            .filter_map(|e| e.value.as_ref().and_then(|r| r.as_ref().ok()))
            .map(|b| b.len())
            .sum()
    }
    fn has_pending(&self) -> bool {
        self.state
            .lock()
            .unwrap()
            .entries
            .values()
            .any(|e| e.value.is_none())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_unsigned_canonical_artwork_urls_can_enter_cache() {
        let uri =
            "https://resources.tidal.com/images/12345678/1234/1234/1234/123456789abc/80x80.jpg";
        assert!(key(uri).is_some());
        assert!(key(&uri.replace("80x80", "1080x720")).is_some());
        for bad in [
            format!("{uri}?token=secret"),
            uri.replace("resources.tidal.com", "example.invalid"),
            uri.replace("80x80", "9999x9999"),
            uri.replace("/1234/", "/../"),
        ] {
            assert!(key(&bad).is_none());
        }
        assert!(!owned("session.json"));
        assert!(!owned("../secret.jpg"));
    }
    #[test]
    fn loader_reads_disk_after_restart_and_disabled_loader_does_no_io() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("art");
        let uri =
            "https://resources.tidal.com/images/12345678/1234/1234/1234/123456789abc/80x80.jpg";
        let ctx = egui::Context::default();
        let disabled = ArtworkCache::new(path.clone(), false).unwrap();
        assert!(matches!(
            disabled.load(&ctx, uri),
            Err(LoadError::NotSupported)
        ));
        assert!(!path.exists());
        let mut bytes = Vec::new();
        image::codecs::jpeg::JpegEncoder::new(&mut bytes)
            .encode(&[1, 2, 3], 1, 1, image::ExtendedColorType::Rgb8)
            .unwrap();
        save(&path, &key(uri).unwrap(), &bytes).unwrap();
        let loader = ArtworkCache::new(path, true).unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        loop {
            match loader.load(&ctx, uri).unwrap() {
                BytesPoll::Ready { bytes: found, .. } => {
                    assert_eq!(found.as_ref(), bytes.as_slice());
                    break;
                }
                BytesPoll::Pending { .. } => {
                    assert!(std::time::Instant::now() < deadline);
                    std::thread::sleep(Duration::from_millis(5));
                }
            }
        }
        loader.set_enabled(false);
        assert_eq!(loader.byte_size(), 0);
        assert!(!loader.has_pending());
    }

    #[test]
    fn png_artwork_is_supported_without_assuming_the_url_extension_is_the_encoding() {
        use image::ImageEncoder;
        let mut bytes = Vec::new();
        image::codecs::png::PngEncoder::new(&mut bytes)
            .write_image(&[1, 2, 3], 1, 1, image::ExtendedColorType::Rgb8)
            .unwrap();
        assert!(valid_image(&bytes).is_ok());
    }

    #[test]
    fn exiting_finishes_pending_clear_and_disables_late_cache_writes() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join(format!("{:064x}.jpg", 1));
        fs::write(&path, b"old cached data").unwrap();
        let cache = ArtworkCache::new(root.path().into(), true).unwrap();
        cache.state.lock().unwrap().clear_pending = true;
        cache.finish_on_exit().unwrap();
        assert!(!path.exists());
        let state = cache.state.lock().unwrap();
        assert!(!state.enabled && !state.clear_pending);
    }

    #[test]
    fn artwork_byte_quota_counts_owned_crash_leftovers() {
        let root = tempfile::tempdir().unwrap();
        let mut bytes = Vec::new();
        image::codecs::jpeg::JpegEncoder::new(&mut bytes)
            .encode(&[0, 0, 0], 1, 1, image::ExtendedColorType::Rgb8)
            .unwrap();
        fs::File::create(root.path().join(".tidal-art-Abc123"))
            .unwrap()
            .set_len(MAX_DISK)
            .unwrap();
        save(root.path(), &format!("{:064x}.jpg", 1), &bytes).unwrap();
        assert!(!root.path().join(".tidal-art-Abc123").exists());
        assert!(files(root.path()).unwrap().iter().map(|f| f.2).sum::<u64>() <= MAX_DISK);
    }

    #[test]
    fn artwork_roundtrip_and_pruning_preserve_unrelated_files() {
        let root = tempfile::tempdir().unwrap();
        let mut bytes = Vec::new();
        image::codecs::jpeg::JpegEncoder::new(&mut bytes)
            .encode(&[0, 0, 0], 1, 1, image::ExtendedColorType::Rgb8)
            .unwrap();
        fs::write(root.path().join("unrelated"), b"keep").unwrap();
        for i in 0..MAX_FILES + 2 {
            save(root.path(), &format!("{i:064x}.jpg"), &bytes).unwrap();
        }
        assert_eq!(files(root.path()).unwrap().len(), MAX_FILES);
        assert_eq!(
            read(root.path(), &format!("{:064x}.jpg", MAX_FILES + 1)).unwrap(),
            bytes
        );
        assert_eq!(fs::read(root.path().join("unrelated")).unwrap(), b"keep");
        assert!(valid_image(&vec![0; MAX_IMAGE + 1]).is_err());
    }
}
