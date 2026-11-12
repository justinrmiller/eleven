//! Streaming sample-rate conversion of interleaved stereo audio (rubato FFT resampler).

use rubato::audioadapter_buffers::direct::InterleavedSlice;
use rubato::{Fft, FixedSync, Indexing, Resampler};

use crate::error::{AudioError, Result};

const CHANNELS: usize = 2;
const CHUNK_FRAMES: usize = 1024;

/// Converts a continuous interleaved stereo stream from one fixed rate to another.
///
/// The resampler's start-up delay is trimmed and [`StreamResampler::finish`] flushes the tail,
/// so the output lines up with the input and has the expected length.
pub struct StreamResampler {
    inner: Fft<f32>,
    from: u32,
    to: u32,
    pending: Vec<f32>,
    out_buf: Vec<f32>,
    delay_left: usize,
    frames_in: u64,
    frames_out: u64,
}

impl std::fmt::Debug for StreamResampler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StreamResampler")
            .field("from", &self.from)
            .field("to", &self.to)
            .finish_non_exhaustive()
    }
}

impl StreamResampler {
    /// Creates a resampler from `from` Hz to `to` Hz.
    ///
    /// # Errors
    /// Fails if either rate is zero or the ratio is unsupported.
    pub fn new(from: u32, to: u32) -> Result<Self> {
        if from == 0 || to == 0 {
            return Err(AudioError::Unsupported(format!(
                "sample rate {from} -> {to}"
            )));
        }
        let inner = Fft::<f32>::new(
            from as usize,
            to as usize,
            CHUNK_FRAMES,
            CHANNELS,
            FixedSync::Input,
        )
        .map_err(|e| AudioError::Unsupported(format!("resampler: {e}")))?;
        let out_buf = vec![0.0; inner.output_frames_max() * CHANNELS];
        let delay_left = inner.output_delay();
        Ok(Self {
            inner,
            from,
            to,
            pending: Vec::with_capacity(CHUNK_FRAMES * CHANNELS * 2),
            out_buf,
            delay_left,
            frames_in: 0,
            frames_out: 0,
        })
    }

    /// Input rate in Hz.
    #[must_use]
    pub fn from_rate(&self) -> u32 {
        self.from
    }

    /// Output rate in Hz.
    #[must_use]
    pub fn to_rate(&self) -> u32 {
        self.to
    }

    /// Feeds interleaved stereo input and appends any completed output to `out`.
    pub fn process(&mut self, input: &[f32], out: &mut Vec<f32>) {
        self.frames_in += (input.len() / CHANNELS) as u64;
        self.pending.extend_from_slice(input);
        loop {
            let needed = self.inner.input_frames_next();
            if self.pending.len() < needed * CHANNELS {
                break;
            }
            self.run_chunk(needed, None, out);
            self.pending.drain(..needed * CHANNELS);
        }
    }

    /// Flushes buffered input at the end of the stream, appending the remaining output.
    pub fn finish(&mut self, out: &mut Vec<f32>) {
        let expected = self.expected_output_frames();
        // A few zero-padded chunks are enough to push out the filter delay.
        for _ in 0..8 {
            if self.frames_out >= expected {
                break;
            }
            let needed = self.inner.input_frames_next();
            let valid = (self.pending.len() / CHANNELS).min(needed);
            self.pending.resize(needed * CHANNELS, 0.0);
            let before = out.len();
            self.run_chunk(needed, Some(valid), out);
            self.pending.clear();
            // Trim anything beyond the expected length.
            let excess = usize::try_from(self.frames_out.saturating_sub(expected)).unwrap_or(0);
            if excess > 0 {
                let new_len = out.len().saturating_sub(excess * CHANNELS).max(before);
                out.truncate(new_len);
                self.frames_out = expected;
            }
        }
    }

    /// Discards all buffered audio (after a seek).
    pub fn reset(&mut self) {
        self.inner.reset();
        self.pending.clear();
        self.delay_left = self.inner.output_delay();
        self.frames_in = 0;
        self.frames_out = 0;
    }

