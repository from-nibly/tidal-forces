use super::*;

#[derive(Default)]
struct Fake {
    fixture: Option<Fixture>,
    busy: bool,
    muted: bool,
    fail_load: bool,
    fail_launch: bool,
    playback_error: bool,
    never_finishes: bool,
    foreign_media: bool,
    external_volume: bool,
    changed_media_session: bool,
    launched: bool,
    loaded: bool,
    level: f64,
    calls: Vec<Value>,
}
impl Fake {
    fn fresh() -> Self {
        Self {
            level: 0.73,
            ..Self::default()
        }
    }
    fn receiver(&self) -> Value {
        let applications = if self.busy {
            json!([{"appId":"other","sessionId":"other"}])
        } else if self.launched {
            json!([{"appId":APP,"sessionId":"test-session","transportId":"test-transport"}])
        } else {
            json!([])
        };
        json!({"type":"RECEIVER_STATUS","status":{"applications":applications,"volume":{"level":self.level,"muted":self.muted}}})
    }
}
impl Channel for Fake {
    fn send(&mut self, _: &str, ns: &str, body: Value) -> ToneResult<()> {
        assert_eq!(ns, CONNECTION);
        assert_eq!(body["type"], "CONNECT");
        Ok(())
    }
    fn request(&mut self, _: &str, ns: &str, body: Value) -> ToneResult<Value> {
        let sample = sample(self.fixture.unwrap_or(Fixture::Tone));
        self.calls.push(body.clone());
        match (ns, body["type"].as_str().unwrap()) {
            (RECEIVER, "SET_VOLUME") => {
                self.level = body["volume"]["level"].as_f64().unwrap();
            }
            (RECEIVER, "LAUNCH") => {
                assert!(!self.busy);
                assert_eq!(body["appId"], APP);
                if self.fail_launch {
                    return Err("Synthetic launch failure");
                }
                self.launched = true;
            }
            (RECEIVER, "STOP") => {
                assert_eq!(body["sessionId"], "test-session");
                self.launched = false;
                self.loaded = false;
            }
            (RECEIVER, "GET_STATUS") => {}
            (MEDIA, "LOAD") => {
                assert_eq!(body["media"]["contentId"], sample.content_id);
                assert_eq!(body["media"]["contentType"], sample.content_type);
                assert_eq!(self.level, sample.level);
                assert!(!self.loaded && !self.foreign_media);
                if self.fail_load {
                    return Ok(json!({"type":"LOAD_FAILED"}));
                }
                self.loaded = true;
                if self.external_volume {
                    self.level = 0.2;
                }
            }
            (MEDIA, "GET_STATUS") => {
                if self.loaded {
                    assert_eq!(body["mediaSessionId"], 1);
                }
            }
            _ => panic!("Unexpected test command"),
        }
        if ns == RECEIVER {
            return Ok(self.receiver());
        }
        let rows = if self.foreign_media {
            json!([{"media":{"contentId":"other"},"mediaSessionId":99}])
        } else if self.loaded {
            let id = if self.changed_media_session && body["type"] == "GET_STATUS" {
                2
            } else {
                1
            };
            json!([{"media":{"contentId":sample.content_id},"mediaSessionId":id,"playerState":if self.never_finishes { "PLAYING" } else { "IDLE" },"idleReason":if self.playback_error { "ERROR" } else { "FINISHED" },"currentTime":1.0}])
        } else {
            json!([])
        };
        Ok(json!({"type":"MEDIA_STATUS","status":rows}))
    }
    fn pause(&mut self, _: Duration) {}
}
#[test]
fn tone_finishes_and_restores_prior_volume() {
    let mut f = Fake::fresh();
    exercise(&mut f).unwrap();
    assert_eq!(f.level, 0.73);
    assert!(!f.launched);
    assert_eq!(f.calls.iter().filter(|v| v["type"] == "LOAD").count(), 1);
    assert_eq!(f.calls.iter().filter(|v| v["type"] == "STOP").count(), 1);
}
#[test]
fn active_or_muted_receiver_is_never_modified() {
    for muted in [false, true] {
        let mut f = Fake {
            busy: !muted,
            muted,
            ..Fake::fresh()
        };
        assert!(exercise(&mut f).is_err());
        assert_eq!(f.calls.len(), 1);
        assert_eq!(f.calls[0]["type"], "GET_STATUS");
        assert_eq!(f.level, 0.73);
    }
}
#[test]
fn failed_load_still_stops_owned_app_and_restores() {
    let mut f = Fake {
        fail_load: true,
        ..Fake::fresh()
    };
    assert!(exercise(&mut f).is_err());
    assert_eq!(f.level, 0.73);
    assert!(!f.launched);
}
#[test]
fn foreign_media_is_neither_replaced_nor_stopped() {
    let mut f = Fake {
        foreign_media: true,
        ..Fake::fresh()
    };
    assert!(exercise(&mut f).is_err());
    assert!(
        !f.calls
            .iter()
            .any(|v| v["type"] == "LOAD" || v["type"] == "STOP")
    );
}
#[test]
fn external_volume_change_is_not_overwritten() {
    let mut f = Fake {
        external_volume: true,
        ..Fake::fresh()
    };
    assert!(exercise(&mut f).is_err());
    assert_eq!(f.level, 0.2);
    assert!(!f.launched);
}
#[test]
fn changed_media_session_is_not_stopped_even_for_same_url() {
    let mut f = Fake {
        changed_media_session: true,
        ..Fake::fresh()
    };
    assert!(exercise(&mut f).is_err());
    assert!(!f.calls.iter().any(|v| v["type"] == "STOP"));
}
#[test]
fn malformed_or_non_idle_background_app_is_not_treated_as_idle() {
    for apps in [
        json!(null),
        json!([{"appId":"other","isIdleScreen":true}]),
        json!([{"appId":"E8C28D3C","isIdleScreen":false}]),
    ] {
        let v = json!({"type":"RECEIVER_STATUS","status":{"applications":apps}});
        assert!(!idle(&v).unwrap_or(false));
    }
}

