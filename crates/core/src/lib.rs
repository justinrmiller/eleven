//! Pure, I/O-light logic for the eleven audio player.
//!
//! Nothing in this crate touches audio devices, the GUI or the network, so it can be tested
//! exhaustively and quickly:
//!
//! * [`playlist`] — the playlist model with selection and shuffle/repeat navigation.
//! * [`playlist_io`] — M3U/EXTM3U and PLS reading and writing.
//! * [`eq`] — equaliser settings, presets and biquad design.
//! * [`analyzer`] — spectrum analyser and oscilloscope maths.
//! * [`icy`] — Icecast/Shoutcast in-band metadata.
//! * [`format`](mod@format) — clock, duration, marquee and indicator text.
//! * [`config`] — persisted settings.
//! * [`state`] — playback state and engine events.

pub mod analyzer;
pub mod config;
pub mod eq;
pub mod format;
pub mod icy;
pub mod playlist;
pub mod playlist_io;
pub mod state;

pub use config::{Config, VisMode};
pub use eq::{EqPreset, EqSettings};
pub use format::TimeMode;
pub use playlist::{Entry, EntryId, Location, Playlist, SelectMode, SortKey};
pub use state::{PlaybackState, PlayerEvent, TrackInfo};

/// File extensions (lower-case) the player will add from folders and drops.
pub const AUDIO_EXTENSIONS: &[&str] = &[
    "mp3", "mp2", "mp1", "flac", "ogg", "oga", "wav", "wave", "aif", "aiff", "m4a", "mp4", "aac",
    "caf", "mka", "webm",
];

/// Returns `true` if `path` has an audio file extension the player understands.
#[must_use]
pub fn is_audio_path(path: &std::path::Path) -> bool {
    path.extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .is_some_and(|e| AUDIO_EXTENSIONS.contains(&e.as_str()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn audio_extensions() {
        assert!(is_audio_path(Path::new("/x/Song.MP3")));
        assert!(is_audio_path(Path::new("a.flac")));
        assert!(!is_audio_path(Path::new("cover.jpg")));
        assert!(!is_audio_path(Path::new("noext")));
    }
}
