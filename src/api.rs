use crate::{
    model::{Album, Playlist, Track},
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
const API: &str = "https://api.tidal.com/v1";

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
        self.accept_token(v)?;
        self.identify().await?;
        Ok(LoginPoll::Complete)
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
            country: v["user"]["countryCode"]
                .as_str()
                .unwrap_or(&old.country)
                .into(),
        });
        store::save(self.session.as_ref().unwrap())
    }

    async fn refresh(&mut self) -> Result<()> {
        let session = self.session.as_ref().context("Sign in to TIDAL first")?;
        let r = self
            .client
            .post(format!("{AUTH}/token"))
            .form(&[
                ("client_id", self.client_id.as_str()),
                ("client_secret", self.client_secret.as_str()),
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
                .get(format!("{API}/{path}"))
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
                    ("types", "TRACKS,ALBUMS"),
                    ("limit", "50"),
                ],
            )
            .await?;
        Ok(Search {
            tracks: items(&v["tracks"])?,
            albums: items(&v["albums"])?,
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

    pub async fn playlists(&mut self) -> Result<Vec<Playlist>> {
        let id = self.session.as_ref().context("Sign in first")?.user_id;
        let mut playlists = Vec::new();
        // This endpoint rejects limits over 50; fetch two pages for the sidebar.
        for offset in [0, 50] {
            let v = self
                .get(
                    &format!("users/{id}/playlistsAndFavoritePlaylists"),
                    &[("limit", "50"), ("offset", &offset.to_string())],
                )
                .await?;
            let page: Vec<Playlist> = items(&v)?;
            let done = page.len() < 50;
            playlists.extend(page);
            if done {
                break;
            }
        }
        Ok(playlists)
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
        parse_stream(&v)
    }
}

pub enum LoginPoll {
    Pending,
    SlowDown,
    Complete,
}
pub struct Stream {
    pub url: String,
    pub quality: String,
}

pub fn parse_stream(v: &Value) -> Result<Stream> {
    anyhow::ensure!(
        v["manifestMimeType"].as_str() == Some("application/vnd.tidal.bts"),
        "This track uses an unsupported DASH/DRM manifest. Try High quality; encrypted streams are not supported."
    );
    let bytes = STANDARD.decode(v["manifest"].as_str().context("No playback manifest")?)?;
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
        url: url.into(),
        quality: v["audioQuality"].as_str().unwrap_or("Unknown").into(),
    })
}

fn items<T: serde::de::DeserializeOwned>(v: &Value) -> Result<Vec<T>> {
    let values = v["items"]
        .as_array()
        .context("TIDAL returned an unexpected collection response")?;
    values
        .iter()
        .map(|v| {
            serde_json::from_value(
                v.get("item")
                    .or_else(|| v.get("playlist"))
                    .unwrap_or(v)
                    .clone(),
            )
            .map_err(Into::into)
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
