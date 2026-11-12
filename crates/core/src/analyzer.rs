//! Visualisation maths: an FFT spectrum analyser with peak-hold bars, and an oscilloscope.

use std::fmt;
use std::sync::Arc;

use rustfft::num_complex::Complex;
use rustfft::{Fft, FftPlanner};

/// Default number of spectrum bars (fits the classic 76-pixel visualiser at 4 px per bar).
pub const DEFAULT_BARS: usize = 19;
/// Default FFT length in samples.
pub const DEFAULT_FFT_SIZE: usize = 1024;
/// Level, in dBFS, that maps to an empty bar.
pub const FLOOR_DB: f32 = -60.0;
/// How fast bars fall, in full heights per second.
pub const BAR_FALL_RATE: f32 = 2.5;
/// How long a peak marker holds before falling, in seconds.
pub const PEAK_HOLD_SECS: f32 = 0.4;
/// How fast peak markers fall, in full heights per second.
pub const PEAK_FALL_RATE: f32 = 0.8;

/// Lowest and highest frequency shown by the analyser.
const MIN_FREQ: f32 = 50.0;
const MAX_FREQ: f32 = 16_000.0;

/// A spectrum analyser producing smoothed bar levels in `0.0..=1.0`.
#[derive(Clone)]
pub struct Analyzer {
    fft: Arc<dyn Fft<f32>>,
    size: usize,
    window: Vec<f32>,
    buffer: Vec<Complex<f32>>,
    scratch: Vec<Complex<f32>>,
    bins: Vec<(usize, usize)>,
    levels: Vec<f32>,
    peaks: Vec<f32>,
    hold: Vec<f32>,
    sample_rate: u32,
}

impl fmt::Debug for Analyzer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Analyzer")
            .field("size", &self.size)
            .field("bars", &self.levels.len())
            .field("sample_rate", &self.sample_rate)
            .finish_non_exhaustive()
    }
}

impl Default for Analyzer {
    fn default() -> Self {
        Self::new(DEFAULT_FFT_SIZE, DEFAULT_BARS, 44_100)
    }
}

