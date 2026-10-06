use anyhow::{Context, Result};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::{fs, io::Write, path::PathBuf};

#[derive(Clone, Default, Deserialize, Serialize)]
pub struct Session {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_at: u64,
    pub user_id: u64,
    pub country: String,
}

pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

pub fn config_dir() -> Result<PathBuf> {
    Ok(ProjectDirs::from("rocks", "tidal-forces", "tidal-forces")
        .context("No home directory available")?
        .config_dir()
        .to_owned())
}

pub fn save(session: &Session) -> Result<()> {
    save_at(&config_dir()?, session)
}

fn save_at(dir: &std::path::Path, session: &Session) -> Result<()> {
    fs::create_dir_all(dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(dir, fs::Permissions::from_mode(0o700))?;
    }
    // NamedTempFile is mode 0600 on Unix. Atomic replacement prevents partial tokens.
    let mut file = tempfile::NamedTempFile::new_in(dir)?;
    file.write_all(&serde_json::to_vec(session)?)?;
    file.as_file().sync_all()?;
    file.persist(dir.join("session.json"))?;
    Ok(())
}

pub fn load() -> Result<Option<Session>> {
    let path = config_dir()?.join("session.json");
    match fs::read(path) {
        Ok(bytes) => Ok(Some(
            serde_json::from_slice(&bytes).context("Saved session is invalid; sign in again")?,
        )),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}

pub fn clear() -> Result<()> {
    match fs::remove_file(config_dir()?.join("session.json")) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn credentials_are_atomic_and_private() {
        let dir = tempfile::tempdir().unwrap();
        let session = Session {
            access_token: "test".into(),
            ..Default::default()
        };
        save_at(dir.path(), &session).unwrap();
        save_at(dir.path(), &session).unwrap();
        let path = dir.path().join("session.json");
        let stored: Session = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(stored.access_token, "test");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
    }
}
