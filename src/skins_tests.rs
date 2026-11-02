use super::*;
use std::io::Write;

fn bitmap(width: u32, height: u32) -> Vec<u8> {
    let mut bytes = Cursor::new(Vec::new());
    image::DynamicImage::new_rgb8(width, height)
        .write_to(&mut bytes, image::ImageFormat::Bmp)
        .unwrap();
    bytes.into_inner()
}
fn archive(files: &[(&str, Vec<u8>)], method: CompressionMethod) -> Vec<u8> {
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in files {
        writer
            .start_file(
                *name,
                zip::write::SimpleFileOptions::default().compression_method(method),
            )
            .unwrap();
        writer.write_all(bytes).unwrap();
    }
    writer.finish().unwrap().into_inner()
}
fn valid() -> Vec<(&'static str, Vec<u8>)> {
    vec![
        ("MAIN.BMP", bitmap(275, 116)),
        ("cbuttons.bmp", bitmap(136, 36)),
    ]
}
#[test]
fn classic_images_are_bounded_case_insensitive_and_not_extracted() {
    for method in [CompressionMethod::Stored, CompressionMethod::Deflated] {
        let mut files = valid();
        files.push(("ignored/script.js", b"never executed".to_vec()));
        let skin = Skin::parse(&archive(&files, method)).unwrap();
        assert_eq!(skin.images.len(), 2);
        assert_eq!(skin.images[&Asset::Main].dimensions(), (275, 116));
    }
}
#[test]
fn traversal_aliases_wrong_formats_and_excessive_counts_are_rejected() {
    for name in [
        "../main.bmp",
        "/main.bmp",
        "C:/main.bmp",
        "folder/../main.bmp",
        "folder\\main.bmp",
    ] {
        // Patch both headers after writing, so the writer cannot normalize hostile paths.
        let placeholder = "q".repeat(name.len());
        let mut bytes = archive(
            &[
                (&placeholder, bitmap(275, 116)),
                ("cbuttons.bmp", bitmap(136, 36)),
            ],
            CompressionMethod::Stored,
        );
        let positions: Vec<_> = bytes
            .windows(name.len())
            .enumerate()
            .filter_map(|(i, v)| (v == placeholder.as_bytes()).then_some(i))
            .collect();
        assert_eq!(positions.len(), 2);
        for pos in positions {
            bytes[pos..pos + name.len()].copy_from_slice(name.as_bytes());
        }
        assert!(Skin::parse(&bytes).is_err(), "accepted unsafe path {name}");
    }
    let mut files = valid();
    files.push(("other/Main.bmp", bitmap(275, 116)));
    assert!(Skin::parse(&archive(&files, CompressionMethod::Stored)).is_err());
    assert!(
        Skin::parse(&archive(
            &[
                ("main.bmp", bitmap(276, 116)),
                ("cbuttons.bmp", bitmap(136, 36))
            ],
            CompressionMethod::Stored
        ))
        .is_err()
    );
    assert!(
        Skin::parse(&archive(
            &[("main.bmp", b"not BMP".to_vec())],
            CompressionMethod::Stored
        ))
        .is_err()
    );
    assert!(Skin::parse(&vec![0; MAX_ARCHIVE + 1]).is_err());
    let names: Vec<_> = (0..129).map(|i| format!("{i}.txt")).collect();
    let files: Vec<_> = names.iter().map(|n| (n.as_str(), vec![])).collect();
    assert!(Skin::parse(&archive(&files, CompressionMethod::Stored)).is_err());
}
#[test]
fn bomb_dimensions_sizes_crc_and_truncated_archives_fail_closed() {
    let mut files = valid();
    files.push(("ignored", vec![0; MAX_FILE + 1]));
    assert!(Skin::parse(&archive(&files, CompressionMethod::Deflated)).is_err());
    let mut bmp = bitmap(275, 116);
    bmp[18..22].copy_from_slice(&i32::MAX.to_le_bytes());
    assert!(
        Skin::parse(&archive(
            &[("main.bmp", bmp), ("cbuttons.bmp", bitmap(136, 36))],
            CompressionMethod::Stored
        ))
        .is_err()
    );
    let bytes = archive(&valid(), CompressionMethod::Stored);
    for end in [0, 4, 30, bytes.len() / 2, bytes.len() - 10] {
        assert!(Skin::parse(&bytes[..end]).is_err());
    }
    let mut corrupt = bytes;
    let start = corrupt.windows(2).position(|v| v == b"BM").unwrap();
    corrupt[start + 60] ^= 1;
    assert!(Skin::parse(&corrupt).is_err());
}
#[test]
fn duplicate_names_encryption_links_and_inconsistent_headers_are_rejected() {
    let mut files = valid();
    files.push(("dupe.bmp", bitmap(275, 116)));
    let mut bytes = archive(&files, CompressionMethod::Stored);
    let positions: Vec<_> = bytes
        .windows(8)
        .enumerate()
        .filter_map(|(i, v)| (v == b"dupe.bmp").then_some(i))
        .collect();
    assert_eq!(positions.len(), 2);
    for pos in positions {
        bytes[pos..pos + 8].copy_from_slice(b"MAIN.BMP");
    }
    assert!(Skin::parse(&bytes).is_err());
    let mut bytes = archive(&valid(), CompressionMethod::Stored);
    let pos = bytes.windows(8).rposition(|v| v == b"MAIN.BMP").unwrap();
    bytes[pos..pos + 8].copy_from_slice(b"main.bmp");
    assert!(Skin::parse(&bytes).is_err());
    let mut bytes = archive(&valid(), CompressionMethod::Stored);
    let local = bytes.windows(4).position(|v| v == b"PK\x03\x04").unwrap();
    let central = bytes.windows(4).position(|v| v == b"PK\x01\x02").unwrap();
    bytes[local + 6] |= 1;
    bytes[central + 8] |= 1;
    assert!(Skin::parse(&bytes).is_err());
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    writer
        .add_symlink("link", "main.bmp", zip::write::SimpleFileOptions::default())
        .unwrap();
    let bytes = writer.finish().unwrap().into_inner();
    assert!(
        Skin::parse(&bytes)
            .err()
            .unwrap()
            .to_string()
            .contains("links")
    );
}

