//! Real-time DSP: the equaliser filter cascade, preamp, clip guard, volume and balance.

use eleven_core::eq::{BANDS, BiquadCoeffs, EqSettings};

/// One biquad section in transposed direct form II, with `f64` state for low-frequency
/// precision.
#[derive(Debug, Clone, Copy)]
struct Biquad {
    c: BiquadCoeffs,
    z1: f64,
    z2: f64,
}

impl Biquad {
    fn new(c: BiquadCoeffs) -> Self {
        Self {
            c,
            z1: 0.0,
            z2: 0.0,
        }
    }

    fn clear(&mut self) {
        self.z1 = 0.0;
        self.z2 = 0.0;
    }

    #[inline]
    fn process(&mut self, x: f64) -> f64 {
        let y = self.c.b0 * x + self.z1;
        self.z1 = self.c.b1 * x - self.c.a1 * y + self.z2;
        self.z2 = self.c.b2 * x - self.c.a2 * y;
        y
    }
}

/// The equaliser for interleaved stereo audio at a fixed sample rate.
#[derive(Debug, Clone)]
pub struct Equalizer {
    sample_rate: u32,
    settings: EqSettings,
    preamp: f32,
    /// One `[left, right]` filter pair per band; `None` while the band is flat (bypassed).
    /// Each band keeps its own slot, so changing one band never disturbs another's state.
    filters: [Option<[Biquad; 2]>; BANDS],
}

impl Equalizer {
    /// Creates a (disabled) equaliser for `sample_rate`.
    #[must_use]
    pub fn new(sample_rate: u32) -> Self {
        Self {
            sample_rate,
            settings: EqSettings::default(),
            preamp: 1.0,
            filters: [None; BANDS],
        }
    }

    /// The current settings.
    #[must_use]
    pub fn settings(&self) -> &EqSettings {
        &self.settings
    }

    /// Applies new settings. Each band keeps its filter state while it stays active, so
    /// dragging a slider (even through 0 dB, which only bypasses that band) does not click.
    pub fn set_settings(&mut self, settings: &EqSettings) {
        self.settings = settings.sanitized();
        self.rebuild();
    }

    /// Changes the sample rate (redesigns the filters and clears their state).
    pub fn set_sample_rate(&mut self, sample_rate: u32) {
        if sample_rate != self.sample_rate {
            self.sample_rate = sample_rate;
            self.filters = [None; BANDS];
            self.rebuild();
        }
    }

    fn rebuild(&mut self) {
        self.preamp = self.settings.preamp_gain();
        let design = self.settings.design_bands(self.sample_rate);
        for (slot, coeffs) in self.filters.iter_mut().zip(design) {
            match (slot.as_mut(), coeffs) {
                // Still active: new coefficients, same state.
                (Some(pair), Some(c)) => {
                    pair[0].c = c;
                    pair[1].c = c;
                }
                // Newly active: a bypassed band starts from rest.
                (None, Some(c)) => *slot = Some([Biquad::new(c), Biquad::new(c)]),
                (_, None) => *slot = None,
            }
        }
    }

    /// Clears filter memory (after a seek or track change).
    pub fn reset(&mut self) {
        for f in self.filters.iter_mut().flatten().flatten() {
            f.clear();
        }
    }

    /// `true` if processing would change the signal.
    #[must_use]
    pub fn is_active(&self) -> bool {
        self.filters.iter().any(Option::is_some) || (self.preamp - 1.0).abs() > f32::EPSILON
    }

    /// Processes interleaved stereo audio in place.
    pub fn process(&mut self, interleaved: &mut [f32]) {
        if !self.is_active() {
            return;
        }
        for frame in interleaved.as_chunks_mut::<2>().0 {
            for (ch, sample) in frame.iter_mut().enumerate() {
                let mut x = f64::from(*sample * self.preamp);
                for pair in self.filters.iter_mut().flatten() {
                    x = pair[ch].process(x);
                }
                // Narrowing back to the f32 output format is intended.
                #[allow(clippy::cast_possible_truncation)]
                let y = x as f32;
                *sample = soft_clip(y);
            }
        }
    }
}

