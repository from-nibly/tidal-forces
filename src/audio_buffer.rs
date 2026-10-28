//! Current-song resources only: anonymous compressed storage and an exclusive decoder lease.
use anyhow::{Result, ensure};
use rodio::{Source, source::SeekError};
use std::{
    collections::BTreeMap,
    fs::File,
    io::{Read, Seek, SeekFrom, Write},
    sync::{Arc, Condvar, Mutex},
    time::{Duration, Instant},
};

pub const MAX_BYTES: usize = 256 * 1024 * 1024;

#[derive(Clone, Debug)]
pub struct AnonymousStorage;
impl stream_download::storage::StorageProvider for AnonymousStorage {
    type Reader = File;
    type Writer = File;
    fn into_reader_writer(self, _: Option<u64>) -> std::io::Result<(File, File)> {
        let temporary = tempfile::NamedTempFile::new()?;
        let writer = temporary.reopen()?;
        // Independent file cursors, no pathname remaining while audio is stored.
        Ok((temporary.into_file(), writer))
    }
}

#[derive(Clone, Debug)]
pub struct CappedStorage;
impl stream_download::storage::StorageProvider for CappedStorage {
    type Reader = stream_download::storage::bounded::BoundedStorageReader<File>;
    type Writer = stream_download::storage::bounded::BoundedStorageWriter<File>;
    fn max_capacity(&self) -> Option<usize> {
        Some(MAX_BYTES)
    }
    fn into_reader_writer(self, _: Option<u64>) -> std::io::Result<(Self::Reader, Self::Writer)> {
        // Do not shrink the ring to Content-Length: decoders may legitimately
        // request a read buffer larger than a very short file. No space is
        // preallocated; the physical file grows only as compressed bytes arrive.
        stream_download::storage::bounded::BoundedStorageProvider::new(
            AnonymousStorage,
            std::num::NonZeroUsize::new(MAX_BYTES).unwrap(),
        )
        .into_reader_writer(None)
    }
}

type Decoder = Box<dyn Source<Item = f32> + Send>;

pub struct Segments {
    file: File,
    entries: BTreeMap<usize, (u64, usize)>,
    bytes: usize,
    limit: usize,
    full: bool,
}
impl Segments {
    pub fn new(limit: usize) -> Result<Self> {
        Ok(Self {
            file: tempfile::NamedTempFile::new()?.into_file(),
            entries: BTreeMap::new(),
            bytes: 0,
            limit,
            full: false,
        })
    }
    pub fn contains(&self, index: usize) -> bool {
        self.entries.contains_key(&index)
    }
    pub fn can_prefetch(&self) -> bool {
        !self.full && self.bytes < self.limit
    }
    pub fn insert(&mut self, index: usize, bytes: &[u8]) -> Result<()> {
        if self.contains(index) {
            return Ok(());
        }
        if bytes.len() > self.limit.saturating_sub(self.bytes) {
            self.full = true;
            return Ok(());
        }
        self.file.seek(SeekFrom::Start(self.bytes as u64))?;
        self.file.write_all(bytes)?;
        self.entries.insert(index, (self.bytes as u64, bytes.len()));
        self.bytes += bytes.len();
        Ok(())
    }
    pub fn get(&mut self, index: usize) -> Result<Option<Vec<u8>>> {
        let Some(&(offset, len)) = self.entries.get(&index) else {
            return Ok(None);
        };
        self.file.seek(SeekFrom::Start(offset))?;
        let mut bytes = vec![0; len];
        self.file.read_exact(&mut bytes)?;
        Ok(Some(bytes))
    }
}

