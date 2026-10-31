use anyhow::{Result, bail, ensure};
use serde_json::Value;
use std::time::Duration;

pub const MAX_BYTES: usize = 512 * 1024;
const MAX_LINES: usize = 2048;
const MAX_TIME: u64 = 31 * 86_400 * 1000;

#[derive(Clone, Debug, PartialEq)]
pub struct Line {
    pub at: Duration,
    pub text: String,
}

#[derive(Clone, Debug)]
pub struct Lyrics {
    pub provider: String,
    pub right_to_left: bool,
    pub plain: String,
    pub lines: Vec<Line>,
}

impl Lyrics {
    pub fn parse(track: u64, value: &Value) -> Result<Option<Self>> {
        ensure!(
            value["trackId"].as_u64() == Some(track) && track != 0,
            "TIDAL lyrics did not match the requested track"
        );
        let text = value["lyrics"].as_str().unwrap_or_default();
        let subtitles = value["subtitles"].as_str().unwrap_or_default();
        for content in [text, subtitles] {
            ensure!(
                content.len() <= MAX_BYTES / 2
                    && content.lines().count() <= MAX_LINES
                    && content.lines().all(|line| line.len() <= 4096),
                "TIDAL lyrics exceed the display limits"
            );
        }
        let provider = value["lyricsProvider"].as_str().unwrap_or("TIDAL");
        ensure!(provider.len() <= 256, "Invalid lyrics attribution");
        let provider = provider.split_whitespace().collect::<Vec<_>>().join(" ");
        let lines = timed(subtitles)
            .filter(|lines| lines.iter().any(|line| !line.text.trim().is_empty()))
            .unwrap_or_default();
        let plain = if text.trim().is_empty() {
            subtitles
                .lines()
                .filter(|line| !metadata(line.trim()))
                .map(without_times)
                .collect::<Vec<_>>()
                .join("\n")
        } else {
            text.to_owned()
        };
        if plain.trim().is_empty() && lines.iter().all(|line| line.text.trim().is_empty()) {
            return Ok(None);
        }
        Ok(Some(Self {
            provider: if provider.is_empty() {
                "TIDAL".into()
            } else {
                provider
            },
            right_to_left: value["isRightToLeft"].as_bool().unwrap_or(false),
            plain,
            lines,
        }))
    }

    pub fn active(&self, position: Duration) -> Option<usize> {
        let end = self.lines.partition_point(|line| line.at <= position);
        let last = end.checked_sub(1)?;
        let at = self.lines[last].at;
        Some(self.lines[..end].partition_point(|line| line.at < at))
    }
}

fn metadata(line: &str) -> bool {
    [
        "ar:", "al:", "ti:", "by:", "re:", "ve:", "length:", "offset:",
    ]
    .iter()
    .any(|prefix| {
        line.strip_prefix('[')
            .is_some_and(|rest| rest.starts_with(prefix) && rest.ends_with(']'))
    })
}

fn stamp(tag: &str) -> Option<u64> {
    let (minutes, seconds) = tag.split_once(':')?;
    if minutes.is_empty() || !minutes.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let minutes: u64 = minutes.parse().ok()?;
    let decimal = seconds.contains('.');
    let (seconds, fraction) = seconds.split_once('.').unwrap_or((seconds, ""));
    if decimal && fraction.is_empty() {
        return None;
    }
    if seconds.len() != 2
        || !seconds.bytes().all(|b| b.is_ascii_digit())
        || fraction.len() > 3
        || !fraction.bytes().all(|b| b.is_ascii_digit())
    {
        return None;
    }
    let seconds: u64 = seconds.parse().ok()?;
    if seconds >= 60 {
        return None;
    }
    let fraction = if fraction.is_empty() {
        0
    } else {
        fraction.parse::<u64>().ok()? * 10u64.pow(3 - fraction.len() as u32)
    };
    let time = minutes
        .checked_mul(60_000)?
        .checked_add(seconds * 1000 + fraction)?;
    (time <= MAX_TIME).then_some(time)
}

fn without_times(mut line: &str) -> &str {
    while let Some(rest) = line.strip_prefix('[') {
        let Some((tag, text)) = rest.split_once(']') else {
            break;
        };
        if stamp(tag).is_none() {
            break;
        }
        line = text;
    }
    line
}

