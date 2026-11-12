//! Error type shared by the audio crate.

/// Everything that can go wrong while opening, decoding or playing audio.
#[derive(Debug, thiserror::Error)]
pub enum AudioError {
    /// The file could not be opened or read.
    #[error("cannot open {path}: {source}")]
    Open {
        /// What was being opened.
        path: String,
        /// The underlying I/O error.
        source: std::io::Error,
    },
    /// The container or codec is not supported.
    #[error("unsupported format: {0}")]
    Unsupported(String),
    /// The data could not be decoded.
    #[error("decode error: {0}")]
    Decode(String),
    /// Seeking failed or is not supported.
    #[error("cannot seek: {0}")]
    Seek(String),
    /// The audio output device failed.
    #[error("audio device error: {0}")]
    Device(String),
    /// A network stream failed.
    #[error("network error: {0}")]
    Network(String),
}

impl AudioError {
    pub(crate) fn from_symphonia(err: symphonia::core::errors::Error) -> Self {
        use symphonia::core::errors::Error as E;
        match err {
            E::Unsupported(what) => Self::Unsupported(what.to_owned()),
            E::IoError(e) => Self::Decode(format!("I/O: {e}")),
            other => Self::Decode(other.to_string()),
        }
    }
}

/// Convenience alias.
pub type Result<T> = std::result::Result<T, AudioError>;
