//! Shared helpers for the audio integration tests.
#![allow(dead_code)]

use std::io::Write;
use std::path::{Path, PathBuf};

/// Path of a committed fixture in `eleven/tests/fixtures`.
pub fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures")
        .join(name)
}

/// Writes a 16-bit PCM WAV file whose samples come from `sample(frame, channel)`.
pub fn write_wav(
    path: &Path,
    rate: u32,
    channels: u16,
    frames: u32,
    mut sample: impl FnMut(u32, u16) -> i16,
) {
    let data_len = frames * u32::from(channels) * 2;
    let mut bytes = Vec::with_capacity(44 + data_len as usize);
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + data_len).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16u32.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&channels.to_le_bytes());
    bytes.extend_from_slice(&rate.to_le_bytes());
    bytes.extend_from_slice(&(rate * u32::from(channels) * 2).to_le_bytes());
    bytes.extend_from_slice(&(channels * 2).to_le_bytes());
    bytes.extend_from_slice(&16u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&data_len.to_le_bytes());
    for f in 0..frames {
        for c in 0..channels {
            bytes.extend_from_slice(&sample(f, c).to_le_bytes());
        }
    }
    std::fs::File::create(path)
        .unwrap()
        .write_all(&bytes)
        .unwrap();
}

/// A stereo WAV whose frames encode their own index: left = index & 0x7fff, right = index >> 15.
pub fn write_index_wav(path: &Path, rate: u32, frames: u32) {
    write_wav(path, rate, 2, frames, |f, c| {
        // Both halves are < 2^15 for the lengths used in tests.
        #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
        let v = if c == 0 {
            (f & 0x7fff) as i16
        } else {
            (f >> 15) as i16
        };
        v
    });
}

/// Recovers the frame index from a decoded stereo frame of an index WAV.
pub fn frame_index(left: f32, right: f32) -> u32 {
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let l = (left * 32768.0).round() as u32;
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let r = (right * 32768.0).round() as u32;
    (r << 15) | l
}

/// A sine-tone WAV of `seconds` at `rate`.
pub fn write_tone_wav(path: &Path, rate: u32, seconds: f32) {
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss
    )]
    let frames = (rate as f32 * seconds) as u32;
    write_wav(path, rate, 2, frames, |f, _| {
        #[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
        let v =
            ((2.0 * std::f32::consts::PI * 440.0 * f as f32 / rate as f32).sin() * 8_000.0) as i16;
        v
    });
}
