//! Audio outputs and the lock-free hand-off between the engine thread and the device callback.
//!
//! The engine pushes interleaved stereo samples into an `rtrb` ring buffer. The output side
//! (a cpal callback, or the [`null_out`] thread) pulls them through [`Renderer::render`], which
//! applies volume and balance, counts played frames, feeds the visualiser and handles flushes.

pub mod cpal_out;
pub mod null_out;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use crate::dsp::{balance_gains, volume_gain};

/// Seconds of audio buffered between the engine and the device.
pub const RING_SECONDS: f32 = 0.25;
/// Capacity of the visualisation tap, in mono samples.
pub const VIS_CAPACITY: usize = 8_192;

/// State shared between the engine, the output callback and the UI.
#[derive(Debug)]
pub struct Shared {
    volume: AtomicU32,
    balance: AtomicU32,
    paused: AtomicBool,
    flush_requested: AtomicU64,
    flush_acked: AtomicU64,
    frames_played: AtomicU64,
    underruns: AtomicU64,
}

impl Default for Shared {
    fn default() -> Self {
        Self {
            volume: AtomicU32::new(0.8f32.to_bits()),
            balance: AtomicU32::new(0.0f32.to_bits()),
            paused: AtomicBool::new(false),
            flush_requested: AtomicU64::new(0),
            flush_acked: AtomicU64::new(0),
            frames_played: AtomicU64::new(0),
            underruns: AtomicU64::new(0),
        }
    }
}

impl Shared {
    /// Sets the volume slider value (`0.0..=1.0`).
    pub fn set_volume(&self, volume: f32) {
        self.volume
            .store(volume.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
    }

    /// The volume slider value.
    #[must_use]
    pub fn volume(&self) -> f32 {
        f32::from_bits(self.volume.load(Ordering::Relaxed))
    }

    /// Sets the balance (`-1.0..=1.0`).
    pub fn set_balance(&self, balance: f32) {
        self.balance
            .store(balance.clamp(-1.0, 1.0).to_bits(), Ordering::Relaxed);
    }

    /// The balance.
    #[must_use]
    pub fn balance(&self) -> f32 {
        f32::from_bits(self.balance.load(Ordering::Relaxed))
    }

    /// Pauses or resumes the output (silence is rendered while paused).
    pub fn set_paused(&self, paused: bool) {
        self.paused.store(paused, Ordering::Relaxed);
    }

    /// Whether the output is paused.
    #[must_use]
    pub fn is_paused(&self) -> bool {
        self.paused.load(Ordering::Relaxed)
    }

    /// Frames played since the last flush.
    #[must_use]
    pub fn frames_played(&self) -> u64 {
        self.frames_played.load(Ordering::Relaxed)
    }

    /// Number of times the output ran dry while playing.
    #[must_use]
    pub fn underruns(&self) -> u64 {
        self.underruns.load(Ordering::Relaxed)
    }

    /// Asks the output to discard everything buffered and reset the played-frame counter, then
    /// waits (up to `timeout`) for it to do so. Returns `false` on timeout, e.g. when no output
    /// is running; the counter is reset either way.
    pub fn flush(&self, timeout: Duration) -> bool {
        let epoch = self.flush_requested.fetch_add(1, Ordering::AcqRel) + 1;
        let deadline = Instant::now() + timeout;
        while self.flush_acked.load(Ordering::Acquire) < epoch {
            if Instant::now() >= deadline {
                self.frames_played.store(0, Ordering::Relaxed);
                return false;
            }
            std::thread::sleep(Duration::from_micros(500));
        }
        true
    }
}

/// The consumer half used inside an output callback.
pub struct Renderer {
    consumer: rtrb::Consumer<f32>,
    vis: rtrb::Producer<f32>,
    shared: Arc<Shared>,
    acked: u64,
}

impl std::fmt::Debug for Renderer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Renderer")
            .field("acked", &self.acked)
            .finish_non_exhaustive()
    }
}

/// The three ends created for one output: engine producer, renderer and visualiser consumer.
pub struct Pipes {
    /// Where the engine writes interleaved stereo samples.
    pub producer: rtrb::Producer<f32>,
    /// What the output callback reads from.
    pub renderer: Renderer,
    /// Mono samples for the visualiser.
    pub vis: rtrb::Consumer<f32>,
}