#[test]
fn only_validated_archives_replace_the_private_copy() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("input.wsz");
    let dest = root.path().join("private");
    let bytes = archive(&valid(), CompressionMethod::Deflated);
    std::fs::write(&path, &bytes).unwrap();
    load(&path, Some(&dest)).unwrap();
    assert_eq!(std::fs::read(dest.join(STORED_NAME)).unwrap(), bytes);
    assert_eq!(std::fs::read_dir(&dest).unwrap().count(), 1);
    std::fs::write(&path, b"invalid archive").unwrap();
    assert!(load(&path, Some(&dest)).is_err());
    assert_eq!(std::fs::read(dest.join(STORED_NAME)).unwrap(), bytes);
    assert!(load(&dest.join(STORED_NAME), None).is_ok());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&dest).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert_eq!(
            std::fs::metadata(dest.join(STORED_NAME))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
}

#[test]
fn selected_files_must_be_local_regular_wsz_files() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("skin.wsz");
    std::fs::write(&path, archive(&valid(), CompressionMethod::Stored)).unwrap();
    assert!(Skin::parse(&read(&path).unwrap()).is_ok());
    assert!(read(dir.path()).is_err());
    let wrong = dir.path().join("skin.wal");
    std::fs::write(&wrong, b"not a classic skin").unwrap();
    assert!(read(&wrong).is_err());
    #[cfg(unix)]
    {
        let link = dir.path().join("link.wsz");
        std::os::unix::fs::symlink(&path, &link).unwrap();
        assert!(read(&link).is_err());
    }
}
