//! Tag reading for playlist rows.

mod common;

use std::sync::Arc;
use std::time::Duration;

use common::{fixture, write_tone_wav};
use eleven_audio::metadata::read_tags;
use eleven_audio::{MetadataScanner, TagInfo};
use eleven_core::{Location, Playlist};

#[test]
fn fixtures_have_tags_and_durations() {
    for name in ["tone.mp3", "tone.flac", "tone.ogg", "tone.m4a", "tone.wav"] {
        let tags = read_tags(&fixture(name)).unwrap_or_else(|| panic!("{name}"));
        assert_eq!(tags.title.as_deref(), Some("Test Tone"), "{name}");
        assert_eq!(tags.artist.as_deref(), Some("eleven"), "{name}");
        let d = tags
            .duration
            .unwrap_or_else(|| panic!("{name}: no duration"));
        assert!((d.as_secs_f64() - 1.0).abs() < 0.1, "{name}: {d:?}");
    }
    assert_eq!(
        read_tags(&fixture("tone.flac")).unwrap().album.as_deref(),
        Some("Fixtures")
    );
}

#[test]
fn untagged_and_unreadable_files() {
    let dir = tempfile::tempdir().unwrap();
    let wav = dir.path().join("plain.wav");
    write_tone_wav(&wav, 8_000, 2.0);
    let tags = read_tags(&wav).unwrap();
    assert_eq!(tags.title, None);
    assert!(
        tags.duration
            .is_some_and(|d| (d.as_secs_f64() - 2.0).abs() < 0.01)
    );
    assert!(read_tags(&fixture("corrupt.mp3")).is_none_or(|t| t.duration.is_none()));
    assert!(read_tags(&dir.path().join("missing.ogg")).is_none());
}

#[test]
fn scanner_reports_results_by_entry_id() {
    let notified = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counter = Arc::clone(&notified);
    let notify: eleven_audio::Notify = Arc::new(move || {
        counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    });
    let scanner = MetadataScanner::new(2, &notify);
    let mut playlist = Playlist::new();
    let a = playlist.push(Location::File(fixture("tone.mp3")));
    let b = playlist.push(Location::File(fixture("missing.mp3")));
    scanner.request(a, fixture("tone.mp3"));
    scanner.request(b, fixture("missing.mp3"));
    let mut results = Vec::new();
    while results.len() < 2 {
        results.push(scanner.wait(Duration::from_secs(10)).expect("scan result"));
    }
    results.extend(scanner.poll());
    let found = results.iter().find(|(id, _)| *id == a).unwrap();
    assert_eq!(found.1.title.as_deref(), Some("Test Tone"));
    let missing = results.iter().find(|(id, _)| *id == b).unwrap();
    assert_eq!(missing.1, TagInfo::default());
    // Workers notify right after sending each result, so the last call can trail `wait`.
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while notified.load(std::sync::atomic::Ordering::Relaxed) < 2 {
        assert!(
            std::time::Instant::now() < deadline,
            "notify was not called per result"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(format!("{scanner:?}").contains("MetadataScanner"));
}
