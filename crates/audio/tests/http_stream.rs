//! Network streams served by an in-process HTTP server (no external network access).

mod common;

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::time::{Duration, Instant};

use common::fixture;
use eleven_audio::{Engine, OutputSpec};
use eleven_core::{Location, PlayerEvent, icy};

/// Serves a single response on a random local port and returns the URL.
fn serve_once(response: impl FnOnce(TcpStream) + Send + 'static) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            // Read the request head.
            let mut head = Vec::new();
            let mut byte = [0u8; 1];
            while !head.ends_with(b"\r\n\r\n") && stream.read(&mut byte).unwrap_or(0) == 1 {
                head.push(byte[0]);
            }
            let request = String::from_utf8_lossy(&head).to_ascii_lowercase();
            assert!(
                request.contains("icy-metadata: 1"),
                "client asks for ICY metadata"
            );
            response(stream);
        }
    });
    format!("http://{addr}/stream.mp3")
}

fn wait_for(engine: &Engine, pred: impl Fn(&PlayerEvent) -> bool) -> Vec<PlayerEvent> {
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut seen = Vec::new();
    while Instant::now() < deadline {
        if let Some(e) = engine.wait_event(Duration::from_millis(50)) {
            let done = pred(&e);
            seen.push(e);
            if done {
                return seen;
            }
        }
    }
    panic!("timed out; saw {seen:?}");
}

fn engine() -> Engine {
    Engine::start(
        &OutputSpec::Null {
            sample_rate: 44_100,
            realtime: false,
        },
        Arc::new(|| {}),
    )
}

#[test]
fn icecast_stream_plays_with_metadata() {
    let audio = std::fs::read(fixture("tone.mp3")).unwrap();
    let body = icy::interleave(&audio, 2_048, |i| {
        (i == 1).then(|| "StreamTitle='Spinal Tap - Stonehenge';".to_owned())
    });
    let url = serve_once(move |mut s| {
        let head = "HTTP/1.0 200 OK\r\nContent-Type: audio/mpeg\r\nicy-metaint: 2048\r\n\
                    icy-br: 128\r\nicy-name: Radio Eleven\r\n\r\n";
        let _ = s.write_all(head.as_bytes());
        let _ = s.write_all(&body);
    });
    let engine = engine();
    engine.play(Location::Url(url));
    let events = wait_for(&engine, |e| *e == PlayerEvent::Finished);
    let started = events.iter().find_map(|e| match e {
        PlayerEvent::Started(info) => Some(info.clone()),
        _ => None,
    });
    let info = started.expect("stream started");
    assert_eq!(info.title.as_deref(), Some("Radio Eleven"));
    assert_eq!(info.bitrate_kbps, Some(128));
    assert!(!info.seekable);
    assert_eq!(info.duration, None);
    assert!(events.contains(&PlayerEvent::StreamTitle("Spinal Tap - Stonehenge".into())));
    assert!(events.contains(&PlayerEvent::Buffering(100)));
}

#[test]
fn http_errors_are_reported() {
    let url = serve_once(|mut s| {
        let _ = s.write_all(b"HTTP/1.0 404 Not Found\r\nContent-Length: 0\r\n\r\n");
    });
    let engine = engine();
    engine.play(Location::Url(url));
    let events = wait_for(&engine, |e| matches!(e, PlayerEvent::Error(_)));
    assert!(
        matches!(events.last(), Some(PlayerEvent::Error(m)) if m.contains("404")),
        "{events:?}"
    );
}

#[test]
fn connection_refused_is_reported() {
    let port = {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        l.local_addr().unwrap().port()
    };
    let engine = engine();
    engine.play(Location::Url(format!("http://127.0.0.1:{port}/")));
    let events = wait_for(&engine, |e| matches!(e, PlayerEvent::Error(_)));
    assert!(matches!(events.last(), Some(PlayerEvent::Error(m)) if m.contains("network")));
}

#[test]
fn stop_interrupts_a_stalled_stream() {
    let url = serve_once(|mut s| {
        let _ = s.write_all(b"HTTP/1.0 200 OK\r\nContent-Type: audio/mpeg\r\n\r\n");
        // Send a trickle and then hang, so the client is stuck pre-buffering.
        let _ = s.write_all(&[0u8; 100]);
        std::thread::sleep(Duration::from_secs(30));
    });
    let engine = engine();
    engine.play(Location::Url(url));
    wait_for(&engine, |e| matches!(e, PlayerEvent::Buffering(_)));
    let start = Instant::now();
    engine.stop();
    engine.play(Location::File(fixture("tone.wav")));
    let events = wait_for(
        &engine,
        |e| matches!(e, PlayerEvent::Started(info) if info.codec.contains("pcm")),
    );
    assert!(
        start.elapsed() < Duration::from_secs(5),
        "stop did not wait for the stalled stream"
    );
    assert!(
        !events.iter().any(|e| matches!(e, PlayerEvent::Error(_))),
        "a user stop is not an error: {events:?}"
    );
}