#[test]
fn inline_dash_preserves_all_ownership_and_cleanup_guards() {
    for fixture in [Fixture::InlineDash, Fixture::InlineDashFullVolume] {
        let sample = sample(fixture);
        for mode in 0..8 {
            let mut f = Fake {
                fixture: Some(fixture),
                busy: mode == 1,
                muted: mode == 2,
                fail_load: mode == 3,
                playback_error: mode == 7,
                foreign_media: mode == 4,
                external_volume: mode == 5,
                changed_media_session: mode == 6,
                ..Fake::fresh()
            };
            let result = exercise_sample(&mut f, &sample);
            assert_eq!(result.is_ok(), mode == 0);
            assert_eq!(
                f.level,
                if mode == 5 {
                    0.2
                } else if mode == 6 {
                    sample.level
                } else {
                    0.73
                }
            );
            if mode == 1 || mode == 2 {
                assert_eq!(f.calls.len(), 1);
            }
            if mode == 4 || mode == 6 {
                assert!(!f.calls.iter().any(|v| v["type"] == "STOP"));
            }
            assert!(f.calls.iter().filter(|v| v["type"] == "LOAD").count() <= 1);
        }
    }
}
#[test]
fn inline_dash_is_only_the_bounded_public_fixture() {
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    let sample = sample(Fixture::InlineDash);
    let encoded = sample
        .content_id
        .strip_prefix("data:application/dash+xml;base64,")
        .unwrap();
    assert_eq!(
        STANDARD.decode(encoded).unwrap(),
        include_bytes!("../../tests/fixtures/cast-inline.mpd")
    );
    assert!(sample.content_id.len() < 4096);
    assert_eq!(sample.level, 0.5);
    assert_eq!(sample.content_type, "application/dash+xml");
    assert_eq!(sample.polls, 24);
}

