use super::*;
use crate::cast_control::Handle;
struct Fake {
    launched: bool,
    loaded: bool,
    paused: bool,
    position: f64,
    content: String,
    calls: Vec<Value>,
    budgets: Vec<(bool, Duration)>,
    state: Option<media_state::State>,
    completed: bool,
    invalid_seek: bool,
    foreign: bool,
    controls: bool,
    handle: Handle,
}
impl Fake {
    fn new(handle: Handle) -> Self {
        Self {
            launched: false,
            loaded: false,
            paused: false,
            position: 0.,
            content: String::new(),
            calls: Vec::new(),
            budgets: Vec::new(),
            state: None,
            completed: false,
            invalid_seek: false,
            foreign: false,
            controls: true,
            handle,
        }
    }
    fn receiver(&self) -> Value {
        json!({"type":"RECEIVER_STATUS","status":{"applications":if self.launched {json!([{"appId":APP,"sessionId":"owned","transportId":"owned-transport"}])} else {json!([])},"volume":{"level":0.73,"muted":false}}})
    }
}
impl Channel for Fake {
    fn send(&mut self, _: &str, _: &str, _: Value) -> ToneResult<()> {
        Ok(())
    }
    fn request_with_budget(
        &mut self,
        dest: &str,
        ns: &str,
        body: Value,
        budget: Duration,
    ) -> ToneResult<Value> {
        self.budgets.push((self.loaded, budget));
        self.request(dest, ns, body)
    }
    fn request(&mut self, _: &str, ns: &str, body: Value) -> ToneResult<Value> {
        self.calls.push(body.clone());
        match body["type"].as_str().unwrap() {
            "LAUNCH" => self.launched = true,
            "STOP" => {
                self.launched = false;
                self.loaded = false;
            }
            "LOAD" => {
                self.loaded = true;
                self.content = body["media"]["contentId"].as_str().unwrap().into();
                self.paused = !body["autoplay"].as_bool().unwrap();
                self.position = body["currentTime"].as_f64().unwrap();
            }
            "PAUSE" => self.paused = true,
            "PLAY" => {
                self.paused = false;
                self.completed = true;
            }
            "SEEK" => {
                if self.invalid_seek {
                    return Ok(json!({"type":"INVALID_REQUEST"}));
                }
                self.position = body["currentTime"].as_f64().unwrap();
                assert_eq!(body["resumeState"], "PLAYBACK_PAUSE");
            }
            "GET_STATUS" => {}
            other => panic!("Unexpected synthetic command: {other}"),
        }
        if ns == RECEIVER {
            return Ok(self.receiver());
        }
        let reply = json!({"type":"MEDIA_STATUS","status":if !self.loaded {json!([])} else {json!([{"mediaSessionId":if self.foreign {2} else {1},"media":{"contentId":if self.foreign {"foreign"} else {&self.content}},"playerState":if self.completed && body["type"]=="GET_STATUS" {"IDLE"} else if self.paused {"PAUSED"} else {"PLAYING"},"idleReason":if self.completed {"FINISHED"} else {""},"currentTime":self.position}])}});
        if let Some(state) = &mut self.state {
            state.observe(&reply, Source::Reply);
        }
        Ok(reply)
    }
    fn bind_media(&mut self, owner: &Owned, sample: &Sample, loaded: &Value) {
        let mut state =
            media_state::State::new(owner.media_session.unwrap(), sample.content_id.clone());
        state.observe(loaded, Source::Load);
        self.state = Some(state);
        if self.controls {
            self.handle.command(Command::Pause(true)).unwrap();
            self.handle
                .command(Command::Seek {
                    serial: 1,
                    position: Duration::from_millis(3125),
                })
                .unwrap();
            self.handle.command(Command::Pause(false)).unwrap();
        } else {
            self.handle.stop();
        }
    }
    fn terminal(&self) -> Option<End> {
        self.state.as_ref().and_then(|s| s.end)
    }
    fn take_finished(&mut self) -> bool {
        self.state.as_mut().is_some_and(|s| s.take_finished())
    }
    fn pause(&mut self, _: Duration) {}
}
fn item() -> Item {
    Item {
        metadata: crate::cast_control::Metadata {
            title: "Synthetic title".into(),
            artist: "Synthetic artist".into(),
            album: "Synthetic album".into(),
            artwork: Some("https://resources.tidal.com/images/synthetic/640x640.jpg".into()),
        },
        xml: include_str!("../../tests/fixtures/cast-inline.mpd").into(),
        duration: Duration::from_secs(5),
        position: Duration::ZERO,
        autoplay: true,
    }
}
#[test]
fn native_session_confirms_pause_fractional_seek_resume_and_natural_finish() {
    let (handle, control) = crate::cast_control::channel();
    let mut fake = Fake::new(handle);
    let mut released = true;
    assert_eq!(
        exercise(
            &mut fake,
            &item(),
            &control,
            &mut || Ok(()),
            &mut released,
            Instant::now() + Duration::from_secs(60)
        ),
        Ok(Outcome::Finished)
    );
    assert!(released);
    assert_eq!(fake.calls.iter().filter(|v| v["type"] == "LOAD").count(), 1);
    assert_eq!(fake.calls.iter().filter(|v| v["type"] == "SEEK").count(), 1);
    assert_eq!(
        fake.budgets.iter().find(|(loaded, _)| *loaded).unwrap().1,
        Duration::from_secs(8)
    );
    assert_eq!(
        fake.budgets
            .iter()
            .filter(|(_, budget)| *budget == Duration::from_secs(8))
            .count(),
        1
    );
    let load = fake.calls.iter().find(|v| v["type"] == "LOAD").unwrap();
    assert_eq!(load["media"]["metadata"]["title"], "Synthetic title");
    assert_eq!(load["media"]["metadata"]["artist"], "Synthetic artist");
    assert_eq!(load["media"]["metadata"]["albumName"], "Synthetic album");
    assert_eq!(
        load["media"]["metadata"]["images"][0]["url"],
        "https://resources.tidal.com/images/synthetic/640x640.jpg"
    );
    assert!(!fake.calls.iter().any(|v| v["type"] == "SET_VOLUME"));
    assert!(fake.calls.iter().any(|v| v["type"] == "STOP"));
}
#[test]
fn seek_rejection_is_not_claimed_as_success_or_retried() {
    let (handle, control) = crate::cast_control::channel();
    let mut fake = Fake::new(handle);
    fake.invalid_seek = true;
    let mut released = true;
    assert!(
        exercise(
            &mut fake,
            &item(),
            &control,
            &mut || Ok(()),
            &mut released,
            Instant::now() + Duration::from_secs(60)
        )
        .is_err()
    );
    assert!(released);
    assert_eq!(fake.calls.iter().filter(|v| v["type"] == "SEEK").count(), 1);
    assert!(!fake.calls.iter().any(|v| v["type"] == "PLAY"));
}
#[test]
fn controls_queued_during_connection_change_load_intent_before_audio() {
    let (handle, control) = crate::cast_control::channel();
    handle.command(Command::Pause(true)).unwrap();
    handle
        .command(Command::Seek {
            serial: 1,
            position: Duration::from_millis(2250),
        })
        .unwrap();
    let mut fake = Fake::new(handle);
    fake.controls = false;
    let mut released = true;
    assert_eq!(
        exercise(
            &mut fake,
            &item(),
            &control,
            &mut || Ok(()),
            &mut released,
            Instant::now() + Duration::from_secs(60)
        ),
        Ok(Outcome::Stopped)
    );
    let load = fake.calls.iter().find(|v| v["type"] == "LOAD").unwrap();
    assert_eq!(load["autoplay"], false);
    assert_eq!(load["currentTime"], 2.25);
    assert!(released);
}
