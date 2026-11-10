//! Explicitly opted-in single-item proof; account media is test-only. No relay.
use super::*;
use serde_json::{Value, json};
#[path = "media_framing.rs"]
mod framing;
use crate::cast_media as media_state;
use media_state::{End, Source};

const RECEIVER: &str = "urn:x-cast:com.google.cast.receiver";
const CONNECTION: &str = "urn:x-cast:com.google.cast.tp.connection";
const HEARTBEAT: &str = "urn:x-cast:com.google.cast.tp.heartbeat";
const MEDIA: &str = "urn:x-cast:com.google.cast.media";
const APP: &str = "CC1AD845";
const LEVEL: f64 = 1.0;
const FIXTURE: &str = "https://raw.githubusercontent.com/from-nibly/tidal-forces/c83b8c2554416044824afd08367c10fdcd50c119/tests/fixtures/tone.flac";
#[derive(Clone, Copy)]
pub(super) enum Fixture {
    Tone,
    InlineDash,
    InlineDashFullVolume,
    #[cfg(test)]
    WholeTrack,
}
pub(super) struct Sample {
    level: f64,
    content_id: String,
    content_type: &'static str,
    polls: usize,
    pub(super) full_duration: Option<Duration>,
}
pub(super) fn sample(fixture: Fixture) -> Sample {
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    match fixture {
        #[cfg(test)]
        Fixture::WholeTrack => current_track_sample(
            include_str!("../../tests/fixtures/cast-inline.mpd"),
            Some(Duration::from_secs(5)),
        )
        .unwrap(),
        Fixture::Tone => Sample {
            level: LEVEL,
            content_id: FIXTURE.into(),
            content_type: "audio/flac",
            polls: 8,
            full_duration: None,
        },
        Fixture::InlineDash | Fixture::InlineDashFullVolume => Sample {
            level: if matches!(fixture, Fixture::InlineDashFullVolume) {
                1.0
            } else {
                0.5
            },
            content_id: format!(
                "data:application/dash+xml;base64,{}",
                STANDARD.encode(include_bytes!("../../tests/fixtures/cast-inline.mpd"))
            ),
            content_type: "application/dash+xml",
            polls: 24,
            full_duration: None,
        },
    }
}
pub(super) fn current_track_sample(
    xml: &str,
    full_duration: Option<Duration>,
) -> ToneResult<Sample> {
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    if xml.is_empty() || xml.len() > 8192 {
        return Err("Bounded account-media manifest exceeds limits");
    }
    if full_duration.is_some_and(|d| d.is_zero() || d > Duration::from_secs(900)) {
        return Err("Whole-track duration exceeds the fifteen-minute test bound");
    }
    Ok(Sample {
        level: 1.0,
        content_id: format!("data:application/dash+xml;base64,{}", STANDARD.encode(xml)),
        content_type: "application/dash+xml",
        polls: full_duration.map_or(24, |d| d.as_secs() as usize + 16),
        full_duration,
    })
}
type ToneResult<T> = std::result::Result<T, &'static str>;

