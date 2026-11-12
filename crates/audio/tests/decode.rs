//! Decoding the committed fixtures and generated WAVs.

mod common;

use std::io::Cursor;
use std::time::Duration;

use common::{fixture, frame_index, write_index_wav};
use eleven_audio::AudioError;
use eleven_audio::decoder::Decoder;
use symphonia::core::io::ReadOnlySource;

fn decode_all(d: &mut Decoder) -> Vec<f32> {
    let mut out = Vec::new();
    while d.decode_next(&mut out).unwrap() {}
    out
}

fn upward_crossings(stereo: &[f32]) -> usize {
    let left: Vec<f32> = stereo.iter().step_by(2).copied().collect();
    left.windows(2)
        .filter(|w| w[0] < 0.0 && w[1] >= 0.0)
        .count()
}

#[test]
fn every_fixture_decodes_with_expected_properties() {
    // (file, sample rate, channels, codec, lossless)
    let cases = [
        ("tone.wav", 22_050, 1, "pcm_s16le", true),
        ("tone.flac", 44_100, 2, "flac", true),
        ("tone.mp3", 44_100, 2, "mp3", false),
        ("tone.ogg", 44_100, 2, "vorbis", false),
        ("tone.m4a", 44_100, 2, "aac", false),
    ];
    for (name, rate, channels, codec, lossless) in cases {
        let mut d = Decoder::open_file(&fixture(name)).unwrap_or_else(|e| panic!("{name}: {e}"));
        let info = d.info().clone();
        assert_eq!(info.sample_rate, rate, "{name}");
        assert_eq!(d.sample_rate(), rate, "{name}");
        assert_eq!(info.channels, channels, "{name}");
        assert!(info.codec.contains(codec), "{name}: codec {}", info.codec);
        assert!(info.seekable, "{name}");
        assert_eq!(info.is_stereo(), channels == 2);
        let duration = info
            .duration
            .unwrap_or_else(|| panic!("{name}: no duration"));
        assert!(
            (duration.as_secs_f64() - 1.0).abs() < 0.08,
            "{name}: {duration:?}"
        );
        assert!(
            info.bitrate_kbps.is_some_and(|k| k > 10),
            "{name}: {:?}",
            info.bitrate_kbps
        );

        let samples = decode_all(&mut d);
        assert_eq!(samples.len() % 2, 0, "always stereo");
        let frames = samples.len() / 2;
        #[allow(clippy::cast_precision_loss)]
        let secs = frames as f64 / f64::from(rate);
        assert!((secs - 1.0).abs() < 0.08, "{name}: decoded {secs} s");
        // ffmpeg's sine source has amplitude 1/8; upmixing to stereo applies a -3 dB pan law.
        let expected_peak = if channels == 1 {
            0.125
        } else {
            0.125 * std::f32::consts::FRAC_1_SQRT_2
        };
        let peak = samples.iter().fold(0.0f32, |m, v| m.max(v.abs()));
        // Lossy codecs may overshoot (pre-echo, priming transients).
        let tolerance = if lossless { 0.005 } else { 0.04 };
        assert!(
            (peak - expected_peak).abs() < tolerance,
            "{name}: peak {peak}"
        );
        if lossless {
            let c = upward_crossings(&samples);
            assert!((995..=1_001).contains(&c), "{name}: {c} crossings");
        }
    }
}

#[test]
fn mono_is_duplicated_to_both_channels() {
    let mut d = Decoder::open_file(&fixture("tone.wav")).unwrap();
    let samples = decode_all(&mut d);
    assert!(
        samples
            .as_chunks::<2>()
            .0
            .iter()
            .all(|[l, r]| (l - r).abs() < f32::EPSILON)
    );
}

#[test]
fn seeking_is_sample_accurate() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("index.wav");
    write_index_wav(&path, 8_000, 8_000 * 6);
    let mut d = Decoder::open_file(&path).unwrap();
    for target in [2.5f64, 0.125, 5.0, 1.0] {
        let actual = d.seek(Duration::from_secs_f64(target)).unwrap();
        assert!((actual.as_secs_f64() - target).abs() < 1e-6);
        let mut out = Vec::new();
        assert!(d.decode_next(&mut out).unwrap());
        let first = frame_index(out[0], out[1]);
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let expected = (target * 8_000.0) as u32;
        assert!(
            first.abs_diff(expected) <= 1,
            "seek {target}: frame {first}, want {expected}"
        );
    }
    // Seeking past the end clamps to just before the end.
    let end = d.seek(Duration::from_secs(60)).unwrap();
    assert!(end < Duration::from_secs(6) && end > Duration::from_millis(5_990));
}

#[test]
fn compressed_fixtures_seek_and_continue() {
    for name in ["tone.flac", "tone.mp3", "tone.ogg"] {
        let mut d = Decoder::open_file(&fixture(name)).unwrap();
        d.seek(Duration::from_millis(500))
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        let rest = decode_all(&mut d);
        #[allow(clippy::cast_precision_loss)]
        let secs = rest.len() as f64 / 2.0 / 44_100.0;
        assert!(
            (secs - 0.5).abs() < 0.1,
            "{name}: {secs} s left after seeking to 0.5 s"
        );
    }
}

#[test]
fn errors_for_missing_and_corrupt_files() {
    let missing = Decoder::open_file(&fixture("does-not-exist.mp3")).unwrap_err();
    assert!(matches!(missing, AudioError::Open { .. }), "{missing}");
    assert!(missing.to_string().contains("does-not-exist"));
    let corrupt = Decoder::open_file(&fixture("corrupt.mp3"));
    assert!(corrupt.is_err(), "garbage must not decode");
}

#[test]
fn unseekable_sources_report_and_refuse_seeks() {
    let bytes = std::fs::read(fixture("tone.wav")).unwrap();
    let source = ReadOnlySource::new(Cursor::new(bytes));
    let mut d = Decoder::open_source(Box::new(source), Some("wav"), Some("audio/wav")).unwrap();
    assert!(!d.info().seekable);
    assert!(matches!(
        d.seek(Duration::from_millis(10)),
        Err(AudioError::Seek(_))
    ));
    assert!(!decode_all(&mut d).is_empty());
    assert!(format!("{d:?}").contains("Decoder"));
    d.info_mut().title = Some("x".into());
    assert_eq!(d.info().title.as_deref(), Some("x"));
}