impl Pipes {
    /// Creates ring buffers sized for `sample_rate`.
    #[must_use]
    pub fn new(shared: Arc<Shared>, sample_rate: u32) -> Self {
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            clippy::cast_precision_loss
        )]
        let capacity = ((sample_rate as f32 * RING_SECONDS) as usize).max(1024) * 2;
        let (producer, consumer) = rtrb::RingBuffer::new(capacity);
        let (vis_producer, vis) = rtrb::RingBuffer::new(VIS_CAPACITY);
        let acked = shared.flush_requested.load(Ordering::Acquire);
        shared.flush_acked.store(acked, Ordering::Release);
        Self {
            producer,
            renderer: Renderer {
                consumer,
                vis: vis_producer,
                shared,
                acked,
            },
            vis,
        }
    }
}

impl Renderer {
    /// Fills `out` (interleaved, `channels` per frame) with the next audio, never blocking.
    ///
    /// Stereo is written to the first two channels (mono devices get the average; extra channels
    /// are silent). While paused, or when the ring buffer runs dry, silence is written.
    pub fn render(&mut self, out: &mut [f32], channels: usize) {
        let channels = channels.max(1);
        self.handle_flush();
        let frames = out.len() / channels;
        let paused = self.shared.is_paused();
        let available = if paused { 0 } else { self.consumer.slots() / 2 };
        let take = frames.min(available);
        let gain = volume_gain(self.shared.volume());
        let (gl, gr) = balance_gains(self.shared.balance());
        if take < frames && !paused {
            self.shared.underruns.fetch_add(1, Ordering::Relaxed);
        }
        if take > 0
            && let Ok(chunk) = self.consumer.read_chunk(take * 2)
        {
            let (a, b) = chunk.as_slices();
            let mut samples = a.iter().chain(b.iter());
            for frame in out.chunks_exact_mut(channels).take(take) {
                let l = *samples.next().unwrap_or(&0.0);
                let r = *samples.next().unwrap_or(&0.0);
                if !self.vis.is_full() {
                    let _ = self.vis.push(0.5 * (l + r));
                }
                write_frame(frame, l * gain * gl, r * gain * gr);
            }
            chunk.commit_all();
            self.shared
                .frames_played
                .fetch_add(take as u64, Ordering::Relaxed);
        }
        for frame in out.chunks_exact_mut(channels).skip(take) {
            frame.fill(0.0);
        }
    }

    fn handle_flush(&mut self) {
        let requested = self.shared.flush_requested.load(Ordering::Acquire);
        if requested != self.acked {
            let n = self.consumer.slots();
            if let Ok(chunk) = self.consumer.read_chunk(n) {
                chunk.commit_all();
            }
            self.shared.frames_played.store(0, Ordering::Relaxed);
            self.acked = requested;
            self.shared.flush_acked.store(requested, Ordering::Release);
        }
    }
}

fn write_frame(frame: &mut [f32], l: f32, r: f32) {
    match frame.len() {
        1 => frame[0] = 0.5 * (l + r),
        _ => {
            frame[0] = l;
            frame[1] = r;
            frame[2..].fill(0.0);
        }
    }
}

/// What kind of output the engine should open.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OutputSpec {
    /// A sound card via cpal; `None` uses the system default device.
    Device(Option<String>),
    /// A silent output that consumes audio at `sample_rate`; `realtime: false` runs as fast as
    /// possible (for tests).
    Null {
        /// Simulated device rate.
        sample_rate: u32,
        /// Pace consumption like a real device.
        realtime: bool,
    },
}

/// An opened output with the engine's producer end and the visualiser tap.
pub type Opened = (Box<dyn Output>, rtrb::Producer<f32>, rtrb::Consumer<f32>);

/// A running output. Dropping it stops the device.
pub trait Output: Send {
    /// The device sample rate.
    fn sample_rate(&self) -> u32;
    /// A human-readable device name.
    fn name(&self) -> String;
}