trait Channel {
    fn send(&mut self, dest: &str, ns: &str, body: Value) -> ToneResult<()>;
    fn request(&mut self, dest: &str, ns: &str, body: Value) -> ToneResult<Value>;
    fn request_with_budget(
        &mut self,
        dest: &str,
        ns: &str,
        body: Value,
        _budget: Duration,
    ) -> ToneResult<Value> {
        self.request(dest, ns, body)
    }
    fn bind_media(&mut self, _: &Owned, _: &Sample, _: &Value) {}
    fn terminal(&self) -> Option<End> {
        None
    }
    fn take_finished(&mut self) -> bool {
        false
    }
    fn awaiting_start(&self) -> bool {
        false
    }
    fn report_terminal(&self) {}
    fn pause(&mut self, duration: Duration) {
        std::thread::sleep(duration);
    }
}
fn receiver(c: &mut impl Channel) -> ToneResult<Value> {
    c.request("receiver-0", RECEIVER, json!({"type":"GET_STATUS"}))
}
fn status(v: &Value) -> ToneResult<&Value> {
    if v["type"] != "RECEIVER_STATUS" {
        return Err("Unexpected receiver status");
    }
    v.get("status")
        .filter(|s| s.is_object())
        .ok_or("Missing receiver status")
}
fn apps(v: &Value) -> ToneResult<&[Value]> {
    match status(v)?.get("applications") {
        None => Ok(&[]),
        Some(Value::Array(a)) if a.len() <= 8 => Ok(a),
        _ => Err("Malformed receiver application list"),
    }
}
fn idle(v: &Value) -> ToneResult<bool> {
    Ok(apps(v)?
        .iter()
        .all(|a| a["appId"] == "E8C28D3C" && a["isIdleScreen"] == true))
}
fn volume(v: &Value) -> ToneResult<f64> {
    let s = status(v)?;
    if s["volume"]["muted"].as_bool() != Some(false) {
        return Err("Receiver is muted or mute state is unknown; unchanged");
    }
    s["volume"]["level"]
        .as_f64()
        .filter(|n| (0.0..=1.0).contains(n))
        .ok_or("Invalid receiver volume")
}
fn close_level(a: f64, b: f64) -> bool {
    (a - b).abs() < 0.001
}
fn identifier(v: &Value, key: &str) -> ToneResult<String> {
    v[key]
        .as_str()
        .filter(|s| !s.is_empty() && s.len() <= 128 && s.bytes().all(|b| b.is_ascii_graphic()))
        .map(str::to_owned)
        .ok_or("Invalid receiver session identity")
}
#[derive(Clone)]
struct Owned {
    session: String,
    transport: String,
    media_session: Option<u64>,
}
fn owned(v: &Value, o: &Owned) -> ToneResult<bool> {
    let a = apps(v)?;
    Ok(a.len() == 1
        && a[0]["appId"] == APP
        && a[0]["sessionId"] == o.session
        && a[0]["transportId"] == o.transport)
}
fn media(c: &mut impl Channel, o: &Owned) -> ToneResult<Value> {
    media_with_budget(c, o, Duration::from_secs(2))
}
fn media_with_budget(c: &mut impl Channel, o: &Owned, budget: Duration) -> ToneResult<Value> {
    let mut request = json!({"type":"GET_STATUS"});
    if let Some(id) = o.media_session {
        request["mediaSessionId"] = json!(id);
    }
    c.request_with_budget(&o.transport, MEDIA, request, budget)
}
fn media_rows(v: &Value) -> ToneResult<&[Value]> {
    if v["type"] != "MEDIA_STATUS" {
        return Err("Receiver rejected media command");
    }
    v["status"]
        .as_array()
        .filter(|a| a.len() <= 1)
        .map(Vec::as_slice)
        .ok_or("Invalid media status")
}
fn our_media(v: &Value, sample: &Sample) -> ToneResult<bool> {
    Ok(media_rows(v)?
        .iter()
        .all(|m| m["media"]["contentId"] == sample.content_id))
}
fn cleanup(
    c: &mut impl Channel,
    owner: Option<&Owned>,
    previous: f64,
    sample: &Sample,
) -> ToneResult<()> {
    let mut s = receiver(c)?;
    if let Some(o) = owner
        && owned(&s, o)?
    {
        let current = media(c, o)?;
        if !our_media(&current, sample)?
            || media_rows(&current)?.iter().any(|m| {
                o.media_session.is_none() || m["mediaSessionId"].as_u64() != o.media_session
            })
        {
            return Err("Another media item appeared; not stopped");
        }
        s = c.request(
            "receiver-0",
            RECEIVER,
            json!({"type":"STOP","sessionId":o.session}),
        )?;
    }
    if !idle(&s)? {
        return Err("Another receiver session is active; not modified");
    }
    if close_level(volume(&s)?, previous) {
        return Ok(());
    }
    if !close_level(volume(&s)?, sample.level) {
        return Err("Volume changed externally; not overwritten");
    }
    let restored = c.request(
        "receiver-0",
        RECEIVER,
        json!({"type":"SET_VOLUME","volume":{"level":previous}}),
    )?;
    if !idle(&restored)? || !close_level(volume(&restored)?, previous) {
        return Err("Volume restoration not confirmed");
    }
    Ok(())
}
#[cfg(test)]
fn exercise(c: &mut impl Channel) -> ToneResult<()> {
    exercise_sample(c, &sample(Fixture::Tone))
}
#[cfg(test)]
fn exercise_sample(c: &mut impl Channel, sample: &Sample) -> ToneResult<()> {
    exercise_guarded(c, sample, &mut || Ok(()))
}
fn exercise_guarded(
    c: &mut impl Channel,
    sample: &Sample,
    guard: &mut dyn FnMut() -> ToneResult<()>,
) -> ToneResult<()> {
    guard()?;
    c.send(
        "receiver-0",
        CONNECTION,
        json!({"type":"CONNECT","origin":{}}),
    )?;
    let first = receiver(c)?;
    if !idle(&first)? {
        return Err("Receiver is active; no launch or volume change performed");
    }
    let previous = volume(&first)?;
    println!(
        "Idle, unmuted receiver confirmed; prior volume {:.1}%; requesting approved {}% volume.",
        previous * 100.0,
        sample.level * 100.0
    );
    let mut owner = None;
    let result = (|| {
        let fresh = receiver(c)?;
        if !idle(&fresh)? || !close_level(volume(&fresh)?, previous) {
            return Err("Receiver is no longer idle");
        }
        guard()?;
        let launched = c.request("receiver-0", RECEIVER, json!({"type":"LAUNCH","appId":APP}))?;
        let a = apps(&launched)?;
        if a.len() != 1 || a[0]["appId"] != APP {
            return Err("Expected receiver application did not launch");
        }
        let mut o = Owned {
            session: identifier(&a[0], "sessionId")?,
            transport: identifier(&a[0], "transportId")?,
            media_session: None,
        };
        owner = Some(o.clone());
        c.send(
            &o.transport,
            CONNECTION,
            json!({"type":"CONNECT","origin":{}}),
        )?;
        if !media_rows(&media(c, &o)?)?.is_empty() {
            return Err("Existing media found; not replaced");
        }
        let fresh = receiver(c)?;
        if !owned(&fresh, &o)? || !close_level(volume(&fresh)?, previous) {
            return Err("Receiver ownership or volume changed before playback");
        }
        // Do not raise volume until our empty app is ready; a failed launch must
        // not leave the receiver at the test level.
        guard()?;
        if !close_level(previous, sample.level) {
            let set = c.request(
                "receiver-0",
                RECEIVER,
                json!({"type":"SET_VOLUME","volume":{"level":sample.level}}),
            )?;
            if !close_level(volume(&set)?, sample.level) {
                return Err("Test volume not confirmed");
            }
        }
        let fresh = receiver(c)?;
        if !owned(&fresh, &o)? || !close_level(volume(&fresh)?, sample.level) {
            return Err("Receiver ownership or volume changed after volume request");
        }
        guard()?;
        println!("Owned idle test app confirmed; loading the approved media item once.");
        let loaded = c.request(&o.transport, MEDIA, json!({"type":"LOAD","autoplay":true,"currentTime":0,"media":{"contentId":sample.content_id,"contentType":sample.content_type,"streamType":"BUFFERED","metadata":{"metadataType":3,"title":"Tidal Forces bounded playback test"}}}))?;
        if !our_media(&loaded, sample)? || media_rows(&loaded)?.is_empty() {
            return Err("Synthetic media load not confirmed");
        }
        let session = media_rows(&loaded)?[0]["mediaSessionId"]
            .as_u64()
            .ok_or("Invalid media session")?;
        o.media_session = Some(session);
        owner = Some(o.clone());
        c.bind_media(&o, sample, &loaded);
        let mut progressed = false;
        let mut finished = false;
        let playback_deadline = Instant::now()
            + sample
                .full_duration
                .map_or(Duration::from_secs(6), |d| d + Duration::from_secs(15));
        for _ in 0..sample.polls {
            if Instant::now() >= playback_deadline {
                break;
            }
            c.pause(if sample.full_duration.is_some() {
                Duration::from_secs(1)
            } else {
                Duration::from_millis(250)
            });
            guard()?;
            let s = media(c, &o)?;
            let rows = media_rows(&s)?;
            if rows.iter().any(|row| {
                row["mediaSessionId"].as_u64() != Some(session)
                    || row.get("media").is_some_and(|media| {
                        !media.is_object()
                            || media
                                .get("contentId")
                                .is_some_and(|id| id.as_str() != Some(sample.content_id.as_str()))
                    })
            }) {
                return Err("Media ownership changed; not replaced");
            }
            if let Some(reason) = c.terminal().and_then(End::failure) {
                return Err(reason);
            }
            let terminal_event = c.take_finished();
            finished |= terminal_event;
            progressed |= terminal_event;
            if rows.is_empty() {
                break;
            }
            let row = &rows[0];
            if row["mediaSessionId"].as_u64() != Some(session) {
                return Err("Media session changed");
            }
            if row["playerState"] == "IDLE" && c.awaiting_start() {
                continue;
            }
            if row["playerState"] == "IDLE" && row["idleReason"] != "FINISHED" {
                return Err("Receiver ended synthetic playback without finishing");
            }
            finished |= row["playerState"] == "IDLE" && row["idleReason"] == "FINISHED";
            progressed |= row["currentTime"].as_f64().is_some_and(|t| t > 0.0)
                || (row["playerState"] == "IDLE" && row["idleReason"] == "FINISHED");
            if row["playerState"] == "IDLE" {
                break;
            }
        }
        if !progressed {
            return Err("Receiver did not report playback progress");
        }
        if sample.full_duration.is_some() && !finished {
            return Err("Natural media completion was not confirmed");
        }
        guard()?;
        Ok(())
    })();
    c.report_terminal();
    let cleanup_owner = if c.terminal() == Some(End::OwnershipLost) {
        None
    } else {
        owner.as_ref()
    };
    let restored = cleanup(c, cleanup_owner, previous, sample);
    if let Err(reason) = restored {
        if let Err(playback_reason) = result {
            println!("Playback check failed: {playback_reason}");
        }
        println!("Cleanup check failed: {reason}");
        return Err(
            "Test stopped; cleanup/volume restoration unconfirmed. Check receiver manually",
        );
    }
    println!("Cleanup confirmed: receiver idle and prior volume restored.");
    result
}

