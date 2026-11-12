//! The playback engine: a worker thread that decodes, resamples, equalises and feeds the output.
//!
//! [`Engine`] is the handle the application talks to. Transport commands travel over a channel;
//! volume, balance and position use shared atomics so they respond instantly.

use std::sync::atomic::{AtomicU8, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::JoinHandle;
use std::time::Duration;

use crossbeam_channel::{Receiver, RecvTimeoutError, Sender};
use eleven_core::{EqSettings, Location, PlaybackState, PlayerEvent};

use crate::decoder::{Decoder, frames_to_duration};
use crate::dsp::Equalizer;
use crate::output::{self, Output, OutputSpec, Shared};
use crate::resample::StreamResampler;
use crate::stream;

/// How long a flush waits for the output to acknowledge.
const FLUSH_TIMEOUT: Duration = Duration::from_millis(250);
/// Poll interval while the ring buffer is full or draining.
const BUSY_WAIT: Duration = Duration::from_millis(5);

/// Messages from the handle to the worker thread.
#[derive(Debug, Clone)]
enum Command {
    Play(Location),
    Pause,
    Resume,
    Stop,
    Seek(Duration),
    SetEq(EqSettings),
    SetOutput(OutputSpec),
    Shutdown,
}

impl Command {
    /// Commands that make any earlier `Play` in the same batch irrelevant.
    fn supersedes_play(&self) -> bool {
        matches!(self, Self::Play(_) | Self::Stop | Self::Shutdown)
    }
}

/// Engine status readable from any thread.
#[derive(Debug, Default)]
struct Status {
    state: AtomicU8,
    base_micros: AtomicU64,
    output_rate: AtomicU32,
    output_name: Mutex<String>,
    output_error: Mutex<Option<String>>,
    /// Bumped by the handle on every transport command so blocked network reads can abort.
    interrupt: Arc<AtomicU64>,
}

impl Status {
    fn set_state(&self, state: PlaybackState) {
        let v = match state {
            PlaybackState::Stopped => 0,
            PlaybackState::Playing => 1,
            PlaybackState::Paused => 2,
        };
        self.state.store(v, Ordering::Release);
    }

    fn state(&self) -> PlaybackState {
        match self.state.load(Ordering::Acquire) {
            1 => PlaybackState::Playing,
            2 => PlaybackState::Paused,
            _ => PlaybackState::Stopped,
        }
    }

    fn set_base(&self, position: Duration) {
        let micros = u64::try_from(position.as_micros()).unwrap_or(u64::MAX);
        self.base_micros.store(micros, Ordering::Release);
    }
}

/// Handle to the playback engine. Dropping it stops playback and joins the worker.
pub struct Engine {
    commands: Sender<Command>,
    events: Receiver<PlayerEvent>,
    shared: Arc<Shared>,
    status: Arc<Status>,
    vis: Arc<Mutex<Option<rtrb::Consumer<f32>>>>,
    thread: Option<JoinHandle<()>>,
}

impl std::fmt::Debug for Engine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Engine")
            .field("state", &self.state())
            .finish_non_exhaustive()
    }
}

/// Callback invoked (from engine threads) whenever new events are available, e.g. to request
/// a UI repaint.
pub type Notify = Arc<dyn Fn() + Send + Sync>;

impl Engine {
    /// Starts the engine with the given output. If a sound card cannot be opened, a silent
    /// real-time output is used instead and [`Engine::output_error`] explains why.
    #[must_use]
    pub fn start(spec: &OutputSpec, notify: Notify) -> Self {
        let shared = Arc::new(Shared::default());
        let status = Arc::new(Status::default());
        let vis = Arc::new(Mutex::new(None));
        let (cmd_tx, cmd_rx) = crossbeam_channel::unbounded();
        let (ev_tx, ev_rx) = crossbeam_channel::unbounded();
        let (out, producer) = open_output(spec, &shared, &status, &vis);
        let mut worker = Worker {
            commands: cmd_rx,
            events: ev_tx,
            notify,
            shared: Arc::clone(&shared),
            status: Arc::clone(&status),
            vis: Arc::clone(&vis),
            output: out,
            producer,
            current: None,
            eq: EqSettings::default(),
        };
        let thread = std::thread::Builder::new()
            .name("eleven-engine".into())
            .spawn(move || worker.run())
            .ok();
        Self {
            commands: cmd_tx,
            events: ev_rx,
            shared,
            status,
            vis,
            thread,
        }
    }

    fn send(&self, cmd: Command) {
        if cmd.supersedes_play() {
            self.status.interrupt.fetch_add(1, Ordering::AcqRel);
        }
        let _ = self.commands.send(cmd);
    }

