//! The 10-band graphic equaliser: settings, presets and peaking-biquad filter design.
//!
//! Filters follow Robert Bristow-Johnson's "Audio EQ Cookbook" peaking EQ. Coefficients are
//! computed in `f64`; [`BiquadCoeffs::response_db`] evaluates the exact frequency response,
//! which the test-suite uses to check the design analytically.

use std::f64::consts::PI;

use serde::{Deserialize, Serialize};

/// Number of equaliser bands.
pub const BANDS: usize = 10;

/// Centre frequencies of the bands in Hz (the classic XMMS layout).
pub const BAND_FREQUENCIES: [f64; BANDS] = [
    60.0, 170.0, 310.0, 600.0, 1_000.0, 3_000.0, 6_000.0, 12_000.0, 14_000.0, 16_000.0,
];

/// Short labels for the band sliders.
pub const BAND_LABELS: [&str; BANDS] = [
    "60", "170", "310", "600", "1K", "3K", "6K", "12K", "14K", "16K",
];

/// Maximum boost/cut of a band or the preamp, in dB.
pub const MAX_GAIN_DB: f32 = 12.0;

/// Quality factor of each peaking filter (roughly one octave wide).
pub const BAND_Q: f64 = 1.2;

/// Bands at or above this fraction of the sample rate are skipped (too close to Nyquist).
pub const MAX_RELATIVE_FREQUENCY: f64 = 0.45;

/// Equaliser state as stored in the config and sent to the DSP.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct EqSettings {
    /// Whether the equaliser (including the preamp) is applied.
    pub enabled: bool,
    /// Gain applied before the bands, in dB.
    pub preamp_db: f32,
    /// Per-band gain in dB, ordered like [`BAND_FREQUENCIES`].
    pub bands_db: [f32; BANDS],
}

impl Default for EqSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            preamp_db: 0.0,
            bands_db: [0.0; BANDS],
        }
    }
}

fn clamp_gain(db: f32) -> f32 {
    if db.is_finite() {
        db.clamp(-MAX_GAIN_DB, MAX_GAIN_DB)
    } else {
        0.0
    }
}

impl EqSettings {
    /// Returns a copy with every gain clamped to ±[`MAX_GAIN_DB`] and NaNs zeroed.
    #[must_use]
    pub fn sanitized(&self) -> Self {
        Self {
            enabled: self.enabled,
            preamp_db: clamp_gain(self.preamp_db),
            bands_db: self.bands_db.map(clamp_gain),
        }
    }

    /// Copies the gains of a preset, keeping `enabled`.
    pub fn apply_preset(&mut self, preset: &EqPreset) {
        self.preamp_db = clamp_gain(preset.preamp_db);
        self.bands_db = preset.bands_db.map(clamp_gain);
    }

    /// Linear gain of the preamp (1.0 when the EQ is disabled).
    #[must_use]
    pub fn preamp_gain(&self) -> f32 {
        if self.enabled {
            db_to_gain(self.preamp_db)
        } else {
            1.0
        }
    }

    /// Designs the filter cascade for `sample_rate`. Disabled EQs and 0 dB bands yield no
    /// filters; bands too close to Nyquist are skipped.
    #[must_use]
    pub fn design(&self, sample_rate: u32) -> Vec<BiquadCoeffs> {
        if !self.enabled || sample_rate == 0 {
            return Vec::new();
        }
        let fs = f64::from(sample_rate);
        BAND_FREQUENCIES
            .iter()
            .zip(self.bands_db)
            .filter(|&(&f0, gain)| gain.abs() > 0.01 && f0 < MAX_RELATIVE_FREQUENCY * fs)
            .map(|(&f0, gain)| BiquadCoeffs::peaking(fs, f0, BAND_Q, f64::from(clamp_gain(gain))))
            .collect()
    }

    /// The overall response (preamp + all bands) at `freq` Hz, in dB.
    #[must_use]
    pub fn response_db(&self, freq: f64, sample_rate: u32) -> f64 {
        let preamp = if self.enabled {
            f64::from(clamp_gain(self.preamp_db))
        } else {
            0.0
        };
        preamp
            + self
                .design(sample_rate)
                .iter()
                .map(|c| c.response_db(freq, f64::from(sample_rate)))
                .sum::<f64>()
    }
}

/// Converts decibels to a linear amplitude factor.
#[must_use]
pub fn db_to_gain(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

/// Normalised biquad coefficients (`a0 == 1`) for a direct-form filter.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BiquadCoeffs {
    /// Feed-forward coefficient for x\[n\].
    pub b0: f64,
    /// Feed-forward coefficient for x\[n-1\].
    pub b1: f64,
    /// Feed-forward coefficient for x\[n-2\].
    pub b2: f64,
    /// Feedback coefficient for y\[n-1\].
    pub a1: f64,
    /// Feedback coefficient for y\[n-2\].
    pub a2: f64,
}

