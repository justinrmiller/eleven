//! Audio engine for the eleven player.
//!
//! * [`decoder`] — symphonia-based decoding to stereo `f32`, with accurate seeking.
//! * [`resample`] — streaming sample-rate conversion (rubato).
//! * [`dsp`] — the equaliser cascade, clip guard, volume and balance curves.
//! * [`stream`] — HTTP/Icecast sources with ICY metadata and pre-buffering.
//! * [`output`] — cpal and silent outputs fed through a lock-free ring buffer.
//! * [`engine`] — the playback thread and its handle.
//! * [`metadata`] — background tag scanning for playlist rows.

pub mod decoder;
pub mod dsp;
pub mod engine;
pub mod error;
pub mod metadata;
pub mod output;
pub mod resample;
pub mod stream;

pub use engine::{Engine, Notify};
pub use error::{AudioError, Result};
pub use metadata::{MetadataScanner, TagInfo};
pub use output::OutputSpec;
pub use output::cpal_out::list_devices;

pub(crate) use eleven_core::AUDIO_EXTENSIONS as KNOWN_EXTENSIONS;
