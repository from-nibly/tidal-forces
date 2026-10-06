use crate::{
    model::{Album, Artist, Home, LibraryEntry, Mix, Playlist, RadioSeed, Track},
    store::{self, Session},
};
use anyhow::{Context, Result, bail};
use base64::{Engine, engine::general_purpose::STANDARD};
use reqwest::{Client, Response, StatusCode};
use serde::Deserialize;
use serde_json::Value;
use std::time::Duration;

// Public compatibility-client identifiers used by python-tidal (not user credentials).
// TIDAL can revoke these; deployments may provide their own approved client via env.
const CLIENT_ID: &str = "fX2JxdmntZWK0ixT";
const CLIENT_SECRET: &str = "1Nn9AfDAjxrgJFJbKNWLeAyKGVGmINuXPPLHVXAvxAg=";
const AUTH: &str = "https://auth.tidal.com/v1/oauth2";

#[derive(Clone)]
pub struct Api {
    client: Client,
    pub session: Option<Session>,
    client_id: String,
    client_secret: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceLogin {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri_complete: String,
    pub expires_in: u64,
    pub interval: u64,
}

#[derive(Default)]
pub struct Search {
    pub tracks: Vec<Track>,
    pub albums: Vec<Album>,
    pub artists: Vec<Artist>,
}

impl Api {
    pub fn new() -> Result<Self> {
        Ok(Self {
            client: Client::builder()
                .connect_timeout(Duration::from_secs(10))
                .timeout(Duration::from_secs(30))
                .user_agent("TidalForces/0.1")
                .build()?,
            session: None,
            client_id: std::env::var("TIDAL_CLIENT_ID").unwrap_or_else(|_| CLIENT_ID.into()),
            client_secret: std::env::var("TIDAL_CLIENT_SECRET")
                .unwrap_or_else(|_| CLIENT_SECRET.into()),
        })
    }

    pub async fn begin_login(&self) -> Result<DeviceLogin> {
        let r = self
            .client
            .post(format!("{AUTH}/device_authorization"))
            .form(&[
                ("client_id", self.client_id.as_str()),
                ("scope", "r_usr w_usr w_sub"),
            ])
            .send()
            .await?;
        Ok(check(r).await?.json().await?)
    }

    pub async fn poll_login(&mut self, code: &str) -> Result<LoginPoll> {
        let r = self
            .client
            .post(format!("{AUTH}/token"))
            .form(&[
                ("client_id", self.client_id.as_str()),
                ("client_secret", self.client_secret.as_str()),
                ("device_code", code),
                ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
                ("scope", "r_usr w_usr w_sub"),
            ])
            .send()
            .await?;
        let status = r.status();
        let v: Value = r.json().await?;
        if !status.is_success() {
            return match v["error"].as_str() {
                Some("authorization_pending") => Ok(LoginPoll::Pending),
                Some("slow_down") => Ok(LoginPoll::SlowDown),
                Some("expired_token") => bail!("Sign-in expired. Please try again."),
                Some("access_denied") => bail!("Sign-in was declined."),
                _ => bail!("TIDAL sign-in failed (HTTP {status}). Please retry."),
            };
        }
        if let Some(session) = self.session.as_mut() {
            session.pkce = false;
        }
        self.accept_token(v)?;
        self.identify().await?;
        Ok(LoginPoll::Complete)
    }

    pub async fn finish_pkce(&mut self, pkce: &crate::auth::Pkce, redirect: &str) -> Result<()> {
        let code = pkce.code(redirect)?;
        let r = self
            .client
            .post(format!("{AUTH}/token"))
            .form(&[
                ("code", code.as_str()),
                ("client_id", crate::auth::PKCE_CLIENT_ID),
                ("grant_type", "authorization_code"),
                ("redirect_uri", crate::auth::REDIRECT),
                ("scope", "r_usr+w_usr+w_sub"),
                ("code_verifier", &pkce.verifier),
                ("client_unique_key", &pkce.unique_key),
            ])
            .send()
            .await?;
        let v = check(r).await?.json().await?;
        let old = self.session.clone();
        if let Some(s) = self.session.as_mut() {
            s.pkce = true;
        } else {
            self.session = Some(Session {
                pkce: true,
                ..Default::default()
            });
        }
        if let Err(e) = self.accept_token(v) {
            self.session = old;
            return Err(e);
        }
        self.identify().await
    }

