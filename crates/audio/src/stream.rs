//! HTTP / Shoutcast / Icecast streams.
//!
//! A background thread reads the HTTP body, strips in-band ICY metadata (reporting
//! `StreamTitle` changes) and fills a bounded buffer. The decoder reads from that buffer through
//! [`StreamSource`], which pre-buffers before the first byte (and after an underrun) and reports
//! progress as [`PlayerEvent::Buffering`].

use std::collections::VecDeque;
use std::io::Read;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::time::Duration;

use eleven_core::PlayerEvent;
use eleven_core::icy::{IcyDemuxer, parse_stream_title};
use symphonia::core::io::ReadOnlySource;

use crate::decoder::Decoder;
use crate::error::{AudioError, Result};

/// Callback used to report stream events from the reader thread.
pub type OnEvent = Arc<dyn Fn(PlayerEvent) + Send + Sync>;

/// Bytes to buffer before playback starts (and after an underrun).
pub const PREBUFFER_BYTES: usize = 48 * 1024;
/// Maximum bytes held in memory before the reader thread waits.
pub const MAX_BUFFER_BYTES: usize = 1024 * 1024;
const READ_CHUNK: usize = 8 * 1024;
const WAIT_SLICE: Duration = Duration::from_millis(50);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const RESPONSE_TIMEOUT: Duration = Duration::from_secs(15);
/// How often a pending connect checks for an interrupt.
const CONNECT_POLL: Duration = Duration::from_millis(20);

/// Response headers relevant to playback.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StreamHeaders {
    /// `Content-Type`, lower-cased, without parameters.
    pub content_type: Option<String>,
    /// `icy-metaint`: bytes of audio between metadata blocks (0 = none).
    pub metaint: usize,
    /// `icy-br`: nominal bitrate in kbit/s.
    pub bitrate_kbps: Option<u32>,
    /// `icy-name`: the station name.
    pub name: Option<String>,
}

/// Maps a content type (or, failing that, the URL's extension) to symphonia probe hints.
#[must_use]
pub fn format_hint(
    content_type: Option<&str>,
    url: &str,
) -> (Option<&'static str>, Option<String>) {
    let ext = match content_type.unwrap_or("") {
        "audio/mpeg" | "audio/mp3" | "audio/mpeg3" => Some("mp3"),
        "audio/aac" | "audio/aacp" | "audio/x-aac" => Some("aac"),
        "audio/ogg" | "application/ogg" | "audio/vorbis" => Some("ogg"),
        "audio/flac" | "audio/x-flac" => Some("flac"),
        "audio/wav" | "audio/x-wav" | "audio/wave" => Some("wav"),
        "audio/mp4" | "audio/x-m4a" => Some("m4a"),
        _ => None,
    };
    let ext = ext.or_else(|| {
        let path = url.split(['?', '#']).next().unwrap_or(url);
        let tail = path.rsplit('/').next().unwrap_or("");
        let (_, ext) = tail.rsplit_once('.')?;
        crate::KNOWN_EXTENSIONS
            .iter()
            .copied()
            .find(|k| k.eq_ignore_ascii_case(ext))
    });
    (ext, content_type.map(str::to_owned))
}

fn is_loopback(url: &str) -> bool {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    let host = rest.split(['/', '?', '#']).next().unwrap_or("");
    let host = host.rsplit_once('@').map_or(host, |(_, h)| h);
    let host = if host.starts_with('[') {
        host.split(']').next().unwrap_or("").trim_start_matches('[')
    } else {
        host.split(':').next().unwrap_or("")
    };
    matches!(host, "localhost" | "127.0.0.1" | "::1")
}

