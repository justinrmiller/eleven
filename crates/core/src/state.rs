//! Playback state and the events the audio engine reports back to the application.

use std::time::Duration;

/// High-level transport state of the player.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PlaybackState {
    /// Nothing is playing; the position is reset.
    #[default]
    Stopped,
    /// Audio is being decoded and sent to the output.
    Playing,
    /// Playback is suspended but the position is kept.
    Paused,
}

impl PlaybackState {
    /// Returns `true` while audio is playing or paused (i.e. a track is loaded).
    #[must_use]
    pub fn is_active(self) -> bool {
        !matches!(self, Self::Stopped)
    }
}

/// Technical information about the track that is currently loaded.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TrackInfo {
    /// Title reported by the stream or file tags, if any.
    pub title: Option<String>,
    /// Native sample rate of the source in Hz.
    pub sample_rate: u32,
    /// Number of channels in the source (1 = mono, 2 = stereo, ...).
    pub channels: u16,
    /// Average (or nominal, for streams) bitrate in kbit/s.
    pub bitrate_kbps: Option<u32>,
    /// Total duration; `None` for live streams or unknown lengths.
    pub duration: Option<Duration>,
    /// Whether the source supports seeking.
    pub seekable: bool,
    /// Short codec name, e.g. `"mp3"` or `"flac"`.
    pub codec: String,
}

impl TrackInfo {
    /// Returns `true` when the source has more than one channel.
    #[must_use]
    pub fn is_stereo(&self) -> bool {
        self.channels >= 2
    }
}

/// Asynchronous notifications emitted by the audio engine.
#[derive(Debug, Clone, PartialEq)]
pub enum PlayerEvent {
    /// A track was opened successfully and playback has begun.
    Started(TrackInfo),
    /// A network stream is filling its buffer (0–100 %).
    Buffering(u8),
    /// A live stream announced a new title (Icecast `StreamTitle`).
    StreamTitle(String),
    /// The current track reached its natural end.
    Finished,
    /// The current track could not be opened or decoded.
    Error(String),
}