fn timed(content: &str) -> Option<Vec<Line>> {
    let mut offset = None;
    for line in content.lines().map(str::trim) {
        if let Some(value) = line.strip_prefix("[offset:") {
            let value: i64 = value.strip_suffix(']')?.trim().parse().ok()?;
            if value.unsigned_abs() > 60_000 || offset.is_some() {
                return None;
            }
            offset = Some(value);
        }
    }
    let mut lines = Vec::new();
    let mut expanded_bytes = 0usize;
    for line in content.lines().map(str::trim) {
        if line.is_empty() || metadata(line) {
            continue;
        }
        let mut rest = line;
        let mut times = Vec::new();
        while let Some(value) = rest.strip_prefix('[') {
            let (tag, text) = value.split_once(']')?;
            let Some(time) = stamp(tag) else {
                if times.is_empty() || tag.bytes().next().is_some_and(|b| b.is_ascii_digit()) {
                    return None;
                }
                break;
            };
            if times.len() >= 8 {
                return None;
            }
            times.push(time.checked_add_signed(offset.unwrap_or(0))?);
            rest = text;
        }
        if times.is_empty() {
            return None;
        }
        for time in times {
            if time > MAX_TIME || lines.len() >= MAX_LINES {
                return None;
            }
            expanded_bytes = expanded_bytes.checked_add(rest.len())?;
            if expanded_bytes > MAX_BYTES {
                return None;
            }
            lines.push(Line {
                at: Duration::from_millis(time),
                text: rest.trim().to_owned(),
            });
        }
    }
    lines.sort_by_key(|line| line.at);
    Some(lines)
}

pub async fn fetch(request: reqwest::RequestBuilder, track: u64) -> Result<Option<Lyrics>> {
    let mut response = request
        .send()
        .await
        .map_err(|_| anyhow::anyhow!("Could not reach TIDAL lyrics. Try again later."))?;
    match response.status().as_u16() {
        404 | 204 => return Ok(None),
        401 | 403 => {
            bail!("TIDAL did not authorize lyrics for this sign-in. Playback is unaffected.")
        }
        429 => bail!("TIDAL is limiting lyrics requests. Try again later."),
        200 => {}
        _ => bail!("TIDAL lyrics are temporarily unavailable. Try again later."),
    }
    ensure!(
        response
            .content_length()
            .is_none_or(|size| size <= MAX_BYTES as u64),
        "TIDAL lyrics exceed the response limit"
    );
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| anyhow::anyhow!("TIDAL lyrics could not be read. Try again later."))?
    {
        ensure!(
            chunk.len() <= MAX_BYTES.saturating_sub(bytes.len()),
            "TIDAL lyrics exceed the response limit"
        );
        bytes.extend_from_slice(&chunk);
    }
    let value: Value = serde_json::from_slice(&bytes)
        .map_err(|_| anyhow::anyhow!("TIDAL returned an invalid lyrics response"))?;
    Lyrics::parse(track, &value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn lyrics(subtitles: &str) -> Lyrics {
        Lyrics::parse(9, &json!({"trackId":9,"lyrics":"Plain fallback","subtitles":subtitles,"lyricsProvider":"Fixture"})).unwrap().unwrap()
    }
    #[test]
    fn timed_lines_sort_stably_and_keep_duplicates_offsets_and_precision() {
        let value = lyrics(
            "[offset:125]\n[00:02.50]Second\n[00:01.000][00:03.0]First\n[00:02.50]Translation",
        );
        assert_eq!(value.lines.len(), 4);
        assert_eq!(value.lines[0].at, Duration::from_millis(1125));
        assert_eq!(value.lines[1].text, "Second");
        assert_eq!(value.lines[2].text, "Translation");
        assert_eq!(value.active(Duration::ZERO), None);
        assert_eq!(value.active(Duration::from_millis(2625)), Some(1));
    }
    #[test]
    fn untrusted_timing_falls_back_without_inventing_seek_targets() {
        for input in [
            "Unsynced words",
            "[00:60.000]Bad seconds",
            "[00:NaN]Bad",
            "[00:01.1234]Precision",
            "[00:01.]Missing fraction",
            "[00:01]\n[00:02]",
            "[offset:-1000]\n[00:00.1]Before start",
            "[offset:1]\n[offset:2]\n[00:01]Conflicting",
            "[00:01]Valid\nUntimed line",
            "[999999999999999999999:00]Overflow",
        ] {
            let value = lyrics(input);
            assert!(value.lines.is_empty(), "{input}");
            assert_eq!(value.plain, "Plain fallback");
        }
    }
    #[test]
    fn identity_limits_direction_and_missing_text_are_explicit() {
        assert!(Lyrics::parse(8, &json!({"trackId":9})).is_err());
        assert!(Lyrics::parse(9, &json!({"trackId":9})).unwrap().is_none());
        assert!(Lyrics::parse(9, &json!({"trackId":9,"lyrics":"x".repeat(4097)})).is_err());
        let value = Lyrics::parse(9, &json!({"trackId":9,"lyrics":"שלום","isRightToLeft":true,"lyricsProvider":" Fixture\n Provider "})).unwrap().unwrap();
        assert!(value.right_to_left);
        assert_eq!(value.plain, "שלום");
        assert_eq!(value.provider, "Fixture Provider");
    }
}
