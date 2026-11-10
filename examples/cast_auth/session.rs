//! Single native media session. Queue ownership stays in the application.
use super::*;
#[cfg(test)]
#[path = "session_tests.rs"]
mod tests;
use crate::cast_control::{
    Command, Control, End as Outcome, Event, Media as Item, Snapshot, State,
};

fn snapshot(
    value: &Value,
    owner: &Owned,
    sample: &Sample,
    seek_serial: u64,
) -> ToneResult<Option<Snapshot>> {
    let rows = media_rows(value)?;
    if rows.is_empty() {
        return Ok(None);
    }
    if rows.len() != 1 {
        return Err("Ambiguous receiver media state");
    }
    let row = &rows[0];
    if row["mediaSessionId"].as_u64() != owner.media_session
        || row.get("media").is_some_and(|m| {
            !m.is_object()
                || m.get("contentId")
                    .is_some_and(|id| id.as_str() != Some(&sample.content_id))
        })
    {
        return Err("Receiver media ownership changed");
    }
    let state = match row["playerState"].as_str() {
        Some("PLAYING") => State::Playing,
        Some("PAUSED") => State::Paused,
        Some("BUFFERING" | "IDLE") => State::Buffering,
        _ => return Err("Unrecognized receiver playback state"),
    };
    let position = row["currentTime"]
        .as_f64()
        .ok_or("Receiver position was not reported")?;
    if !position.is_finite()
        || position < 0.0
        || position > sample.full_duration.unwrap().as_secs_f64() + 2.0
    {
        return Err("Invalid receiver position");
    }
    Ok(Some(Snapshot {
        state,
        position: Duration::from_secs_f64(position),
        seek_serial,
        observed: Instant::now(),
    }))
}
fn check_owner(c: &mut impl Channel, owner: &Owned) -> ToneResult<()> {
    if !owned(&receiver(c)?, owner)? {
        return Err("Receiver application ownership changed");
    }
    Ok(())
}
fn release(c: &mut impl Channel, owner: Option<&Owned>, sample: &Sample) -> ToneResult<()> {
    let state = receiver(c)?;
    if idle(&state)? {
        return Ok(());
    }
    let Some(owner) = owner else {
        return Err("Receiver launch ownership was not confirmed");
    };
    if !owned(&state, owner)? {
        return Ok(());
    } // Never stop the replacement application.
    let current = media(c, owner)?;
    if !media_rows(&current)?.is_empty()
        && (!our_media(&current, sample)?
            || media_rows(&current)?
                .iter()
                .any(|r| Some(r["mediaSessionId"].as_u64().unwrap_or(0)) != owner.media_session))
    {
        return Err("Receiver media changed; no stop command sent");
    }
    c.request(
        "receiver-0",
        RECEIVER,
        json!({"type":"STOP","sessionId":owner.session}),
    )?;
    if !idle(&receiver(c)?)? {
        return Err("Receiver release was not confirmed");
    }
    Ok(())
}
fn exercise(
    c: &mut impl Channel,
    item: &Item,
    control: &Control,
    guard: &mut dyn FnMut() -> ToneResult<()>,
    released: &mut bool,
    session_deadline: Instant,
) -> ToneResult<Outcome> {
    let sample = current_track_sample(&item.xml, Some(item.duration))?;
    if item.position >= item.duration {
        return Err("Cast start position is outside the track");
    }
    guard()?;
    if control.stopped() {
        return Ok(Outcome::Stopped);
    }
    c.send(
        "receiver-0",
        CONNECTION,
        json!({"type":"CONNECT","origin":{}}),
    )?;
    if !idle(&receiver(c)?)? {
        return Err("Receiver is active; playback was not replaced");
    }
    let mut owner = None;
    let result = (|| {
        guard()?;
        if control.stopped() {
            return Ok(Outcome::Stopped);
        }
        let status = receiver(c)?;
        if !idle(&status)? {
            return Err("Receiver is no longer idle");
        }
        control.emit(Event::Volume(volume(&status)?))?;
        control.emit(Event::Phase(crate::cast_control::Phase::Launching))?;
        *released = false; // LAUNCH may have taken effect even if its reply is lost.
        let response = c.request("receiver-0", RECEIVER, json!({"type":"LAUNCH","appId":APP}))?;
        let apps = apps(&response)?;
        if apps.len() != 1 || apps[0]["appId"] != APP {
            return Err("Receiver application launch was not confirmed");
        }
        let mut app = Owned {
            session: identifier(&apps[0], "sessionId")?,
            transport: identifier(&apps[0], "transportId")?,
            media_session: None,
        };
        owner = Some(app.clone());
        c.send(
            &app.transport,
            CONNECTION,
            json!({"type":"CONNECT","origin":{}}),
        )?;
        control.emit(Event::Phase(crate::cast_control::Phase::CheckingEmptyApp))?;
        if !media_rows(&media(c, &app)?)?.is_empty() {
            return Err("Existing receiver media was not replaced");
        }
        check_owner(c, &app)?;
        guard()?;
        if control.stopped() {
            return Ok(Outcome::Stopped);
        }
        let mut autoplay = item.autoplay;
        let mut start = item.position;
        let mut serial = 0;
        let mut level = None;
        for _ in 0..16 {
            let Some(command) = control.next() else {
                break;
            };
            match command {
                Command::Pause(paused) => autoplay = !paused,
                Command::Seek {
                    serial: next,
                    position,
                } => {
                    serial = next;
                    start = position.min(item.duration.saturating_sub(Duration::from_secs(1)));
                }
                Command::Volume(value) => level = Some(value),
            }
        }
        if let Some(level) = level {
            if !level.is_finite() || !(0.0..=1.0).contains(&level) {
                return Err("Invalid Cast volume");
            }
            check_owner(c, &app)?;
            let ack = c.request(
                "receiver-0",
                RECEIVER,
                json!({"type":"SET_VOLUME","volume":{"level":level}}),
            )?;
            if !owned(&ack, &app)? || !close_level(volume(&ack)?, level) {
                return Err("Receiver volume was not confirmed");
            }
            control.emit(Event::Volume(level))?;
        }
        guard()?;
        if control.stopped() {
            return Ok(Outcome::Stopped);
        }
        control.emit(Event::Phase(crate::cast_control::Phase::Loading))?;
        let loaded=c.request(&app.transport,MEDIA,json!({"type":"LOAD","autoplay":autoplay,"currentTime":start.as_secs_f64(),"media":{"contentId":sample.content_id,"contentType":sample.content_type,"streamType":"BUFFERED","metadata":item.metadata.receiver_json()}}))?;
        if !our_media(&loaded, &sample)? || media_rows(&loaded)?.len() != 1 {
            return Err("Cast LOAD was not confirmed");
        }
        app.media_session = Some(
            media_rows(&loaded)?[0]["mediaSessionId"]
                .as_u64()
                .ok_or("Invalid media session")?,
        );
        owner = Some(app.clone());
        c.bind_media(&app, &sample, &loaded);
        let mut paused_since = (!autoplay).then(std::time::SystemTime::now);
        let mut playing = autoplay;
        control.emit(Event::Phase(crate::cast_control::Phase::Monitoring))?;
        let mut next_poll = Instant::now() + Duration::from_millis(250);
        let mut starting = true;
        let startup_deadline = Instant::now() + Duration::from_secs(30);
        loop {
            if starting && Instant::now() >= startup_deadline {
                return Err("Receiver did not finish starting playback");
            }
            if Instant::now() + Duration::from_secs(20) >= session_deadline {
                return Err("Cast session validity window ended; reconnect explicitly");
            }
            if control.stopped() {
                return Ok(Outcome::Stopped);
            }
            guard()?;
            if let Some(command) = control.next() {
                check_owner(c, &app)?;
                let current =
                    media_with_budget(c, &app, Duration::from_secs(if starting { 8 } else { 2 }))?;
                let current = snapshot(&current, &app, &sample, serial)?
                    .ok_or("Receiver media disappeared before control")?;
                starting &= current.state == State::Buffering;
                guard()?;
                match command {
                    Command::Pause(paused) => {
                        if !paused
                            && paused_since.is_some_and(|t| {
                                t.elapsed()
                                    .map_or(true, |elapsed| elapsed >= Duration::from_secs(60))
                            })
                        {
                            return Ok(Outcome::Renew);
                        }
                        let ack=c.request(&app.transport,MEDIA,json!({"type":if paused {"PAUSE"} else {"PLAY"},"mediaSessionId":app.media_session.unwrap()}))?;
                        let state = snapshot(&ack, &app, &sample, serial)?
                            .ok_or("Receiver did not confirm playback control")?;
                        if paused && state.state != State::Paused
                            || !paused && !matches!(state.state, State::Playing | State::Buffering)
                        {
                            return Err("Receiver did not confirm requested playback state");
                        }
                        paused_since = paused.then(std::time::SystemTime::now);
                        playing = !paused;
                        control.emit(Event::Snapshot(state))?;
                    }
                    Command::Seek {
                        serial: request,
                        position,
                    } => {
                        let position =
                            position.min(item.duration.saturating_sub(Duration::from_secs(1)));
                        playing = match current.state {
                            State::Paused => false,
                            State::Playing => true,
                            State::Buffering => playing,
                        };
                        let ack=c.request(&app.transport,MEDIA,json!({"type":"SEEK","mediaSessionId":app.media_session.unwrap(),"currentTime":position.as_secs_f64(),"resumeState":if playing {"PLAYBACK_START"} else {"PLAYBACK_PAUSE"}}))?;
                        snapshot(&ack, &app, &sample, serial)?.ok_or("Receiver rejected seek")?;
                        let deadline = Instant::now() + Duration::from_secs(8);
                        loop {
                            if control.stopped() {
                                return Ok(Outcome::Stopped);
                            }
                            guard()?;
                            let status = media(c, &app)?;
                            let state = snapshot(&status, &app, &sample, request)?
                                .ok_or("Media disappeared during seek")?;
                            let close = if playing {
                                state.position + Duration::from_millis(250) >= position
                                    && state.position <= position + Duration::from_secs(2)
                            } else {
                                state.position.abs_diff(position) <= Duration::from_millis(250)
                            };
                            if close
                                && state.state != State::Buffering
                                && (playing || state.state == State::Paused)
                            {
                                serial = request;
                                control.emit(Event::Snapshot(state))?;
                                break;
                            }
                            if Instant::now() >= deadline {
                                return Err("Receiver seek was not confirmed");
                            }
                            c.pause(Duration::from_millis(100));
                        }
                    }
                    Command::Volume(level) => {
                        if !level.is_finite() || !(0.0..=1.0).contains(&level) {
                            return Err("Invalid Cast volume");
                        }
                        let ack = c.request(
                            "receiver-0",
                            RECEIVER,
                            json!({"type":"SET_VOLUME","volume":{"level":level}}),
                        )?;
                        if !owned(&ack, &app)? || !close_level(volume(&ack)?, level) {
                            return Err("Receiver volume was not confirmed");
                        }
                        control.emit(Event::Volume(level))?;
                    }
                }
                next_poll = Instant::now();
            }
            if Instant::now() >= next_poll {
                check_owner(c, &app)?;
                let status =
                    media_with_budget(c, &app, Duration::from_secs(if starting { 8 } else { 2 }))?;
                if let Some(reason) = c.terminal().and_then(End::failure) {
                    return Err(reason);
                }
                if c.take_finished() {
                    return Ok(Outcome::Finished);
                }
                let state = snapshot(&status, &app, &sample, serial)?
                    .ok_or("Receiver media ended without confirmed completion")?;
                starting &= state.state == State::Buffering;
                control.emit(Event::Snapshot(state))?;
                next_poll = Instant::now() + Duration::from_secs(1);
            }
            c.pause(Duration::from_millis(50));
        }
    })();
    if !*released {
        let _ = control.emit(Event::Phase(crate::cast_control::Phase::Releasing));
        *released = release(c, owner.as_ref(), &sample).is_ok();
    }
    result
}
pub(in super::super) fn run(
    stream: &mut StreamOwned<ClientConnection, BudgetIo<'_>>,
    item: &Item,
    control: &Control,
    guard: &mut dyn FnMut() -> ToneResult<()>,
    released: &mut bool,
) -> ToneResult<Outcome> {
    let deadline = stream.sock.deadline;
    let mut channel = Live {
        stream,
        deadline,
        next: 0,
        poisoned: false,
        frames: framing::Frames::default(),
        watch: None,
    };
    exercise(&mut channel, item, control, guard, released, deadline)
}