/// Opens an output per `spec`, returning it with the engine's producer and the visualiser tap.
///
/// # Errors
/// Fails if a cpal device cannot be opened.
pub fn open(spec: &OutputSpec, shared: &Arc<Shared>) -> crate::Result<Opened> {
    match spec {
        OutputSpec::Device(name) => cpal_out::CpalOutput::open(name.as_deref(), Arc::clone(shared)),
        OutputSpec::Null {
            sample_rate,
            realtime,
        } => Ok(null_out::NullOutput::open(
            *sample_rate,
            *realtime,
            Arc::clone(shared),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pipes() -> (Arc<Shared>, Pipes) {
        let shared = Arc::new(Shared::default());
        let p = Pipes::new(Arc::clone(&shared), 8_000);
        (shared, p)
    }

    #[test]
    fn renders_with_volume_and_balance() {
        let (shared, mut p) = pipes();
        shared.set_volume(1.0);
        shared.set_balance(0.5);
        for _ in 0..4 {
            p.producer.push(0.5).unwrap();
            p.producer.push(-0.5).unwrap();
        }
        let mut out = vec![9.0; 8];
        p.renderer.render(&mut out, 2);
        assert_eq!(&out[..2], &[0.25, -0.5]);
        assert_eq!(shared.frames_played(), 4);
        assert_eq!(shared.underruns(), 0);
        // The visualiser got the pre-volume mono mix.
        assert_eq!(p.vis.slots(), 4);
        assert!(p.vis.pop().unwrap().abs() < f32::EPSILON);
        assert!((shared.volume() - 1.0).abs() < f32::EPSILON);
        assert!((shared.balance() - 0.5).abs() < f32::EPSILON);
    }

    #[test]
    fn underrun_pads_with_silence() {
        let (shared, mut p) = pipes();
        p.producer.push(1.0).unwrap();
        p.producer.push(1.0).unwrap();
        let mut out = vec![9.0; 6];
        p.renderer.render(&mut out, 2);
        assert!(out[0] > 0.0);
        assert_eq!(&out[2..], &[0.0; 4]);
        assert_eq!(shared.underruns(), 1);
    }

    #[test]
    fn paused_renders_silence_without_consuming() {
        let (shared, mut p) = pipes();
        p.producer.push(1.0).unwrap();
        p.producer.push(1.0).unwrap();
        shared.set_paused(true);
        assert!(shared.is_paused());
        let mut out = vec![9.0; 4];
        p.renderer.render(&mut out, 2);
        assert_eq!(out, [0.0; 4]);
        assert_eq!(p.renderer.consumer.slots(), 2);
        assert_eq!(shared.underruns(), 0, "pausing is not an underrun");
    }

    #[test]
    fn mono_and_multichannel_devices() {
        let (shared, mut p) = pipes();
        shared.set_volume(1.0);
        for v in [1.0, 0.0, 1.0, 0.0] {
            p.producer.push(v).unwrap();
        }
        let mut mono = vec![9.0; 1];
        p.renderer.render(&mut mono, 1);
        assert!((mono[0] - 0.5).abs() < f32::EPSILON);
        let mut surround = vec![9.0; 6];
        p.renderer.render(&mut surround, 6);
        assert_eq!(surround, [1.0, 0.0, 0.0, 0.0, 0.0, 0.0]);
    }

    #[test]
    fn flush_discards_buffer_and_resets_counter() {
        let (shared, mut p) = pipes();
        for _ in 0..10 {
            p.producer.push(0.1).unwrap();
        }
        let mut out = vec![0.0; 2];
        p.renderer.render(&mut out, 2);
        assert_eq!(shared.frames_played(), 1);
        // No renderer is running on another thread, so the flush times out but still resets.
        assert!(!shared.flush(Duration::from_millis(5)));
        assert_eq!(shared.frames_played(), 0);
        p.renderer.render(&mut out, 2);
        assert_eq!(out, [0.0, 0.0], "stale audio was dropped");
        assert_eq!(p.renderer.consumer.slots(), 0);
    }

    #[test]
    fn flush_is_acknowledged_by_a_running_renderer() {
        let (shared, mut p) = pipes();
        let stop = Arc::new(AtomicBool::new(false));
        let stop2 = Arc::clone(&stop);
        let mut renderer = p.renderer;
        let handle = std::thread::spawn(move || {
            let mut out = vec![0.0; 64];
            while !stop2.load(Ordering::Relaxed) {
                renderer.render(&mut out, 2);
                std::thread::sleep(Duration::from_micros(200));
            }
        });
        p.producer.push(0.0).unwrap();
        assert!(shared.flush(Duration::from_secs(2)));
        stop.store(true, Ordering::Relaxed);
        handle.join().unwrap();
    }

    #[test]
    fn volume_and_balance_are_clamped() {
        let shared = Shared::default();
        shared.set_volume(4.0);
        shared.set_balance(-4.0);
        assert!((shared.volume() - 1.0).abs() < f32::EPSILON);
        assert!((shared.balance() + 1.0).abs() < f32::EPSILON);
    }
}