    /// Starts playing `location` (stopping whatever was playing).
    pub fn play(&self, location: Location) {
        self.send(Command::Play(location));
    }

    /// Pauses playback.
    pub fn pause(&self) {
        self.send(Command::Pause);
    }

    /// Resumes paused playback.
    pub fn resume(&self) {
        self.send(Command::Resume);
    }

    /// Stops playback.
    pub fn stop(&self) {
        self.send(Command::Stop);
    }

    /// Seeks within the current track (ignored for live streams).
    pub fn seek(&self, position: Duration) {
        self.send(Command::Seek(position));
    }

    /// Updates the equaliser.
    pub fn set_eq(&self, settings: &EqSettings) {
        self.send(Command::SetEq(settings.clone()));
    }

    /// Switches to another output device.
    pub fn set_output(&self, spec: OutputSpec) {
        self.send(Command::SetOutput(spec));
    }

    /// Sets the volume slider value (`0.0..=1.0`). Takes effect immediately.
    pub fn set_volume(&self, volume: f32) {
        self.shared.set_volume(volume);
    }

    /// Sets the balance (`-1.0..=1.0`). Takes effect immediately.
    pub fn set_balance(&self, balance: f32) {
        self.shared.set_balance(balance);
    }

    /// The transport state.
    #[must_use]
    pub fn state(&self) -> PlaybackState {
        self.status.state()
    }

    /// Position within the current track (zero when stopped).
    #[must_use]
    pub fn position(&self) -> Duration {
        if self.state() == PlaybackState::Stopped {
            return Duration::ZERO;
        }
        let base = Duration::from_micros(self.status.base_micros.load(Ordering::Acquire));
        let rate = self.status.output_rate.load(Ordering::Acquire);
        base + frames_to_duration(self.shared.frames_played(), rate)
    }

    /// Takes all pending events.
    #[must_use]
    pub fn poll_events(&self) -> Vec<PlayerEvent> {
        self.events.try_iter().collect()
    }

    /// Waits up to `timeout` for the next event (useful in tests and tools).
    #[must_use]
    pub fn wait_event(&self, timeout: Duration) -> Option<PlayerEvent> {
        self.events.recv_timeout(timeout).ok()
    }

    /// Appends the newest mono samples for the visualiser to `out`.
    pub fn drain_vis(&self, out: &mut Vec<f32>) {
        let mut guard = self.vis.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(consumer) = guard.as_mut() {
            let n = consumer.slots();
            if let Ok(chunk) = consumer.read_chunk(n) {
                let (a, b) = chunk.as_slices();
                out.extend_from_slice(a);
                out.extend_from_slice(b);
                chunk.commit_all();
            }
        }
    }