/// Issues the HTTP request and returns the headers and body reader.
///
/// # Errors
/// Fails on connection errors, timeouts and non-success status codes.
pub fn connect(url: &str) -> Result<(StreamHeaders, Box<dyn Read + Send + Sync>)> {
    let proxy = if is_loopback(url) {
        None
    } else {
        ureq::Proxy::try_from_env()
    };
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_connect(Some(CONNECT_TIMEOUT))
        .timeout_recv_response(Some(RESPONSE_TIMEOUT))
        .proxy(proxy)
        .user_agent(concat!("eleven/", env!("CARGO_PKG_VERSION")))
        .build()
        .into();
    let response = agent
        .get(url)
        .header("Icy-MetaData", "1")
        .call()
        .map_err(|e| AudioError::Network(e.to_string()))?;
    let header = |name: &str| {
        response
            .headers()
            .get(name)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.trim().to_owned())
    };
    let headers = StreamHeaders {
        content_type: header("content-type").map(|c| {
            c.split(';')
                .next()
                .unwrap_or("")
                .trim()
                .to_ascii_lowercase()
        }),
        metaint: header("icy-metaint")
            .and_then(|v| v.parse().ok())
            .unwrap_or(0),
        bitrate_kbps: header("icy-br")
            .and_then(|v| v.split(',').next().and_then(|b| b.trim().parse().ok())),
        name: header("icy-name").filter(|n| !n.is_empty()),
    };
    let reader = response.into_body().into_reader();
    Ok((headers, Box::new(reader)))
}

/// Like [`connect`], but gives up as soon as `interrupt` moves away from `epoch`.
///
/// The request runs on a helper thread, so DNS lookups and connect/response timeouts (which
/// can take tens of seconds) never block the caller once it has been interrupted. An abandoned
/// request finishes in the background and closes its connection.
///
/// # Errors
/// Fails like [`connect`], or with [`AudioError::Network`] when interrupted.
pub fn connect_interruptible(
    url: &str,
    interrupt: &AtomicU64,
    epoch: u64,
) -> Result<(StreamHeaders, Box<dyn Read + Send + Sync>)> {
    let (tx, rx) = crossbeam_channel::bounded(1);
    let owned = url.to_owned();
    std::thread::Builder::new()
        .name("eleven-stream-connect".into())
        .spawn(move || {
            let _ = tx.send(connect(&owned));
        })
        .map_err(|e| AudioError::Network(format!("cannot start connection: {e}")))?;
    loop {
        if interrupt.load(Ordering::Acquire) != epoch {
            return Err(AudioError::Network("connection cancelled".into()));
        }
        match rx.recv_timeout(CONNECT_POLL) {
            Ok(result) => return result,
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => {}
            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => {
                return Err(AudioError::Network("connection thread failed".into()));
            }
        }
    }
}

/// Connects to `url` and builds a decoder reading from it.
///
/// `interrupt`/`epoch` let the engine abort the connection and blocking reads: once the
/// counter differs from `epoch`, they fail.
///
/// # Errors
/// Fails on network errors or if the stream's format cannot be decoded.
pub fn open_decoder(
    url: &str,
    interrupt: Arc<AtomicU64>,
    epoch: u64,
    on_event: OnEvent,
) -> Result<Decoder> {
    let (headers, body) = connect_interruptible(url, &interrupt, epoch)?;
    let source = StreamSource::spawn(body, headers.metaint, interrupt, epoch, on_event);
    let (ext, mime) = format_hint(headers.content_type.as_deref(), url);
    let mut decoder =
        Decoder::open_source(Box::new(ReadOnlySource::new(source)), ext, mime.as_deref())?;
    let info = decoder.info_mut();
    info.seekable = false;
    info.duration = None;
    info.bitrate_kbps = headers.bitrate_kbps.or(info.bitrate_kbps);
    info.title = headers.name;
    Ok(decoder)
}

#[derive(Default)]
struct Buffer {
    data: VecDeque<u8>,
    closed: bool,
    error: Option<String>,
    /// While `true`, readers wait for [`PREBUFFER_BYTES`] before consuming.
    buffering: bool,
    last_percent: Option<u8>,
}

struct Inner {
    buffer: Mutex<Buffer>,
    changed: Condvar,
    interrupt: Arc<AtomicU64>,
    epoch: u64,
    /// Set when the [`StreamSource`] is dropped, e.g. after a decode error.
    dropped: AtomicBool,
    on_event: OnEvent,
}