fn request_stage(ns: &str, body: &Value) -> &'static str {
    match (ns, body["type"].as_str()) {
        (RECEIVER, Some("GET_STATUS")) => "receiver status",
        (RECEIVER, Some("SET_VOLUME")) => "receiver volume",
        (RECEIVER, Some("LAUNCH")) => "receiver launch",
        (RECEIVER, Some("STOP")) => "receiver stop",
        (MEDIA, Some("GET_STATUS")) => "media status",
        (MEDIA, Some("LOAD")) => "media load",
        _ => "other",
    }
}

fn request_timeout(ns: &str, body: &Value) -> Duration {
    match (ns, body["type"].as_str()) {
        (RECEIVER, Some("LAUNCH")) | (MEDIA, Some("LOAD")) => Duration::from_secs(8),
        _ => Duration::from_secs(2),
    }
}

struct Watch {
    owner: Owned,
    state: media_state::State,
    ignored_terminal_replies: usize,
}
impl Watch {
    fn observe(&mut self, source: &str, namespace: &str, value: &Value, correlated: bool) {
        // A positive nonmatching request ID is a stale reply, not an event.
        if !correlated && value.get("requestId").is_some() && value["requestId"].as_u64() != Some(0)
        {
            if source == self.owner.transport
                && namespace == MEDIA
                && value["type"] == "MEDIA_STATUS"
                && value["status"].as_array().is_some_and(|rows| {
                    rows.iter().any(|row| {
                        row["playerState"] == "IDLE"
                            && row["mediaSessionId"].as_u64() == self.owner.media_session
                    })
                })
            {
                self.ignored_terminal_replies = self.ignored_terminal_replies.saturating_add(1);
            }
            return;
        }
        if source == self.owner.transport && namespace == MEDIA {
            self.state.observe(
                value,
                if correlated {
                    Source::Reply
                } else {
                    Source::Event
                },
            );
        } else if source == "receiver-0"
            && namespace == RECEIVER
            && value["type"] == "RECEIVER_STATUS"
            && value["status"].get("applications").is_some()
            && owned(value, &self.owner) != Ok(true)
        {
            self.state.lose_ownership();
        }
    }
}

