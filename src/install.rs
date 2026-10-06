use anyhow::{Context, Result};
use directories::BaseDirs;
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

pub fn install() -> Result<PathBuf> {
    let dirs = BaseDirs::new().context("No home directory")?;
    let bin = dirs.home_dir().join(".local/bin/tidal-forces");
    let source = std::env::current_exe()?;
    fs::create_dir_all(bin.parent().unwrap())?;
    if source.canonicalize()? != bin.canonicalize().unwrap_or_default() {
        let mut temp = tempfile::NamedTempFile::new_in(bin.parent().unwrap())?;
        std::io::copy(&mut fs::File::open(source)?, &mut temp)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            temp.as_file()
                .set_permissions(fs::Permissions::from_mode(0o755))?;
        }
        temp.as_file().sync_all()?;
        temp.persist(&bin)?;
    }
    let apps = dirs.data_dir().join("applications");
    let icons = dirs.data_dir().join("icons/hicolor/scalable/apps");
    fs::create_dir_all(&apps)?;
    fs::create_dir_all(&icons)?;
    fs::write(
        icons.join("rocks.tidalforces.Player.svg"),
        include_str!("../assets/icon.svg"),
    )?;
    let desktop = format!(
        "[Desktop Entry]\nType=Application\nVersion=1.0\nName=Tidal Forces\nGenericName=Music Player\nComment=A fast, native TIDAL player\nExec={} %u\nMimeType=x-scheme-handler/tidal;\nIcon=rocks.tidalforces.Player\nTerminal=false\nCategories=AudioVideo;Audio;Player;\nKeywords=tidal;music;lossless;flac;\nStartupWMClass=rocks.tidalforces.Player\nStartupNotify=true\n",
        desktop_exec(&bin)?
    );
    let mut file = fs::File::create(apps.join("rocks.tidalforces.Player.desktop"))?;
    file.write_all(desktop.as_bytes())?;
    let _ = std::process::Command::new("update-desktop-database")
        .arg(&apps)
        .status();
    let status = std::process::Command::new("xdg-mime")
        .args([
            "default",
            "rocks.tidalforces.Player.desktop",
            "x-scheme-handler/tidal",
        ])
        .status();
    if !status.is_ok_and(|s| s.success()) {
        eprintln!(
            "Could not set the default tidal:// handler; select Tidal Forces in your desktop settings."
        );
    }
    println!(
        "Installed {}\nDesktop launcher: {}",
        bin.display(),
        apps.join("rocks.tidalforces.Player.desktop").display()
    );
    Ok(bin)
}

fn desktop_exec(path: &Path) -> Result<String> {
    let path = path.to_str().context("Installation path must be UTF-8")?;
    anyhow::ensure!(
        !path.contains(['\n', '\r']),
        "Unsupported newline in installation path"
    );
    let escaped = path
        .replace('\\', "\\\\\\\\")
        .replace('"', "\\\\\"")
        .replace('`', "\\\\`")
        .replace('$', "\\\\$")
        .replace('%', "%%");
    Ok(format!("\"{escaped}\""))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn safely_quotes_desktop_paths() {
        assert_eq!(
            desktop_exec(Path::new("/home/a b/player")).unwrap(),
            "\"/home/a b/player\""
        );
        assert_eq!(
            desktop_exec(Path::new("/a%u/player")).unwrap(),
            "\"/a%%u/player\""
        );
        assert!(desktop_exec(Path::new("/a\nb/player")).is_err());
    }
}