impl Analyzer {
    /// Creates an analyser. `fft_size` is rounded up to a power of two (minimum 64) and
    /// `bars` to at least one.
    #[must_use]
    pub fn new(fft_size: usize, bars: usize, sample_rate: u32) -> Self {
        let size = fft_size.max(64).next_power_of_two();
        let bars = bars.max(1);
        let fft = FftPlanner::new().plan_fft_forward(size);
        let scratch = vec![Complex::default(); fft.get_inplace_scratch_len()];
        #[allow(clippy::cast_precision_loss)]
        let window = (0..size)
            .map(|i| 0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / (size - 1) as f32).cos())
            .collect();
        let mut analyzer = Self {
            fft,
            size,
            window,
            buffer: vec![Complex::default(); size],
            scratch,
            bins: Vec::new(),
            levels: vec![0.0; bars],
            peaks: vec![0.0; bars],
            hold: vec![0.0; bars],
            sample_rate: 0,
        };
        analyzer.set_sample_rate(sample_rate);
        analyzer
    }

    /// FFT length in samples.
    #[must_use]
    pub fn fft_size(&self) -> usize {
        self.size
    }

    /// Current sample rate.
    #[must_use]
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// Recomputes the bar-to-bin mapping for a new sample rate.
    pub fn set_sample_rate(&mut self, sample_rate: u32) {
        let sample_rate = sample_rate.max(1_000);
        if sample_rate == self.sample_rate {
            return;
        }
        self.sample_rate = sample_rate;
        #[allow(clippy::cast_precision_loss)]
        let fs = sample_rate as f32;
        #[allow(clippy::cast_precision_loss)]
        let n = self.size as f32;
        let bars = self.levels.len();
        let hi = MAX_FREQ.min(0.48 * fs);
        let lo = MIN_FREQ.min(hi / 2.0);
        let max_bin = self.size / 2 - 1;
        #[allow(clippy::cast_precision_loss)]
        let edge = |i: usize| lo * (hi / lo).powf(i as f32 / bars as f32);
        // Bin indices are small and non-negative, so float-to-usize casts are exact enough.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let to_bin = |f: f32| (f * n / fs) as usize;
        self.bins = (0..bars)
            .map(|i| {
                let first = to_bin(edge(i)).clamp(1, max_bin);
                let last = to_bin(edge(i + 1)).clamp(first, max_bin);
                (first, last)
            })
            .collect();
    }

    /// Bar heights in `0.0..=1.0`.
    #[must_use]
    pub fn levels(&self) -> &[f32] {
        &self.levels
    }

    /// Peak-hold marker heights in `0.0..=1.0`.
    #[must_use]
    pub fn peaks(&self) -> &[f32] {
        &self.peaks
    }

    /// Instantly empties all bars and peaks.
    pub fn reset(&mut self) {
        self.levels.fill(0.0);
        self.peaks.fill(0.0);
        self.hold.fill(0.0);
    }

    /// Instantaneous (unsmoothed) bar levels for the most recent `fft_size` mono samples;
    /// shorter input is zero-padded at the front.
    pub fn spectrum(&mut self, mono: &[f32]) -> Vec<f32> {
        let take = mono.len().min(self.size);
        let pad = self.size - take;
        let tail = &mono[mono.len() - take..];
        for (i, slot) in self.buffer.iter_mut().enumerate() {
            let sample = if i < pad { 0.0 } else { tail[i - pad] };
            *slot = Complex::new(sample * self.window[i], 0.0);
        }
        self.fft
            .process_with_scratch(&mut self.buffer, &mut self.scratch);
        // A full-scale sine through a Hann window peaks at |X| = n/4.
        #[allow(clippy::cast_precision_loss)]
        let norm = 4.0 / self.size as f32;
        self.bins
            .iter()
            .map(|&(first, last)| {
                let mag = self.buffer[first..=last]
                    .iter()
                    .map(|c| c.norm())
                    .fold(0.0f32, f32::max);
                let db = 20.0 * (mag * norm).max(1e-9).log10();
                ((db - FLOOR_DB) / -FLOOR_DB).clamp(0.0, 1.0)
            })
            .collect()
    }

    /// Feeds new audio and advances the smoothing by `dt` seconds.
    ///
    /// Bars jump up instantly and fall at [`BAR_FALL_RATE`]; peaks hold for
    /// [`PEAK_HOLD_SECS`] and then fall at [`PEAK_FALL_RATE`].
    pub fn update(&mut self, mono: &[f32], dt: f32) {
        let target = self.spectrum(mono);
        self.apply(&target, dt);
    }

    /// Lets the bars and peaks fall without new audio (paused/stopped).
    pub fn decay(&mut self, dt: f32) {
        let zeros = vec![0.0; self.levels.len()];
        self.apply(&zeros, dt);
    }

    fn apply(&mut self, target: &[f32], dt: f32) {
        let dt = dt.max(0.0);
        for (i, &t) in target.iter().enumerate() {
            let level = &mut self.levels[i];
            *level = if t >= *level {
                t
            } else {
                (*level - BAR_FALL_RATE * dt).max(t)
            };
            if *level >= self.peaks[i] {
                self.peaks[i] = *level;
                self.hold[i] = PEAK_HOLD_SECS;
            } else if self.hold[i] > 0.0 {
                self.hold[i] = (self.hold[i] - dt).max(0.0);
            } else {
                self.peaks[i] = (self.peaks[i] - PEAK_FALL_RATE * dt).max(*level);
            }
        }
    }
}

/// Averages interleaved stereo frames to mono. A trailing half-frame is ignored.
#[must_use]
pub fn downmix_stereo(interleaved: &[f32]) -> Vec<f32> {
    interleaved
        .chunks_exact(2)
        .map(|f| 0.5 * (f[0] + f[1]))
        .collect()
}