#[test]
fn launch_failure_never_raises_volume() {
    let mut f = Fake {
        fixture: Some(Fixture::InlineDashFullVolume),
        fail_launch: true,
        ..Fake::fresh()
    };
    assert!(exercise_sample(&mut f, &sample(Fixture::InlineDashFullVolume)).is_err());
    assert_eq!(f.level, 0.73);
    assert!(
        !f.calls
            .iter()
            .any(|v| v["type"] == "SET_VOLUME" || v["type"] == "LOAD")
    );
}
#[test]
fn already_matching_volume_needs_no_mutation() {
    for fixture in [
        Fixture::Tone,
        Fixture::InlineDash,
        Fixture::InlineDashFullVolume,
    ] {
        let sample = sample(fixture);
        let mut f = Fake {
            fixture: Some(fixture),
            level: sample.level,
            ..Fake::fresh()
        };
        exercise_sample(&mut f, &sample).unwrap();
        assert!(!f.calls.iter().any(|v| v["type"] == "SET_VOLUME"));
        assert_eq!(f.level, sample.level);
        assert!(!f.launched);
    }
}
#[test]
fn request_diagnostics_do_not_echo_untrusted_text() {
    assert_eq!(
        request_stage("untrusted namespace", &json!({"type":"private text"})),
        "other"
    );
    assert_eq!(
        request_stage(
            MEDIA,
            &json!({"type":"LOAD","media":{"contentId":"private URL"}})
        ),
        "media load"
    );
    assert_eq!(
        request_stage(RECEIVER, &json!({"type":"LAUNCH"})),
        "receiver launch"
    );
}

#[test]
fn only_launch_and_load_receive_longer_bounded_reply_windows() {
    for (ns, command, seconds) in [
        (RECEIVER, "LAUNCH", 8),
        (MEDIA, "LOAD", 8),
        (RECEIVER, "GET_STATUS", 2),
        (MEDIA, "GET_STATUS", 2),
        (RECEIVER, "SET_VOLUME", 2),
        (RECEIVER, "STOP", 2),
        (MEDIA, "LAUNCH", 2),
        (RECEIVER, "LOAD", 2),
    ] {
        assert_eq!(
            request_timeout(ns, &json!({"type":command})),
            Duration::from_secs(seconds)
        );
    }
}

#[test]
fn changed_account_guard_stops_only_owned_media_and_restores() {
    for fail_at in 1..=6 {
        let mut f = Fake {
            fixture: Some(Fixture::InlineDashFullVolume),
            ..Fake::fresh()
        };
        let mut checks = 0;
        let mut guard = || {
            checks += 1;
            if checks == fail_at {
                Err("Synthetic account guard changed")
            } else {
                Ok(())
            }
        };
        assert!(
            exercise_guarded(&mut f, &sample(Fixture::InlineDashFullVolume), &mut guard).is_err()
        );
        assert_eq!(f.level, 0.73);
        assert!(!f.launched);
        if fail_at <= 4 {
            assert!(!f.calls.iter().any(|v| v["type"] == "LOAD"));
        }
    }
}
#[test]
fn whole_track_requires_completion_and_keeps_finite_limits() {
    let xml = include_str!("../../tests/fixtures/cast-inline.mpd");
    assert!(current_track_sample(xml, Some(Duration::ZERO)).is_err());
    assert!(current_track_sample(xml, Some(Duration::from_secs(901))).is_err());
    let mut sample = sample(Fixture::WholeTrack);
    sample.polls = 2;
    let mut f = Fake {
        fixture: Some(Fixture::WholeTrack),
        never_finishes: true,
        ..Fake::fresh()
    };
    assert_eq!(
        exercise_sample(&mut f, &sample),
        Err("Natural media completion was not confirmed")
    );
    assert_eq!(f.level, 0.73);
    assert!(!f.launched);
    assert_eq!(f.calls.iter().filter(|v| v["type"] == "LOAD").count(), 1);
}

