//! Explicitly opted-in preflight and bounded current-track Cast proof.
use crate::{api, credentials, store};
use crate::{cast_auth, cast_source as clip};
use anyhow::{Context, Result, ensure};
use serde_json::Value;
use std::{collections::HashMap, io::Read, path::Path, time::Duration};
use zbus::zvariant::OwnedValue;

fn bounded(path: &Path, max: u64) -> Result<Vec<u8>> {
    let file = std::fs::File::open(path).context("Required local state is unavailable")?;
    let mut data = Vec::new();
    file.take(max + 1)
        .read_to_end(&mut data)
        .context("Could not read local state")?;
    ensure!(data.len() as u64 <= max, "Local state exceeds limit");
    Ok(data)
}
#[derive(PartialEq, Eq)]
struct CredentialRevision {
    metadata: [u8; 32],
    legacy: Option<[u8; 32]>,
}
fn credential_revision(root: &Path) -> Result<CredentialRevision> {
    use sha2::{Digest, Sha256};
    let legacy = root.join("session.json");
    Ok(CredentialRevision {
        metadata: Sha256::digest(bounded(&root.join("credentials.json"), 64 * 1024)?).into(),
        legacy: if legacy
            .try_exists()
            .context("Credential revision unavailable")?
        {
            Some(Sha256::digest(bounded(&legacy, 64 * 1024)?).into())
        } else {
            None
        },
    })
}
#[derive(PartialEq, Eq)]
struct Current {
    id: u64,
    occurrence: u64,
    quality: String,
    title: String,
    artist: String,
    album: String,
    artwork: Option<String>,
}
fn current(v: &Value, user: u64) -> Result<Current> {
    ensure!(
        v["version"] == 1 && v["user"].as_u64() == Some(user) && user != 0,
        "Saved playback account mismatch"
    );
    let entry = &v["queue"]["current"];
    let occurrence = entry["occurrence"]
        .as_u64()
        .context("No current queue occurrence")?;
    ensure!(
        v["progress"]["occurrence"].as_u64() == Some(occurrence),
        "Current occurrence mismatch"
    );
    let text = |field: &Value| -> Result<String> {
        Ok(field
            .as_str()
            .filter(|s| s.len() <= 4096)
            .context("Invalid current track metadata")?
            .to_owned())
    };
    let quality = text(&v["progress"]["quality"])?;
    ensure!(
        ["LOSSLESS", "HIGH", "LOW"].contains(&quality.as_str()),
        "Unknown current quality"
    );
    let track = &entry["track"];
    Ok(Current {
        id: track["id"]
            .as_u64()
            .filter(|id| *id > 0)
            .context("No current track")?,
        occurrence,
        quality,
        title: text(&track["title"])?,
        artist: text(&track["artist"]["name"])?,
        album: text(&track["album"]["title"])?,
        artwork: crate::model::cover_url(track["album"]["cover"].as_str(), 640),
    })
}
async fn confirm_player(p: &zbus::Proxy<'_>, track: &Current) -> Result<()> {
    let state: String = p
        .get_property("PlaybackStatus")
        .await
        .map_err(|_| anyhow::anyhow!("Could not read local playback status"))?;
    ensure!(
        state == "Paused",
        "Pause the local player before this remote test; no local control was sent"
    );
    let mut values: HashMap<String, OwnedValue> = p
        .get_property("Metadata")
        .await
        .map_err(|_| anyhow::anyhow!("Could not read current track metadata"))?;
    let title = String::try_from(values.remove("xesam:title").context("No current title")?)
        .context("Invalid current title")?;
    let artist =
        Vec::<String>::try_from(values.remove("xesam:artist").context("No current artist")?)
            .context("Invalid current artist")?;
    let album = String::try_from(values.remove("xesam:album").context("No current album")?)
        .context("Invalid current album")?;
    ensure!(
        title == track.title && artist == [track.artist.clone()] && album == track.album,
        "Saved track does not match the running player"
    );
    Ok(())
}
async fn player_owner(bus: &zbus::Connection) -> Result<String> {
    let dbus = zbus::fdo::DBusProxy::new(bus).await?;
    dbus.get_name_owner("org.mpris.MediaPlayer2.tidalforces".try_into().unwrap())
        .await
        .map(|owner| owner.to_string())
        .map_err(|_| anyhow::anyhow!("Running player owner unavailable"))
}
async fn preflight(
    target: Option<std::net::Ipv4Addr>,
    whole: bool,
    finish_excerpt: bool,
) -> Result<()> {
    let native = std::env::var("TIDAL_CAST_NATIVE_CONTROL_TEST").as_deref() == Ok("1");
    let until = if native {
        let until = std::env::var("TIDAL_CAST_TEST_UNTIL")?.parse::<u64>()?;
        ensure!(
            store::now() < until && until <= store::now() + 86400,
            "Device-test permission window expired"
        );
        Some(until)
    } else {
        None
    };
    let root = store::config_dir()?;
    let before = credential_revision(&root)?;
    // Reuse the real metadata/digest/keyring loader; never refresh, save or migrate.
    let session = credentials::Store::new()?
        .load()
        .await
        .map_err(|_| anyhow::anyhow!("Credential read failed; no fallback or migration attempted"))?
        .context("Sign in first")?;
    ensure!(
        session.expires_at > store::now() + 60,
        "Saved sign-in has insufficient recorded validity for this probe; no refresh or token mutation attempted"
    );
    let state_path = root
        .join("accounts")
        .join(session.user_id.to_string())
        .join("player-state.json");
    let read_current = || -> Result<Current> {
        let v: Value = serde_json::from_slice(&bounded(&state_path, 32 * 1024 * 1024)?)
            .context("Saved playback state is invalid")?;
        current(&v, session.user_id)
    };
    let track = read_current()?;
    let bus = zbus::Connection::session()
        .await
        .context("Session bus unavailable")?;
    let owner = player_owner(&bus).await?;
    let player: zbus::Proxy<'static> = zbus::ProxyBuilder::new_bare(&bus)
        .destination(owner.clone())?
        .path("/org/mpris/MediaPlayer2")?
        .interface("org.mpris.MediaPlayer2.Player")?
        .cache_properties(zbus::CacheProperties::No)
        .build()
        .await
        .context("Running player unavailable")?;
    confirm_player(&player, &track).await?;
    ensure!(
        credential_revision(&root)? == before
            && read_current()? == track
            && player_owner(&bus).await? == owner,
        "Account/current track/player changed before lookup"
    );
    if std::env::var("TIDAL_CAST_READINESS_ONLY").as_deref() == Ok("1") {
        println!(
            "Local account/player readiness confirmed; no playback lookup or receiver contact."
        );
        return Ok(());
    }
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .no_proxy()
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(10))
        .build()
        .context("Could not construct HTTP client")?;
    let alternate_aac = std::env::var("TIDAL_CAST_PREFLIGHT_AAC").as_deref() == Ok("1");
    let native_quality = std::env::var("TIDAL_CAST_TEST_QUALITY").ok();
    ensure!(
        native || native_quality.is_none(),
        "Quality matrix requires its dedicated native test"
    );
    ensure!(
        native_quality
            .as_ref()
            .is_none_or(|q| ["LOW", "HIGH", "LOSSLESS"].contains(&q.as_str())),
        "Unreviewed quality request"
    );
    let requested_quality = if let Some(quality) = native_quality.as_deref() {
        quality
    } else if alternate_aac {
        "HIGH"
    } else {
        track.quality.as_str()
    };
    if alternate_aac {
        println!(
            "Checking explicitly approved temporary HIGH/AAC quality; player settings unchanged."
        );
    }
    let mut response = client
        .get(format!(
            "https://api.tidal.com/v1/tracks/{}/playbackinfopostpaywall",
            track.id
        ))
        .bearer_auth(&session.access_token)
        .query(&[
            ("countryCode", session.country.as_str()),
            ("playbackmode", "STREAM"),
            ("audioquality", requested_quality),
            ("assetpresentation", "FULL"),
        ])
        .send()
        .await
        .map_err(|_| anyhow::anyhow!("Playback authorization request failed; no retry"))?;
    ensure!(
        response.status().is_success(),
        "Playback authorization rejected (HTTP {}); no refresh or retry",
        response.status().as_u16()
    );
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| anyhow::anyhow!("Playback response read failed"))?
    {
        ensure!(
            bytes.len() + chunk.len() <= 1024 * 1024,
            "Playback response exceeds limit"
        );
        bytes.extend_from_slice(&chunk);
    }
    let value: Value =
        serde_json::from_slice(&bytes).context("Playback response is not valid JSON")?;
    let stream = api::parse_stream(&value).map_err(|_| {
        anyhow::anyhow!("Protected, unsupported or invalid stream; no workaround attempted")
    })?;
    ensure!(
        credential_revision(&root)? == before && read_current()? == track,
        "Account or current track changed during preflight"
    );
    confirm_player(&player, &track).await?;
    ensure!(
        player_owner(&bus).await? == owner,
        "Running player owner changed"
    );
    if std::env::var("TIDAL_CAST_MANIFEST_STRUCTURE_ONLY").as_deref() == Ok("1") {
        ensure!(target.is_none(), "Structural inspection cannot send media");
        let api::StreamSource::Dash(ref manifest) = stream.source else {
            anyhow::bail!("No DASH structure to inspect");
        };
        println!(
            "Reviewed schema names/counts only; no values, URLs, text, IDs or receiver contact:\n{}",
            clip::structure(&manifest.xml)?
        );
        return Ok(());
    }
    if let Some(ip) = target {
        ensure!(
            stream.quality == requested_quality,
            "Returned quality differs from the approved current quality; no downgrade"
        );
        let api::StreamSource::Dash(ref manifest) = stream.source else {
            anyhow::bail!("A finite DASH excerpt is required; no full-track direct-URL fallback");
        };
        println!(
            "Authorized requested quality: {requested_quality}; manifest codec: {}; sample rate: {} Hz",
            manifest.codec.label(),
            manifest.sample_rate
        );
        let excerpt = if whole {
            clip::full(&manifest.xml, manifest)?
        } else {
            clip::build(manifest)?
        };
        let full_duration = (whole || finish_excerpt).then_some(excerpt.duration);
        if whole {
            ensure!(
                session.expires_at > store::now().saturating_add(excerpt.duration.as_secs() + 60),
                "Recorded session validity is insufficient for a whole track; no refresh attempted"
            );
        }
        println!(
            "Authorized clear requested-quality {} at {} Hz; {} {:.3} seconds. No account URLs logged or saved.",
            manifest.codec.label(),
            manifest.sample_rate,
            if native && whole {
                "full manifest for bounded control test"
            } else if whole {
                "one whole track"
            } else {
                "bounded excerpt"
            },
            excerpt.duration.as_secs_f64()
        );
        let metadata = crate::cast_control::Metadata {
            title: track.title.clone(),
            artist: track.artist.clone(),
            album: track.album.clone(),
            artwork: track.artwork.clone(),
        };
        let user = session.user_id;
        let expires = session.expires_at;
        let cancel_path = std::env::var_os("TIDAL_CAST_CANCEL_FILE").map(std::path::PathBuf::from);
        let handle = tokio::runtime::Handle::current();
        tokio::task::spawn_blocking(move || {
            let mut guard = || -> std::result::Result<(), &'static str> {
                handle.block_on(async {
                    tokio::time::timeout(Duration::from_secs(2), async {
                        ensure!(!cancel_path.as_ref().is_some_and(|p| p.exists()), "Operator cancelled the test");
                        ensure!(until.is_none_or(|until|store::now()<until),"Device-test permission window ended");
                        ensure!(expires > store::now() + 15, "Session safety margin expired");
                        ensure!(credential_revision(&root)? == before, "Account credentials changed");
                        let value: Value = serde_json::from_slice(&bounded(&state_path, 32 * 1024 * 1024)?)?;
                        ensure!(current(&value, user)? == track, "Account/current occurrence changed");
                        ensure!(player_owner(&bus).await? == owner, "Player owner changed");
                        confirm_player(&player, &track).await?;
                        Ok::<(), anyhow::Error>(())
                    }).await
                }).map_err(|_| "Local account/player guard timed out")?
                  .map_err(|_| "Local account/current occurrence/paused player guard changed or became unavailable")
            };
            if native { native_controls(ip,excerpt.xml,excerpt.duration,metadata,&mut guard) } else {
                cast_auth::current_track(ip, &excerpt.xml, full_duration, &mut guard)
            }
        }).await.map_err(|_| anyhow::anyhow!("Bounded Cast worker failed; check receiver manually"))?
            .map_err(anyhow::Error::msg)?;
        return Ok(());
    }
    match stream.source {
        api::StreamSource::Direct(_) => println!(
            "Current track has an authorized clear direct HTTPS source. No URL retained or sent to a receiver."
        ),
        api::StreamSource::Dash(_) => println!(
            "Current track returns inline DASH, not a receiver-fetchable manifest URL. No relay or media delivery attempted."
        ),
    }
    Ok(())
}
fn native_controls(
    ip: std::net::Ipv4Addr,
    xml: String,
    duration: Duration,
    metadata: crate::cast_control::Metadata,
    guard: &mut dyn FnMut() -> std::result::Result<(), &'static str>,
) -> std::result::Result<(), &'static str> {
    use crate::cast_control::{Command, End, Event, Media, State};
    let (handle, control) = crate::cast_control::channel();
    let observer = std::thread::spawn(move || {
        let deadline = std::time::Instant::now() + Duration::from_secs(90);
        let mut stage = 0;
        loop {
            if std::time::Instant::now() >= deadline {
                handle.stop();
            }
            if std::time::Instant::now() >= deadline + Duration::from_secs(30) {
                return Err("Native control test cleanup deadline exceeded");
            }
            match handle.next_event() {
                Some(Event::Snapshot(s)) => match stage {
                    0 if s.state == State::Playing && s.position > Duration::ZERO => {
                        println!("Native Cast PLAYING observed; checking pause");
                        handle.command(Command::Pause(true))?;
                        stage = 1;
                    }
                    1 if s.state == State::Paused => {
                        println!(
                            "Native Cast PAUSED confirmed; checking forward seek while paused"
                        );
                        handle.command(Command::Seek {
                            serial: 1,
                            position: duration.mul_f64(0.65),
                        })?;
                        stage = 2;
                    }
                    2 if s.state == State::Paused && s.seek_serial == 1 => {
                        println!(
                            "Paused forward seek confirmed at {:.3}s",
                            s.position.as_secs_f64()
                        );
                        handle.command(Command::Seek {
                            serial: 2,
                            position: Duration::from_secs(1),
                        })?;
                        stage = 3;
                    }
                    3 if s.state == State::Paused && s.seek_serial == 2 => {
                        println!(
                            "Paused backward seek confirmed at {:.3}s",
                            s.position.as_secs_f64()
                        );
                        handle.command(Command::Pause(false))?;
                        stage = 4;
                    }
                    4 if s.state == State::Playing => {
                        println!("Native Cast resume confirmed; checking seek while playing");
                        handle.command(Command::Seek {
                            serial: 3,
                            position: duration.mul_f64(0.5),
                        })?;
                        stage = 5;
                    }
                    5 if s.state == State::Playing && s.seek_serial == 3 => {
                        println!(
                            "Playing seek confirmed at {:.3}s; stopping owned session",
                            s.position.as_secs_f64()
                        );
                        handle.stop();
                        stage = 6;
                    }
                    _ => {}
                },
                Some(Event::Closed { result, released }) => {
                    if !released {
                        return Err("Native Cast release unconfirmed; check receiver manually");
                    }
                    println!("Native worker release confirmed; no volume controls were sent");
                    result?;
                    if stage == 6 && matches!(result, Ok(End::Stopped | End::Finished)) {
                        return Ok(());
                    }
                    return Err("Native control sequence did not complete");
                }
                Some(Event::Phase(phase)) => println!("Native Cast phase: {phase:?}"),
                Some(Event::Volume(level)) => {
                    println!("Receiver starting volume {:.1}%; preserved", level * 100.)
                }
                None => std::thread::sleep(Duration::from_millis(25)),
            }
        }
    });
    let (result, released) = cast_auth::run_runtime(
        std::net::SocketAddrV4::new(ip, 8009),
        true,
        Media {
            metadata,
            xml,
            duration,
            position: Duration::ZERO,
            autoplay: true,
        },
        control,
        guard,
    );
    let observed = observer
        .join()
        .map_err(|_| "Native control observer failed")?;
    if !released {
        return Err("Native Cast release unconfirmed; check receiver manually");
    }
    result?;
    observed
}