impl BiquadCoeffs {
    /// A pass-through filter.
    pub const IDENTITY: Self = Self {
        b0: 1.0,
        b1: 0.0,
        b2: 0.0,
        a1: 0.0,
        a2: 0.0,
    };

    /// RBJ peaking EQ centred at `f0` Hz with quality `q` and `gain_db` boost/cut.
    #[must_use]
    pub fn peaking(fs: f64, f0: f64, q: f64, gain_db: f64) -> Self {
        let a = 10f64.powf(gain_db / 40.0);
        let w0 = 2.0 * PI * f0 / fs;
        let (sin, cos) = w0.sin_cos();
        let alpha = sin / (2.0 * q);
        let a0 = 1.0 + alpha / a;
        Self {
            b0: (1.0 + alpha * a) / a0,
            b1: (-2.0 * cos) / a0,
            b2: (1.0 - alpha * a) / a0,
            a1: (-2.0 * cos) / a0,
            a2: (1.0 - alpha / a) / a0,
        }
    }

    /// Magnitude response at `freq` Hz for sample rate `fs`, in dB.
    #[must_use]
    pub fn response_db(&self, freq: f64, fs: f64) -> f64 {
        let w = 2.0 * PI * freq / fs;
        // H(e^jw) = (b0 + b1 z^-1 + b2 z^-2) / (1 + a1 z^-1 + a2 z^-2), z = e^jw.
        let (s1, c1) = w.sin_cos();
        let (s2, c2) = (2.0 * w).sin_cos();
        let num_re = self.b0 + self.b1 * c1 + self.b2 * c2;
        let num_im = -(self.b1 * s1 + self.b2 * s2);
        let den_re = 1.0 + self.a1 * c1 + self.a2 * c2;
        let den_im = -(self.a1 * s1 + self.a2 * s2);
        let num = num_re.hypot(num_im);
        let den = den_re.hypot(den_im);
        20.0 * (num / den).log10()
    }
}

/// A named set of equaliser gains.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EqPreset {
    /// Display name.
    pub name: String,
    /// Preamp gain in dB.
    pub preamp_db: f32,
    /// Band gains in dB.
    pub bands_db: [f32; BANDS],
}

impl EqPreset {
    /// Captures the gains of `settings` under `name`.
    #[must_use]
    pub fn from_settings(name: &str, settings: &EqSettings) -> Self {
        Self {
            name: name.to_owned(),
            preamp_db: settings.preamp_db,
            bands_db: settings.bands_db,
        }
    }
}