#[test]
fn terminal_events_require_owned_routes_and_reject_stale_replies() {
    let finish = json!({"type":"MEDIA_STATUS","status":[{"mediaSessionId":1,"playerState":"IDLE","idleReason":"FINISHED"}]});
    let watch = || Watch {
        owner: Owned {
            session: "app-session".into(),
            transport: "owned-transport".into(),
            media_session: Some(1),
        },
        state: media_state::State::new(1, "synthetic".into()),
        ignored_terminal_replies: 0,
    };
    for (source, namespace, id) in [
        ("foreign-transport", MEDIA, None),
        ("owned-transport", RECEIVER, None),
        ("owned-transport", MEDIA, Some(json!(9))),
        ("owned-transport", MEDIA, Some(Value::Null)),
    ] {
        let mut watch = watch();
        let mut value = finish.clone();
        if let Some(id) = id {
            value["requestId"] = id;
        }
        watch.observe(source, namespace, &value, false);
        assert!(!watch.state.take_finished());
        assert_eq!(
            watch.ignored_terminal_replies,
            usize::from(source == "owned-transport" && namespace == MEDIA)
        );
    }
    let mut w = watch();
    w.observe("owned-transport", MEDIA, &finish, false);
    assert!(w.state.take_finished());
    w.observe("owned-transport", MEDIA, &finish, false);
    assert!(!w.state.take_finished());
    let mut w = watch();
    w.observe("receiver-0",RECEIVER,&json!({"type":"RECEIVER_STATUS","status":{"applications":[{"appId":APP,"sessionId":"foreign","transportId":"owned-transport"}]}}),false);
    w.observe("owned-transport", MEDIA, &finish, false);
    assert_eq!(w.state.end, Some(End::OwnershipLost));
    assert!(!w.state.take_finished());
}

#[test]
fn account_media_constructor_is_bounded_and_has_no_implicit_volume_choice() {
    assert!(current_track_sample("", None).is_err());
    assert!(current_track_sample(&"x".repeat(8193), None).is_err());
    let sample =
        current_track_sample(include_str!("../../tests/fixtures/cast-inline.mpd"), None).unwrap();
    assert_eq!(sample.level, 1.0);
    assert_eq!(sample.content_type, "application/dash+xml");
}

fn broadcast(stream: &mut impl Write, source: &str, value: Value) {
    let event = proto::Envelope {
        version: Some(0),
        source: Some(source.into()),
        destination: Some("*".into()),
        namespace: Some(MEDIA.into()),
        payload_type: Some(0),
        text: Some(value.to_string()),
        binary: None,
        continued: None,
        remaining: None,
    }
    .encode_to_vec();
    stream
        .write_all(&(event.len() as u32).to_be_bytes())
        .unwrap();
    stream.write_all(&event).unwrap();
}

