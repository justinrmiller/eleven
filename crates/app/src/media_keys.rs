//! OS media keys and "Now Playing" integration via souvlaki: MPRIS on Linux,
//! `MPRemoteCommandCenter` / `MPNowPlayingInfoCenter` on macOS.
//!
//! Excluded from the coverage gate: it needs a D-Bus session or a macOS app session.

use std::time::Duration;

use crossbeam_channel::{Receiver, Sender};
use eleven_core::PlaybackState;
use souvlaki::{
    MediaControlEvent, MediaControls as Souvlaki, MediaMetadata, MediaPlayback, MediaPosition,
    PlatformConfig,
};

use crate::backend::{MediaCommand, MediaControls, NoMediaControls};

/// Media controls backed by souvlaki.
pub struct SystemMediaControls {
    controls: Souvlaki,
    events: Receiver<MediaCommand>,
}

impl std::fmt::Debug for SystemMediaControls {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SystemMediaControls")
            .finish_non_exhaustive()
    }
}

/// Maps a souvlaki event to a command (seeking and volume requests are ignored).
#[must_use]
pub fn map_event(event: &MediaControlEvent) -> Option<MediaCommand> {
    Some(match event {
        MediaControlEvent::Play => MediaCommand::Play,
        MediaControlEvent::Pause => MediaCommand::Pause,
        MediaControlEvent::Toggle => MediaCommand::Toggle,
        MediaControlEvent::Next => MediaCommand::Next,
        MediaControlEvent::Previous => MediaCommand::Previous,
        MediaControlEvent::Stop => MediaCommand::Stop,
        _ => return None,
    })
}

impl SystemMediaControls {
    /// Registers with the OS. `notify` is called (from souvlaki's thread) on each event so the
    /// UI can wake up.
    ///
    /// # Errors
    /// Returns a description if the platform service is unavailable.
    pub fn new(notify: impl Fn() + Send + Sync + 'static) -> Result<Self, String> {
        let config = PlatformConfig {
            display_name: "eleven",
            dbus_name: "eleven",
            hwnd: None,
        };
        let mut controls = Souvlaki::new(config).map_err(|e| format!("{e:?}"))?;
        let (tx, rx): (Sender<MediaCommand>, Receiver<MediaCommand>) =
            crossbeam_channel::unbounded();
        controls
            .attach(move |event| {
                if let Some(cmd) = map_event(&event) {
                    let _ = tx.send(cmd);
                    notify();
                }
            })
            .map_err(|e| format!("{e:?}"))?;
        Ok(Self {
            controls,
            events: rx,
        })
    }
}

impl MediaControls for SystemMediaControls {
    fn poll(&mut self) -> Vec<MediaCommand> {
        self.events.try_iter().collect()
    }

    fn set_metadata(&mut self, title: &str, artist: Option<&str>, duration: Option<Duration>) {
        let meta = MediaMetadata {
            title: Some(title),
            artist,
            duration,
            ..MediaMetadata::default()
        };
        if let Err(e) = self.controls.set_metadata(meta) {
            log::debug!("media metadata: {e:?}");
        }
    }

    fn set_playback(&mut self, state: PlaybackState, position: Duration) {
        let progress = Some(MediaPosition(position));
        let playback = match state {
            PlaybackState::Playing => MediaPlayback::Playing { progress },
            PlaybackState::Paused => MediaPlayback::Paused { progress },
            PlaybackState::Stopped => MediaPlayback::Stopped,
        };
        if let Err(e) = self.controls.set_playback(playback) {
            log::debug!("media playback: {e:?}");
        }
    }
}

/// Whether a D-Bus session bus is reachable (MPRIS needs one; souvlaki panics on its own thread
/// without it). Always `true` off Linux.
#[must_use]
pub fn session_bus_available() -> bool {
    if !cfg!(target_os = "linux") {
        return true;
    }
    if std::env::var_os("DBUS_SESSION_BUS_ADDRESS").is_some_and(|v| !v.is_empty()) {
        return true;
    }
    std::env::var_os("XDG_RUNTIME_DIR")
        .map(std::path::PathBuf::from)
        .is_some_and(|dir| dir.join("bus").exists())
}

/// Creates system media controls, or a no-op implementation if they are unavailable.
#[must_use]
pub fn create(notify: impl Fn() + Send + Sync + 'static) -> Box<dyn MediaControls> {
    if !session_bus_available() {
        log::info!("media keys unavailable: no D-Bus session bus");
        return Box::new(NoMediaControls);
    }
    match SystemMediaControls::new(notify) {
        Ok(c) => Box::new(c),
        Err(e) => {
            log::info!("media keys unavailable: {e}");
            Box::new(NoMediaControls)
        }
    }
}
