use crate::{
    model::{Album, Artist, Track},
    store,
    ui::navigation::{Location, Page},
};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::{fs, io::Read, path::Path};

pub const MAX_VIEWS: usize = 8;
pub const MAX_ROWS: usize = 10_000;
const MAX_BYTES: u64 = 8 * 1024 * 1024;

#[derive(Clone, Serialize, Deserialize)]
pub struct Snapshot {
    pub location: Location,
    pub tracks: Vec<Track>,
    pub albums: Vec<Album>,
    pub artists: Vec<Artist>,
}
impl Snapshot {
    pub fn rows(&self) -> usize {
        self.tracks.len() + self.albums.len() + self.artists.len()
    }
    pub fn valid(&self) -> bool {
        location_valid(&self.location)
            && self.rows() <= MAX_ROWS
            && self.tracks.iter().all(|t| {
                t.id != 0
                    && t.title.len() <= 512
                    && artist_valid(&t.artist)
                    && album_valid(&t.album)
            })
            && self.albums.iter().all(|a| a.id != 0 && album_valid(a))
            && self.artists.iter().all(|a| a.id != 0 && artist_valid(a))
    }
}
fn location_valid(location: &Location) -> bool {
    if location.query.len() > 512 {
        return false;
    }
    match &location.page {
        Page::Home | Page::Settings | Page::Playlists => false,
        Page::Collection { kind, id, title } => {
            matches!(kind.as_str(), "albums" | "playlists")
                && id.len() <= 128
                && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
                && !id.is_empty()
                && title.len() <= 512
        }
        Page::Mix { id, title } => {
            id.len() <= 128
                && id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
                && !id.is_empty()
                && title.len() <= 512
        }
        Page::Radio(crate::model::RadioSeed::Track { id, title }) => *id != 0 && title.len() <= 512,
        Page::Radio(crate::model::RadioSeed::Artist { id, name }) => *id != 0 && name.len() <= 512,
        Page::Artist(id) | Page::Track(id) => *id != 0,
        _ => true,
    }
}
fn asset_valid(asset: &Option<String>) -> bool {
    asset.as_ref().is_none_or(|s| {
        s.len() == 36
            && s.bytes().enumerate().all(|(i, b)| {
                if [8, 13, 18, 23].contains(&i) {
                    b == b'-'
                } else {
                    b.is_ascii_hexdigit()
                }
            })
    })
}
fn artist_valid(a: &Artist) -> bool {
    a.name.len() <= 512 && asset_valid(&a.picture)
}
fn album_valid(a: &Album) -> bool {
    a.title.len() <= 512 && asset_valid(&a.cover) && artist_valid(&a.artist)
}

#[derive(Serialize, Deserialize)]
pub struct Record {
    version: u32,
    pub user: u64,
    pub entries: Vec<Snapshot>,
}
impl Record {
    pub fn new(user: u64, entries: Vec<Snapshot>) -> Self {
        Self {
            version: 1,
            user,
            entries,
        }
    }
    fn validate(&self, user: u64) -> Result<()> {
        ensure!(
            user != 0 && self.user == user && self.version == 1,
            "Invalid cache scope or version"
        );
        ensure!(
            self.entries.len() <= MAX_VIEWS
                && self.entries.iter().map(Snapshot::rows).sum::<usize>() <= MAX_ROWS
                && self.entries.iter().all(Snapshot::valid),
            "Invalid cached metadata"
        );
        Ok(())
    }
}
pub fn load(root: &Path, user: u64) -> Result<Record> {
    ensure!(user != 0, "Missing cache account");
    let path = root.join(format!("browse-cache-{user}.json"));
    match fs::symlink_metadata(&path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Record::new(user, vec![])),
        Err(e) => return Err(e.into()),
        Ok(meta) => ensure!(
            meta.is_file() && !meta.file_type().is_symlink() && meta.len() <= MAX_BYTES,
            "Invalid cache file"
        ),
    }
    let mut bytes = Vec::new();
    fs::File::open(path)?
        .take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)?;
    ensure!(bytes.len() as u64 <= MAX_BYTES, "Cache size limit exceeded");
    let record: Record = serde_json::from_slice(&bytes)?;
    record.validate(user)?;
    Ok(record)
}
pub fn save(root: &Path, mut record: Record) -> Result<()> {
    record.validate(record.user)?;
    loop {
        let bytes = serde_json::to_vec(&record)?;
        if bytes.len() as u64 <= MAX_BYTES {
            return store::save_bytes(root, &format!("browse-cache-{}.json", record.user), &bytes);
        }
        record.entries.remove(0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn snapshot() -> Snapshot {
        Snapshot {
            location: Location {
                page: Page::Track(1),
                query: String::new(),
            },
            tracks: vec![Track {
                id: 1,
                title: "Fixture".into(),
                ..Default::default()
            }],
            albums: vec![],
            artists: vec![],
        }
    }
    #[test]
    fn cache_is_private_account_scoped_and_excludes_authority_and_urls() {
        let root = tempfile::tempdir().unwrap();
        save(root.path(), Record::new(7, vec![snapshot()])).unwrap();
        assert_eq!(load(root.path(), 7).unwrap().entries[0].tracks[0].id, 1);
        assert!(load(root.path(), 8).unwrap().entries.is_empty());
        let path = root.path().join("browse-cache-7.json");
        let text = fs::read_to_string(&path).unwrap();
        for forbidden in [
            "etag",
            "editable",
            "access_token",
            "stream_url",
            "next_offset",
        ] {
            assert!(!text.contains(forbidden));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        let mut bad = snapshot();
        bad.tracks[0].album.cover = Some("https://example.invalid/private?token=fixture".into());
        assert!(!bad.valid());
        fs::copy(&path, root.path().join("browse-cache-8.json")).unwrap();
        assert!(load(root.path(), 8).is_err());
        save(root.path(), Record::new(7, vec![])).unwrap();
        assert!(load(root.path(), 7).unwrap().entries.is_empty());
    }
    #[test]
    fn malformed_oversized_and_excessive_caches_are_rejected() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("browse-cache-7.json");
        fs::write(&path, b"broken").unwrap();
        assert!(load(root.path(), 7).is_err());
        fs::File::create(&path)
            .unwrap()
            .set_len(MAX_BYTES + 1)
            .unwrap();
        assert!(load(root.path(), 7).is_err());
        assert!(save(root.path(), Record::new(7, vec![snapshot(); MAX_VIEWS + 1])).is_err());
        let mut oversized = snapshot();
        oversized.tracks = vec![oversized.tracks[0].clone(); MAX_ROWS + 1];
        assert!(!oversized.valid());
    }
}