    fn accept_token(&mut self, v: Value) -> Result<()> {
        let old = self.session.clone().unwrap_or_default();
        self.session = Some(Session {
            access_token: v["access_token"]
                .as_str()
                .context("TIDAL returned no access token")?
                .into(),
            refresh_token: v["refresh_token"]
                .as_str()
                .unwrap_or(&old.refresh_token)
                .into(),
            expires_at: store::now() + v["expires_in"].as_u64().unwrap_or(300),
            user_id: v["user"]["userId"].as_u64().unwrap_or(old.user_id),
            pkce: old.pkce,
            country: v["user"]["countryCode"]
                .as_str()
                .unwrap_or(&old.country)
                .into(),
        });
        store::save(self.session.as_ref().unwrap())
    }

    async fn refresh(&mut self) -> Result<()> {
        let session = self.session.as_ref().context("Sign in to TIDAL first")?;
        let (client_id, client_secret) = if session.pkce {
            (crate::auth::PKCE_CLIENT_ID, crate::auth::PKCE_CLIENT_SECRET)
        } else {
            (self.client_id.as_str(), self.client_secret.as_str())
        };
        let r = self
            .client
            .post(format!("{AUTH}/token"))
            .form(&[
                ("client_id", client_id),
                ("client_secret", client_secret),
                ("grant_type", "refresh_token"),
                ("refresh_token", session.refresh_token.as_str()),
            ])
            .send()
            .await?;
        let value = check(r).await?.json().await?;
        self.accept_token(value)
    }

    pub async fn identify(&mut self) -> Result<()> {
        let v = self.get("sessions", &[]).await?;
        let session = self.session.as_mut().context("No session")?;
        session.user_id = v["userId"].as_u64().context("TIDAL returned no user ID")?;
        session.country = v["countryCode"]
            .as_str()
            .context("TIDAL returned no country")?
            .into();
        store::save(session)
    }

    async fn get(&mut self, path: &str, params: &[(&str, &str)]) -> Result<Value> {
        self.get_version("v1", path, params).await
    }

    async fn get_version(
        &mut self,
        version: &str,
        path: &str,
        params: &[(&str, &str)],
    ) -> Result<Value> {
        if self
            .session
            .as_ref()
            .context("Sign in to TIDAL first")?
            .expires_at
            <= store::now() + 60
        {
            self.refresh().await?;
        }
        for attempt in 0..2 {
            let s = self.session.as_ref().context("Sign in to TIDAL first")?;
            let r = self
                .client
                .get(format!("https://api.tidal.com/{version}/{path}"))
                .bearer_auth(&s.access_token)
                .query(&[("countryCode", s.country.as_str())])
                .query(params)
                .send()
                .await?;
            if r.status() == StatusCode::UNAUTHORIZED && attempt == 0 {
                self.refresh().await?;
                continue;
            }
            return Ok(check(r).await?.json().await?);
        }
        bail!("Session expired; sign in again")
    }

    pub async fn search(&mut self, query: &str) -> Result<Search> {
        let v = self
            .get(
                "search",
                &[
                    ("query", query),
                    ("types", "TRACKS,ALBUMS,ARTISTS"),
                    ("limit", "50"),
                ],
            )
            .await?;
        Ok(Search {
            tracks: items(&v["tracks"])?,
            albums: items(&v["albums"])?,
            artists: items(&v["artists"])?,
        })
    }

    pub async fn favorites(&mut self, offset: usize) -> Result<Vec<Track>> {
        let id = self.session.as_ref().context("Sign in first")?.user_id;
        let v = self
            .get(
                &format!("users/{id}/favorites/tracks"),
                &[("limit", "100"), ("offset", &offset.to_string())],
            )
            .await?;
        items(&v)
    }

    pub async fn folder(&mut self, id: &str) -> Result<Vec<LibraryEntry>> {
        let mut all = Vec::new();
        let mut cursor = String::new();
        let mut seen = std::collections::HashSet::new();
        loop {
            let mut params = vec![
                ("folderId", id),
                ("limit", "50"),
                ("includeOnly", ""),
                ("order", "NAME"),
                ("orderDirection", "ASC"),
            ];
            if !cursor.is_empty() {
                params.push(("cursor", &cursor));
            }
            let v = self
                .get_version("v2", "my-collection/playlists/folders", &params)
                .await?;
            all.extend(parse_folder(&v)?);
            let Some(next) = v["cursor"].as_str().filter(|c| !c.is_empty()) else {
                break;
            };
            anyhow::ensure!(
                seen.insert(next.to_owned()),
                "TIDAL returned a repeated folder cursor"
            );
            cursor = next.to_owned();
        }
        Ok(all)
    }

