//! OS media keys and "Now Playing" integration via souvlaki: MPRIS on Linux,
//! `MPRemoteCommandCenter` / `MPNowPlayingInfoCenter` on macOS, and the System Media Transport
//! Controls on Windows.
//!
//! Excluded from the coverage gate: it needs a D-Bus session, a macOS app session or a Windows
//! window.

use std::ffi::c_void;
use std::time::Duration;

use crossbeam_channel::{Receiver, Sender};
use eleven_core::PlaybackState;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use souvlaki::{
    MediaControlEvent, MediaControls as Souvlaki, MediaMetadata, MediaPlayback, MediaPosition,
    PlatformConfig,
};

use crate::backend::{MediaCommand, MediaControls, NoMediaControls};

/// Whether the controls need the main window: on Windows they attach to its `HWND`, so they
/// can only be created once eframe has opened it (see [`window_hwnd`]).
pub const NEEDS_WINDOW: bool = cfg!(windows);

/// The Win32 `HWND` of `window`, or `None` for other kinds of window or if it has no handle.
#[must_use]
pub fn window_hwnd(window: &impl HasWindowHandle) -> Option<*mut c_void> {
    match window.window_handle().ok()?.as_raw() {
        // The handle is an opaque integer to us; souvlaki passes it back to Windows.
        RawWindowHandle::Win32(handle) => Some(handle.hwnd.get() as *mut c_void),
        _ => None,
    }
}

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
    /// UI can wake up. `hwnd` is the main window, which Windows requires (see [`NEEDS_WINDOW`]).
    ///
    /// # Errors
    /// Returns a description if the platform service is unavailable, or if Windows has no
    /// window to attach to.
    pub fn new(
        notify: impl Fn() + Send + Sync + 'static,
        hwnd: Option<*mut c_void>,
    ) -> Result<Self, String> {
        // souvlaki panics without a window on Windows.
        if NEEDS_WINDOW && hwnd.is_none() {
            return Err("no window handle".into());
        }
        let config = PlatformConfig {
            display_name: "eleven",
            dbus_name: "eleven",
            hwnd,
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

/// Creates system media controls, or a no-op implementation if they are unavailable. `hwnd`
/// is the main window, which Windows requires (see [`NEEDS_WINDOW`]).
#[must_use]
pub fn create(
    notify: impl Fn() + Send + Sync + 'static,
    hwnd: Option<*mut c_void>,
) -> Box<dyn MediaControls> {
    if !session_bus_available() {
        log::info!("media keys unavailable: no D-Bus session bus");
        return Box::new(NoMediaControls);
    }
    match SystemMediaControls::new(notify, hwnd) {
        Ok(c) => Box::new(c),
        Err(e) => {
            log::info!("media keys unavailable: {e}");
            Box::new(NoMediaControls)
        }
    }
}
