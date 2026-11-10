//! Bounded native Cast control interface. Media documents remain memory-only.
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::Duration,
};

#[derive(Default)]
pub(crate) struct Metadata {
    pub title: String,
    pub artist: String,
    pub album: String,
    pub artwork: Option<String>,
}
impl Metadata {
    pub fn receiver_json(&self) -> serde_json::Value {
        let text = |value: &str| {
            value
                .chars()
                .filter(|c| !c.is_control())
                .take(256)
                .collect::<String>()
        };
        let mut value = serde_json::json!({"metadataType":3,"title":text(&self.title),"artist":text(&self.artist),"albumName":text(&self.album)});
        if let Some(artwork) = self.artwork.as_deref().filter(|url| {
            reqwest::Url::parse(url).is_ok_and(|u| {
                u.scheme() == "https"
                    && u.host_str() == Some("resources.tidal.com")
                    && u.port().is_none()
                    && u.username().is_empty()
                    && u.password().is_none()
                    && u.query().is_none()
                    && u.fragment().is_none()
                    && u.path().starts_with("/images/")
                    && url.len() <= 2048
            })
        }) {
            value["images"] = serde_json::json!([{"url":artwork}]);
        }
        value
    }
}
pub(crate) struct Media {
    pub metadata: Metadata,
    pub xml: String,
    pub duration: Duration,
    pub position: Duration,
    pub autoplay: bool,
}
#[derive(Clone, Copy, Debug)]
pub(crate) enum Command {
    Pause(bool),
    Seek { serial: u64, position: Duration },
    Volume(f64),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum State {
    Buffering,
    Playing,
    Paused,
}
#[derive(Clone, Copy, Debug)]
pub(crate) struct Snapshot {
    pub state: State,
    pub position: Duration,
    pub seek_serial: u64,
    pub observed: std::time::Instant,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum End {
    Finished,
    Stopped,
    Renew,
}
#[derive(Clone, Copy, Debug)]
pub(crate) enum Phase {
    Authenticating,
    Launching,
    CheckingEmptyApp,
    Loading,
    Monitoring,
    Releasing,
}
#[derive(Clone, Copy, Debug)]
pub(crate) enum Event {
    Phase(Phase),
    Snapshot(Snapshot),
    Volume(f64),
    Closed {
        result: Result<End, &'static str>,
        released: bool,
    },
}
pub(crate) struct Handle {
    commands: mpsc::SyncSender<Command>,
    stop: Arc<AtomicBool>,
    events: mpsc::Receiver<Event>,
    closed: mpsc::Receiver<Event>,
    terminal_seen: std::cell::Cell<bool>,
}
pub(crate) struct Control {
    commands: mpsc::Receiver<Command>,
    stop: Arc<AtomicBool>,
    events: mpsc::SyncSender<Event>,
    closed: mpsc::SyncSender<Event>,
    wake: Option<Box<dyn Fn() + Send + Sync>>,
}
pub(crate) fn channel() -> (Handle, Control) {
    let (commands, receive) = mpsc::sync_channel(16);
    let (events, output) = mpsc::sync_channel(32);
    let (closed, closure) = mpsc::sync_channel(1);
    let stop = Arc::new(AtomicBool::new(false));
    (
        Handle {
            commands,
            stop: stop.clone(),
            events: output,
            closed: closure,
            terminal_seen: std::cell::Cell::new(false),
        },
        Control {
            commands: receive,
            stop,
            events,
            closed,
            wake: None,
        },
    )
}
impl Handle {
    pub fn next_event(&self) -> Option<Event> {
        if self.terminal_seen.get() {
            return None;
        }
        let progress = self.events.try_recv();
        if let Ok(event) = progress {
            return Some(event);
        }
        let terminal = match self.closed.try_recv() {
            Ok(event) => Some(event),
            Err(mpsc::TryRecvError::Disconnected)
                if matches!(progress, Err(mpsc::TryRecvError::Disconnected)) =>
            {
                Some(Event::Closed {
                    result: Err("Cast worker closed without a release acknowledgement"),
                    released: false,
                })
            }
            _ => None,
        };
        if terminal.is_some() {
            self.terminal_seen.set(true);
        }
        terminal
    }
    pub fn command(&self, command: Command) -> Result<(), &'static str> {
        if self.stop.load(Ordering::Acquire) {
            return Err("Cast session is stopping");
        }
        self.commands
            .try_send(command)
            .map_err(|_| "Cast command queue is busy or closed")
    }
    pub fn stop(&self) {
        self.stop.store(true, Ordering::Release);
    }
}
impl Drop for Handle {
    fn drop(&mut self) {
        self.stop();
    }
}
impl Control {
    pub fn set_waker(&mut self, wake: impl Fn() + Send + Sync + 'static) {
        self.wake = Some(Box::new(wake));
    }
    pub fn stopped(&self) -> bool {
        self.stop.load(Ordering::Acquire)
    }
    pub fn next(&self) -> Option<Command> {
        self.commands.try_recv().ok()
    }
    pub fn emit(&self, event: Event) -> Result<(), &'static str> {
        let result = self
            .events
            .try_send(event)
            .map_err(|_| "Cast event queue is busy or closed");
        if let Some(wake) = &self.wake {
            wake();
        }
        result
    }
    pub fn close(&self, result: Result<End, &'static str>, released: bool) {
        // Reserved terminal slot cannot be occupied by progress snapshots.
        let _ = self.closed.try_send(Event::Closed { result, released });
        if let Some(wake) = &self.wake {
            wake();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn artwork_is_public_https_only_and_metadata_is_bounded() {
        let mut metadata = Metadata {
            title: "x".repeat(300),
            artist: "Artist".into(),
            album: "Album".into(),
            artwork: Some("https://resources.tidal.com/images/ab/cd/640x640.jpg".into()),
        };
        let value = metadata.receiver_json();
        assert_eq!(
            value["images"][0]["url"],
            metadata.artwork.as_deref().unwrap()
        );
        assert_eq!(value["title"].as_str().unwrap().len(), 256);
        for url in [
            "http://resources.tidal.com/images/a.jpg",
            "https://evil.test/images/a.jpg",
            "https://resources.tidal.com/images/a.jpg?token=private",
            "https://user@resources.tidal.com/images/a.jpg",
            "https://resources.tidal.com/images/a.jpg#secret",
        ] {
            metadata.artwork = Some(url.into());
            assert!(metadata.receiver_json().get("images").is_none());
        }
    }
    #[test]
    fn cancellation_is_not_blocked_by_a_full_command_queue() {
        let (handle, control) = channel();
        for _ in 0..16 {
            handle.command(Command::Pause(true)).unwrap();
        }
        assert!(handle.command(Command::Pause(false)).is_err());
        handle.stop();
        assert!(control.stopped());
        assert!(handle.command(Command::Volume(0.5)).is_err());
    }
    #[test]
    fn delivered_release_cannot_turn_into_disappearance_after_worker_exit() {
        let (handle, control) = channel();
        control.close(Ok(End::Stopped), true);
        drop(control);
        assert!(matches!(
            handle.next_event(),
            Some(Event::Closed {
                result: Ok(End::Stopped),
                released: true
            })
        ));
        assert!(handle.next_event().is_none());
    }
    #[test]
    fn worker_disappearance_is_not_a_successful_release() {
        let (handle, control) = channel();
        drop(control);
        assert!(matches!(
            handle.next_event(),
            Some(Event::Closed {
                result: Err(_),
                released: false
            })
        ));
    }
    #[test]
    fn closure_has_a_reserved_slot_when_snapshots_are_full() {
        let (handle, control) = channel();
        for _ in 0..32 {
            control.emit(Event::Volume(0.5)).unwrap();
        }
        assert!(control.emit(Event::Volume(0.5)).is_err());
        control.close(Ok(End::Stopped), true);
        for _ in 0..32 {
            assert!(matches!(handle.next_event(), Some(Event::Volume(_))));
        }
        assert!(matches!(
            handle.next_event(),
            Some(Event::Closed {
                result: Ok(End::Stopped),
                released: true
            })
        ));
    }
    #[test]
    fn dropping_the_owner_requests_cleanup() {
        let (handle, control) = channel();
        drop(handle);
        assert!(control.stopped());
    }
}
