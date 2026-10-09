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
    #[serde(default)]
    pub pkce: bool,
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

#[cfg(test)]
fn save_at(dir: &std::path::Path, session: &Session) -> Result<()> {
    save_json(dir, "session.json", session)
}

fn save_json(dir: &std::path::Path, name: &str, value: &impl Serialize) -> Result<()> {
    save_bytes(dir, name, &serde_json::to_vec(value)?)
}

pub(crate) fn private_dir(dir: &std::path::Path) -> Result<()> {
    fs::create_dir_all(dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(dir, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

pub(crate) fn save_bytes(dir: &std::path::Path, name: &str, bytes: &[u8]) -> Result<()> {
    private_dir(dir)?;
    // NamedTempFile is mode 0600 on Unix. Atomic replacement prevents partial tokens.
    let mut file = tempfile::NamedTempFile::new_in(dir)?;
    file.write_all(bytes)?;
    file.as_file().sync_all()?;
    file.persist(dir.join(name))?;
    #[cfg(unix)]
    fs::File::open(dir)?.sync_all()?;
    Ok(())
}

#[derive(Deserialize, Serialize)]
#[serde(default)]
pub struct Appearance {
    pub player_bar_visualizer: crate::visualizer::Mode,
    pub compact_rows: bool,
    pub zoom: f32,
}
impl Default for Appearance {
    fn default() -> Self {
        Self {
            player_bar_visualizer: crate::visualizer::Mode::Off,
            compact_rows: false,
            zoom: 1.,
        }
    }
}
impl Appearance {
    pub fn load() -> Result<Self> {
        match fs::read(config_dir()?.join("appearance.json")) {
            Ok(bytes) => {
                let mut appearance: Self = serde_json::from_slice(&bytes)
                    .context("Saved appearance settings are invalid")?;
                if !appearance.zoom.is_finite() || !(0.75..=1.5).contains(&appearance.zoom) {
                    appearance.zoom = 1.;
                }
                Ok(appearance)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e.into()),
        }
    }
    pub fn save(&self) -> Result<()> {
        save_json(&config_dir()?, "appearance.json", self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn appearance_round_trips_without_touching_credentials() {
        use crate::visualizer::Mode;
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("session.json"), b"do not modify").unwrap();
        assert_eq!(Appearance::default().player_bar_visualizer, Mode::Off);
        for mode in [Mode::Off, Mode::Spectrum, Mode::Waveform] {
            save_json(
                dir.path(),
                "appearance.json",
                &Appearance {
                    player_bar_visualizer: mode,
                    ..Default::default()
                },
            )
            .unwrap();
            let value: Appearance =
                serde_json::from_slice(&fs::read(dir.path().join("appearance.json")).unwrap())
                    .unwrap();
            assert_eq!(value.player_bar_visualizer, mode);
        }
        assert_eq!(
            fs::read(dir.path().join("session.json")).unwrap(),
            b"do not modify"
        );
    }

    #[test]
    fn old_appearance_files_keep_readable_defaults() {
        let value: Appearance =
            serde_json::from_str(r#"{"player_bar_visualizer":"spectrum"}"#).unwrap();
        assert_eq!(value.zoom, 1.);
        assert!(!value.compact_rows);
    }

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
