use crate::model::Track;
use anyhow::{Result, bail, ensure};

const MAX_BYTES: usize = 1024 * 1024;
pub const MAX_PAGE: usize = 100;

pub async fn fetch(request: reqwest::RequestBuilder) -> Result<Vec<Track>> {
    let mut response = request
        .send()
        .await
        .map_err(|_| anyhow::anyhow!("Recommendation request failed"))?;
    match response.status().as_u16() {
        200 => {}
        204 | 404 => return Ok(Vec::new()),
        401 | 403 => bail!("TIDAL recommendations are unavailable for this sign-in"),
        429 => bail!("TIDAL recommendation rate limit reached"),
        _ => bail!("TIDAL recommendations are temporarily unavailable"),
    }
    ensure!(
        response
            .content_length()
            .is_none_or(|size| size <= MAX_BYTES as u64),
        "Recommendation response is too large"
    );
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| anyhow::anyhow!("Recommendation response could not be read"))?
    {
        ensure!(
            bytes.len().saturating_add(chunk.len()) <= MAX_BYTES,
            "Recommendation response is too large"
        );
        bytes.extend_from_slice(&chunk);
    }
    parse(&bytes)
}

fn parse(bytes: &[u8]) -> Result<Vec<Track>> {
    ensure!(
        bytes.len() <= MAX_BYTES,
        "Recommendation response is too large"
    );
    let value: serde_json::Value = serde_json::from_slice(bytes)
        .map_err(|_| anyhow::anyhow!("Invalid recommendation response"))?;
    let items = value["items"]
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("Invalid recommendation collection"))?;
    ensure!(items.len() <= MAX_PAGE, "Too many recommendation entries");
    let mut tracks = Vec::new();
    for entry in items {
        let mut item = entry.get("item").unwrap_or(entry).clone();
        if [entry.get("type"), item.get("type")]
            .into_iter()
            .flatten()
            .any(|kind| !matches!(kind.as_str(), Some("TRACK" | "track")))
        {
            continue;
        }
        if item["artist"].is_null() && item["artists"][0].is_object() {
            item["artist"] = item["artists"][0].clone();
        }
        let track: Track = serde_json::from_value(item)
            .map_err(|_| anyhow::anyhow!("Invalid recommended track"))?;
        ensure!(
            track.id != 0
                && track.duration > 0
                && track.duration <= 31 * 86400
                && !track.title.trim().is_empty()
                && track.title.len() <= 4096
                && track.artist.name.len() <= 4096
                && track.album.title.len() <= 4096
                && track
                    .album
                    .cover
                    .as_ref()
                    .is_none_or(|cover| cover.len() <= 4096),
            "Invalid recommended track metadata"
        );
        tracks.push(track);
    }
    Ok(tracks)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn recommendations_are_bounded_and_do_not_treat_video_as_audio() {
        let track = serde_json::json!({"id":1,"title":"Track","duration":180,"artists":[{"id":2,"name":"Artist"}]});
        let bytes = serde_json::to_vec(&serde_json::json!({"items":[{"type":"TRACK","item":track.clone()},{"type":"VIDEO","item":track.clone()}]})).unwrap();
        let parsed = parse(&bytes).unwrap();
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].artist.name, "Artist");
        assert!(
            parse(&serde_json::to_vec(&serde_json::json!({"items":vec![track;101]})).unwrap())
                .is_err()
        );
        assert!(parse(br#"{"items":[{"id":0,"title":"Invalid","duration":180}]}"#).is_err());
        assert!(parse(&vec![b' '; MAX_BYTES + 1]).is_err());
    }
}