    fn expected_output_frames(&self) -> u64 {
        // Round to the nearest frame: frames_in * to / from.
        (self.frames_in * u64::from(self.to) + u64::from(self.from) / 2) / u64::from(self.from)
    }

    fn run_chunk(&mut self, needed: usize, partial: Option<usize>, out: &mut Vec<f32>) {
        let out_frames = self.out_buf.len() / CHANNELS;
        let Ok(input) = InterleavedSlice::new(&self.pending[..needed * CHANNELS], CHANNELS, needed)
        else {
            return;
        };
        let Ok(mut output) = InterleavedSlice::new_mut(&mut self.out_buf, CHANNELS, out_frames)
        else {
            return;
        };
        let mut indexing = Indexing::new();
        indexing.partial_len = partial;
        let Ok((_, produced)) =
            self.inner
                .process_into_buffer(&input, &mut output, Some(&indexing))
        else {
            return;
        };
        let skip = self.delay_left.min(produced);
        self.delay_left -= skip;
        out.extend_from_slice(&self.out_buf[skip * CHANNELS..produced * CHANNELS]);
        self.frames_out += (produced - skip) as u64;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[allow(clippy::cast_precision_loss)]
    fn stereo_sine(freq: f32, rate: u32, frames: usize) -> Vec<f32> {
        (0..frames)
            .flat_map(|i| {
                let v = (2.0 * std::f32::consts::PI * freq * i as f32 / rate as f32).sin() * 0.5;
                [v, v]
            })
            .collect()
    }

    /// Counts upward zero crossings of the left channel.
    fn crossings(stereo: &[f32]) -> usize {
        let left: Vec<f32> = stereo.iter().step_by(2).copied().collect();
        left.windows(2)
            .filter(|w| w[0] < 0.0 && w[1] >= 0.0)
            .count()
    }

    #[test]
    fn rejects_zero_rates() {
        assert!(StreamResampler::new(0, 48_000).is_err());
        assert!(StreamResampler::new(44_100, 0).is_err());
    }

    #[test]
    fn output_length_matches_ratio_for_odd_chunking() {
        for (from, to) in [
            (22_050, 48_000),
            (44_100, 48_000),
            (48_000, 44_100),
            (96_000, 44_100),
        ] {
            let mut r = StreamResampler::new(from, to).unwrap();
            assert_eq!((r.from_rate(), r.to_rate()), (from, to));
            let input = stereo_sine(440.0, from, from as usize); // one second
            let mut out = Vec::new();
            for chunk in input.chunks(2 * 333) {
                r.process(chunk, &mut out);
            }
            r.finish(&mut out);
            assert_eq!(out.len() % 2, 0);
            let frames = out.len() / 2;
            assert!(
                (frames as i64 - i64::from(to)).abs() <= 1,
                "{from}->{to}: {frames} frames"
            );
        }
    }

    #[test]
    fn frequency_is_preserved() {
        let mut r = StreamResampler::new(22_050, 48_000).unwrap();
        let mut out = Vec::new();
        r.process(&stereo_sine(1_000.0, 22_050, 22_050), &mut out);
        r.finish(&mut out);
        // One second of a 1 kHz tone has ~1000 upward zero crossings at any rate.
        let c = crossings(&out);
        assert!((995..=1_001).contains(&c), "{c} crossings");
        // The amplitude survives too.
        let peak = out.iter().fold(0.0f32, |m, v| m.max(v.abs()));
        assert!((peak - 0.5).abs() < 0.05, "peak {peak}");
    }

    #[test]
    fn reset_clears_state() {
        let mut r = StreamResampler::new(44_100, 48_000).unwrap();
        let mut out = Vec::new();
        r.process(&stereo_sine(440.0, 44_100, 5_000), &mut out);
        r.reset();
        let mut out2 = Vec::new();
        r.finish(&mut out2);
        assert!(out2.is_empty(), "nothing to flush after a reset");
        assert!(format!("{r:?}").contains("44100"));
    }
}
