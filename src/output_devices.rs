use serde::{Deserialize, Serialize};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Target {
    #[default]
    SystemDefault,
    Pulse(String, String),
    CompatibilityDefault,
    RequireSelection,
}
pub const DEFAULT_LABEL: &str = if cfg!(target_os = "linux") {
    "System default · lock when opened"
} else {
    "System default · no physical route guard"
};

#[derive(Clone)]
pub struct Device {
    pub target: Target,
    pub label: String,
}
#[derive(Clone, Default)]
pub struct Snapshot {
    pub devices: Vec<Device>,
    pub unavailable: bool,
    pub requested: bool,
}
#[derive(Default)]
pub struct Catalog {
    state: Arc<Mutex<Snapshot>>,
    busy: Arc<AtomicBool>,
    events: Option<crate::backend::Events>,
}
impl Catalog {
    pub fn new(events: crate::backend::Events) -> Self {
        Self {
            events: Some(events),
            ..Self::default()
        }
    }
    #[cfg(any(test, debug_assertions))]
    pub fn fixture(devices: Vec<Device>) -> Self {
        Self {
            state: Arc::new(Mutex::new(Snapshot {
                devices,
                requested: true,
                unavailable: false,
            })),
            ..Self::default()
        }
    }
    pub fn snapshot(&self) -> Snapshot {
        self.state.lock().unwrap().clone()
    }
    pub fn busy(&self) -> bool {
        self.busy.load(Ordering::Acquire)
    }
    pub fn refresh(&self) {
        let Some(events) = self.events.clone() else {
            return;
        };
        if self.busy.swap(true, Ordering::AcqRel) {
            return;
        }
        self.state.lock().unwrap().requested = true;
        let state = self.state.clone();
        let busy = self.busy.clone();
        let spawn = std::thread::Builder::new()
            .name("audio-outputs".into())
            .spawn(move || {
                #[cfg(target_os = "linux")]
                let result = crate::pulse_output::devices();
                #[cfg(not(target_os = "linux"))]
                let result: anyhow::Result<Vec<Device>> = Ok(Vec::new());
                *state.lock().unwrap() = match result {
                    Ok(devices) => Snapshot {
                        devices,
                        unavailable: false,
                        requested: true,
                    },
                    Err(_) => Snapshot {
                        unavailable: true,
                        requested: true,
                        ..Default::default()
                    },
                };
                busy.store(false, Ordering::Release);
                events.send(crate::backend::Event::OutputsChanged);
            });
        if spawn.is_err() {
            self.busy.store(false, Ordering::Release);
            self.state.lock().unwrap().unavailable = true;
        }
    }
}
