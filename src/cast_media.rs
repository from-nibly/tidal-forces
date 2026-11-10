//! Incremental status for one confirmed LOAD. Never infer completion from time or disappearance.
use serde_json::Value;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum End {
    Finished,
    Cancelled,
    Interrupted,
    Error,
    UnknownIdle,
    OwnershipLost,
    Ambiguous,
}
impl End {
    pub(super) fn failure(self) -> Option<&'static str> {
        match self {
            Self::Finished => None,
            Self::Cancelled => Some("Receiver reported cancelled playback"),
            Self::Interrupted => Some("Receiver reported interrupted playback"),
            Self::Error => Some("Receiver reported a playback error"),
            Self::UnknownIdle => Some("Receiver reported an unknown idle reason"),
            Self::OwnershipLost => Some("Media ownership changed"),
            Self::Ambiguous => Some("Receiver reported conflicting media state"),
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Source {
    Load,
    Reply,
    Event,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Phase {
    Empty,
    Idle,
    Playing,
    Paused,
    Buffering,
    Loading,
    Absent,
    Other,
    Invalid,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Observation {
    source: Source,
    phase: Phase,
    reason: Option<End>,
    media_information_present: bool,
}
fn idle_end(row: &Value) -> End {
    match row["idleReason"].as_str() {
        Some("FINISHED") => End::Finished,
        Some("CANCELLED") => End::Cancelled,
        Some("INTERRUPTED") => End::Interrupted,
        Some("ERROR") => End::Error,
        _ => End::UnknownIdle,
    }
}
// Do not derive Debug: content_id may contain a signed account-media manifest.
pub(super) struct State {
    session: u64,
    content_id: String,
    pub(super) end: Option<End>,
    pub(super) source: Option<Source>,
    pub(super) empty: bool,
    delivered: bool,
    pub(super) awaiting_start: bool,
    pub(super) trace: [Option<Observation>; 16],
}
impl State {
    pub(super) fn new(session: u64, content_id: String) -> Self {
        Self {
            session,
            content_id,
            end: None,
            source: None,
            empty: false,
            delivered: false,
            awaiting_start: true,
            trace: [None; 16],
        }
    }
    pub(super) fn lose_ownership(&mut self) {
        self.end = Some(End::OwnershipLost);
        self.source = None;
    }
    fn terminal(&mut self, end: End, source: Source) {
        self.awaiting_start = false;
        if self.end == Some(End::OwnershipLost) || self.end == Some(End::Ambiguous) {
            return;
        }
        match self.end {
            None => {
                self.end = Some(end);
                self.source = Some(source);
            }
            Some(previous) if previous == end => {}
            Some(_) => {
                self.end = Some(End::Ambiguous);
                self.source = None;
            }
        }
    }
    pub(super) fn observe(&mut self, value: &Value, source: Source) {
        if value["type"] != "MEDIA_STATUS" {
            return;
        }
        let row = &value["status"][0];
        let phase = match value["status"].as_array() {
            Some(rows) if rows.is_empty() => Phase::Empty,
            Some(rows) if rows.len() == 1 => match row["playerState"].as_str() {
                Some("IDLE") => Phase::Idle,
                Some("PLAYING") => Phase::Playing,
                Some("PAUSED") => Phase::Paused,
                Some("BUFFERING") => Phase::Buffering,
                Some("LOADING") => Phase::Loading,
                None if row.get("playerState").is_none() => Phase::Absent,
                _ => Phase::Other,
            },
            _ => Phase::Invalid,
        };
        let observation = Observation {
            source,
            phase,
            reason: (phase == Phase::Idle).then(|| idle_end(row)),
            media_information_present: row.get("media").is_some(),
        };
        if self.trace.last() != Some(&Some(observation)) {
            self.trace.rotate_left(1);
            self.trace[15] = Some(observation);
        }
        let Some(rows) = value["status"].as_array().filter(|rows| rows.len() <= 1) else {
            self.terminal(End::Ambiguous, source);
            return;
        };
        let Some(row) = rows.first() else {
            self.empty = true;
            return;
        };
        if row["mediaSessionId"].as_u64() != Some(self.session) {
            self.lose_ownership();
            return;
        }
        if let Some(media) = row.get("media")
            && (!media.is_object()
                || media
                    .get("contentId")
                    .is_some_and(|id| id.as_str() != Some(self.content_id.as_str())))
        {
            self.lose_ownership();
            return;
        }
        // Unsolicited deltas can omit MediaInformation. Identity is inherited only
        // inside this confirmed LOAD's media session, never across a rebind.
        match row["playerState"].as_str() {
            Some("IDLE") if self.awaiting_start && idle_end(row) == End::UnknownIdle => {}
            Some("IDLE") => self.terminal(idle_end(row), source),
            Some("PLAYING" | "BUFFERING" | "PAUSED") if self.end.is_some() => {
                self.terminal(End::Ambiguous, source)
            }
            Some("PLAYING" | "BUFFERING" | "PAUSED") => {
                self.awaiting_start = false;
            }
            None if row.get("playerState").is_none() => {}
            _ => self.terminal(End::Ambiguous, source),
        }
    }
    pub(super) fn take_finished(&mut self) -> bool {
        if self.end != Some(End::Finished) || self.delivered {
            return false;
        }
        self.delivered = true;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn state() -> State {
        let mut state = State::new(7, "private synthetic content".into());
        state.observe(
            &json!({"type":"MEDIA_STATUS","status":[{"mediaSessionId":7,"playerState":"PLAYING"}]}),
            Source::Load,
        );
        state
    }
    fn terminal(reason: &str) -> Value {
        json!({"type":"MEDIA_STATUS","status":[{"mediaSessionId":7,"playerState":"IDLE","idleReason":reason}]})
    }
    #[test]
    fn initial_idle_ack_waits_for_startup_but_never_becomes_completion() {
        let mut state = State::new(7, "synthetic".into());
        let idle =
            json!({"type":"MEDIA_STATUS","status":[{"mediaSessionId":7,"playerState":"IDLE"}]});
        state.observe(&idle, Source::Load);
        state.observe(&idle, Source::Reply);
        assert!(state.awaiting_start);
        assert_eq!(state.end, None);
        assert!(!state.take_finished());
        state.observe(&json!({"type":"MEDIA_STATUS","status":[{"mediaSessionId":7,"playerState":"BUFFERING"}]}),Source::Event);
        assert!(!state.awaiting_start);
        assert_eq!(state.end, None);
        state.observe(&terminal("FINISHED"), Source::Event);
        assert!(state.take_finished());
        let mut stalled = State::new(7, "synthetic".into());
        stalled.observe(&idle, Source::Load);
        stalled.observe(&json!({"type":"MEDIA_STATUS","status":[]}), Source::Reply);
        assert!(!stalled.take_finished());
        for reason in ["ERROR", "CANCELLED", "INTERRUPTED"] {
            let mut state = State::new(7, "synthetic".into());
            state.observe(&terminal(reason), Source::Load);
            assert!(!state.awaiting_start);
            assert!(state.end.unwrap().failure().is_some());
        }
    }
    #[test]
    fn unsolicited_delta_survives_empty_snapshot_and_is_delivered_once() {
        let mut state = state();
        state.observe(&terminal("FINISHED"), Source::Event);
        state.observe(&json!({"type":"MEDIA_STATUS","status":[]}), Source::Reply);
        assert!(state.empty);
        assert_eq!(state.source, Some(Source::Event));
        assert!(state.take_finished());
        state.observe(&terminal("FINISHED"), Source::Reply);
        assert!(!state.take_finished());
    }
    #[test]
    fn stop_error_unknown_idle_and_disappearance_are_not_completion() {
        for reason in ["CANCELLED", "INTERRUPTED", "ERROR", "UNKNOWN", ""] {
            let mut state = state();
            state.observe(&terminal(reason), Source::Event);
            assert!(!state.take_finished());
            assert!(state.end.unwrap().failure().is_some());
            state.observe(&terminal("FINISHED"), Source::Event);
            assert!(!state.take_finished());
        }
        let mut state = state();
        state.observe(&json!({"type":"MEDIA_STATUS","status":[{"mediaSessionId":7,"playerState":"PLAYING","currentTime":999999}]}), Source::Reply);
        state.observe(&json!({"type":"MEDIA_STATUS","status":[]}), Source::Reply);
        assert!(!state.take_finished());
        assert_eq!(state.end, None);
    }
    #[test]
    fn identity_changes_and_conflicting_states_invalidate_completion() {
        for change in [
            json!({"mediaSessionId":8,"playerState":"IDLE","idleReason":"FINISHED"}),
            json!({"mediaSessionId":7,"media":{"contentId":"foreign"}}),
            json!({"mediaSessionId":7,"media":null}),
            json!({"mediaSessionId":7,"playerState":"PLAYING"}),
        ] {
            let mut state = state();
            state.observe(&terminal("FINISHED"), Source::Event);
            state.observe(
                &json!({"type":"MEDIA_STATUS","status":[change]}),
                Source::Reply,
            );
            assert!(!state.take_finished());
        }
    }
    #[test]
    fn transition_trace_is_bounded_and_never_echoes_peer_values() {
        let mut state = state();
        for _ in 0..100 {
            state.observe(&serde_json::json!({"type":"MEDIA_STATUS","status":[{"mediaSessionId":7,"playerState":"private-state","idleReason":"private-reason","media":{"contentId":"private-url"}}]}), Source::Event);
            state.observe(
                &serde_json::json!({"type":"MEDIA_STATUS","status":[]}),
                Source::Reply,
            );
        }
        assert_eq!(state.trace.len(), 16);
        let trace = format!("{:?}", state.trace);
        assert!(!trace.contains("private"));
        assert!(trace.contains("Other") && trace.contains("Empty"));
    }
    #[test]
    fn a_new_load_cannot_inherit_old_completion() {
        let mut old = state();
        old.observe(&terminal("FINISHED"), Source::Event);
        assert!(old.take_finished());
        let mut new = State::new(8, "private synthetic content".into());
        assert!(!new.take_finished());
        new.observe(&terminal("FINISHED"), Source::Event);
        assert!(!new.take_finished());
        assert_eq!(new.end, Some(End::OwnershipLost));
    }
}