pub struct Reusable {
    decoder: Mutex<Option<Decoder>>,
    ready: Condvar,
}
pub struct Lease {
    decoder: Option<Decoder>,
    owner: Arc<Reusable>,
}
impl Reusable {
    pub fn new(decoder: Decoder) -> (Arc<Self>, Lease) {
        let owner = Arc::new(Self {
            decoder: Mutex::new(None),
            ready: Condvar::new(),
        });
        (
            owner.clone(),
            Lease {
                decoder: Some(decoder),
                owner,
            },
        )
    }
    // Called on a blocking worker. Cancellation returns the old decoder here
    // only after its producer has stopped, so old/new seeks cannot share a cursor.
    pub fn acquire(self: &Arc<Self>, current: impl Fn() -> bool) -> Result<Lease> {
        let deadline = Instant::now() + Duration::from_secs(30);
        let mut slot = self.decoder.lock().unwrap();
        loop {
            ensure!(current(), "Playback changed while preparing the seek");
            if let Some(decoder) = slot.take() {
                return Ok(Lease {
                    decoder: Some(decoder),
                    owner: self.clone(),
                });
            }
            ensure!(
                Instant::now() < deadline,
                "The previous stream read is still stopping. Press play to retry."
            );
            slot = self
                .ready
                .wait_timeout(slot, Duration::from_millis(50))
                .unwrap()
                .0;
        }
    }
}
impl Drop for Lease {
    fn drop(&mut self) {
        *self.owner.decoder.lock().unwrap() = self.decoder.take();
        self.owner.ready.notify_all();
    }
}
impl Iterator for Lease {
    type Item = f32;
    fn next(&mut self) -> Option<f32> {
        self.decoder.as_mut().unwrap().next()
    }
}
impl Source for Lease {
    fn current_span_len(&self) -> Option<usize> {
        self.decoder.as_ref().unwrap().current_span_len()
    }
    fn channels(&self) -> u16 {
        self.decoder.as_ref().unwrap().channels()
    }
    fn sample_rate(&self) -> u32 {
        self.decoder.as_ref().unwrap().sample_rate()
    }
    fn total_duration(&self) -> Option<Duration> {
        self.decoder.as_ref().unwrap().total_duration()
    }
    fn try_seek(&mut self, position: Duration) -> std::result::Result<(), SeekError> {
        self.decoder.as_mut().unwrap().try_seek(position)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn anonymous_segments_are_capped_and_duplicates_do_not_grow_storage() {
        let mut store = Segments::new(6).unwrap();
        store.insert(0, &[1, 2, 3]).unwrap();
        store.insert(0, &[9, 9, 9]).unwrap();
        store.insert(1, &[4, 5, 6, 7]).unwrap();
        assert!(!store.can_prefetch());
        assert_eq!(store.get(0).unwrap(), Some(vec![1, 2, 3]));
        assert_eq!(store.get(1).unwrap(), None);
        assert_eq!(store.file.metadata().unwrap().len(), 3);
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            store.file.metadata().unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    #[test]
    fn direct_storage_has_private_unlinked_files_and_independent_cursors() {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        use stream_download::storage::StorageProvider;
        let (mut reader, mut writer) = AnonymousStorage.into_reader_writer(None).unwrap();
        assert_eq!(
            reader.metadata().unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(reader.metadata().unwrap().nlink(), 0);
        writer.write_all(&[1, 2, 3, 4]).unwrap();
        let mut bytes = [0; 4];
        reader.read_exact(&mut bytes).unwrap();
        assert_eq!(bytes, [1, 2, 3, 4]);
        reader.seek(SeekFrom::Start(0)).unwrap();
        assert_eq!(writer.stream_position().unwrap(), 4);
    }

    #[test]
    fn a_short_direct_stream_allows_normal_decoder_read_sizes() {
        use stream_download::storage::StorageProvider;
        let (mut reader, mut writer) = CappedStorage.into_reader_writer(Some(4)).unwrap();
        writer.write_all(&[1, 2, 3, 4]).unwrap();
        let mut bytes = [0; 8192];
        assert_eq!(reader.read(&mut bytes).unwrap(), 4);
        assert_eq!(&bytes[..4], &[1, 2, 3, 4]);
    }

    #[test]
    fn direct_window_stays_capped_and_rejects_evicted_bytes() {
        use stream_download::storage::{StorageProvider, bounded::BoundedStorageProvider};
        struct Observed(Arc<Mutex<Option<File>>>);
        impl StorageProvider for Observed {
            type Reader = File;
            type Writer = File;
            fn into_reader_writer(self, length: Option<u64>) -> std::io::Result<(File, File)> {
                let (reader, writer) = AnonymousStorage.into_reader_writer(length)?;
                *self.0.lock().unwrap() = Some(reader.try_clone()?);
                Ok((reader, writer))
            }
        }
        let observed = Arc::new(Mutex::new(None));
        let storage = BoundedStorageProvider::new(
            Observed(observed.clone()),
            std::num::NonZeroUsize::new(16).unwrap(),
        );
        let (mut reader, mut writer) = storage.into_reader_writer(Some(100)).unwrap();
        writer.write_all(&[1; 16]).unwrap();
        let mut bytes = [0; 16];
        reader.read_exact(&mut bytes).unwrap();
        writer.write_all(&[2; 16]).unwrap();
        assert_eq!(
            observed
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .metadata()
                .unwrap()
                .len(),
            16
        );
        reader.seek(SeekFrom::Start(0)).unwrap();
        assert!(
            reader.read(&mut bytes).is_err(),
            "Evicted bytes must not be mistaken for the requested audio"
        );
    }

    #[test]
    fn exclusive_decoder_is_returned_and_rewinds_without_reopening() {
        let (session, mut lease) = Reusable::new(Box::new(rodio::buffer::SamplesBuffer::new(
            1,
            1000,
            vec![0.25; 100],
        )));
        assert_eq!(lease.next(), Some(0.25));
        assert!(session.acquire(|| false).is_err());
        drop(lease);
        let mut lease = session.acquire(|| true).unwrap();
        lease.try_seek(Duration::ZERO).unwrap();
        assert_eq!(lease.count(), 100);
    }
}
