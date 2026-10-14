use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{collections::HashSet, fs::File, io::Read, path::Path};

pub const MAX_PINS: usize = 64;
const MAX_BYTES: u64 = 128 * 1024;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Pin {
    pub id: String,
    pub title: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Pins {
    version: u32,
    pub user: u64,
    pub entries: Vec<Pin>,
}
impl Pins {
    pub fn new(user: u64) -> Self {
        Self {
            version: 1,
            user,
            entries: vec![],
        }
    }
    fn validate(&self, user: u64) -> Result<()> {
        ensure!(
            user != 0 && self.user == user && self.version == 1,
            "Invalid local pins account or version"
        );
        ensure!(self.entries.len() <= MAX_PINS, "Too many local pins");
        let mut ids = HashSet::new();
        for pin in &self.entries {
            ensure!(
                !pin.id.is_empty()
                    && pin.id.len() <= 128
                    && pin
                        .id
                        .bytes()
                        .all(|c| c.is_ascii_alphanumeric() || c == b'-'),
                "Invalid pinned playlist identifier"
            );
            ensure!(
                !pin.title.trim().is_empty() && pin.title.chars().count() <= 100,
                "Invalid pinned playlist title"
            );
            ensure!(ids.insert(&pin.id), "Duplicate local pin");
        }
        Ok(())
    }
    pub fn load(root: &Path, user: u64) -> Result<Self> {
        ensure!(user != 0, "No signed-in account");
        let file = match File::open(root.join(format!("pins-{user}.json"))) {
            Ok(file) => file,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::new(user)),
            Err(e) => return Err(e.into()),
        };
        let mut bytes = Vec::new();
        file.take(MAX_BYTES + 1).read_to_end(&mut bytes)?;
        ensure!(
            bytes.len() as u64 <= MAX_BYTES,
            "Local pins file exceeds its size limit"
        );
        let pins: Self = serde_json::from_slice(&bytes).context("Invalid local pins file")?;
        pins.validate(user)?;
        Ok(pins)
    }
    pub fn save(&self, root: &Path, user: u64) -> Result<()> {
        self.validate(user)?;
        crate::store::save_bytes(
            root,
            &format!("pins-{user}.json"),
            &serde_json::to_vec(self)?,
        )
    }
    pub fn contains(&self, id: &str) -> bool {
        self.entries.iter().any(|pin| pin.id == id)
    }
    pub fn toggle(&mut self, id: &str, title: &str) -> Result<()> {
        if self.contains(id) {
            self.entries.retain(|pin| pin.id != id);
            return Ok(());
        }
        ensure!(
            self.entries.len() < MAX_PINS,
            "Local pins are limited to 64 playlists"
        );
        let pin = Pin {
            id: id.into(),
            title: title.chars().take(100).collect(),
        };
        let mut next = self.clone();
        next.entries.push(pin);
        next.validate(self.user)?;
        *self = next;
        Ok(())
    }
    pub fn move_pin(&mut self, id: &str, up: bool) {
        if let Some(from) = self.entries.iter().position(|pin| pin.id == id) {
            let to = if up {
                from.saturating_sub(1)
            } else {
                (from + 1).min(self.entries.len() - 1)
            };
            self.entries.swap(from, to);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pins_round_trip_order_and_account_isolation_privately() {
        let root = tempfile::tempdir().unwrap();
        let mut pins = Pins::new(7);
        pins.toggle("one", "One").unwrap();
        pins.toggle("two", "Two").unwrap();
        pins.move_pin("two", true);
        pins.save(root.path(), 7).unwrap();
        let loaded = Pins::load(root.path(), 7).unwrap();
        assert_eq!(loaded.entries[0].id, "two");
        assert!(Pins::load(root.path(), 8).unwrap().entries.is_empty());
        assert!(pins.save(root.path(), 8).is_err());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(root.path().join("pins-7.json"))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
        pins.toggle("two", "Two").unwrap();
        assert_eq!(pins.entries.len(), 1);
    }
    #[test]
    fn malformed_oversized_and_wrong_account_files_are_not_replaced() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("pins-7.json");
        for bytes in [
            b"broken".to_vec(),
            vec![b' '; MAX_BYTES as usize + 1],
            serde_json::to_vec(&Pins::new(8)).unwrap(),
        ] {
            std::fs::write(&path, &bytes).unwrap();
            assert!(Pins::load(root.path(), 7).is_err());
            assert_eq!(std::fs::read(&path).unwrap(), bytes);
        }
    }
    #[test]
    fn pins_are_bounded_and_reject_unsafe_identifiers() {
        let mut pins = Pins::new(7);
        assert!(pins.toggle("../other", "Other").is_err());
        assert!(pins.entries.is_empty());
        for i in 0..MAX_PINS {
            pins.toggle(&format!("list-{i}"), "Playlist").unwrap();
        }
        assert!(pins.toggle("overflow", "Playlist").is_err());
        pins.move_pin("list-0", true);
        assert_eq!(pins.entries[0].id, "list-0");
        pins.toggle("list-0", "Playlist").unwrap();
        assert_eq!(pins.entries.len(), 63);
    }
}
