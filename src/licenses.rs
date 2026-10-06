use anyhow::Result;
use std::io::Write;

const FDK_SOURCE: &[u8] = include_bytes!("../assets/sources/fdk-aac-sys-0.5.0.crate");

pub fn print() {
    println!(
        "Tidal Forces\n{}\n\nBundled FDK AAC codec (unmodified upstream source)\n{}\n\nInter fonts\n{}",
        include_str!("../LICENSE"),
        include_str!("../assets/licenses/FDK-AAC.txt"),
        include_str!("../assets/fonts/OFL.txt")
    );
    println!(
        "Complete bundled FDK source: run --export-fdk-source PATH to extract the original fdk-aac-sys-0.5.0.crate (gzip tar archive). The Rust application source and Cargo.lock are available at https://github.com/from-nibly/tidal-forces."
    );
}

pub fn export(path: &str) -> Result<()> {
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    file.write_all(FDK_SOURCE)?;
    println!("Exported complete FDK source to {path}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};
    #[test]
    fn bundled_source_matches_the_locked_codec_dependency() {
        let hash = format!("{:x}", Sha256::digest(FDK_SOURCE));
        let package = include_str!("../Cargo.lock")
            .split("[[package]]")
            .find(|s| s.contains("name = \"fdk-aac-sys\""))
            .unwrap();
        assert!(package.contains(&format!("checksum = \"{hash}\"")));
    }
}