    pub async fn home(&mut self) -> Result<Home> {
        let v = self
            .get("pages/my_collection_my_mixes", &[("deviceType", "BROWSER")])
            .await?;
        let mut mixes = Vec::new();
        for module in modules(&v)? {
            if module["type"] == "MIX_LIST" {
                mixes.extend(items::<Mix>(&module["pagedList"])?);
            }
        }
        mixes.retain(|m| !m.mix_type.contains("VIDEO"));
        let daily = mixes
            .iter()
            .find(|m| m.mix_type == "DISCOVERY_MIX")
            .cloned();
        let tracks = if let Some(daily) = &daily {
            self.mix_tracks(&daily.id).await?
        } else {
            Vec::new()
        };
        Ok(Home {
            daily,
            mixes,
            tracks,
        })
    }

    pub async fn mix_tracks(&mut self, id: &str) -> Result<Vec<Track>> {
        let v = self
            .get("pages/mix", &[("deviceType", "BROWSER"), ("mixId", id)])
            .await?;
        let mut tracks = Vec::new();
        for module in modules(&v)? {
            if module["type"] != "TRACK_LIST" {
                continue;
            }
            let list = &module["pagedList"];
            let mut page: Vec<Track> = items(list)?;
            let total = list["totalNumberOfItems"]
                .as_u64()
                .unwrap_or(page.len() as u64) as usize;
            let mut offset = page.len();
            tracks.append(&mut page);
            while offset < total {
                let path = list["dataApiPath"]
                    .as_str()
                    .context("Missing mix pagination path")?;
                anyhow::ensure!(
                    path.starts_with("pages/data/") && !path.contains(".."),
                    "Invalid mix pagination path"
                );
                let v = self
                    .get(
                        path,
                        &[
                            ("offset", &offset.to_string()),
                            ("limit", "100"),
                            ("deviceType", "BROWSER"),
                        ],
                    )
                    .await?;
                let mut page: Vec<Track> = items(&v)?;
                anyhow::ensure!(!page.is_empty(), "TIDAL returned an incomplete mix");
                offset += page.len();
                tracks.append(&mut page);
            }
        }
        anyhow::ensure!(!tracks.is_empty(), "No playable tracks in this mix");
        Ok(tracks)
    }

    pub async fn radio(&mut self, seed: &RadioSeed) -> Result<Vec<Track>> {
        let v = self
            .get(&seed.path(), &[("limit", "100"), ("offset", "0")])
            .await?;
        let tracks = items(&v)?;
        anyhow::ensure!(
            !tracks.is_empty(),
            "TIDAL has no radio tracks for this item"
        );
        Ok(tracks)
    }

    pub async fn collection(&mut self, kind: &str, id: &str, offset: usize) -> Result<Vec<Track>> {
        anyhow::ensure!(matches!(kind, "albums" | "playlists"), "Invalid collection");
        anyhow::ensure!(
            id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'),
            "Invalid ID"
        );
        let v = self
            .get(
                &format!("{kind}/{id}/tracks"),
                &[("limit", "100"), ("offset", &offset.to_string())],
            )
            .await?;
        items(&v)
    }