/// Transparent below ±0.9, then smoothly saturates so that boosts never exceed ±1.0.
#[inline]
#[must_use]
pub fn soft_clip(x: f32) -> f32 {
    const KNEE: f32 = 0.9;
    let a = x.abs();
    if a <= KNEE {
        x
    } else {
        x.signum() * (KNEE + (1.0 - KNEE) * ((a - KNEE) / (1.0 - KNEE)).tanh())
    }
}

/// Maps the volume slider (`0.0..=1.0`) to a linear gain using a perceptual square curve.
#[must_use]
pub fn volume_gain(volume: f32) -> f32 {
    let v = if volume.is_finite() {
        volume.clamp(0.0, 1.0)
    } else {
        0.0
    };
    v * v
}

/// Left/right gains for a balance in `-1.0..=1.0`: the far side is attenuated linearly.
#[must_use]
pub fn balance_gains(balance: f32) -> (f32, f32) {
    let b = if balance.is_finite() {
        balance.clamp(-1.0, 1.0)
    } else {
        0.0
    };
    ((1.0 - b).min(1.0), (1.0 + b).min(1.0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use eleven_core::eq::{BAND_FREQUENCIES, BANDS};

    const FS: u32 = 44_100;

    #[allow(clippy::cast_precision_loss)]
    fn sine(freq: f64, amp: f32, frames: usize) -> Vec<f32> {
        (0..frames)
            .flat_map(|i| {
                #[allow(clippy::cast_possible_truncation)]
                let v = amp
                    * (2.0 * std::f64::consts::PI * freq * i as f64 / f64::from(FS)).sin() as f32;
                [v, v]
            })
            .collect()
    }

    fn rms(x: &[f32]) -> f64 {
        #[allow(clippy::cast_precision_loss)]
        let n = x.len() as f64;
        (x.iter().map(|&v| f64::from(v) * f64::from(v)).sum::<f64>() / n).sqrt()
    }

    #[test]
    fn disabled_eq_is_bit_transparent() {
        let mut eq = Equalizer::new(FS);
        let input = sine(1_000.0, 0.5, 512);
        let mut buf = input.clone();
        eq.process(&mut buf);
        assert_eq!(buf, input);
        assert!(!eq.is_active());
    }

    #[test]
    fn measured_gain_matches_analytic_response() {
        // Boost one band at a time and compare the steady-state RMS with the design response.
        for (band, &f0) in BAND_FREQUENCIES.iter().enumerate().step_by(2) {
            let mut settings = EqSettings {
                enabled: true,
                ..EqSettings::default()
            };
            settings.bands_db[band] = 6.0;
            let mut eq = Equalizer::new(FS);
            eq.set_settings(&settings);
            assert_eq!(eq.settings(), &settings);
            let input = sine(f0, 0.1, 44_100);
            let mut out = input.clone();
            eq.process(&mut out);
            // Skip the first 0.25 s while the filters settle.
            let measured = 20.0 * (rms(&out[22_050..]) / rms(&input[22_050..])).log10();
            let expected = settings.response_db(f0, FS);
            assert!(
                (measured - expected).abs() < 0.2,
                "band {band}: {measured} vs {expected}"
            );
        }
    }

    #[test]
    fn preamp_scales_the_signal() {
        let mut eq = Equalizer::new(FS);
        eq.set_settings(&EqSettings {
            enabled: true,
            preamp_db: -6.0,
            bands_db: [0.0; BANDS],
        });
        assert!(eq.is_active());
        let mut buf = vec![0.5f32; 8];
        eq.process(&mut buf);
        assert!(buf.iter().all(|&v| (v - 0.5 * 0.501_187).abs() < 1e-4));
    }

    #[test]
    fn heavy_boost_never_exceeds_full_scale() {
        let mut eq = Equalizer::new(FS);
        eq.set_settings(&EqSettings {
            enabled: true,
            preamp_db: 12.0,
            bands_db: [12.0; BANDS],
        });
        let mut buf = sine(100.0, 1.0, 10_000);
        eq.process(&mut buf);
        assert!(buf.iter().all(|v| v.abs() <= 1.0 && v.is_finite()));
    }

    #[test]
    fn settings_changes_keep_or_rebuild_filters() {
        let mut eq = Equalizer::new(FS);
        let mut s = EqSettings {
            enabled: true,
            ..EqSettings::default()
        };
        s.bands_db[0] = 3.0;
        eq.set_settings(&s);
        let mut buf = sine(60.0, 0.5, 256);
        eq.process(&mut buf);
        let state = |eq: &Equalizer, band: usize| eq.filters[band].map(|p| p[0].z1);
        let state_before = state(&eq, 0);
        assert!(state_before.is_some_and(|z| z != 0.0));
        s.bands_db[0] = 4.0; // same band, new gain: state kept
        eq.set_settings(&s);
        assert_eq!(state(&eq, 0), state_before);
        s.bands_db[1] = 4.0; // another band switched on: band 0 untouched, band 1 at rest
        eq.set_settings(&s);
        assert_eq!(state(&eq, 0), state_before);
        assert_eq!(state(&eq, 1), Some(0.0));
        eq.reset();
        assert!(
            eq.filters
                .iter()
                .flatten()
                .flatten()
                .all(|f| f.z1 == 0.0 && f.z2 == 0.0)
        );
        eq.set_sample_rate(22_050);
        eq.set_sample_rate(22_050);
        assert_eq!(eq.filters.iter().flatten().count(), 2);
        s.bands_db = [6.0; BANDS];
        eq.set_settings(&s);
        assert_eq!(
            eq.filters.iter().flatten().count(),
            7,
            "bands above 0.45 fs are skipped at 22.05 kHz"
        );
    }

    #[test]
    fn a_band_crossing_zero_leaves_the_others_alone() {
        let mut s = EqSettings {
            enabled: true,
            ..EqSettings::default()
        };
        s.bands_db[2] = 6.0; // 310 Hz
        s.bands_db[4] = 6.0; // 1 kHz
        let mut eq = Equalizer::new(FS);
        eq.set_settings(&s);
        let mut buf = sine(310.0, 0.3, 4_096);
        eq.process(&mut buf);
        let before = eq.filters[4];
        // Dragging the 310 Hz slider through the centre bypasses only that band...
        s.bands_db[2] = 0.0;
        eq.set_settings(&s);
        assert!(eq.filters[2].is_none());
        let after = eq.filters[4].unwrap();
        assert_eq!(before.unwrap()[0].z1, after[0].z1, "1 kHz state kept");
        // ...and a band switched on at the same time starts from rest instead of inheriting
        // another band's state (the old code kept state per position in a compacted list).
        s.bands_db[8] = 6.0; // 12 kHz
        eq.set_settings(&s);
        assert!(eq.filters[8].is_some_and(|p| p[0].z1 == 0.0 && p[1].z2 == 0.0));
        // Output matches a fresh equaliser with the same bands, apart from band 4's history.
        let mut fresh = Equalizer::new(FS);
        fresh.set_settings(&s);
        fresh.filters[4] = eq.filters[4];
        let mut a = sine(1_000.0, 0.3, 512);
        let mut b = a.clone();
        eq.process(&mut a);
        fresh.process(&mut b);
        assert_eq!(a, b);
    }

    #[test]
    fn soft_clip_shape() {
        assert!((soft_clip(0.5) - 0.5).abs() < f32::EPSILON);
        assert!((soft_clip(-0.9) + 0.9).abs() < f32::EPSILON);
        assert!(soft_clip(1.0) > 0.9 && soft_clip(1.0) < 1.0);
        assert!(soft_clip(100.0) <= 1.0);
        assert!(soft_clip(-100.0) >= -1.0);
        // Monotonic.
        let mut prev = -2.0f32;
        for i in -300..300 {
            #[allow(clippy::cast_precision_loss)]
            let y = soft_clip(i as f32 / 100.0);
            assert!(y >= prev);
            prev = y;
        }
    }

    #[test]
    fn volume_and_balance_curves() {
        assert!(volume_gain(0.0).abs() < f32::EPSILON);
        assert!((volume_gain(1.0) - 1.0).abs() < f32::EPSILON);
        assert!((volume_gain(0.5) - 0.25).abs() < f32::EPSILON);
        assert!((volume_gain(3.0) - 1.0).abs() < f32::EPSILON);
        assert!(volume_gain(f32::NAN).abs() < f32::EPSILON);
        assert_eq!(balance_gains(0.0), (1.0, 1.0));
        assert_eq!(balance_gains(-1.0), (1.0, 0.0));
        assert_eq!(balance_gains(0.5), (0.5, 1.0));
        assert_eq!(balance_gains(9.0), (0.0, 1.0));
        assert_eq!(balance_gains(f32::NAN), (1.0, 1.0));
    }
}