impl Inner {
    fn interrupted(&self) -> bool {
        self.dropped.load(Ordering::Acquire) || self.interrupt.load(Ordering::Acquire) != self.epoch
    }
}

/// The decoder-facing end of a network stream.
pub struct StreamSource {
    inner: Arc<Inner>,
}

impl StreamSource {
    /// Starts the reader thread over `body`, demuxing ICY metadata every `metaint` bytes.
    #[must_use]
    pub fn spawn(
        body: Box<dyn Read + Send + Sync>,
        metaint: usize,
        interrupt: Arc<AtomicU64>,
        epoch: u64,
        on_event: OnEvent,
    ) -> Self {
        let inner = Arc::new(Inner {
            buffer: Mutex::new(Buffer {
                buffering: true,
                ..Buffer::default()
            }),
            changed: Condvar::new(),
            interrupt,
            epoch,
            dropped: AtomicBool::new(false),
            on_event,
        });
        let worker = Arc::clone(&inner);
        let spawned = std::thread::Builder::new()
            .name("eleven-stream-reader".into())
            .spawn(move || reader_thread(body, metaint, &worker));
        if let Err(e) = spawned {
            let mut buf = inner.buffer.lock().unwrap_or_else(PoisonError::into_inner);
            buf.closed = true;
            buf.error = Some(format!("cannot start reader: {e}"));
        }
        Self { inner }
    }
}

fn reader_thread(mut body: Box<dyn Read + Send + Sync>, metaint: usize, inner: &Inner) {
    let mut demux = IcyDemuxer::new(metaint);
    let mut raw = vec![0u8; READ_CHUNK];
    let mut audio = Vec::with_capacity(READ_CHUNK);
    let mut last_title: Option<String> = None;
    loop {
        if inner.interrupted() {
            break;
        }
        let n = match body.read(&mut raw) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => {
                inner
                    .buffer
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .error = Some(e.to_string());
                break;
            }
        };
        // The read may have blocked for a while: don't report titles for a stopped stream.
        if inner.interrupted() {
            break;
        }
        audio.clear();
        for block in demux.feed(&raw[..n], &mut audio) {
            if let Some(title) = parse_stream_title(&block)
                && last_title.as_ref() != Some(&title)
            {
                last_title = Some(title.clone());
                (inner.on_event)(PlayerEvent::StreamTitle(title));
            }
        }
        let mut buf = inner.buffer.lock().unwrap_or_else(PoisonError::into_inner);
        while buf.data.len() >= MAX_BUFFER_BYTES && !inner.interrupted() {
            buf = inner
                .changed
                .wait_timeout(buf, WAIT_SLICE)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
        buf.data.extend(&audio);
        drop(buf);
        inner.changed.notify_all();
    }
    inner
        .buffer
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .closed = true;
    inner.changed.notify_all();
}

impl Read for StreamSource {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        if out.is_empty() {
            return Ok(0);
        }
        let inner = &*self.inner;
        let mut buf = inner.buffer.lock().unwrap_or_else(PoisonError::into_inner);
        loop {
            if inner.interrupted() {
                // Not `Interrupted`: by convention readers retry on that kind (symphonia's
                // `read_buf_exact` does), which would spin forever.
                return Err(std::io::Error::new(
                    std::io::ErrorKind::ConnectionAborted,
                    "stream stopped",
                ));
            }
            if buf.buffering && !buf.closed && buf.data.len() < PREBUFFER_BYTES {
                // Report pre-buffer progress in 10 % steps.
                let pct = u8::try_from(buf.data.len() * 100 / PREBUFFER_BYTES).unwrap_or(100);
                let step = pct / 10 * 10;
                if buf.last_percent != Some(step) {
                    buf.last_percent = Some(step);
                    (inner.on_event)(PlayerEvent::Buffering(step));
                }
            } else if !buf.data.is_empty() {
                if buf.buffering {
                    buf.buffering = false;
                    buf.last_percent = None;
                    (inner.on_event)(PlayerEvent::Buffering(100));
                }
                let n = out.len().min(buf.data.len());
                for (dst, src) in out.iter_mut().zip(buf.data.drain(..n)) {
                    *dst = src;
                }
                if buf.data.is_empty() && !buf.closed {
                    // Underrun: refill before handing out more.
                    buf.buffering = true;
                }
                drop(buf);
                inner.changed.notify_all();
                return Ok(n);
            } else if buf.closed {
                return match buf.error.take() {
                    Some(e) => Err(std::io::Error::other(e)),
                    None => Ok(0),
                };
            } else {
                buf.buffering = true;
            }
            buf = inner
                .changed
                .wait_timeout(buf, WAIT_SLICE)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
    }
}

