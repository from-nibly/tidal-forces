use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Artist {
    #[serde(default)]
    pub name: String,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Album {
    pub id: u64,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub cover: Option<String>,
    #[serde(default)]
    pub artist: Artist,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Track {
    pub id: u64,
    pub title: String,
    #[serde(default)]
    pub duration: u64,
    #[serde(default)]
    pub artist: Artist,
    #[serde(default)]
    pub album: Album,
    #[serde(default)]
    pub explicit: bool,
}

impl Track {
    pub fn cover_url(&self, size: u32) -> Option<String> {
        cover_url(self.album.cover.as_deref(), size)
    }
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Playlist {
    pub uuid: String,
    pub title: String,
    #[serde(default)]
    pub number_of_tracks: u64,
}

pub fn cover_url(id: Option<&str>, size: u32) -> Option<String> {
    id.filter(|s| !s.is_empty()).map(|s| {
        format!(
            "https://resources.tidal.com/images/{}/{size}x{size}.jpg",
            s.replace('-', "/")
        )
    })
}

pub fn time(seconds: u64) -> String {
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_track_and_formats_metadata() {
        let t: Track = serde_json::from_str(r#"{"id":1,"title":"Test","artist":{"name":"Artist"},"album":{"id":2,"title":"Album","cover":"ab-cd"}}"#).unwrap();
        assert_eq!(
            t.cover_url(320).unwrap(),
            "https://resources.tidal.com/images/ab/cd/320x320.jpg"
        );
        assert_eq!(time(185), "3:05");
        assert_eq!(cover_url(None, 320), None);
    }
}