struct Live<'a, 'b> {
    stream: &'a mut StreamOwned<ClientConnection, BudgetIo<'b>>,
    deadline: Instant,
    next: u64,
    poisoned: bool,
    frames: framing::Frames,
    watch: Option<Watch>,
}
impl Live<'_, '_> {
    fn receive(&mut self) -> ToneResult<proto::Envelope> {
        let result = (|| {
            let b = match self.frames.read(self.stream) {
                Ok(bytes) => bytes,
                Err(framing::Error::Read(kind)) => {
                    println!("Receiver frame read IO kind: {kind:?}");
                    if matches!(kind, io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock) {
                        return Err("Receiver reply timed out");
                    }
                    return Err("Receiver read failed");
                }
                Err(framing::Error::Bounds) => return Err("Receiver frame limit exceeded"),
            };
            let e =
                proto::Envelope::decode(b.as_slice()).map_err(|_| "Malformed receiver frame")?;
            if e.version != Some(0)
                || e.payload_type != Some(0)
                || e.binary.is_some()
                || e.continued == Some(true)
                || e.remaining.unwrap_or(0) != 0
                || !matches!(e.destination.as_deref(), Some("sender-0" | "*"))
                || e.source
                    .as_ref()
                    .is_none_or(|s| s.len() > 128 || s.is_empty())
            {
                return Err("Unexpected receiver frame");
            }
            Ok(e)
        })();
        if result.is_err() && result != Err("Receiver reply timed out") {
            self.poisoned = true;
        }
        result
    }
}
impl Channel for Live<'_, '_> {
    fn bind_media(&mut self, owner: &Owned, sample: &Sample, loaded: &Value) {
        let mut state = media_state::State::new(
            owner.media_session.expect("confirmed media session"),
            sample.content_id.clone(),
        );
        state.observe(loaded, Source::Load);
        self.watch = Some(Watch {
            owner: owner.clone(),
            state,
            ignored_terminal_replies: 0,
        });
    }
    fn terminal(&self) -> Option<End> {
        self.watch.as_ref().and_then(|w| w.state.end)
    }
    fn awaiting_start(&self) -> bool {
        self.watch
            .as_ref()
            .is_some_and(|w| w.state.awaiting_start && w.state.end.is_none())
    }
    fn take_finished(&mut self) -> bool {
        self.watch.as_mut().is_some_and(|w| w.state.take_finished())
    }
    fn report_terminal(&self) {
        if let Some(w) = &self.watch {
            println!(
                "Media terminal evidence: {:?}; source: {:?}; empty status seen: {}",
                w.state.end, w.state.source, w.state.empty
            );
            println!(
                "Nonmatching terminal replies ignored: {}",
                w.ignored_terminal_replies
            );
            println!(
                "Media transitions (schema enums only): {:?}",
                w.state.trace.iter().flatten().collect::<Vec<_>>()
            );
        }
    }
    fn send(&mut self, dest: &str, ns: &str, body: Value) -> ToneResult<()> {
        if self.poisoned {
            return Err("Receiver channel is unusable");
        }
        let e = proto::Envelope {
            version: Some(0),
            source: Some("sender-0".into()),
            destination: Some(dest.into()),
            namespace: Some(ns.into()),
            payload_type: Some(0),
            text: Some(body.to_string()),
            binary: None,
            continued: None,
            remaining: None,
        }
        .encode_to_vec();
        if e.len() > MAX_MESSAGE {
            return Err("Outgoing frame limit exceeded");
        }
        let result = self
            .stream
            .write_all(&(e.len() as u32).to_be_bytes())
            .and_then(|()| self.stream.write_all(&e))
            .and_then(|()| self.stream.flush());
        if result.is_err() {
            self.poisoned = true;
            return Err("Receiver write failed");
        }
        Ok(())
    }
    fn request(&mut self, dest: &str, ns: &str, body: Value) -> ToneResult<Value> {
        let budget = request_timeout(ns, &body);
        self.request_with_budget(dest, ns, body, budget)
    }
    fn request_with_budget(
        &mut self,
        dest: &str,
        ns: &str,
        mut body: Value,
        budget: Duration,
    ) -> ToneResult<Value> {
        if budget.is_zero() || budget > Duration::from_secs(8) {
            return Err("Invalid receiver reply budget");
        }
        if self.poisoned {
            return Err("Receiver channel is unusable");
        }
        let stage = request_stage(ns, &body);
        self.next += 1;
        let id = self.next;
        body["requestId"] = json!(id);
        self.stream.sock.deadline = self.deadline.min(Instant::now() + budget);
        let result = (|| {
            self.send(dest, ns, body)?;
            for _ in 0..32 {
                let e = self.receive()?;
                let v: Value =
                    serde_json::from_str(e.text.as_deref().ok_or("Missing receiver JSON")?)
                        .map_err(|_| "Malformed receiver JSON")?;
                let correlated = e.namespace.as_deref() == Some(ns)
                    && e.source.as_deref() == Some(dest)
                    && v["requestId"].as_u64() == Some(id);
                if let Some(watch) = &mut self.watch {
                    watch.observe(
                        e.source.as_deref().unwrap_or_default(),
                        e.namespace.as_deref().unwrap_or_default(),
                        &v,
                        correlated,
                    );
                }
                if e.namespace.as_deref() == Some(HEARTBEAT)
                    && e.source.as_deref() == Some("receiver-0")
                    && v["type"] == "PING"
                {
                    self.send("receiver-0", HEARTBEAT, json!({"type":"PONG"}))?;
                } else if correlated {
                    return Ok(v);
                }
            }
            Err("Receiver reply limit exceeded")
        })();
        self.stream.sock.deadline = self.deadline;
        if let Err(reason) = result {
            println!("Failed request: {stage}; {reason}");
        }
        result
    }
}