#[test]
#[ignore = "real account codec/control validation on explicitly approved receiver; time-limited permission required"]
fn authorized_current_track_native_controls() {
    for name in [
        "TIDAL_CAST_NATIVE_CONTROL_TEST",
        "TIDAL_CAST_CURRENT_TRACK_PLAYBACK",
        "TIDAL_CAST_ALLOW_LEGACY_DEVICE_CERTIFICATES",
    ] {
        assert_eq!(
            std::env::var(name).as_deref(),
            Ok("1"),
            "Explicit native control test permission required"
        );
    }
    for name in [
        "TIDAL_CAST_WHOLE_TRACK",
        "TIDAL_CAST_REQUIRE_COMPLETION",
        "TIDAL_CAST_PREFLIGHT_AAC",
        "TIDAL_CAST_READINESS_ONLY",
        "TIDAL_CAST_MANIFEST_STRUCTURE_ONLY",
    ] {
        assert!(
            std::env::var_os(name).is_none(),
            "Conflicting diagnostic scope"
        );
    }
    let mut input = String::new();
    std::io::stdin()
        .take(65)
        .read_to_string(&mut input)
        .unwrap();
    assert!(input.len() <= 64);
    let target: std::net::Ipv4Addr = input
        .trim()
        .parse()
        .expect("Explicit private target input required");
    assert!(
        target.is_private()
            && !target.is_loopback()
            && !target.is_link_local()
            && !target.is_broadcast()
    );
    let full = std::env::var("TIDAL_CAST_NATIVE_FULL").as_deref() == Ok("1");
    let result =
        tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(preflight(Some(target), full, false));
    if let Err(error) = result {
        panic!("Native Cast validation stopped: {error}");
    }
}

