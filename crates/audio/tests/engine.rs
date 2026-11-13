//! End-to-end engine tests through the silent output.

mod common;

use std::sync::Arc;
use std::time::{Duration, Instant};

use common::{fixture, write_tone_wav};
use eleven_audio::{Engine, OutputSpec};
use eleven_core::{EqSettings, Location, PlaybackState, PlayerEvent};

fn fast_engine(rate: u32) -> Engine {
    Engine::start(
        &OutputSpec::Null {
            sample_rate: rate,
            realtime: false,
        },
        Arc::new(|| {}),
    )
}

fn realtime_engine() -> Engine {
    Engine::start(
        &OutputSpec::Null {
            sample_rate: 44_100,
            realtime: true,
        },
        Arc::new(|| {}),
    )
}

/// Waits for an event matching `pred`, returning it (panics after `secs`).
fn wait_for(engine: &Engine, secs: u64, pred: impl Fn(&PlayerEvent) -> bool) -> PlayerEvent {
    let deadline = Instant::now() + Duration::from_secs(secs);
    while Instant::now() < deadline {
        if let Some((_, e)) = engine.wait_event(Duration::from_millis(50))
            && pred(&e)
        {
            return e;
        }
    }
    panic!("timed out waiting for event");
}

fn wait_until(secs: u64, cond: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(secs);
    while !cond() {
        assert!(Instant::now() < deadline, "condition not met in time");
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn file(name: &str) -> Location {
    Location::File(fixture(name))
}

#[test]
fn plays_every_fixture_to_the_end() {
    // 48 kHz forces resampling for every fixture.
    let engine = fast_engine(48_000);
    assert_eq!(engine.output_rate(), 48_000);
    assert!(engine.output_name().contains("Null"));
    assert!(engine.output_error().is_none());
    for name in ["tone.wav", "tone.flac", "tone.mp3", "tone.ogg", "tone.m4a"] {
        engine.play(file(name));
        let started = wait_for(&engine, 10, |e| matches!(e, PlayerEvent::Started(_)));
        let PlayerEvent::Started(info) = started else {
            unreachable!()
        };
        assert!(info.duration.is_some(), "{name}");
        wait_for(&engine, 10, |e| *e == PlayerEvent::Finished);
        assert_eq!(engine.state(), PlaybackState::Stopped, "{name}");
        assert_eq!(engine.position(), Duration::ZERO);
    }
    let mut vis = Vec::new();
    engine.drain_vis(&mut vis);
    assert!(!vis.is_empty(), "the visualiser tap received audio");
    assert!(format!("{engine:?}").contains("Stopped"));
}

#[test]
fn events_carry_the_generation_of_their_play() {
    let engine = fast_engine(44_100);
    let first = engine.play(file("tone.wav"));
    let mut tags = Vec::new();
    while !tags.contains(&(first, PlayerEvent::Finished)) {
        let event = engine.wait_event(Duration::from_secs(5)).expect("event");
        tags.push(event);
    }
    assert!(tags.iter().all(|(g, _)| *g == first), "{tags:?}");
    let second = engine.play(file("missing.wav"));
    assert!(second > first);
    let (generation, event) = engine.wait_event(Duration::from_secs(5)).expect("error");
    assert_eq!(generation, second);
    assert!(matches!(event, PlayerEvent::Error(_)));
}

#[test]
fn chained_streams_resample_each_segment_at_its_own_rate() {
    // 1 s at 44.1 kHz followed by 1 s at 22.05 kHz, played on a 44.1 kHz output: without
    // rebuilding the converter the second second would play in half the time (double speed).
    let engine = realtime_engine();
    engine.play(file("chained.ogg"));
    let started = wait_for(&engine, 5, |e| matches!(e, PlayerEvent::Started(_)));
    assert!(matches!(started, PlayerEvent::Started(info) if info.sample_rate == 44_100));
    let deadline = Instant::now() + Duration::from_secs(10);
    let (mut changed, mut finished, mut longest) = (None, false, Duration::ZERO);
    while !finished {
        assert!(Instant::now() < deadline, "did not finish");
        longest = longest.max(engine.position());
        match engine.wait_event(Duration::from_millis(5)) {
            Some((_, PlayerEvent::FormatChanged(info))) => changed = Some(info),
            Some((_, PlayerEvent::Finished)) => finished = true,
            _ => {}
        }
    }
    let changed = changed.expect("FormatChanged reported");
    assert_eq!(changed.sample_rate, 22_050);
    assert!(
        longest > Duration::from_millis(1_850) && longest < Duration::from_millis(2_100),
        "played {longest:?} of audio for 2 s of source"
    );
}

#[test]
fn errors_are_reported_and_leave_the_engine_stopped() {
    let engine = fast_engine(44_100);
    engine.play(file("missing.flac"));
    let e = wait_for(&engine, 5, |e| matches!(e, PlayerEvent::Error(_)));
    assert!(matches!(e, PlayerEvent::Error(msg) if msg.contains("missing.flac")));
    engine.play(file("corrupt.mp3"));
    wait_for(&engine, 5, |e| matches!(e, PlayerEvent::Error(_)));
    assert_eq!(engine.state(), PlaybackState::Stopped);
    // The engine still works afterwards.
    engine.play(file("tone.wav"));
    wait_for(&engine, 5, |e| *e == PlayerEvent::Finished);
}

#[test]
fn pause_resume_seek_and_stop() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("long.wav");
    write_tone_wav(&path, 44_100, 30.0);
    let engine = realtime_engine();
    engine.set_volume(0.5);
    engine.set_balance(-0.25);
    engine.play(Location::File(path));
    wait_for(&engine, 5, |e| matches!(e, PlayerEvent::Started(_)));
    assert_eq!(engine.state(), PlaybackState::Playing);
    wait_until(5, || engine.position() > Duration::from_millis(150));

    engine.pause();
    wait_until(5, || engine.state() == PlaybackState::Paused);
    let paused_at = engine.position();
    std::thread::sleep(Duration::from_millis(150));
    assert!(
        engine.position().abs_diff(paused_at) < Duration::from_millis(20),
        "frozen while paused"
    );

    engine.resume();
    wait_until(5, || engine.state() == PlaybackState::Playing);
    wait_until(5, || {
        engine.position() > paused_at + Duration::from_millis(100)
    });

    engine.seek(Duration::from_secs(20));
    wait_until(5, || engine.position() >= Duration::from_secs(20));
    assert!(engine.position() < Duration::from_secs(21));

    engine.set_eq(&EqSettings {
        enabled: true,
        preamp_db: -3.0,
        bands_db: [3.0; 10],
    });
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(engine.state(), PlaybackState::Playing);

    engine.stop();
    wait_until(5, || engine.state() == PlaybackState::Stopped);
    assert_eq!(engine.position(), Duration::ZERO);
    assert!(
        engine
            .poll_events()
            .iter()
            .all(|(_, e)| *e != PlayerEvent::Finished)
    );
}

#[test]
fn rapid_play_commands_play_only_the_last() {
    let engine = fast_engine(44_100);
    engine.play(file("missing-1.wav"));
    engine.play(file("missing-2.wav"));
    engine.play(file("tone.flac"));
    // Whether or not the superseded plays were batched (and skipped), the last one wins.
    let started = wait_for(&engine, 5, |e| matches!(e, PlayerEvent::Started(_)));
    assert!(matches!(started, PlayerEvent::Started(info) if info.codec == "flac"));
    wait_for(&engine, 5, |e| *e == PlayerEvent::Finished);
}

#[test]
fn switching_output_keeps_playing() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("long.wav");
    write_tone_wav(&path, 44_100, 10.0);
    let engine = realtime_engine();
    engine.play(Location::File(path));
    wait_for(&engine, 5, |e| matches!(e, PlayerEvent::Started(_)));
    wait_until(5, || engine.position() > Duration::from_millis(300));
    let before = engine.position();
    let switched = Instant::now();
    engine.set_output(OutputSpec::Null {
        sample_rate: 48_000,
        realtime: true,
    });
    wait_until(5, || engine.output_rate() == 48_000);
    let pos = engine.position();
    // The clock carries on from what was heard: no jump and no audio skipped.
    assert!(
        pos >= before && pos <= before + switched.elapsed() + Duration::from_millis(50),
        "{before:?} -> {pos:?}"
    );
    wait_until(5, || engine.position() > pos + Duration::from_millis(100));
    assert_eq!(engine.state(), PlaybackState::Playing);
    // Paused tracks stay paused across a switch.
    engine.pause();
    wait_until(5, || engine.state() == PlaybackState::Paused);
    engine.set_output(OutputSpec::Null {
        sample_rate: 44_100,
        realtime: true,
    });
    wait_until(5, || engine.output_rate() == 44_100);
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(engine.state(), PlaybackState::Paused);
}

#[test]
fn switching_output_while_stopped_just_switches() {
    let engine = fast_engine(44_100);
    engine.set_output(OutputSpec::Null {
        sample_rate: 32_000,
        realtime: false,
    });
    wait_until(5, || engine.output_rate() == 32_000);
    assert_eq!(engine.state(), PlaybackState::Stopped);
    engine.play(file("tone.wav"));
    wait_for(&engine, 5, |e| *e == PlayerEvent::Finished);
}

#[test]
fn seek_and_pause_without_a_track_are_ignored() {
    let engine = fast_engine(44_100);
    engine.seek(Duration::from_secs(3));
    engine.pause();
    engine.resume();
    std::thread::sleep(Duration::from_millis(30));
    assert_eq!(engine.state(), PlaybackState::Stopped);
    let _ = engine.underruns();
}

#[test]
fn dropping_a_playing_engine_shuts_down() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("long.wav");
    write_tone_wav(&path, 44_100, 10.0);
    let engine = realtime_engine();
    engine.play(Location::File(path));
    wait_for(&engine, 5, |e| matches!(e, PlayerEvent::Started(_)));
    let start = Instant::now();
    drop(engine);
    assert!(start.elapsed() < Duration::from_secs(2));
}