pub(super) fn inspect_idle(
    stream: &mut StreamOwned<ClientConnection, BudgetIo<'_>>,
) -> ToneResult<bool> {
    let deadline = stream.sock.deadline;
    let mut channel = Live {
        stream,
        deadline,
        next: 0,
        poisoned: false,
        frames: framing::Frames::default(),
        watch: None,
    };
    channel.send(
        "receiver-0",
        CONNECTION,
        json!({"type":"CONNECT","origin":{}}),
    )?;
    idle(&receiver(&mut channel)?)
}

pub(super) fn run_sample(
    stream: &mut StreamOwned<ClientConnection, BudgetIo<'_>>,
    sample: &Sample,
    guard: &mut dyn FnMut() -> ToneResult<()>,
) -> Result<()> {
    let deadline = stream.sock.deadline;
    let mut channel = Live {
        stream,
        deadline,
        next: 0,
        poisoned: false,
        frames: framing::Frames::default(),
        watch: None,
    };
    exercise_guarded(&mut channel, sample, guard).map_err(Failure::Media)?;
    println!(
        "Receiver reported {}; test app stopped and prior volume restored. Audible result needs operator confirmation.",
        if sample.full_duration.is_some() {
            "natural media completion"
        } else {
            "playback progress"
        }
    );
    Ok(())
}

#[path = "session.rs"]
mod session;
pub(super) use session::run as run_runtime;

#[cfg(test)]
#[path = "tone_tests.rs"]
pub(super) mod tests;