#[test]
#[ignore = "real account read, explicitly approved current-track preflight only; no receiver IO"]
fn authorized_current_track_delivery_preflight() {
    assert_eq!(
        std::env::var("TIDAL_CAST_CURRENT_TRACK_PREFLIGHT").as_deref(),
        Ok("1"),
        "Explicit current-track permission is required"
    );
    let result = tokio::runtime::Runtime::new().unwrap().block_on(async {
        tokio::time::timeout(Duration::from_secs(25), preflight(None, false, false)).await
    });
    match result {
        Ok(Ok(())) => (),
        Ok(Err(error)) => panic!("Current-track preflight stopped: {error}"),
        Err(_) => panic!("Current-track preflight deadline exceeded"),
    }
}
#[test]
#[ignore = "real account lookup and audible receiver effects; separate explicit opt-in required"]
fn authorized_current_track_bounded_playback() {
    run_approved_playback(false, false);
}
#[test]
#[ignore = "whole real track at100%; separate explicit operator approval required"]
fn authorized_current_track_whole_playback() {
    run_approved_playback(true, false);
}
#[test]
#[ignore = "one real account excerpt played to natural completion; separate explicit approval required"]
fn authorized_current_track_completion_playback() {
    run_approved_playback(false, true);
}
fn run_approved_playback(whole: bool, finish_excerpt: bool) {
    assert!(
        std::env::var_os("TIDAL_CAST_NATIVE_CONTROL_TEST").is_none(),
        "Native controls require their dedicated test"
    );
    assert!(!(whole && finish_excerpt));
    if finish_excerpt {
        assert_eq!(
            std::env::var("TIDAL_CAST_REQUIRE_COMPLETION").as_deref(),
            Ok("1"),
            "Explicit natural-completion excerpt scope required"
        );
    } else {
        assert!(
            std::env::var_os("TIDAL_CAST_REQUIRE_COMPLETION").is_none(),
            "Completion-excerpt flag requires its dedicated test"
        );
    }
    assert!(
        std::env::var_os("TIDAL_CAST_MANIFEST_STRUCTURE_ONLY").is_none(),
        "Inspection cannot masquerade as playback"
    );
    if whole {
        assert_eq!(
            std::env::var("TIDAL_CAST_WHOLE_TRACK").as_deref(),
            Ok("1"),
            "Explicit whole-track scope required"
        );
    } else {
        assert!(
            std::env::var_os("TIDAL_CAST_WHOLE_TRACK").is_none(),
            "An excerpt cannot implicitly become whole-track playback"
        );
    }
    for (name, expected) in [
        ("TIDAL_CAST_CURRENT_TRACK_PLAYBACK", "1"),
        ("TIDAL_CAST_VOLUME_PERCENT", "100"),
        ("TIDAL_CAST_ALLOW_LEGACY_DEVICE_CERTIFICATES", "1"),
    ] {
        assert_eq!(
            std::env::var(name).as_deref(),
            Ok(expected),
            "Explicit current-track scope required"
        );
    }
    assert!(
        std::env::var_os("TIDAL_CAST_PREFLIGHT_AAC").is_none(),
        "No quality override authorized"
    );
    assert!(
        std::env::var_os("TIDAL_CAST_READINESS_ONLY").is_none(),
        "Playback cannot use readiness-only mode"
    );
    let mut input = String::new();
    std::io::stdin()
        .take(65)
        .read_to_string(&mut input)
        .expect("Private target input required");
    assert!(input.len() <= 64, "Target input exceeds bound");
    let target: std::net::Ipv4Addr = input
        .trim()
        .parse()
        .expect("One explicitly approved IPv4 target required");
    assert!(
        target.is_private()
            && !target.is_loopback()
            && !target.is_link_local()
            && !target.is_broadcast()
    );
    let runtime = tokio::runtime::Runtime::new().unwrap();
    // The blocking transport owns its deadline and cleanup; do not detach it with
    // an outer async timeout once receiver effects have begun.
    if let Err(error) = runtime.block_on(preflight(Some(target), whole, finish_excerpt)) {
        panic!("Bounded current-track test stopped: {error}");
    }
}

#[test]
fn current_track_requires_matching_account_and_occurrence() {
    let mut v = serde_json::json!({"version":1,"user":7,"queue":{"current":{"occurrence":12,"track":{"id":99,"title":"Synthetic","artist":{"name":"Test"},"album":{"title":"Fixture"}}}},"progress":{"occurrence":12,"quality":"LOSSLESS"}});
    assert!(current(&v, 7).is_ok());
    assert!(current(&v, 8).is_err());
    v["progress"]["occurrence"] = 13.into();
    assert!(current(&v, 7).is_err());
}
