//! Bounded, image-only classic Winamp main-panel skins. Never extract archive paths.
use anyhow::{Context, Result, ensure};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::{Cursor, Read},
    path::Path,
};
use zip::{CompressionMethod, unstable::read::ZipIterable};

pub const MAX_ARCHIVE: usize = 8 * 1024 * 1024;
const MAX_FILE: usize = 4 * 1024 * 1024;
const MAX_PIXELS: usize = 8 * 1024 * 1024;
pub const STORED_NAME: &str = "classic-skin.wsz";

pub fn read_enabled<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<bool, D::Error> {
    use serde::Deserialize;
    let value = serde_json::Value::deserialize(deserializer)?;
    Ok(value.as_bool().unwrap_or(false))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum Asset {
    Main,
    Buttons,
    Title,
    Position,
    Volume,
    Shuffle,
    Status,
}
impl Asset {
    fn named(name: &str) -> Option<Self> {
        Some(match name {
            "main.bmp" => Self::Main,
            "cbuttons.bmp" => Self::Buttons,
            "titlebar.bmp" => Self::Title,
            "posbar.bmp" => Self::Position,
            "volume.bmp" => Self::Volume,
            "shufrep.bmp" => Self::Shuffle,
            "playpaus.bmp" => Self::Status,
            _ => return None,
        })
    }
    fn minimum(self) -> (u32, u32) {
        match self {
            Self::Main => (275, 116),
            Self::Buttons => (136, 36),
            Self::Title => (302, 29),
            Self::Position => (307, 10),
            Self::Volume => (68, 433),
            Self::Shuffle => (75, 73),
            Self::Status => (27, 9),
        }
    }
}
pub struct Skin {
    pub images: BTreeMap<Asset, image::RgbaImage>,
}

pub fn read(path: &Path) -> Result<Vec<u8>> {
    ensure!(
        path.extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("wsz")),
        "Choose a classic .wsz file, not a modern .wal skin"
    );
    ensure!(
        std::fs::symlink_metadata(path)
            .context("Cannot inspect the selected skin")?
            .is_file(),
        "Skin must be a regular file, not a link or device"
    );
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let file = options
        .open(path)
        .context("Cannot open the selected skin")?;
    let metadata = file.metadata()?;
    ensure!(
        metadata.is_file() && metadata.len() <= MAX_ARCHIVE as u64,
        "Skin must be a regular file no larger than 8 MiB"
    );
    let mut bytes = Vec::new();
    file.take(MAX_ARCHIVE as u64 + 1).read_to_end(&mut bytes)?;
    ensure!(bytes.len() <= MAX_ARCHIVE, "Skin exceeds 8 MiB");
    Ok(bytes)
}

pub fn load(path: &Path, save_dir: Option<&Path>) -> Result<Skin> {
    let bytes = read(path)?;
    let skin = Skin::parse(&bytes)?;
    if let Some(dir) = save_dir {
        crate::store::save_bytes(dir, STORED_NAME, &bytes)?;
    }
    Ok(skin)
}

impl Skin {
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        ensure!(
            bytes.len() <= MAX_ARCHIVE && bytes.starts_with(b"PK\x03\x04"),
            "Expected a classic ZIP skin no larger than 8 MiB"
        );
        // The iterable API bounds metadata before indexing and preserves duplicate names.
        // Its unstable API is deliberately pinned to an exact reviewed zip version.
        let mut archive = ZipIterable::try_new(Cursor::new(bytes), Default::default())?
            .enforce_header_consistency(true);
        ensure!(archive.len() <= 128, "Skin exceeds 128 archive entries");
        let entries = archive.files()?;
        let mut images = BTreeMap::new();
        let mut names = BTreeSet::new();
        let mut expanded = 0u64;
        let mut pixels = 0usize;
        while let Some(entry) = entries.next() {
            let entry = entry?;
            let name =
                std::str::from_utf8(entry.name_raw()).context("Skin filenames must be UTF-8")?;
            ensure!(
                !name.is_empty()
                    && name.len() <= 256
                    && !name.contains(['\\', ':'])
                    && !name.chars().any(char::is_control)
                    && name
                        .trim_end_matches('/')
                        .split('/')
                        .all(|s| !s.is_empty() && s != "." && s != ".."),
                "Unsafe skin entry name"
            );
            ensure!(
                !entry.is_symlink() && !entry.encrypted(),
                "Skin links and encrypted entries are unsupported"
            );
            ensure!(
                matches!(
                    entry.compression(),
                    CompressionMethod::Stored | CompressionMethod::Deflated
                ),
                "Only stored/deflate classic ZIP skins are supported"
            );
            ensure!(
                names.insert(name.to_ascii_lowercase()),
                "Duplicate skin entry"
            );
            ensure!(entry.size() <= MAX_FILE as u64, "Skin entry exceeds 4 MiB");
            expanded = expanded
                .checked_add(entry.size())
                .context("Skin size overflow")?;
            ensure!(expanded <= 16 * 1024 * 1024, "Skin expands beyond 16 MiB");
            if entry.is_dir() {
                continue;
            }
            let basename = name.rsplit('/').next().unwrap().to_ascii_lowercase();
            let Some(asset) = Asset::named(&basename) else {
                continue;
            };
            ensure!(
                !images.contains_key(&asset),
                "Ambiguous duplicate skin bitmap"
            );
            let size = entry.size();
            let file = entry.with_reader(entries.reader_mut(), Default::default())?;
            let mut data = Vec::new();
            file.take(MAX_FILE as u64 + 1)
                .read_to_end(&mut data)
                .context("Invalid skin bitmap checksum or compression")?;
            ensure!(
                data.len() <= MAX_FILE && data.len() as u64 == size,
                "Skin bitmap size mismatch"
            );
            let mut reader =
                image::ImageReader::with_format(Cursor::new(&data), image::ImageFormat::Bmp);
            let mut limits = image::Limits::default();
            limits.max_image_width = Some(1024);
            limits.max_image_height = Some(1024);
            limits.max_alloc = Some(MAX_PIXELS as u64);
            reader.limits(limits);
            let bitmap = reader
                .decode()
                .context("Invalid or oversized classic BMP")?;
            let (width, height) = asset.minimum();
            ensure!(
                bitmap.width() >= width && bitmap.height() >= height,
                "Classic bitmap is too small for its sprite layout"
            );
            if asset == Asset::Main {
                ensure!(
                    bitmap.width() == width && bitmap.height() == height,
                    "Classic main.bmp must be 275 × 116 pixels"
                );
            }
            pixels = pixels
                .checked_add(bitmap.width() as usize * bitmap.height() as usize * 4)
                .context("Skin pixel size overflow")?;
            ensure!(pixels <= MAX_PIXELS, "Decoded skin exceeds 8 MiB");
            images.insert(asset, bitmap.crop_imm(0, 0, width, height).to_rgba8());
        }
        ensure!(
            images.contains_key(&Asset::Main) && images.contains_key(&Asset::Buttons),
            "Classic skin needs main.bmp and cbuttons.bmp"
        );
        Ok(Self { images })
    }
}

#[cfg(any(test, debug_assertions))]
#[path = "skins_fixture.rs"]
pub(crate) mod fixture;

#[cfg(test)]
#[path = "skins_tests.rs"]
mod tests;