    /// Name of the active output device.
    #[must_use]
    pub fn output_name(&self) -> String {
        self.status
            .output_name
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Sample rate of the active output device.
    #[must_use]
    pub fn output_rate(&self) -> u32 {
        self.status.output_rate.load(Ordering::Acquire)
    }

    /// Why the requested output could not be opened, if a fallback is in use.
    #[must_use]
    pub fn output_error(&self) -> Option<String> {
        self.status
            .output_error
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Number of output underruns so far (diagnostics).
    #[must_use]
    pub fn underruns(&self) -> u64 {
        self.shared.underruns()
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        self.send(Command::Shutdown);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// Opens the requested output, falling back to a silent real-time output on failure.
fn open_output(
    spec: &OutputSpec,
    shared: &Arc<Shared>,
    status: &Status,
    vis: &Mutex<Option<rtrb::Consumer<f32>>>,
) -> (Box<dyn Output>, rtrb::Producer<f32>) {
    let (out, producer, vis_consumer) = match output::open(spec, shared) {
        Ok(parts) => {
            *status
                .output_error
                .lock()
                .unwrap_or_else(PoisonError::into_inner) = None;
            parts
        }
        Err(e) => {
            log::warn!("falling back to silent output: {e}");
            *status
                .output_error
                .lock()
                .unwrap_or_else(PoisonError::into_inner) = Some(e.to_string());
            output::null_out::NullOutput::open(44_100, true, Arc::clone(shared))
        }
    };
    status
        .output_rate
        .store(out.sample_rate(), Ordering::Release);
    *status
        .output_name
        .lock()
        .unwrap_or_else(PoisonError::into_inner) = out.name();
    *vis.lock().unwrap_or_else(PoisonError::into_inner) = Some(vis_consumer);
    (out, producer)
}

/// The track being played.
struct Current {
    decoder: Decoder,
    resampler: Option<StreamResampler>,
    eq: Equalizer,
    decoded: Vec<f32>,
    pending: Vec<f32>,
    pending_pos: usize,
    eof: bool,
    paused: bool,
    /// Interrupt epoch when the track started (see [`Worker::superseded`]).
    epoch: u64,
}

/// What the worker should do after a processing step.
enum Step {
    /// More work is ready immediately.
    Continue,
    /// Wait a little (ring buffer full or draining).
    Wait,
    /// Nothing to do until a command arrives.
    Idle,
}

struct Worker {
    commands: Receiver<Command>,
    events: Sender<PlayerEvent>,
    notify: Notify,
    shared: Arc<Shared>,
    status: Arc<Status>,
    vis: Arc<Mutex<Option<rtrb::Consumer<f32>>>>,
    output: Box<dyn Output>,
    producer: rtrb::Producer<f32>,
    current: Option<Current>,
    eq: EqSettings,
}

impl Worker {
    fn emit(&self, event: PlayerEvent) {
        let _ = self.events.send(event);
        (self.notify)();
    }

    fn run(&mut self) {
        let mut step = Step::Idle;
        loop {
            // Wait for commands according to what the last step needs.
            let first = match step {
                Step::Continue => self.commands.try_recv().ok(),
                Step::Wait => match self.commands.recv_timeout(BUSY_WAIT) {
                    Ok(c) => Some(c),
                    Err(RecvTimeoutError::Timeout) => None,
                    Err(RecvTimeoutError::Disconnected) => Some(Command::Shutdown),
                },
                Step::Idle => Some(self.commands.recv().unwrap_or(Command::Shutdown)),
            };
            let mut batch: Vec<Command> = first.into_iter().collect();
            batch.extend(self.commands.try_iter());
            if !self.handle_batch(batch) {
                break;
            }
            step = self.step();
        }
        self.stop_playback();
    }

    /// Executes a batch of commands; returns `false` on shutdown.
    fn handle_batch(&mut self, batch: Vec<Command>) -> bool {
        // Only the last Play in a batch matters if something later supersedes it.
        let last_superseding = batch.iter().rposition(Command::supersedes_play);
        for (i, cmd) in batch.into_iter().enumerate() {
            match cmd {
                Command::Play(_) if last_superseding.is_some_and(|l| l > i) => {}
                Command::Play(location) => self.start(&location),
                Command::Pause => self.set_paused(true),
                Command::Resume => self.set_paused(false),
                Command::Stop => self.stop_playback(),
                Command::Seek(pos) => self.seek(pos),
                Command::SetEq(settings) => {
                    self.eq = settings;
                    if let Some(cur) = &mut self.current {
                        cur.eq.set_settings(&self.eq);
                    }
                }
                Command::SetOutput(spec) => self.switch_output(&spec),
                Command::Shutdown => return false,
            }
        }
        true
    }

    fn flush(&mut self) {
        self.shared.flush(FLUSH_TIMEOUT);
    }

    /// `true` if a newer transport command has been issued since `epoch`.
    fn superseded(&self, epoch: u64) -> bool {
        self.status.interrupt.load(Ordering::Acquire) != epoch
    }

    fn start(&mut self, location: &Location) {
        self.stop_playback();
        let epoch = self.status.interrupt.load(Ordering::Acquire);
        let opened = match location {
            Location::File(path) => Decoder::open_file(path),
            Location::Url(url) => {
                let events = self.events.clone();
                let notify = Arc::clone(&self.notify);
                let on_event: stream::OnEvent = Arc::new(move |e| {
                    let _ = events.send(e);
                    notify();
                });
                stream::open_decoder(url, Arc::clone(&self.status.interrupt), epoch, on_event)
            }
        };
        let decoder = match opened {
            Ok(d) => d,
            // A newer command (e.g. Stop while buffering) aborted the open: not an error.
            Err(_) if self.superseded(epoch) => return,
            Err(e) => {
                self.emit(PlayerEvent::Error(e.to_string()));
                return;
            }
        };
        let out_rate = self.output.sample_rate();
        let resampler = if decoder.sample_rate() == out_rate {
            None
        } else {
            match StreamResampler::new(decoder.sample_rate(), out_rate) {
                Ok(r) => Some(r),
                Err(e) => {
                    self.emit(PlayerEvent::Error(e.to_string()));
                    return;
                }
            }
        };
        let mut eq = Equalizer::new(out_rate);
        eq.set_settings(&self.eq);
        let info = decoder.info().clone();
        self.flush();
        self.status.set_base(Duration::ZERO);
        self.shared.set_paused(false);
        self.current = Some(Current {
            decoder,
            resampler,
            eq,
            decoded: Vec::new(),
            pending: Vec::new(),
            pending_pos: 0,
            eof: false,
            paused: false,
            epoch,
        });
        self.status.set_state(PlaybackState::Playing);
        self.emit(PlayerEvent::Started(info));
    }

    fn stop_playback(&mut self) {
        if self.current.take().is_some() {
            self.flush();
        }
        self.shared.set_paused(false);
        self.status.set_base(Duration::ZERO);
        self.status.set_state(PlaybackState::Stopped);
    }

    fn set_paused(&mut self, paused: bool) {
        if let Some(cur) = &mut self.current {
            cur.paused = paused;
            self.shared.set_paused(paused);
            self.status.set_state(if paused {
                PlaybackState::Paused
            } else {
                PlaybackState::Playing
            });
        }
    }

    fn seek(&mut self, position: Duration) {
        let Some(cur) = &mut self.current else {
            return;
        };
        match cur.decoder.seek(position) {
            Ok(actual) => {
                cur.pending.clear();
                cur.pending_pos = 0;
                cur.eof = false;
                if let Some(r) = &mut cur.resampler {
                    r.reset();
                }
                cur.eq.reset();
                self.shared.flush(FLUSH_TIMEOUT);
                self.status.set_base(actual);
            }
            Err(e) => log::debug!("seek ignored: {e}"),
        }
    }

    fn switch_output(&mut self, spec: &OutputSpec) {
        // Restart the current track on the new device at the same position.
        let resume = self
            .current
            .as_ref()
            .map(|c| (c.paused, self.position_estimate()));
        let (out, producer) = open_output(spec, &self.shared, &self.status, &self.vis);
        // Drop the old output before installing the new one.
        self.output = out;
        self.producer = producer;
        if let Some(cur) = &mut self.current {
            let out_rate = self.output.sample_rate();
            cur.resampler = if cur.decoder.sample_rate() == out_rate {
                None
            } else {
                StreamResampler::new(cur.decoder.sample_rate(), out_rate).ok()
            };
            cur.eq.set_sample_rate(out_rate);
            cur.pending.clear();
            cur.pending_pos = 0;
        }
        if let Some((paused, pos)) = resume {
            self.status.set_base(pos);
            self.shared.flush(FLUSH_TIMEOUT);
            self.set_paused(paused);
        }
    }

    fn position_estimate(&self) -> Duration {
        let base = Duration::from_micros(self.status.base_micros.load(Ordering::Acquire));
        base + frames_to_duration(self.shared.frames_played(), self.output.sample_rate())
    }

    /// Does one unit of work for the current track.
    fn step(&mut self) -> Step {
        let Some(cur) = &mut self.current else {
            return Step::Idle;
        };
        if cur.paused {
            return Step::Idle;
        }
        // 1. Push pending audio.
        if cur.pending_pos < cur.pending.len() {
            let free = self.producer.slots() & !1;
            if free == 0 {
                return Step::Wait;
            }
            let end = (cur.pending_pos + free).min(cur.pending.len());
            let (pushed, _) = self
                .producer
                .push_partial_slice(&cur.pending[cur.pending_pos..end]);
            cur.pending_pos += pushed.len();
            if cur.pending_pos >= cur.pending.len() {
                cur.pending.clear();
                cur.pending_pos = 0;
            }
            return Step::Continue;
        }
        // 2. Decode more.
        if !cur.eof {
            cur.decoded.clear();
            match cur.decoder.decode_next(&mut cur.decoded) {
                Ok(true) => {
                    match &mut cur.resampler {
                        Some(r) => r.process(&cur.decoded, &mut cur.pending),
                        None => cur.pending.extend_from_slice(&cur.decoded),
                    }
                    cur.eq.process(&mut cur.pending);
                }
                Ok(false) => {
                    cur.eof = true;
                    if let Some(r) = &mut cur.resampler {
                        r.finish(&mut cur.pending);
                        cur.eq.process(&mut cur.pending);
                    }
                }
                Err(e) => {
                    let msg = e.to_string();
                    let superseded = self.status.interrupt.load(Ordering::Acquire) != cur.epoch;
                    self.stop_playback();
                    if !superseded {
                        self.emit(PlayerEvent::Error(msg));
                    }
                    return Step::Idle;
                }
            }
            return Step::Continue;
        }
        // 3. End of track: wait for the output to drain, then report.
        if self.producer.slots() < self.producer.buffer().capacity() {
            return Step::Wait;
        }
        self.current = None;
        self.status.set_state(PlaybackState::Stopped);
        self.status.set_base(Duration::ZERO);
        self.emit(PlayerEvent::Finished);
        Step::Idle
    }
}
