//! Verified, copy-on-write Secret Service storage. Never fall back from keyring to plaintext.
use crate::store::{self, Session};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{fs, io::Read, path::Path};

mod secret_service;
use secret_service::NativeVault;
const LIMIT: u64 = 64 * 1024;
const MAX_PENDING: usize = 64;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Storage {
    #[default]
    Legacy,
    Keyring,
}
#[derive(Default, Serialize, Deserialize)]
struct Metadata {
    version: u32,
    storage: Storage,
    signed_out: bool,
    entry: Option<String>,
    secret_digest: Option<String>,
    pending: Vec<String>,
    legacy_digest: Option<String>,
}
impl Metadata {
    fn read(root: &Path) -> Result<Self> {
        let Some(bytes) = read_file(root, "credentials.json")? else {
            return Ok(Self {
                version: 1,
                ..Self::default()
            });
        };
        let value: Self = serde_json::from_slice(&bytes)
            .context("Credential settings are unreadable; no credentials were replaced")?;
        ensure!(
            value.version == 1 && value.pending.len() <= MAX_PENDING,
            "Unsupported credential settings; no credentials were replaced"
        );
        let valid = |key: &str| key.len() == 32 && key.bytes().all(|ch| ch.is_ascii_hexdigit());
        ensure!(
            value.entry.as_deref().is_none_or(valid) && value.pending.iter().all(|key| valid(key)),
            "Invalid keyring references"
        );
        ensure!(
            !value
                .entry
                .as_ref()
                .is_some_and(|key| value.pending.contains(key)),
            "Conflicting keyring references"
        );
        ensure!(
            value.storage == Storage::Keyring || value.entry.is_none(),
            "Invalid credential storage mode"
        );
        ensure!(
            value.entry.is_none()
                || value
                    .secret_digest
                    .as_ref()
                    .is_some_and(|digest| digest.len() == 64
                        && digest.bytes().all(|ch| ch.is_ascii_hexdigit())),
            "Invalid keyring verification metadata"
        );
        Ok(value)
    }
    fn save(&self, root: &Path) -> Result<()> {
        store::save_bytes(root, "credentials.json", &serde_json::to_vec(self)?)
    }
}
#[derive(Clone)]
pub struct Status {
    pub storage: Storage,
    pub signed_out: bool,
    pub cleanup_pending: bool,
}
fn status_at(root: &Path) -> Result<Status> {
    let meta = Metadata::read(root)?;
    Ok(Status {
        storage: meta.storage,
        signed_out: meta.signed_out,
        cleanup_pending: !meta.pending.is_empty()
            || meta.legacy_digest.is_some()
            || (meta.signed_out && root.join("session.json").exists()),
    })
}
fn read_file(root: &Path, name: &str) -> Result<Option<Vec<u8>>> {
    let file = match fs::File::open(root.join(name)) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let mut bytes = Vec::new();
    file.take(LIMIT + 1).read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() as u64 <= LIMIT,
        "Saved credentials exceed the storage limit"
    );
    Ok(Some(bytes))
}
fn decode(bytes: &[u8]) -> Result<Session> {
    ensure!(
        bytes.len() as u64 <= LIMIT,
        "Saved credentials exceed the storage limit"
    );
    let session: Session = serde_json::from_slice(bytes)
        .context("Saved credentials are invalid; the stored copy has not been replaced")?;
    ensure!(
        !session.access_token.is_empty(),
        "Saved credentials are empty; the stored copy has not been replaced"
    );
    Ok(session)
}
fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn remove_legacy(root: &Path) -> Result<()> {
    match fs::remove_file(root.join("session.json")) {
        Ok(()) => {
            #[cfg(unix)]
            fs::File::open(root)?.sync_all()?;
            Ok(())
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}
trait Vault {
    async fn read(&self, key: &str) -> Result<Option<Vec<u8>>>;
    async fn write(&self, key: &str, bytes: &[u8]) -> Result<()>;
    async fn delete(&self, key: &str) -> Result<()>;
}
async fn load_at(root: &Path, vault: &impl Vault) -> Result<Option<Session>> {
    let meta = Metadata::read(root)?;
    if meta.signed_out {
        return Ok(None);
    }
    match meta.storage {
        Storage::Legacy => read_file(root, "session.json")?
            .map(|bytes| decode(&bytes))
            .transpose(),
        Storage::Keyring => match meta.entry {
            None => Ok(None),
            Some(ref key) => {
                let bytes = vault.read(key).await?.context(
                    "Saved keyring credentials are missing; no plaintext fallback was used",
                )?;
                ensure!(
                    meta.secret_digest.as_ref() == Some(&digest(&bytes)),
                    "Keyring contents changed unexpectedly; no fallback or deletion was performed"
                );
                Ok(Some(decode(&bytes)?))
            }
        },
    }
}
async fn cleanup_at(root: &Path, meta: &mut Metadata, vault: &impl Vault) -> Result<()> {
    if meta.signed_out {
        remove_legacy(root)?;
        meta.legacy_digest = None;
    } else if let Some(expected) = &meta.legacy_digest {
        if let Some(bytes) = read_file(root, "session.json")? {
            ensure!(
                &digest(&bytes) == expected,
                "A different client changed the legacy credential file; it was left intact. Close older clients before removing saved credentials"
            );
            // Active keyring metadata is already committed and its secret was read-verified.
            remove_legacy(root)?;
        }
        meta.legacy_digest = None;
        meta.save(root)?;
    }
    while let Some(key) = meta.pending.last() {
        vault.delete(key).await?;
        meta.pending.pop();
        meta.save(root)?;
    }
    meta.save(root)
}
async fn save_at(root: &Path, session: &Session, vault: &impl Vault, migrate: bool) -> Result<()> {
    let bytes = serde_json::to_vec(session)?;
    ensure!(
        bytes.len() as u64 <= LIMIT,
        "Credentials exceed the storage limit"
    );
    let mut meta = Metadata::read(root)?;
    if !migrate && meta.storage == Storage::Legacy {
        store::save_bytes(root, "session.json", &bytes)?;
        meta.signed_out = false;
        return meta.save(root);
    }
    ensure!(
        !session.access_token.is_empty(),
        "No active sign-in to store in the keyring"
    );
    if meta.storage == Storage::Keyring
        && !meta.signed_out
        && meta.secret_digest.as_ref() == Some(&digest(&bytes))
        && let Some(key) = &meta.entry
        && vault.read(key).await?.as_deref() == Some(bytes.as_slice())
    {
        return cleanup_at(root, &mut meta, vault).await;
    }
    // Journal before contacting the vault: interrupted writes remain discoverable for cleanup.
    ensure!(
        meta.pending.len() + usize::from(meta.entry.is_some()) < MAX_PENDING,
        "Remove pending credential copies before storing another sign-in"
    );
    let key = format!("{:032x}", rand::random::<u128>());
    meta.pending.push(key.clone());
    if meta.storage == Storage::Legacy {
        meta.legacy_digest = read_file(root, "session.json")?.map(|bytes| digest(&bytes));
    }
    meta.save(root)?;
    vault.write(&key, &bytes).await?;
    ensure!(
        vault.read(&key).await?.as_deref() == Some(bytes.as_slice()),
        "Keyring read-back verification failed; previous credentials were kept"
    );
    if let Some(previous) = meta.entry.replace(key.clone()) {
        meta.pending.push(previous);
    }
    meta.pending.retain(|pending| pending != &key);
    meta.storage = Storage::Keyring;
    meta.secret_digest = Some(digest(&bytes));
    meta.signed_out = false;
    meta.save(root)?;
    cleanup_at(root, &mut meta, vault).await.context("New credentials are verified, but old-copy cleanup is incomplete; retry credential cleanup")
}
async fn clear_at(root: &Path, vault: &impl Vault) -> Result<()> {
    let mut meta = Metadata::read(root)?;
    if let Some(key) = meta.entry.take() {
        meta.pending.push(key);
    }
    meta.signed_out = true;
    // Persist the signed-out intent first, even if a locked vault prevents deletion.
    meta.save(root)?;
    cleanup_at(root, &mut meta, vault).await.context("Signed out, but saved-credential removal is incomplete; unlock the keyring and retry removal")
}

async fn cleanup(root: &Path, vault: &impl Vault) -> Result<()> {
    let mut meta = Metadata::read(root)?;
    if meta.storage == Storage::Keyring && !meta.signed_out {
        load_at(root, vault)
            .await?
            .context("No verified keyring copy available for cleanup")?;
    }
    // An uncommitted migration must never delete its only legacy copy.
    if meta.storage == Storage::Legacy && !meta.signed_out {
        meta.legacy_digest = None;
    }
    cleanup_at(root, &mut meta, vault).await
}
#[derive(Clone)]
pub struct Store {
    root: std::path::PathBuf,
}
impl Store {
    pub fn new() -> Result<Self> {
        Ok(Self {
            root: store::config_dir()?,
        })
    }
    #[cfg(test)]
    pub fn at(root: &Path) -> Self {
        Self { root: root.into() }
    }
    pub fn status(&self) -> Result<Status> {
        status_at(&self.root)
    }
    pub async fn load(&self) -> Result<Option<Session>> {
        deadline(load_at(&self.root, &NativeVault)).await
    }
    pub async fn save(&self, session: &Session) -> Result<()> {
        deadline(save_at(&self.root, session, &NativeVault, false)).await
    }
    pub async fn migrate(&self, session: &Session) -> Result<()> {
        deadline(save_at(&self.root, session, &NativeVault, true)).await
    }
    pub async fn clear(&self) -> Result<()> {
        deadline(clear_at(&self.root, &NativeVault)).await
    }
    pub async fn cleanup(&self) -> Result<()> {
        deadline(cleanup(&self.root, &NativeVault)).await
    }
}
async fn deadline<T>(operation: impl std::future::Future<Output = Result<T>>) -> Result<T> {
    tokio::time::timeout(std::time::Duration::from_secs(30), operation).await.context("Credential storage timed out; keep the app open and retry. No plaintext fallback was used")?
}

#[cfg(test)]
#[path = "credentials_tests.rs"]
mod tests;