impl Drop for StreamSource {
    fn drop(&mut self) {
        // Stop the reader thread (and with it the HTTP connection): nobody reads any more.
        // It exits at its next check, at the latest when its current read returns.
        self.inner.dropped.store(true, Ordering::Release);
        self.inner
            .buffer
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .data
            .clear();
        self.inner.changed.notify_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn collector() -> (OnEvent, Arc<Mutex<Vec<PlayerEvent>>>) {
        let events = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&events);
        let on: OnEvent = Arc::new(move |e| sink.lock().unwrap().push(e));
        (on, events)
    }

    #[test]
    fn hints_from_content_type_and_url() {
        assert_eq!(
            format_hint(Some("audio/mpeg"), "http://x/live").0,
            Some("mp3")
        );
        assert_eq!(
            format_hint(Some("application/ogg"), "http://x/").0,
            Some("ogg")
        );
        assert_eq!(format_hint(Some("audio/aacp"), "http://x").0, Some("aac"));
        assert_eq!(
            format_hint(None, "http://x/a/stream.FLAC?token=1").0,
            Some("flac")
        );
        assert_eq!(format_hint(Some("text/html"), "http://x/noext").0, None);
        assert_eq!(
            format_hint(Some("audio/mpeg"), "u").1.as_deref(),
            Some("audio/mpeg")
        );
    }

    #[test]
    fn loopback_detection() {
        assert!(is_loopback("http://127.0.0.1:8000/s"));
        assert!(is_loopback("http://localhost/s"));
        assert!(is_loopback("http://user:pw@localhost:1/s"));
        assert!(is_loopback("http://[::1]:80/"));
        assert!(!is_loopback("https://radio.example.org/live"));
    }

    #[test]
    fn source_strips_metadata_and_reports_titles() {
        let audio: Vec<u8> = (0..200_000u32).map(|i| (i % 251) as u8).collect();
        let stream = eleven_core::icy::interleave(&audio, 1000, |i| {
            Some(format!("StreamTitle='Song {}';", i / 50))
        });
        let (on, events) = collector();
        let mut src = StreamSource::spawn(
            Box::new(Cursor::new(stream)),
            1000,
            Arc::new(AtomicU64::new(0)),
            0,
            on,
        );
        let mut out = Vec::new();
        src.read_to_end(&mut out).unwrap();
        assert_eq!(out, audio);
        let events = events.lock().unwrap();
        let titles: Vec<&PlayerEvent> = events
            .iter()
            .filter(|e| matches!(e, PlayerEvent::StreamTitle(_)))
            .collect();
        assert_eq!(titles.len(), 4, "titles are de-duplicated: {titles:?}");
        assert!(events.contains(&PlayerEvent::Buffering(100)));
    }

    #[test]
    fn short_stream_ends_without_full_prebuffer() {
        let (on, _events) = collector();
        let mut src = StreamSource::spawn(
            Box::new(Cursor::new(b"tiny".to_vec())),
            0,
            Arc::new(AtomicU64::new(0)),
            0,
            on,
        );
        let mut out = Vec::new();
        src.read_to_end(&mut out).unwrap();
        assert_eq!(out, b"tiny");
        assert_eq!(src.read(&mut []).unwrap(), 0);
    }