/// The presets shipped with eleven.
#[must_use]
pub fn builtin_presets() -> Vec<EqPreset> {
    let p = |name: &str, preamp_db: f32, bands_db: [f32; BANDS]| EqPreset {
        name: name.to_owned(),
        preamp_db,
        bands_db,
    };
    vec![
        p("Flat", 0.0, [0.0; BANDS]),
        p(
            "Rock",
            -3.0,
            [5.0, 3.0, -2.0, -4.0, -1.0, 2.0, 5.0, 6.0, 6.0, 6.0],
        ),
        p(
            "Pop",
            -2.0,
            [-1.0, 2.0, 4.0, 5.0, 3.0, 0.0, -1.0, -1.0, -1.0, -1.0],
        ),
        p(
            "Jazz",
            -1.0,
            [3.0, 2.0, 0.0, 2.0, -1.0, -1.0, 0.0, 2.0, 3.0, 3.0],
        ),
        p(
            "Classical",
            0.0,
            [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, -4.0, -4.0, -4.0, -6.0],
        ),
        p(
            "Dance",
            -4.0,
            [6.0, 5.0, 2.0, 0.0, 0.0, -3.0, -4.0, -4.0, 0.0, 0.0],
        ),
        p(
            "Vocal",
            -2.0,
            [-2.0, -3.0, -3.0, 1.0, 4.0, 4.0, 3.0, 1.0, 0.0, -2.0],
        ),
        p(
            "Full Bass",
            -6.0,
            [7.0, 7.0, 6.0, 3.0, 1.0, -2.0, -4.0, -5.0, -6.0, -6.0],
        ),
        p(
            "Full Treble",
            -7.0,
            [-6.0, -6.0, -6.0, -2.0, 1.0, 6.0, 9.0, 9.0, 9.0, 10.0],
        ),
        p(
            "Eleven",
            -5.0,
            [6.0, 5.0, 3.0, 0.0, -1.0, 1.0, 3.0, 5.0, 6.0, 6.0],
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    const FS: u32 = 44_100;

    fn single_band(band: usize, gain: f32) -> EqSettings {
        let mut s = EqSettings {
            enabled: true,
            ..EqSettings::default()
        };
        s.bands_db[band] = gain;
        s
    }

    #[test]
    fn identity_is_flat() {
        for f in [20.0, 1_000.0, 20_000.0] {
            assert!(BiquadCoeffs::IDENTITY.response_db(f, 44_100.0).abs() < 1e-12);
        }
    }

    #[test]
    fn each_band_hits_its_gain_at_centre() {
        for (band, &f0) in BAND_FREQUENCIES.iter().enumerate() {
            for gain in [-12.0f32, -6.0, 3.0, 12.0] {
                let s = single_band(band, gain);
                let got = s.response_db(f0, FS);
                assert!(
                    (got - f64::from(gain)).abs() < 0.1,
                    "band {band} gain {gain}: {got}"
                );
            }
        }
    }

    #[test]
    fn single_band_returns_to_unity_far_away() {
        let s = single_band(4, 12.0); // 1 kHz
        assert!(s.response_db(30.0, FS).abs() < 0.5);
        assert!(s.response_db(15_000.0, FS).abs() < 0.5);
        // ...and is boosted (less than the peak) half an octave away.
        let half_octave = s.response_db(1_414.0, FS);
        assert!(half_octave > 3.0 && half_octave < 12.0, "{half_octave}");
    }

    #[test]
    fn flat_and_disabled_are_unity() {
        let flat = EqSettings {
            enabled: true,
            ..EqSettings::default()
        };
        assert!(flat.design(FS).is_empty());
        let mut disabled = single_band(2, 9.0);
        disabled.enabled = false;
        disabled.preamp_db = 6.0;
        assert!(disabled.design(FS).is_empty());
        assert!((disabled.preamp_gain() - 1.0).abs() < f32::EPSILON);
        for f in [50.0, 500.0, 5_000.0] {
            assert!(flat.response_db(f, FS).abs() < 1e-9);
            assert!(disabled.response_db(f, FS).abs() < 1e-9);
        }
        assert!(
            flat.design(0).is_empty(),
            "a zero sample rate designs nothing"
        );
    }

    #[test]
    fn preamp_adds_constant_gain() {
        let s = EqSettings {
            enabled: true,
            preamp_db: -6.0,
            ..EqSettings::default()
        };
        assert!((s.response_db(1_000.0, FS) + 6.0).abs() < 1e-9);
        assert!((s.preamp_gain() - 0.501_187).abs() < 1e-4);
    }

    #[test]
    fn bands_near_nyquist_are_skipped() {
        let mut s = EqSettings {
            enabled: true,
            ..EqSettings::default()
        };
        s.bands_db = [6.0; BANDS];
        assert_eq!(s.design(44_100).len(), 10);
        // At 22.05 kHz the cut-off is 0.45 * fs = 9.9 kHz, so 12k/14k/16k are dropped.
        assert_eq!(s.design(22_050).len(), 7);
        // At 8 kHz the cut-off is 3.6 kHz: only 60 Hz .. 3 kHz remain.
        assert_eq!(s.design(8_000).len(), 6);
    }

    #[test]
    fn sanitize_clamps_and_zeroes_nan() {
        let s = EqSettings {
            enabled: true,
            preamp_db: f32::NAN,
            bands_db: [
                99.0,
                -99.0,
                f32::INFINITY,
                1.0,
                0.0,
                0.0,
                0.0,
                0.0,
                0.0,
                0.0,
            ],
        };
        let c = s.sanitized();
        assert!(c.preamp_db.abs() < f32::EPSILON);
        assert!((c.bands_db[0] - MAX_GAIN_DB).abs() < f32::EPSILON);
        assert!((c.bands_db[1] + MAX_GAIN_DB).abs() < f32::EPSILON);
        assert!(c.bands_db[2].abs() < f32::EPSILON);
        assert!((c.bands_db[3] - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn presets_are_valid_and_apply() {
        let presets = builtin_presets();
        assert!(presets.iter().any(|p| p.name == "Flat"));
        assert!(presets.iter().any(|p| p.name == "Eleven"));
        for p in &presets {
            assert!(
                p.bands_db
                    .iter()
                    .chain([&p.preamp_db])
                    .all(|g| g.abs() <= MAX_GAIN_DB)
            );
        }
        let mut s = EqSettings {
            enabled: true,
            ..EqSettings::default()
        };
        s.apply_preset(&presets[1]);
        assert_eq!(s.bands_db, presets[1].bands_db);
        assert!(s.enabled);
        let saved = EqPreset::from_settings("Mine", &s);
        assert_eq!(saved.name, "Mine");
        assert_eq!(saved.bands_db, s.bands_db);
    }

    #[test]
    fn db_conversion() {
        assert!((db_to_gain(0.0) - 1.0).abs() < 1e-6);
        assert!((db_to_gain(20.0) - 10.0).abs() < 1e-4);
        assert!((db_to_gain(-20.0) - 0.1).abs() < 1e-6);
    }

    proptest! {
        #[test]
        fn filters_are_stable(band in 0usize..BANDS, gain in -12.0f32..12.0, fs in prop::sample::select(vec![8_000u32, 22_050, 44_100, 48_000, 96_000])) {
            for c in single_band(band, gain).design(fs) {
                // Poles inside the unit circle: |a2| < 1 and |a1| < 1 + a2.
                prop_assert!(c.a2.abs() < 1.0);
                prop_assert!(c.a1.abs() < 1.0 + c.a2);
            }
        }
    }
}
