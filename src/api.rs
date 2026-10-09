use crate::{
    library::{Favorite, FavoriteKind},
    model::{Album, Artist, Home, LibraryEntry, Mix, Playlist, PlaylistPage, RadioSeed, Track},
    queue::Continuation,
    store::{self, Session},
};
use anyhow::{Context, Result, bail};
use base64::{Engine, engine::general_purpose::STANDARD};
use reqwest::{Client, Method, Response, StatusCode};
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
    base: String,
    pub session: Option<Session>,
    pub credentials: crate::credentials::Store,
    pub credential_warning: Option<String>,
    pub credentials_dirty: bool,
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

pub struct LibraryPage {
    pub data: Search,
    pub next_offset: usize,
    pub more: bool,
    pub total: Option<u64>,
}

#[derive(Debug)]
pub struct ContextPage {
    pub tracks: Vec<Track>,
    pub continuation: Option<Continuation>,
}

impl Api {
    pub fn new() -> Result<Self> {
        Ok(Self {
            client: Client::builder()
                .connect_timeout(Duration::from_secs(10))
                .timeout(Duration::from_secs(30))
                .user_agent(concat!("TidalForces/", env!("CARGO_PKG_VERSION")))
                .build()?,
            base: "https://api.tidal.com".into(),
            session: None,
            credentials: crate::credentials::Store::new()?,
            credential_warning: None,
            credentials_dirty: false,
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
        let old = self.session.clone();
        if let Some(session) = self.session.as_mut() {
            session.pkce = false;
        }
        if let Err(error) = self.accept_token(v).await {
            self.session = old;
            return Err(error);
        }
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
        if let Err(e) = self.accept_token(v).await {
            self.session = old;
            return Err(e);
        }
        self.identify().await
    }

    async fn accept_token(&mut self, v: Value) -> Result<()> {
        let old = self.session.clone().unwrap_or_default();
        self.session = Some(Session {
            access_token: v["access_token"]
                .as_str()
                .filter(|token| !token.is_empty())
                .context("TIDAL returned no access token")?
                .into(),
            refresh_token: v["refresh_token"]
                .as_str()
                .unwrap_or(&old.refresh_token)
                .into(),
            expires_at: store::now().saturating_add(v["expires_in"].as_u64().unwrap_or(300)),
            user_id: v["user"]["userId"].as_u64().unwrap_or(old.user_id),
            pkce: old.pkce,
            country: v["user"]["countryCode"]
                .as_str()
                .unwrap_or(&old.country)
                .into(),
        });
        self.persist_session().await;
        Ok(())
    }

    pub async fn persist_session(&mut self) {
        if let Some(session) = &self.session {
            self.credential_warning = self.credentials.save(session).await.err().map(|error| format!("Fresh sign-in credentials are still in memory, but storage could not be confirmed: {error}. Keep this app open, unlock/fix credential storage and retry saving. No fallback to another storage mode was used."));
            self.credentials_dirty = self.credential_warning.is_some();
        }
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
        self.accept_token(value).await
    }

    pub async fn identify(&mut self) -> Result<()> {
        let v = self.get("sessions", &[]).await?;
        let session = self.session.as_mut().context("No session")?;
        session.user_id = v["userId"].as_u64().context("TIDAL returned no user ID")?;
        session.country = v["countryCode"]
            .as_str()
            .context("TIDAL returned no country")?
            .into();
        self.persist_session().await;
        Ok(())
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
        Ok(self
            .request(Method::GET, version, path, params, &[], None)
            .await?
            .json()
            .await?)
    }

    async fn request(
        &mut self,
        method: Method,
        version: &str,
        path: &str,
        params: &[(&str, &str)],
        form: &[(&str, &str)],
        revision: Option<&str>,
    ) -> Result<Response> {
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
            let mut request = self
                .client
                .request(method.clone(), format!("{}/{version}/{path}", self.base))
                .bearer_auth(&s.access_token)
                .query(&[("countryCode", s.country.as_str())])
                .query(params);
            if !form.is_empty() {
                request = request.form(form);
            }
            // TIDAL's playlist API uses If-None-Match as its revision guard.
            if let Some(etag) = revision {
                request = request.header("If-None-Match", etag);
            }
            let r = request.send().await.map_err(|e| {
                if method == Method::GET { anyhow::Error::from(e) }
                else { anyhow::anyhow!("Could not confirm the change with TIDAL. It may have completed; refresh before retrying.") }
            })?;
            if r.status() == StatusCode::UNAUTHORIZED && attempt == 0 {
                self.refresh().await?;
                continue;
            }
            if r.status() == StatusCode::PRECONDITION_FAILED || r.status() == StatusCode::CONFLICT {
                bail!("The playlist changed elsewhere. Refresh it before editing again.");
            }
            if method != Method::GET
                && (r.status().is_server_error() || r.status() == StatusCode::REQUEST_TIMEOUT)
            {
                bail!(
                    "Could not confirm the change with TIDAL. It may have completed; refresh before retrying."
                );
            }
            return check(r).await;
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

    pub async fn library(&mut self, kind: FavoriteKind, offset: usize) -> Result<LibraryPage> {
        let user = self.session.as_ref().context("Sign in first")?.user_id;
        let value = self
            .get(
                &format!("users/{user}/favorites/{}", kind.path()),
                &[("limit", "100"), ("offset", &offset.to_string())],
            )
            .await?;
        parse_library_page(value, kind, offset)
    }

    pub async fn set_favorite(&mut self, user: u64, item: Favorite, saved: bool) -> Result<()> {
        anyhow::ensure!(item.id != 0, "This item has no valid TIDAL ID");
        anyhow::ensure!(
            self.session.as_ref().is_some_and(|s| s.user_id == user),
            "The account changed. Refresh your library before editing favorites."
        );
        let base = format!("users/{user}/favorites/{}", item.kind.path());
        let id = item.id.to_string();
        if saved {
            self.request(
                Method::POST,
                "v1",
                &base,
                &[],
                &[(item.kind.form_key(), &id)],
                None,
            )
            .await?;
        } else {
            self.request(
                Method::DELETE,
                "v1",
                &format!("{base}/{id}"),
                &[],
                &[],
                None,
            )
            .await?;
        }
        Ok(())
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
        validate_collection(kind, id)?;
        let v = self
            .get(
                &format!("{kind}/{id}/tracks"),
                &[("limit", "100"), ("offset", &offset.to_string())],
            )
            .await?;
        items(&v)
    }

    pub async fn context_page(&mut self, user: u64, source: &Continuation) -> Result<ContextPage> {
        anyhow::ensure!(
            self.session
                .as_ref()
                .is_some_and(|session| session.user_id == user),
            "The playback account changed"
        );
        let offset = match source {
            Continuation::Playlist { offset, .. }
            | Continuation::Album { offset, .. }
            | Continuation::Favorites { offset, .. } => *offset,
        };
        anyhow::ensure!(
            offset <= 1_000_000,
            "This playback source exceeds the pagination limit"
        );
        match source {
            Continuation::Playlist { id, etag, offset } => {
                let page = self.playlist_page(id, *offset, Some(etag)).await?;
                Ok(ContextPage {
                    tracks: page.rows.into_iter().map(|(_, track)| track).collect(),
                    continuation: page.more.then(|| Continuation::Playlist {
                        id: id.clone(),
                        etag: page.etag,
                        offset: page.next_offset,
                    }),
                })
            }
            Continuation::Album { id, offset } => {
                let value = self
                    .get(
                        &format!("albums/{id}/tracks"),
                        &[("limit", "100"), ("offset", &offset.to_string())],
                    )
                    .await?;
                let page = parse_library_page(value, FavoriteKind::Tracks, *offset)?;
                Ok(ContextPage {
                    tracks: page.data.tracks,
                    continuation: page.more.then_some(Continuation::Album {
                        id: *id,
                        offset: page.next_offset,
                    }),
                })
            }
            Continuation::Favorites { offset, total } => {
                let page = self.library(FavoriteKind::Tracks, *offset).await?;
                anyhow::ensure!(
                    total.is_none() || page.total == *total,
                    "Favorites changed while loading the queue. Refresh the library and play it again."
                );
                Ok(ContextPage {
                    tracks: page.data.tracks,
                    continuation: page.more.then_some(Continuation::Favorites {
                        offset: page.next_offset,
                        total: page.total,
                    }),
                })
            }
        }
    }

    pub async fn track(&mut self, id: u64) -> Result<Track> {
        Ok(serde_json::from_value(
            self.get(&format!("tracks/{id}"), &[]).await?,
        )?)
    }

    pub async fn artist(&mut self, id: u64) -> Result<Search> {
        let artist: Artist =
            serde_json::from_value(self.get(&format!("artists/{id}"), &[]).await?)?;
        let tracks = items(
            &self
                .get(&format!("artists/{id}/toptracks"), &[("limit", "100")])
                .await?,
        )?;
        let albums = items(
            &self
                .get(&format!("artists/{id}/albums"), &[("limit", "50")])
                .await?,
        )?;
        Ok(Search {
            tracks,
            albums,
            artists: vec![artist],
        })
    }

    pub async fn collection_title(&mut self, kind: &str, id: &str) -> Result<String> {
        validate_collection(kind, id)?;
        Ok(self.get(&format!("{kind}/{id}"), &[]).await?["title"]
            .as_str()
            .unwrap_or("TIDAL collection")
            .to_owned())
    }

    async fn playlist_metadata(&mut self, id: &str) -> Result<(Playlist, String)> {
        validate_collection("playlists", id)?;
        let response = self
            .request(
                Method::GET,
                "v1",
                &format!("playlists/{id}"),
                &[],
                &[],
                None,
            )
            .await?;
        let etag = response
            .headers()
            .get("etag")
            .context("TIDAL did not supply a playlist revision")?
            .to_str()?
            .to_owned();
        anyhow::ensure!(
            !etag.trim().is_empty(),
            "TIDAL did not supply a valid playlist revision"
        );
        Ok((response.json().await?, etag))
    }

    pub async fn playlist_page(
        &mut self,
        id: &str,
        offset: usize,
        expected: Option<&str>,
    ) -> Result<PlaylistPage> {
        let (playlist, etag) = self.playlist_metadata(id).await?;
        if let Some(expected) = expected {
            ensure_revision(expected, &etag)?;
        }
        let response = self
            .request(
                Method::GET,
                "v1",
                &format!("playlists/{id}/items"),
                &[("limit", "100"), ("offset", &offset.to_string())],
                &[],
                None,
            )
            .await?;
        ensure_revision(
            &etag,
            response
                .headers()
                .get("etag")
                .context("Missing playlist item revision")?
                .to_str()?,
        )?;
        let value: Value = response.json().await?;
        let raw = value["items"]
            .as_array()
            .context("Missing playlist items")?;
        let mut rows = Vec::new();
        for (index, item) in raw.iter().enumerate() {
            if item["type"]
                .as_str()
                .is_some_and(|t| t.eq_ignore_ascii_case("track"))
                && !item["item"].is_null()
            {
                rows.push((
                    offset + index,
                    serde_json::from_value(item["item"].clone())?,
                ));
            }
        }
        let next_offset = offset + raw.len();
        let total = value["totalNumberOfItems"]
            .as_u64()
            .context("Missing playlist item count")? as usize;
        anyhow::ensure!(
            next_offset >= total || !raw.is_empty(),
            "Incomplete playlist page"
        );
        let editable = self.owns(&playlist);
        Ok(PlaylistPage {
            playlist,
            etag,
            editable,
            rows,
            next_offset,
            more: next_offset < total,
        })
    }

    fn owns(&self, playlist: &Playlist) -> bool {
        self.session.as_ref().is_some_and(|s| {
            s.user_id != 0
                && playlist.kind == "USER"
                && playlist
                    .creator
                    .as_ref()
                    .is_some_and(|creator| creator.id == s.user_id)
        })
    }

    pub async fn owned_playlists(&mut self) -> Result<Vec<Playlist>> {
        let user = self.session.as_ref().context("Sign in first")?.user_id;
        let mut playlists = Vec::new();
        loop {
            let value = self
                .get(
                    &format!("users/{user}/playlists"),
                    &[("limit", "50"), ("offset", &playlists.len().to_string())],
                )
                .await?;
            let page: Vec<Playlist> = items(&value)?;
            let count = page.len();
            playlists.extend(page);
            if playlists.len()
                >= value["totalNumberOfItems"]
                    .as_u64()
                    .context("Missing playlist count")? as usize
            {
                break;
            }
            anyhow::ensure!(count > 0, "Incomplete playlist listing");
        }
        playlists.retain(|p| self.owns(p));
        playlists.sort_by_key(|p| p.title.to_lowercase());
        Ok(playlists)
    }

    pub async fn create_playlist(&mut self, title: &str, description: &str) -> Result<Playlist> {
        anyhow::ensure!(
            !title.trim().is_empty() && title.chars().count() <= 200,
            "Use a playlist name between 1 and 200 characters"
        );
        anyhow::ensure!(
            description.chars().count() <= 1000,
            "Description is too long"
        );
        let response = self
            .request(
                Method::PUT,
                "v2",
                "my-collection/playlists/folders/create-playlist",
                &[
                    ("name", title.trim()),
                    ("description", description),
                    ("folderId", "root"),
                ],
                &[],
                None,
            )
            .await?;
        let value: Value = response.json().await?;
        Ok(serde_json::from_value(value["data"].clone())?)
    }

    pub async fn create_queue_playlist(
        &mut self,
        user: u64,
        title: &str,
        description: &str,
        cancelled: &std::sync::atomic::AtomicBool,
    ) -> Result<Playlist> {
        anyhow::ensure!(
            !cancelled.load(std::sync::atomic::Ordering::Acquire),
            "Queue export was cancelled"
        );
        anyhow::ensure!(
            user != 0
                && self
                    .session
                    .as_ref()
                    .is_some_and(|session| session.user_id == user),
            "Account changed; queue export was stopped"
        );
        self.create_playlist(title, description).await
    }

    pub async fn playlist_duplicate_page(
        &mut self,
        user: u64,
        id: &str,
        offset: usize,
        count: usize,
        expected: &str,
        cancelled: &std::sync::atomic::AtomicBool,
    ) -> Result<Vec<u64>> {
        use std::sync::atomic::Ordering;
        anyhow::ensure!(
            !expected.trim().is_empty() && count <= crate::queue::MAX_ENTRIES && offset <= count,
            "Cannot check these songs for duplicates"
        );
        anyhow::ensure!(
            !cancelled.load(Ordering::Acquire),
            "Duplicate check cancelled"
        );
        anyhow::ensure!(
            user != 0
                && self
                    .session
                    .as_ref()
                    .is_some_and(|session| session.user_id == user),
            "Account changed; paste again"
        );
        let page = self.playlist_page(id, offset, Some(expected)).await?;
        anyhow::ensure!(
            page.editable && page.playlist.uuid == id && page.playlist.number_of_videos == 0,
            "This playlist cannot be edited"
        );
        anyhow::ensure!(
            page.playlist.number_of_tracks == count as u64
                && page.next_offset <= count
                && page.more == (page.next_offset < count),
            "The playlist changed. Refresh it and paste again."
        );
        anyhow::ensure!(
            page.rows.len() == page.next_offset - offset
                && page.rows.iter().all(|(_, track)| track.id != 0),
            "Some songs could not be checked for duplicates. Nothing has been added."
        );
        anyhow::ensure!(
            !page.more || page.next_offset > offset,
            "Could not finish checking this playlist"
        );
        anyhow::ensure!(
            !cancelled.load(Ordering::Acquire),
            "Duplicate check cancelled"
        );
        Ok(page.rows.into_iter().map(|(_, track)| track.id).collect())
    }

    /// Shared verified append path for queue export and clipboard playlist paste.
    pub async fn append_queue_batch(
        &mut self,
        user: u64,
        id: &str,
        tracks: &[u64],
        offset: usize,
        expected: Option<&str>,
        cancelled: &std::sync::atomic::AtomicBool,
    ) -> Result<String> {
        use std::sync::atomic::Ordering;
        anyhow::ensure!(
            !cancelled.load(Ordering::Acquire),
            "Playlist append was cancelled"
        );
        anyhow::ensure!(
            user != 0
                && self
                    .session
                    .as_ref()
                    .is_some_and(|session| session.user_id == user),
            "Account changed; playlist append was stopped"
        );
        anyhow::ensure!(
            !tracks.is_empty()
                && tracks.len() <= 100
                && tracks.iter().all(|id| *id != 0)
                && offset
                    .checked_add(tracks.len())
                    .is_some_and(|end| end <= 50_000)
                && (offset == 0 || expected.is_some()),
            "Invalid playlist append batch"
        );
        let (playlist, etag) = self.playlist_metadata(id).await?;
        anyhow::ensure!(
            self.owns(&playlist),
            "Only your own playlists can be edited"
        );
        if let Some(expected) = expected {
            ensure_revision(expected, &etag)?;
        }
        anyhow::ensure!(
            playlist.number_of_tracks == offset as u64 && playlist.number_of_videos == 0,
            "The destination playlist changed; inspect it before saving again"
        );
        anyhow::ensure!(
            !cancelled.load(Ordering::Acquire),
            "Playlist append was cancelled before the next write"
        );
        let track_ids = tracks
            .iter()
            .map(u64::to_string)
            .collect::<Vec<_>>()
            .join(",");
        let response = self
            .request(
                Method::POST,
                "v1",
                &format!("playlists/{id}/items"),
                &[],
                &[
                    ("trackIds", &track_ids),
                    ("toIndex", &offset.to_string()),
                    ("onDupes", "ADD"),
                    ("onArtifactNotFound", "FAIL"),
                ],
                Some(&etag),
            )
            .await?;
        let written_revision = response.headers().get("etag").context("TIDAL did not return a write revision. The playlist may contain this batch; inspect it before retrying")?.to_str()?.to_owned();
        anyhow::ensure!(
            !written_revision.trim().is_empty(),
            "Write revision is empty. The playlist may contain this batch; inspect it before retrying"
        );
        // Pin read-back to the write revision, not an intervening external edit.
        let page = self
            .playlist_page(id, offset, Some(&written_revision))
            .await?;
        anyhow::ensure!(
            page.editable
                && page.playlist.number_of_tracks == (offset + tracks.len()) as u64
                && page.playlist.number_of_videos == 0
                && !page.more
                && page.next_offset == offset + tracks.len()
                && page.rows.len() == tracks.len()
                && page
                    .rows
                    .iter()
                    .zip(tracks)
                    .enumerate()
                    .all(|(i, ((position, track), wanted))| *position == offset + i
                        && track.id == *wanted),
            "Could not verify the appended batch. Some tracks may have been added; inspect the playlist before retrying"
        );
        Ok(page.etag)
    }

    pub async fn add_to_playlist(&mut self, id: &str, track: u64) -> Result<bool> {
        let (playlist, etag) = self.playlist_metadata(id).await?;
        anyhow::ensure!(
            self.owns(&playlist),
            "Only your own playlists can be edited"
        );
        let response = self
            .request(
                Method::POST,
                "v1",
                &format!("playlists/{id}/items"),
                &[],
                &[
                    ("trackIds", &track.to_string()),
                    (
                        "toIndex",
                        &(playlist.number_of_tracks + playlist.number_of_videos).to_string(),
                    ),
                    ("onDupes", "SKIP"),
                    ("onArtifactNotFound", "FAIL"),
                ],
                Some(&etag),
            )
            .await?;
        let value: Value = response.json().await?;
        Ok(value["addedItemIds"]
            .as_array()
            .context("TIDAL did not confirm the playlist edit")?
            .iter()
            .any(|v| v.as_u64() == Some(track) || v.as_str() == Some(&track.to_string())))
    }

    pub async fn remove_from_playlist(
        &mut self,
        id: &str,
        index: usize,
        track: u64,
        expected: &str,
    ) -> Result<()> {
        let page = self.playlist_page(id, index, Some(expected)).await?;
        anyhow::ensure!(page.editable, "Only your own playlists can be edited");
        anyhow::ensure!(
            page.rows
                .first()
                .is_some_and(|(i, t)| *i == index && t.id == track),
            "Playlist item changed. Refresh before removing it."
        );
        self.request(
            Method::DELETE,
            "v1",
            &format!("playlists/{id}/items/{index}"),
            &[],
            &[],
            Some(expected),
        )
        .await?;
        Ok(())
    }

    #[cfg(test)]
    async fn delete_test_playlist(&mut self, id: &str) -> Result<()> {
        self.request(
            Method::DELETE,
            "v1",
            &format!("playlists/{id}"),
            &[],
            &[],
            None,
        )
        .await?;
        Ok(())
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
        // TIDAL may return a lower-quality asset when lossless isn't available.
        // Decode that authorized stream and label its actual quality, not the request.
        parse_stream(&v)
    }
}

#[cfg(test)]
#[path = "context_tests.rs"]
mod context_tests;
#[cfg(test)]
#[path = "export_tests.rs"]
mod export_tests;
#[cfg(test)]
#[path = "favorite_tests.rs"]
mod favorite_tests;
#[cfg(test)]
#[path = "playlist_tests.rs"]
mod playlist_tests;

fn validate_collection(kind: &str, id: &str) -> Result<()> {
    anyhow::ensure!(
        matches!(kind, "albums" | "playlists")
            && !id.is_empty()
            && id.len() <= 100
            && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'),
        "Invalid collection ID"
    );
    Ok(())
}

fn ensure_revision(expected: &str, actual: &str) -> Result<()> {
    anyhow::ensure!(
        !expected.is_empty() && expected == actual,
        "The playlist changed elsewhere. Refresh it before editing again."
    );
    Ok(())
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
        let codec = manifest.codec.label().to_owned();
        return Ok(Stream {
            source: StreamSource::Dash(manifest),
            quality,
            codec,
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

fn parse_library_page(mut value: Value, kind: FavoriteKind, offset: usize) -> Result<LibraryPage> {
    let total = value["totalNumberOfItems"].as_u64();
    let rows = value["items"]
        .as_array_mut()
        .context("Invalid library page")?;
    let raw_count = rows.len();
    let next_offset = offset
        .checked_add(raw_count)
        .context("Invalid library offset")?;
    let more = total.map_or(raw_count == 100, |total| (next_offset as u64) < total);
    anyhow::ensure!(
        !more || raw_count != 0,
        "TIDAL returned an empty unfinished library page. Refresh to retry."
    );
    // Unavailable entries consume an API position even though they cannot render.
    rows.retain(|row| !row.is_null() && !row.get("item").is_some_and(Value::is_null));
    let mut data = Search::default();
    match kind {
        FavoriteKind::Tracks => data.tracks = items(&value)?,
        FavoriteKind::Albums => data.albums = items(&value)?,
        FavoriteKind::Artists => data.artists = items(&value)?,
    }
    Ok(LibraryPage {
        data,
        next_offset,
        more,
        total,
    })
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
    fn accepts_lossy_fallback_and_labels_the_returned_quality() {
        for (codec, label) in [
            ("mp4a.40.2", "AAC-LC"),
            ("mp4a.40.5", "HE-AAC"),
            ("mp4a.40.29", "HE-AAC v2"),
        ] {
            let xml = format!(
                r#"<MPD><Period><AdaptationSet><Representation codecs="{codec}" audioSamplingRate="44100"><SegmentTemplate timescale="44100" initialization="https://audio.tidal.com/0.mp4" media="https://audio.tidal.com/$Number$.mp4"><SegmentTimeline><S d="44100"/></SegmentTimeline></SegmentTemplate></Representation></AdaptationSet></Period></MPD>"#
            );
            let value = json!({"manifestMimeType":"application/dash+xml","manifest":STANDARD.encode(&xml),"audioQuality":"LOW"});
            let stream = parse_stream(&value).unwrap();
            assert!(matches!(stream.source, StreamSource::Dash(_)));
            assert_eq!(stream.label(), format!("LOW · {label}"));
            let protected = xml.replace("<AdaptationSet>", "<AdaptationSet><ContentProtection/>");
            assert!(parse_stream(&json!({"manifestMimeType":"application/dash+xml","manifest":STANDARD.encode(protected),"audioQuality":"LOW"})).is_err());
        }
    }

    #[tokio::test]
    async fn failed_token_persistence_keeps_fresh_tokens_in_memory_and_old_copy_untouched() {
        let root = tempfile::tempdir().unwrap();
        let mut api = Api::new().unwrap();
        api.credentials = crate::credentials::Store::at(root.path());
        api.session = Some(Session {
            access_token: "synthetic-old".into(),
            refresh_token: "old-refresh".into(),
            ..Session::default()
        });
        api.persist_session().await;
        assert!(!api.credentials_dirty);
        let previous = std::fs::read(root.path().join("session.json")).unwrap();
        std::fs::write(root.path().join("credentials.json"), b"invalid settings").unwrap();
        api.accept_token(
            json!({"access_token":"synthetic-new","refresh_token":"new-refresh","expires_in":3600}),
        )
        .await
        .unwrap();
        assert_eq!(api.session.as_ref().unwrap().access_token, "synthetic-new");
        assert_eq!(api.session.as_ref().unwrap().refresh_token, "new-refresh");
        assert!(api.credentials_dirty && api.credential_warning.is_some());
        assert_eq!(
            std::fs::read(root.path().join("session.json")).unwrap(),
            previous
        );
        std::fs::remove_file(root.path().join("credentials.json")).unwrap();
        api.persist_session().await;
        assert!(!api.credentials_dirty && api.credential_warning.is_none());
        assert_eq!(
            api.credentials.load().await.unwrap().unwrap().access_token,
            "synthetic-new"
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
