use super::*;
use crate::folders::{Change, Folder, Outcome, valid_id};
use std::{
    collections::HashSet,
    sync::atomic::{AtomicBool, Ordering},
};

impl Api {
    pub async fn folder_for(&mut self, user: u64, id: &str) -> Result<Vec<LibraryEntry>> {
        let cancelled = AtomicBool::new(false);
        self.folder_scope(user, &cancelled)?;
        let entries = self.folder(id).await?;
        self.folder_scope(user, &cancelled)?;
        Ok(entries)
    }
    fn folder_scope(&self, user: u64, cancelled: &AtomicBool) -> Result<()> {
        anyhow::ensure!(
            !cancelled.load(Ordering::Acquire),
            "Folder operation cancelled"
        );
        anyhow::ensure!(
            user != 0 && self.session.as_ref().is_some_and(|s| s.user_id == user),
            "The signed-in account changed"
        );
        Ok(())
    }
    async fn folder_snapshot(
        &mut self,
        user: u64,
        id: &str,
        cancelled: &AtomicBool,
    ) -> Result<Vec<LibraryEntry>> {
        anyhow::ensure!(valid_id(id), "Invalid folder ID");
        let mut all = Vec::new();
        let mut cursor = String::new();
        let mut cursors = HashSet::new();
        let mut seen = HashSet::new();
        let mut total = None;
        for _ in 0..20 {
            self.folder_scope(user, cancelled)?;
            let mut params = vec![
                ("folderId", id),
                ("limit", "50"),
                ("includeOnly", ""),
                ("order", "NAME"),
                ("orderDirection", "ASC"),
            ];
            if !cursor.is_empty() {
                params.push(("cursor", cursor.as_str()));
            }
            let value = self
                .get_version("v2", "my-collection/playlists/folders", &params)
                .await?;
            self.folder_scope(user, cancelled)?;
            let raw = value["items"]
                .as_array()
                .context("Invalid folder listing")?;
            anyhow::ensure!(raw.len() <= 50, "Folder page limit exceeded");
            let entries = parse_folder(&value)?;
            anyhow::ensure!(
                entries.len() == raw.len(),
                "Folder listing contains unsupported items"
            );
            let observed = value["totalNumberOfItems"].as_u64();
            if all.is_empty() {
                total = observed;
            } else {
                anyhow::ensure!(
                    observed == total,
                    "Folder listing changed during pagination"
                );
            }
            for entry in &entries {
                let (kind, id) = match entry {
                    LibraryEntry::Folder { id, .. } => ("folder", id),
                    LibraryEntry::Playlist(p) => ("playlist", &p.uuid),
                };
                anyhow::ensure!(
                    valid_id(id) && id != "root" && seen.insert((kind, id.clone())),
                    "Invalid or repeated folder entry"
                );
            }
            all.extend(entries);
            match value.get("cursor") {
                Some(Value::String(next)) if !next.is_empty() => {
                    anyhow::ensure!(
                        !raw.is_empty() && cursors.insert(next.clone()),
                        "Folder cursor did not advance"
                    );
                    cursor = next.clone();
                }
                Some(Value::Null) | Some(Value::String(_)) => {
                    anyhow::ensure!(
                        total.is_none_or(|n| n == all.len() as u64),
                        "Incomplete folder listing"
                    );
                    return Ok(all);
                }
                None if total == Some(all.len() as u64) => return Ok(all),
                _ => bail!("Folder completeness could not be established"),
            }
        }
        bail!("Folder verification is limited to 1,000 entries")
    }
    async fn check_folder(
        &mut self,
        user: u64,
        folder: &Folder,
        cancelled: &AtomicBool,
    ) -> Result<()> {
        folder.validate()?;
        if let Some(parent) = &folder.parent {
            let entries = self.folder_snapshot(user, parent, cancelled).await?;
            anyhow::ensure!(
                has_folder(&entries, &folder.id, &folder.name),
                "The folder moved or was renamed; Refresh first"
            );
        } else {
            self.folder_scope(user, cancelled)?;
        }
        Ok(())
    }
    pub async fn manage_folder(
        &mut self,
        user: u64,
        change: &Change,
        cancelled: &AtomicBool,
    ) -> Result<Outcome> {
        change.validate()?;
        self.folder_scope(user, cancelled)?;
        match change {
            Change::Create { parent, name } => {
                self.check_folder(user, parent, cancelled).await?;
                anyhow::ensure!(
                    self.folder_snapshot(user, &parent.id, cancelled)
                        .await?
                        .len()
                        < 1000,
                    "Folder size verification limit reached"
                );
                self.folder_scope(user, cancelled)?;
                let response = self
                    .request(
                        Method::PUT,
                        "v2",
                        "my-collection/playlists/folders/create-folder",
                        &[("folderId", &parent.id), ("name", name.trim())],
                        &[],
                        None,
                    )
                    .await?;
                let value: Value = response.json().await?;
                let id = value["data"]["id"]
                    .as_str()
                    .filter(|s| valid_id(s) && *s != "root")
                    .context("Created folder could not be identified; Refresh before retrying")?
                    .to_owned();
                let folder = Folder {
                    id,
                    parent: Some(parent.id.clone()),
                    name: name.trim().into(),
                };
                let verified = self
                    .folder_snapshot(user, &parent.id, cancelled)
                    .await
                    .is_ok_and(|entries| has_folder(&entries, &folder.id, &folder.name));
                Ok(Outcome {
                    created: Some(folder),
                    verified,
                })
            }
            Change::Rename { folder, name } => {
                self.check_folder(user, folder, cancelled).await?;
                self.folder_scope(user, cancelled)?;
                self.request(
                    Method::PUT,
                    "v2",
                    "my-collection/playlists/folders/rename",
                    &[
                        ("trn", &format!("trn:folder:{}", folder.id)),
                        ("name", name.trim()),
                    ],
                    &[],
                    None,
                )
                .await?;
                let verified = self
                    .folder_snapshot(user, folder.parent.as_deref().unwrap(), cancelled)
                    .await
                    .is_ok_and(|entries| has_folder(&entries, &folder.id, name.trim()));
                Ok(Outcome {
                    created: None,
                    verified,
                })
            }
            Change::MovePlaylist {
                id,
                title,
                source,
                destination,
            } => {
                let entries = self.folder_snapshot(user, source, cancelled).await?;
                anyhow::ensure!(
                    entries.iter().any(
                        |e| matches!(e,LibraryEntry::Playlist(p) if &p.uuid==id && &p.title==title)
                    ),
                    "The playlist changed folders; Refresh first"
                );
                self.check_folder(user, destination, cancelled).await?;
                let destination_items = self
                    .folder_snapshot(user, &destination.id, cancelled)
                    .await?;
                anyhow::ensure!(
                    destination_items.len() < 1000
                        && !destination_items
                            .iter()
                            .any(|e| matches!(e,LibraryEntry::Playlist(p) if &p.uuid==id)),
                    "Destination is full or already contains this playlist; Refresh first"
                );
                self.folder_scope(user, cancelled)?;
                self.request(
                    Method::PUT,
                    "v2",
                    "my-collection/playlists/folders/move",
                    &[
                        ("trns", &format!("trn:playlist:{id}")),
                        ("folderId", &destination.id),
                    ],
                    &[],
                    None,
                )
                .await?;
                let absent = self
                    .folder_snapshot(user, source, cancelled)
                    .await
                    .is_ok_and(|entries| {
                        !entries
                            .iter()
                            .any(|e| matches!(e,LibraryEntry::Playlist(p) if &p.uuid==id))
                    });
                let present = self
                    .folder_snapshot(user, &destination.id, cancelled)
                    .await
                    .is_ok_and(|entries| {
                        entries
                            .iter()
                            .any(|e| matches!(e,LibraryEntry::Playlist(p) if &p.uuid==id))
                    });
                Ok(Outcome {
                    created: None,
                    verified: absent && present,
                })
            }
        }
    }
}
fn has_folder(entries: &[LibraryEntry], id: &str, name: &str) -> bool {
    entries
        .iter()
        .any(|e| matches!(e,LibraryEntry::Folder{id:i,name:n,..} if i==id && n==name))
}

#[cfg(test)]
#[path = "folder_live_tests.rs"]
mod live_tests;

#[cfg(test)]
#[path = "folder_api_tests.rs"]
mod tests;
