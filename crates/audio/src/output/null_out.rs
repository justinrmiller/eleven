//! A silent output that consumes audio on its own thread.
//!
//! Used when no sound card is available (headless machines, CI) and by the test-suite, where
//! `realtime: false` lets a one-second clip "play" in a few milliseconds.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::Duration;

use super::{Opened, Output, Pipes, Shared};

/// The null output; stops its thread when dropped.
pub struct NullOutput {
    sample_rate: u32,
    realtime: bool,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl NullOutput {
    /// Starts the null output and returns it with the producer and visualiser ends.
    #[must_use]
    pub fn open(sample_rate: u32, realtime: bool, shared: Arc<Shared>) -> Opened {
        let sample_rate = sample_rate.max(1_000);
        let Pipes {
            producer,
            mut renderer,
            vis,
        } = Pipes::new(shared, sample_rate);
        let stop = Arc::new(AtomicBool::new(false));
        let stop_flag = Arc::clone(&stop);
        // 10 ms blocks when pacing like a device, larger blocks when running flat out.
        let block_frames = if realtime {
            (sample_rate / 100) as usize
        } else {
            4_096
        };
        let thread = std::thread::Builder::new()
            .name("eleven-null-output".into())
            .spawn(move || {
                let mut buf = vec![0.0f32; block_frames * 2];
                let pause = if realtime {
                    Duration::from_millis(10)
                } else {
                    Duration::from_micros(100)
                };
                while !stop_flag.load(Ordering::Relaxed) {
                    renderer.render(&mut buf, 2);
                    std::thread::sleep(pause);
                }
            })
            .ok();
        let output = Self {
            sample_rate,
            realtime,
            stop,
            thread,
        };
        (Box::new(output), producer, vis)
    }
}

impl Output for NullOutput {
    fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    fn name(&self) -> String {
        if self.realtime {
            "Silent output".to_owned()
        } else {
            "Null output (fast)".to_owned()
        }
    }
}

impl Drop for NullOutput {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn consumes_pushed_audio() {
        let shared = Arc::new(Shared::default());
        let (out, mut producer, mut vis) = NullOutput::open(8_000, false, Arc::clone(&shared));
        assert_eq!(out.sample_rate(), 8_000);
        assert!(out.name().contains("Null"));
        for _ in 0..1_000 {
            producer.push(0.25).unwrap();
        }
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while shared.frames_played() < 500 && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(shared.frames_played(), 500);
        assert!(vis.pop().is_ok());
        drop(out);
    }

    #[test]
    fn realtime_mode_names_itself() {
        let shared = Arc::new(Shared::default());
        let (out, _p, _v) = NullOutput::open(10, true, shared);
        assert_eq!(
            out.sample_rate(),
            1_000,
            "rates are clamped to something sane"
        );
        assert_eq!(out.name(), "Silent output");
    }
}
