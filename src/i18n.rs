//! English source-message catalogs. Provider metadata and widget IDs are not messages.
use anyhow::{Result, ensure};
use serde::Deserialize;
use std::{collections::BTreeMap, sync::OnceLock};

const ENGLISH: &[u8] = include_bytes!("../assets/i18n/en.json");

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Catalog {
    pub locale: String,
    messages: BTreeMap<String, String>,
}
impl Catalog {
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        ensure!(
            bytes.len() <= 512 * 1024,
            "Translation catalog exceeds 512 KiB"
        );
        let catalog: Self = serde_json::from_slice(bytes)?;
        let mut tag = catalog.locale.split('-');
        let language = tag.next().unwrap_or_default();
        ensure!(
            (2..=8).contains(&language.len())
                && language.bytes().all(|c| c.is_ascii_alphabetic())
                && catalog.locale.len() <= 35
                && tag.all(|part| !part.is_empty()
                    && part.len() <= 8
                    && part.bytes().all(|c| c.is_ascii_alphanumeric())),
            "Invalid catalog locale"
        );
        ensure!(catalog.messages.len() <= 1024, "Too many catalog messages");
        for (source, translated) in &catalog.messages {
            ensure!(
                !source.trim().is_empty()
                    && source.len() <= 1024
                    && !translated.trim().is_empty()
                    && translated.len() <= 4096,
                "Invalid catalog message length"
            );
            ensure!(
                source
                    .chars()
                    .chain(translated.chars())
                    .all(|c| (!c.is_control() || c == '\n' || c == '\t')
                        && !matches!(c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')),
                "Unsupported catalog control character"
            );
            ensure!(
                slots(source)? == slots(translated)?,
                "Translation placeholders must match the source message"
            );
        }
        Ok(catalog)
    }
    pub fn text<'a>(&'a self, source: &'a str) -> &'a str {
        self.messages.get(source).map_or(source, String::as_str)
    }
}
fn slots(text: &str) -> Result<BTreeMap<&str, usize>> {
    let mut found = BTreeMap::new();
    let mut rest = text;
    while let Some(start) = rest.find(['{', '}']) {
        let brace = rest.as_bytes()[start] as char;
        rest = &rest[start + 1..];
        if rest.starts_with(brace) {
            rest = &rest[1..];
            continue;
        }
        ensure!(brace == '{', "Unmatched translation brace");
        let end = rest
            .find('}')
            .ok_or_else(|| anyhow::anyhow!("Unclosed translation placeholder"))?;
        let name = &rest[..end];
        ensure!(
            name.len() <= 64 && !name.contains('{'),
            "Invalid translation placeholder"
        );
        *found.entry(name).or_default() += 1;
        rest = &rest[end + 1..];
    }
    Ok(found)
}
pub fn tr(source: &str) -> &str {
    static CATALOG: OnceLock<Catalog> = OnceLock::new();
    CATALOG
        .get_or_init(|| {
            let catalog = Catalog::parse(ENGLISH).expect("Bundled English catalog must validate");
            assert_eq!(catalog.locale, "en");
            catalog
        })
        .text(source)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shipped_catalog_is_english_and_preserves_every_message() {
        let catalog = Catalog::parse(ENGLISH).unwrap();
        assert_eq!(catalog.locale, "en");
        assert!(catalog.messages.len() >= 100);
        for (source, translated) in &catalog.messages {
            assert_eq!(source, translated);
            assert_eq!(tr(source), source);
        }
        assert_eq!(
            tr("Unknown provider-supplied diagnostic"),
            "Unknown provider-supplied diagnostic"
        );
    }
    #[test]
    fn literal_ui_messages_are_registered() {
        let catalog = Catalog::parse(ENGLISH).unwrap();
        for source in [
            include_str!("ui.rs"),
            include_str!("ui/appearance.rs"),
            include_str!("ui/classic.rs"),
            include_str!("ui/settings.rs"),
            include_str!("ui/mini.rs"),
            include_str!("ui/now_playing.rs"),
            include_str!("ui/side_panel.rs"),
            include_str!("ui/credentials.rs"),
            include_str!("ui/history.rs"),
            include_str!("ui/export.rs"),
            include_str!("ui/album_load.rs"),
            include_str!("ui/playlist_delete.rs"),
            include_str!("ui/library_view.rs"),
            include_str!("ui/library.rs"),
            include_str!("ui/reorder.rs"),
            include_str!("ui/playlist_metadata.rs"),
            include_str!("ui/folder_edit.rs"),
            include_str!("ui/library_load.rs"),
            include_str!("ui/track_table.rs"),
            include_str!("ui/playlist_load.rs"),
            include_str!("ui/favorite_cache.rs"),
            include_str!("ui/radio.rs"),
            include_str!("ui/pins.rs"),
        ] {
            for tail in source.split("tr(\"").skip(1) {
                let literal = format!("\"{tail}");
                let message =
                    String::deserialize(&mut serde_json::Deserializer::from_str(&literal)).unwrap();
                assert!(
                    catalog.messages.contains_key(&message),
                    "Unregistered UI message: {message}"
                );
            }
        }
    }

    #[test]
    fn partial_catalogs_fall_back_without_translating_unknown_text() {
        let catalog = Catalog::parse(br#"{"locale":"test","messages":{"Play":"Example action","Count: {count}":"Example: {count}"}}"#).unwrap();
        assert_eq!(catalog.text("Play"), "Example action");
        assert_eq!(catalog.text("An artist's title"), "An artist's title");
        assert!(
            Catalog::parse(br#"{"locale":"test","messages":{"Count: {count}":"Missing"}}"#)
                .is_err()
        );
        assert!(Catalog::parse(br#"{"locale":"test","messages":{"{x} {x}":"{x}"}}"#).is_err());
    }
    #[test]
    fn invalid_catalogs_are_bounded_and_rejected() {
        assert!(Catalog::parse(&vec![b' '; 512 * 1024 + 1]).is_err());
        for value in [
            serde_json::json!({"locale":"../bad", "messages":{}}),
            serde_json::json!({"locale":"en", "messages":{"Play":""}}),
            serde_json::json!({"locale":"en--US", "messages":{}}),
            serde_json::json!({"locale":"en", "messages":{"Play":"}"}}),
            serde_json::json!({"locale":"en", "messages":{"Play":"\u{202e}bad"}}),
            serde_json::json!({"locale":"en", "messages":{"Play":"x"}, "script":"no"}),
        ] {
            assert!(Catalog::parse(&serde_json::to_vec(&value).unwrap()).is_err());
        }
    }
}