pub(crate) fn serve(stream: &mut (impl Read + Write), fixture: Fixture, delay: Option<&str>) {
    let mut delayed = false;
    let mut media_polls = 0;
    let terminal_scenario = matches!(
        delay,
        Some(
            "FINISHED_EVENT"
                | "EMPTY_ONLY"
                | "ERROR_EVENT"
                | "CANCELLED_EVENT"
                | "FOREIGN_EVENT"
                | "STALE_EVENT"
        )
    );
    let mut fake = Fake {
        fixture: Some(fixture),
        never_finishes: terminal_scenario,
        ..Fake::fresh()
    };
    for _ in 0..40 {
        let mut header = [0; 4];
        if stream.read_exact(&mut header).is_err() {
            break;
        }
        let size = u32::from_be_bytes(header) as usize;
        assert!(size <= MAX_MESSAGE);
        let mut bytes = vec![0; size];
        stream.read_exact(&mut bytes).unwrap();
        let e = proto::Envelope::decode(bytes.as_slice()).unwrap();
        assert_eq!(e.source.as_deref(), Some("sender-0"));
        let b: Value = serde_json::from_str(e.text.as_deref().unwrap()).unwrap();
        let ns = e.namespace.as_deref().unwrap();
        if ns == CONNECTION {
            fake.send(e.destination.as_deref().unwrap(), ns, b).unwrap();
            continue;
        }
        if !delayed && delay.is_some() && b["type"].as_str() == delay {
            std::thread::sleep(Duration::from_millis(2250));
            delayed = true;
        }
        let interrupted_status = !delayed
            && ns == MEDIA
            && b["type"] == "GET_STATUS"
            && fake.loaded
            && matches!(
                delay,
                Some("STATUS_HEADER" | "STATUS_PAYLOAD" | "STATUS_MISSING")
            );
        let terminal_poll =
            terminal_scenario && ns == MEDIA && b["type"] == "GET_STATUS" && fake.loaded;
        if terminal_poll {
            media_polls += 1;
        }
        let is_load = b["type"] == "LOAD" && ns == MEDIA;
        let id = b["requestId"].clone();
        let mut r = fake
            .request(e.destination.as_deref().unwrap(), ns, b)
            .unwrap();
        r["requestId"] = id;
        if delay == Some("FINISHED_EVENT") && is_load {
            r["status"][0]["playerState"] = json!("IDLE");
            r["status"][0].as_object_mut().unwrap().remove("idleReason");
        }
        if delay == Some("FINISHED_EVENT") && terminal_poll && media_polls == 1 {
            broadcast(
                stream,
                e.destination.as_deref().unwrap(),
                json!({"type":"MEDIA_STATUS","status":[{"mediaSessionId":1,"playerState":"BUFFERING"}]}),
            );
            r["status"][0]["playerState"] = json!("BUFFERING");
        }
        if terminal_poll
            && media_polls
                == if delay == Some("FINISHED_EVENT") {
                    3
                } else {
                    2
                }
        {
            delayed = true;
            if delay != Some("EMPTY_ONLY") {
                let mut value = json!({"type":"MEDIA_STATUS","requestId":0,"status":[{"mediaSessionId":1,"playerState":"IDLE","idleReason":"FINISHED","currentTime":5.0}]});
                if delay == Some("ERROR_EVENT") {
                    value["status"][0]["idleReason"] = json!("ERROR");
                }
                if delay == Some("CANCELLED_EVENT") {
                    value["status"][0]["idleReason"] = json!("CANCELLED");
                }
                if delay == Some("STALE_EVENT") {
                    value["requestId"] = json!(1);
                }
                let event = proto::Envelope {
                    version: Some(0),
                    source: if delay == Some("FOREIGN_EVENT") {
                        Some("foreign-transport".into())
                    } else {
                        e.destination.clone()
                    },
                    destination: Some("*".into()),
                    namespace: Some(MEDIA.into()),
                    payload_type: Some(0),
                    text: Some(value.to_string()),
                    binary: None,
                    continued: None,
                    remaining: None,
                }
                .encode_to_vec();
                stream
                    .write_all(&(event.len() as u32).to_be_bytes())
                    .unwrap();
                stream.write_all(&event).unwrap();
            }
            fake.loaded = false;
            r["status"] = json!([]);
        }
        let reply = proto::Envelope {
            version: Some(0),
            source: e.destination,
            destination: Some("sender-0".into()),
            namespace: e.namespace,
            payload_type: Some(0),
            text: Some(r.to_string()),
            binary: None,
            continued: None,
            remaining: None,
        }
        .encode_to_vec();
        let header = (reply.len() as u32).to_be_bytes();
        if interrupted_status {
            delayed = true;
            if delay == Some("STATUS_MISSING") {
                continue;
            }
            if delay == Some("STATUS_HEADER") {
                stream.write_all(&header[..2]).unwrap();
                stream.flush().unwrap();
                std::thread::sleep(Duration::from_millis(2250));
                stream.write_all(&header[2..]).unwrap();
                stream.write_all(&reply).unwrap();
            } else {
                stream.write_all(&header).unwrap();
                stream.write_all(&reply[..5]).unwrap();
                stream.flush().unwrap();
                std::thread::sleep(Duration::from_millis(2250));
                stream.write_all(&reply[5..]).unwrap();
            }
        } else {
            stream.write_all(&header).unwrap();
            stream.write_all(&reply).unwrap();
        }
        stream.flush().unwrap();
    }
    assert_eq!(delayed, delay.is_some());
    assert_eq!(fake.level, 0.73);
    assert!(!fake.launched);
    assert_eq!(fake.calls.iter().filter(|v| v["type"] == "LOAD").count(), 1);
}
