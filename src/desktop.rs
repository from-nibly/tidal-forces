use crate::model::Track;
use anyhow::Result;
pub use souvlaki::MediaControlEvent;
use souvlaki::{MediaControls, MediaMetadata, MediaPlayback, MediaPosition, PlatformConfig};
use std::{sync::mpsc, time::Duration};

pub struct DesktopControls {
    controls: MediaControls,
    pub events: mpsc::Receiver<MediaControlEvent>,
    track: Option<u64>,
    playback: MediaPlayback,
    volume: u32,
}

impl DesktopControls {
    pub fn new(ctx: eframe::egui::Context) -> Result<Self> {
        let (tx, events) = mpsc::channel();
        let mut controls = MediaControls::new(PlatformConfig {
            dbus_name: "tidalforces",
            display_name: "Tidal Forces",
            hwnd: None,
        })?;
        controls.attach(move |event| {
            let _ = tx.send(event);
            ctx.request_repaint();
        })?;
        Ok(Self {
            controls,
            events,
            track: None,
            playback: MediaPlayback::Stopped,
            volume: u32::MAX,
        })
    }

    pub fn update(
        &mut self,
        track: Option<&Track>,
        paused: bool,
        seconds: u64,
        volume: f32,
    ) -> Result<()> {
        if self.track != track.map(|t| t.id) {
            let cover = track.and_then(|t| t.cover_url(320));
            self.controls.set_metadata(MediaMetadata {
                title: track.map(|t| t.title.as_str()),
                artist: track.map(|t| t.artist.name.as_str()),
                album: track.map(|t| t.album.title.as_str()),
                cover_url: cover.as_deref(),
                duration: track.map(|t| Duration::from_secs(t.duration)),
            })?;
            self.track = track.map(|t| t.id);
        }
        let progress = Some(MediaPosition(Duration::from_secs(seconds)));
        let playback = if track.is_none() {
            MediaPlayback::Stopped
        } else if paused {
            MediaPlayback::Paused { progress }
        } else {
            MediaPlayback::Playing { progress }
        };
        if playback != self.playback {
            self.controls.set_playback(playback.clone())?;
            self.playback = playback;
        }
        if volume.to_bits() != self.volume {
            self.controls.set_volume(volume as f64)?;
            self.volume = volume.to_bits();
        }
        Ok(())
    }
}