/// Picks `points` evenly spaced samples for the oscilloscope, clamped to `-1.0..=1.0`.
#[must_use]
pub fn oscilloscope(mono: &[f32], points: usize) -> Vec<f32> {
    if mono.is_empty() {
        return vec![0.0; points];
    }
    (0..points)
        .map(|i| {
            let idx = i * mono.len() / points.max(1);
            mono[idx.min(mono.len() - 1)].clamp(-1.0, 1.0)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[allow(clippy::cast_precision_loss)]
    fn sine(freq: f32, amp: f32, fs: u32, n: usize) -> Vec<f32> {
        (0..n)
            .map(|i| amp * (2.0 * std::f32::consts::PI * freq * i as f32 / fs as f32).sin())
            .collect()
    }

    fn argmax(v: &[f32]) -> usize {
        v.iter()
            .enumerate()
            .max_by(|a, b| a.1.total_cmp(b.1))
            .unwrap()
            .0
    }

    #[test]
    fn sizes_are_normalised() {
        let a = Analyzer::new(1000, 0, 44_100);
        assert_eq!(a.fft_size(), 1024);
        assert_eq!(a.levels().len(), 1);
        assert_eq!(Analyzer::new(3, 5, 44_100).fft_size(), 64);
        assert_eq!(Analyzer::default().levels().len(), DEFAULT_BARS);
        assert!(format!("{a:?}").contains("Analyzer"));
    }

    #[test]
    fn silence_is_empty() {
        let mut a = Analyzer::default();
        let s = a.spectrum(&vec![0.0; 2048]);
        assert!(s.iter().all(|&l| l == 0.0));
    }

    #[test]
    fn full_scale_tone_fills_its_bar() {
        let mut a = Analyzer::default();
        let s = a.spectrum(&sine(1_000.0, 1.0, 44_100, 1024));
        let peak = argmax(&s);
        assert!(s[peak] > 0.9, "{s:?}");
        // The loudest bar is the one whose bins contain 1 kHz.
        let (first, last) = a.bins[peak];
        let bin_1k = 1_000.0 * 1024.0 / 44_100.0;
        #[allow(clippy::cast_precision_loss)]
        let range = (first as f32 - 1.0)..=(last as f32 + 1.0);
        assert!(
            range.contains(&bin_1k),
            "bar {peak} covers {first}..={last}"
        );
        // Bars far away stay low.
        assert!(s[0] < 0.4 && s[DEFAULT_BARS - 1] < 0.4, "{s:?}");
    }

    #[test]
    fn higher_tones_light_higher_bars() {
        let mut a = Analyzer::default();
        let low = argmax(&a.spectrum(&sine(200.0, 0.8, 44_100, 1024)));
        let mid = argmax(&a.spectrum(&sine(2_000.0, 0.8, 44_100, 1024)));
        let high = argmax(&a.spectrum(&sine(10_000.0, 0.8, 44_100, 1024)));
        assert!(low < mid && mid < high, "{low} {mid} {high}");
    }

    #[test]
    fn quieter_tones_give_lower_bars() {
        let mut a = Analyzer::default();
        let loud = a.spectrum(&sine(1_000.0, 1.0, 44_100, 1024));
        let quiet = a.spectrum(&sine(1_000.0, 0.01, 44_100, 1024)); // -40 dB
        let i = argmax(&loud);
        assert!(
            (loud[i] - quiet[i] - 40.0 / 60.0).abs() < 0.1,
            "{} {}",
            loud[i],
            quiet[i]
        );
    }

    #[test]
    fn short_input_is_zero_padded() {
        let mut a = Analyzer::default();
        let s = a.spectrum(&sine(1_000.0, 1.0, 44_100, 100));
        assert!(s.iter().any(|&l| l > 0.0));
        assert!(a.spectrum(&[]).iter().all(|&l| l == 0.0));
    }

    #[test]
    fn bars_fall_and_peaks_hold() {
        let mut a = Analyzer::default();
        let tone = sine(1_000.0, 1.0, 44_100, 1024);
        a.update(&tone, 0.016);
        let i = argmax(a.levels());
        let top = a.levels()[i];
        assert!((a.peaks()[i] - top).abs() < f32::EPSILON);
        a.decay(0.1);
        let fallen = a.levels()[i];
        assert!(
            (top - fallen - 0.25).abs() < 1e-3,
            "falls at 2.5/s: {top} -> {fallen}"
        );
        assert!(
            (a.peaks()[i] - top).abs() < f32::EPSILON,
            "peak still holding"
        );
        a.decay(0.35); // hold expires
        a.decay(0.1);
        assert!(a.peaks()[i] < top);
        assert!(a.peaks()[i] >= a.levels()[i]);
        for _ in 0..100 {
            a.decay(0.05);
        }
        assert!(a.levels().iter().chain(a.peaks()).all(|&l| l == 0.0));
        a.update(&tone, 0.0);
        a.reset();
        assert!(a.levels().iter().chain(a.peaks()).all(|&l| l == 0.0));
    }

    #[test]
    fn sample_rate_changes_remap_bins() {
        let mut a = Analyzer::default();
        let before = a.bins.clone();
        a.set_sample_rate(8_000);
        assert_eq!(a.sample_rate(), 8_000);
        assert_ne!(a.bins, before);
        assert!(
            a.bins
                .iter()
                .all(|&(f, l)| f >= 1 && l >= f && l < a.fft_size() / 2)
        );
        a.set_sample_rate(0);
        assert_eq!(a.sample_rate(), 1_000, "absurd rates are clamped");
        let s = a.spectrum(&sine(200.0, 1.0, 1_000, 1024));
        assert!(s.iter().any(|&l| l > 0.5));
    }

    #[test]
    fn downmix_and_scope() {
        assert_eq!(downmix_stereo(&[1.0, 0.0, -1.0, -1.0, 0.5]), [0.5, -1.0]);
        assert_eq!(oscilloscope(&[], 3), [0.0, 0.0, 0.0]);
        assert_eq!(oscilloscope(&[0.0, 0.5, 2.0, -3.0], 2), [0.0, 1.0]);
        assert_eq!(oscilloscope(&[0.25], 3), [0.25, 0.25, 0.25]);
        assert!(oscilloscope(&[0.1, 0.2], 0).is_empty());
    }
}