    pub async fn stream(&mut self, id: u64, quality: &str) -> Result<Stream> {
        let v = self
            .get(
                &format!("tracks/{id}/playbackinfopostpaywall"),
                &[
                    ("playbackmode", "STREAM"),
                    ("audioquality", quality),
                    ("assetpresentation", "FULL"),
                ],
            )
            .await?;
        if matches!(quality, "LOSSLESS" | "HI_RES_LOSSLESS") {
            anyhow::ensure!(
                matches!(
                    v["audioQuality"].as_str(),
                    Some("LOSSLESS" | "HI_RES_LOSSLESS")
                ),
                "TIDAL only offered {} for this track. No lossy fallback was played. Try another track, or enable lossless sign-in in Settings if using a device session.",
                v["audioQuality"].as_str().unwrap_or("unknown quality")
            );
        }
        let stream = parse_stream(&v)?;
        if matches!(quality, "LOSSLESS" | "HI_RES_LOSSLESS") {
            anyhow::ensure!(
                stream.codec.eq_ignore_ascii_case("flac")
                    || stream.codec.eq_ignore_ascii_case("alac"),
                "TIDAL did not return a lossless codec"
            );
        }
        Ok(stream)
    }
}

pub enum LoginPoll {
    Pending,
    SlowDown,
    Complete,
}
pub enum StreamSource {
    Direct(String),
    Dash(crate::dash::Manifest),
}
pub struct Stream {
    pub source: StreamSource,
    pub quality: String,
    pub codec: String,
    pub sample_rate: Option<u64>,
    pub bit_depth: Option<u64>,
}
impl Stream {
    pub fn label(&self) -> String {
        if self.codec.eq_ignore_ascii_case("flac") {
            let mut label = format!("{} · FLAC", self.quality.replace('_', " "));
            if let Some(rate) = self.sample_rate {
                let bits = self
                    .bit_depth
                    .map(|b| format!("{b} bit / "))
                    .unwrap_or_default();
                label.push_str(&format!(" · {bits}{} kHz", rate as f64 / 1000.));
            }
            label
        } else {
            format!("{} · {}", self.quality, self.codec)
        }
    }
}

pub fn parse_stream(v: &Value) -> Result<Stream> {
    let bytes = STANDARD.decode(v["manifest"].as_str().context("No playback manifest")?)?;
    let quality = v["audioQuality"].as_str().unwrap_or("Unknown").to_owned();
    let sample_rate = v["sampleRate"].as_u64();
    let bit_depth = v["bitDepth"].as_u64();
    if v["manifestMimeType"] == "application/dash+xml" {
        let manifest = crate::dash::parse(std::str::from_utf8(&bytes)?)?;
        let sample_rate = Some(manifest.sample_rate as u64);
        return Ok(Stream {
            source: StreamSource::Dash(manifest),
            quality,
            codec: "FLAC".into(),
            sample_rate,
            bit_depth,
        });
    }
    anyhow::ensure!(
        v["manifestMimeType"] == "application/vnd.tidal.bts",
        "Unsupported playback manifest"
    );
    let manifest: Value = serde_json::from_slice(&bytes)?;
    anyhow::ensure!(
        manifest["encryptionType"].as_str() == Some("NONE"),
        "This track requires DRM. Tidal Forces cannot decrypt protected streams."
    );
    let url = manifest["urls"][0].as_str().context("No stream URL")?;
    let parsed = reqwest::Url::parse(url)?;
    anyhow::ensure!(
        parsed.scheme() == "https" && parsed.host_str().is_some(),
        "Insecure stream URL rejected"
    );
    Ok(Stream {
        source: StreamSource::Direct(url.into()),
        quality,
        sample_rate,
        bit_depth,
        codec: manifest["codecs"]
            .as_str()
            .unwrap_or("Unknown")
            .to_uppercase(),
    })
}

fn modules(v: &Value) -> Result<Vec<&Value>> {
    let rows = v["rows"]
        .as_array()
        .context("TIDAL returned an invalid page")?;
    Ok(rows
        .iter()
        .flat_map(|row| row["modules"].as_array().into_iter().flatten())
        .collect())
}

fn parse_folder(v: &Value) -> Result<Vec<LibraryEntry>> {
    let values = v["items"].as_array().context("Invalid folder response")?;
    values
        .iter()
        .filter(|v| matches!(v["itemType"].as_str(), Some("FOLDER" | "PLAYLIST")))
        .map(|v| {
            let data = &v["data"];
            if v["itemType"] == "FOLDER" {
                Ok(LibraryEntry::Folder {
                    id: data["id"].as_str().context("Folder has no ID")?.into(),
                    name: v["name"].as_str().context("Folder has no name")?.into(),
                    count: data["totalNumberOfItems"].as_u64().unwrap_or(0),
                })
            } else {
                Ok(LibraryEntry::Playlist(serde_json::from_value::<Playlist>(
                    data.clone(),
                )?))
            }
        })
        .collect()
}

fn items<T: serde::de::DeserializeOwned>(v: &Value) -> Result<Vec<T>> {
    let values = v["items"]
        .as_array()
        .context("TIDAL returned an unexpected collection response")?;
    values
        .iter()
        .map(|v| {
            let mut item = v
                .get("item")
                .or_else(|| v.get("playlist"))
                .unwrap_or(v)
                .clone();
            // Page and radio endpoints expose `artists`, while legacy lists use `artist`.
            if item["artist"].is_null() && item["artists"][0].is_object() {
                item["artist"] = item["artists"][0].clone();
            }
            serde_json::from_value(item).map_err(Into::into)
        })
        .collect()
}

async fn check(r: Response) -> Result<Response> {
    match r.status() {
        s if s.is_success() => Ok(r),
        StatusCode::UNAUTHORIZED => bail!("Your TIDAL session expired. Please sign in again."),
        StatusCode::FORBIDDEN => bail!(
            "TIDAL denied access. Check your subscription and this client's playback permissions."
        ),
        StatusCode::TOO_MANY_REQUESTS => {
            bail!("TIDAL rate limit reached. Please wait before retrying.")
        }
        s => bail!("TIDAL request failed (HTTP {s}). Please retry."),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn manifest(encryption: &str, url: &str) -> Value {
        json!({"manifestMimeType":"application/vnd.tidal.bts", "audioQuality":"LOSSLESS", "manifest":STANDARD.encode(json!({"encryptionType":encryption,"urls":[url]}).to_string())})
    }
    #[test]
    fn accepts_clear_https_audio_only() {
        assert_eq!(
            parse_stream(&manifest("NONE", "https://audio.tidal.com/1.flac"))
                .unwrap()
                .quality,
            "LOSSLESS"
        );
        assert!(parse_stream(&manifest("OLD_AES", "https://audio.tidal.com/1")).is_err());
        assert!(parse_stream(&manifest("NONE", "http://audio.tidal.com/1")).is_err());
        assert!(parse_stream(&json!({"manifestMimeType":"application/dash+xml"})).is_err());
        assert!(
            parse_stream(&json!({"manifestMimeType":"application/vnd.tidal.bts","manifest":"???"}))
                .is_err()
        );
    }
    #[test]
    fn preserves_folder_entries_and_normalizes_page_artists() {
        let entries = parse_folder(&json!({"items":[
            {"itemType":"FOLDER","name":"Nested","data":{"id":"folder-1","totalNumberOfItems":2}},
            {"itemType":"PLAYLIST","name":"Mix","data":{"uuid":"playlist-1","title":"Mix","numberOfTracks":5}}
        ]})).unwrap();
        assert!(
            matches!(&entries[0], LibraryEntry::Folder { id, count: 2, .. } if id == "folder-1")
        );
        assert!(matches!(&entries[1], LibraryEntry::Playlist(p) if p.uuid == "playlist-1"));
        let tracks: Vec<Track> = items(
            &json!({"items":[{"id":7,"title":"Discovery","artists":[{"id":8,"name":"Artist"}]}]}),
        )
        .unwrap();
        assert_eq!(tracks[0].artist.id, 8);
        assert_eq!(tracks[0].artist.name, "Artist");
    }

    #[test]
    fn accepts_clear_lossless_dash_with_format_metadata() {
        let xml = r#"<MPD><Period><AdaptationSet><Representation codecs="flac" audioSamplingRate="96000"><SegmentTemplate timescale="96000" initialization="https://audio.tidal.com/0.mp4" media="https://audio.tidal.com/$Number$.mp4"><SegmentTimeline><S d="96000"/></SegmentTimeline></SegmentTemplate></Representation></AdaptationSet></Period></MPD>"#;
        let stream = parse_stream(&json!({"manifestMimeType":"application/dash+xml","manifest":STANDARD.encode(xml),"audioQuality":"HI_RES_LOSSLESS","bitDepth":24,"sampleRate":96000})).unwrap();
        assert!(matches!(stream.source, StreamSource::Dash(_)));
        assert_eq!(stream.label(), "HI RES LOSSLESS · FLAC · 24 bit / 96 kHz");
    }

    #[test]
    fn unwraps_favorites_and_playlists() {
        let p: Vec<Playlist> =
            items(&json!({"items":[{"item":{"uuid":"a","title":"Mix"}}]})).unwrap();
        assert_eq!(p[0].uuid, "a");
        let p: Vec<Playlist> =
            items(&json!({"items":[{"playlist":{"uuid":"b","title":"Mix"}}]})).unwrap();
        assert_eq!(p[0].uuid, "b");
        assert!(items::<Playlist>(&json!({"error":"no"})).is_err());
    }
}