    struct Failing;
    impl Read for Failing {
        fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
            Err(std::io::Error::other("connection reset"))
        }
    }

    #[test]
    fn reader_errors_surface_to_the_decoder() {
        let (on, _events) = collector();
        let mut src = StreamSource::spawn(Box::new(Failing), 0, Arc::new(AtomicU64::new(0)), 0, on);
        let err = src.read(&mut [0u8; 16]).unwrap_err();
        assert!(err.to_string().contains("connection reset"));
    }

    struct Endless;
    impl Read for Endless {
        fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
            std::thread::sleep(Duration::from_millis(1));
            out[0] = 1;
            Ok(1)
        }
    }

    #[test]
    fn interrupt_aborts_blocked_reads() {
        let (on, _events) = collector();
        let interrupt = Arc::new(AtomicU64::new(5));
        let mut src = StreamSource::spawn(Box::new(Endless), 0, Arc::clone(&interrupt), 5, on);
        let flag = Arc::clone(&interrupt);
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(50));
            flag.fetch_add(1, Ordering::AcqRel);
        });
        // Prebuffering 48 KiB at 1 byte/ms would take ~50 s; the interrupt ends it early.
        let err = src.read(&mut [0u8; 16]).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::ConnectionAborted);
        // Further reads keep failing rather than blocking.
        assert!(src.read(&mut [0u8; 16]).is_err());
    }

    /// Endless body that counts how many reads were made.
    struct Counting(Arc<AtomicU64>);
    impl Read for Counting {
        fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
            std::thread::sleep(Duration::from_millis(1));
            self.0.fetch_add(1, Ordering::AcqRel);
            out.fill(0);
            Ok(out.len())
        }
    }

    #[test]
    fn dropping_the_source_stops_the_reader() {
        let (on, _events) = collector();
        let reads = Arc::new(AtomicU64::new(0));
        // The epoch never changes, as when the engine stops after a decode error.
        let src = StreamSource::spawn(
            Box::new(Counting(Arc::clone(&reads))),
            0,
            Arc::new(AtomicU64::new(0)),
            0,
            on,
        );
        std::thread::sleep(Duration::from_millis(20));
        drop(src);
        std::thread::sleep(Duration::from_millis(50));
        let after_drop = reads.load(Ordering::Acquire);
        std::thread::sleep(Duration::from_millis(200));
        assert_eq!(
            reads.load(Ordering::Acquire),
            after_drop,
            "reader kept downloading"
        );
    }

    #[test]
    fn interrupted_connects_return_promptly() {
        // Accepts the connection but never answers, like a stalled server.
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/live", listener.local_addr().unwrap());
        let interrupt = Arc::new(AtomicU64::new(1));
        let flag = Arc::clone(&interrupt);
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(100));
            flag.fetch_add(1, Ordering::AcqRel);
        });
        let start = std::time::Instant::now();
        let Err(err) = connect_interruptible(&url, &interrupt, 1) else {
            panic!("connect must fail");
        };
        assert!(
            start.elapsed() < Duration::from_secs(2),
            "{:?}",
            start.elapsed()
        );
        assert!(err.to_string().contains("cancelled"), "{err}");
        drop(listener);
    }

    #[test]
    fn interrupt_ends_symphonia_exact_reads() {
        use symphonia::core::io::{MediaSourceStream, ReadBytes};
        let (on, _events) = collector();
        let interrupt = Arc::new(AtomicU64::new(0));
        let src = StreamSource::spawn(Box::new(Endless), 0, Arc::clone(&interrupt), 0, on);
        let mut mss =
            MediaSourceStream::new(Box::new(ReadOnlySource::new(src)), Default::default());
        let flag = Arc::clone(&interrupt);
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(50));
            flag.fetch_add(1, Ordering::AcqRel);
        });
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut buf = [0u8; 64];
            let _ = tx.send(ReadBytes::read_buf_exact(&mut mss, &mut buf).map_err(|e| e.kind()));
        });
        let result = rx
            .recv_timeout(Duration::from_secs(5))
            .expect("exact read must not spin forever");
        assert!(result.is_err());
    }
}
