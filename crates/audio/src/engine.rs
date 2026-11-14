//! The playback engine: a worker thread that decodes, resamples, equalises and feeds the output.
//!
//! [`Engine`] is the handle the application talks to. Transport commands travel over a channel;
//! volume, balance and position use shared atomics so they respond instantly.
//!
//! Every [`Engine::play`] returns a *generation* number, and every event carries the
//! generation of the play request it belongs to. Events can still be in flight when the next
//! track starts, so the application uses the generation to ignore stale ones.

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

/// An engine event tagged with the generation of the [`Engine::play`] call it belongs to.
pub type TaggedEvent = (u64, PlayerEvent);

/// Messages from the handle to the worker thread.
#[derive(Debug, Clone)]
enum Command {
    /// Play a location; the number is its generation (the new interrupt epoch).
    Play(Location, u64),
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
        matches!(self, Self::Play(..) | Self::Stop | Self::Shutdown)
    }
}

/// Engine status readable from any thread.
#[derive(Debug, Default)]
struct Status {
    state: AtomicU8,
    base_micros: AtomicU64,
    /// Position reported while a seek or device switch rebuilds the clock (micros + 1; 0 =
    /// none), so readers never see a half-updated base / frame count.
    held_micros: AtomicU64,
    output_rate: AtomicU32,
    output_name: Mutex<String>,
    output_error: Mutex<Option<String>>,
    /// Bumped by the handle on every Play, Stop and Shutdown so blocked network operations can
    /// abort. A Play's new value is its generation.
    interrupt: Arc<AtomicU64>,
    /// How many outputs have been opened (diagnostics and tests).
    outputs_opened: AtomicU64,
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

    fn hold(&self, position: Option<Duration>) {
        let v = position.map_or(0, |p| {
            u64::try_from(p.as_micros())
                .unwrap_or(u64::MAX - 1)
                .saturating_add(1)
        });
        self.held_micros.store(v, Ordering::Release);
    }

    fn held(&self) -> Option<Duration> {
        match self.held_micros.load(Ordering::Acquire) {
            0 => None,
            v => Some(Duration::from_micros(v - 1)),
        }
    }
}

/// Handle to the playback engine. Dropping it stops playback and joins the worker.
pub struct Engine {
    commands: Sender<Command>,
    events: Receiver<TaggedEvent>,
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
            spec: spec.clone(),
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

    /// Advances the interrupt epoch, aborting blocked network operations; returns the new value.
    fn interrupt(&self) -> u64 {
        self.status.interrupt.fetch_add(1, Ordering::AcqRel) + 1
    }

    fn send(&self, cmd: Command) {
        if cmd.supersedes_play() {
            self.interrupt();
        }
        let _ = self.commands.send(cmd);
    }

    /// Starts playing `location` (stopping whatever was playing). Returns the generation that
    /// tags this track's events.
    pub fn play(&self, location: Location) -> u64 {
        let generation = self.interrupt();
        let _ = self.commands.send(Command::Play(location, generation));
        generation
    }

    /// Pauses playback. Takes effect at once, even while the engine thread waits on a stalled
    /// stream; the engine catches up when it next runs.
    pub fn pause(&self) {
        if self.status.state() == PlaybackState::Playing {
            self.shared.set_paused(true);
            self.status.set_state(PlaybackState::Paused);
        }
        self.send(Command::Pause);
    }

    /// Resumes paused playback (at once, like [`Engine::pause`]).
    pub fn resume(&self) {
        if self.status.state() == PlaybackState::Paused {
            self.shared.set_paused(false);
            self.status.set_state(PlaybackState::Playing);
        }
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
        if let Some(held) = self.status.held() {
            return held;
        }
        let base = Duration::from_micros(self.status.base_micros.load(Ordering::Acquire));
        let rate = self.status.output_rate.load(Ordering::Acquire);
        base + frames_to_duration(self.shared.frames_played(), rate)
    }

    /// Takes all pending events, each tagged with its generation (see [`Engine::play`]).
    #[must_use]
    pub fn poll_events(&self) -> Vec<TaggedEvent> {
        self.events.try_iter().collect()
    }

    /// Waits up to `timeout` for the next event (useful in tests and tools).
    #[must_use]
    pub fn wait_event(&self, timeout: Duration) -> Option<TaggedEvent> {
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

/// A converter from `from` Hz to `to` Hz, or `None` when the rates already match.
fn resampler_for(from: u32, to: u32) -> crate::Result<Option<StreamResampler>> {
    if from == to {
        Ok(None)
    } else {
        StreamResampler::new(from, to).map(Some)
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
    status.outputs_opened.fetch_add(1, Ordering::AcqRel);
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

/// Stands in for the output while a device switch closes the old one.
struct Detached(u32);

impl Output for Detached {
    fn sample_rate(&self) -> u32 {
        self.0
    }
    fn name(&self) -> String {
        String::new()
    }
}

/// The track being played.
struct Current {
    decoder: Decoder,
    /// Sample rate the resampler was built for; chained streams can change it mid-track.
    source_rate: u32,
    /// Source channel count, to notice channel-layout changes in chained streams.
    channels: u16,
    resampler: Option<StreamResampler>,
    eq: Equalizer,
    decoded: Vec<f32>,
    pending: Vec<f32>,
    pending_pos: usize,
    eof: bool,
    paused: bool,
    /// The track's generation, which is also its interrupt epoch (see [`Worker::superseded`]).
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
    events: Sender<TaggedEvent>,
    notify: Notify,
    shared: Arc<Shared>,
    status: Arc<Status>,
    vis: Arc<Mutex<Option<rtrb::Consumer<f32>>>>,
    output: Box<dyn Output>,
    /// What `output` was opened from, to reopen it when its stream is lost.
    spec: OutputSpec,
    producer: rtrb::Producer<f32>,
    current: Option<Current>,
    eq: EqSettings,
}

impl Worker {
    fn emit(&self, generation: u64, event: PlayerEvent) {
        let _ = self.events.send((generation, event));
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
            if self.shared.take_stream_lost() {
                // The device went away (e.g. headphones unplugged): reopen the configured
                // output, which follows the system default or falls back to silence.
                log::warn!("audio stream lost; reopening the output");
                let spec = self.spec.clone();
                self.switch_output(&spec);
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
                Command::Play(..) if last_superseding.is_some_and(|l| l > i) => {}
                Command::Play(location, generation) => self.start(&location, generation),
                Command::Pause => self.set_paused(true),
                Command::Resume => self.set_paused(false),
                Command::Stop => self.stop_playback(),
                Command::Seek(pos) => {
                    self.seek(pos);
                }
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

    fn start(&mut self, location: &Location, epoch: u64) {
        self.stop_playback();
        let opened = match location {
            Location::File(path) => Decoder::open_file(path),
            Location::Url(url) => {
                let events = self.events.clone();
                let notify = Arc::clone(&self.notify);
                let on_event: stream::OnEvent = Arc::new(move |e| {
                    let _ = events.send((epoch, e));
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
                self.emit(epoch, PlayerEvent::Error(e.to_string()));
                return;
            }
        };
        let out_rate = self.output.sample_rate();
        let source_rate = decoder.sample_rate();
        let resampler = match resampler_for(source_rate, out_rate) {
            Ok(r) => r,
            Err(e) => {
                self.emit(epoch, PlayerEvent::Error(e.to_string()));
                return;
            }
        };
        let mut eq = Equalizer::new(out_rate);
        eq.set_settings(&self.eq);
        let info = decoder.info().clone();
        self.flush();
        self.status.set_base(Duration::ZERO);
        self.shared.set_paused(false);
        self.current = Some(Current {
            channels: decoder.info().channels,
            decoder,
            source_rate,
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
        self.emit(epoch, PlayerEvent::Started(info));
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
        } else {
            // The handle may have marked a track paused that ended meanwhile.
            self.shared.set_paused(false);
            self.status.set_state(PlaybackState::Stopped);
        }
    }

    /// Seeks the current track; returns `false` if there is none or it cannot seek.
    fn seek(&mut self, position: Duration) -> bool {
        let Some(cur) = &mut self.current else {
            return false;
        };
        match cur.decoder.seek(position) {
            Ok(actual) => {
                self.status.hold(Some(actual));
                cur.pending.clear();
                cur.pending_pos = 0;
                cur.eof = false;
                if let Some(r) = &mut cur.resampler {
                    r.reset();
                }
                cur.eq.reset();
                self.shared.flush(FLUSH_TIMEOUT);
                self.status.set_base(actual);
                self.status.hold(None);
                true
            }
            Err(e) => {
                log::debug!("seek ignored: {e}");
                false
            }
        }
    }

    fn switch_output(&mut self, spec: &OutputSpec) {
        // Continue the current track on the new device from what was actually heard.
        let resume = self
            .current
            .as_ref()
            .map(|c| (c.paused, self.position_estimate()));
        self.status.hold(resume.map(|(_, position)| position));
        self.spec = spec.clone();
        // Release the old device before opening the new one: ALSA refuses to open a card that
        // is still in use (e.g. `hw:` -> `plughw:` on the same card).
        let rate = self.output.sample_rate();
        drop(std::mem::replace(
            &mut self.output,
            Box::new(Detached(rate)),
        ));
        let (out, producer) = open_output(spec, &self.shared, &self.status, &self.vis);
        self.output = out;
        self.producer = producer;
        let out_rate = self.output.sample_rate();
        let Some((paused, position)) = resume else {
            return;
        };
        self.resume_on_new_output(paused, position, out_rate);
        self.status.hold(None);
    }

    /// Continues the current track at `position` after the output changed to `out_rate` Hz.
    fn resume_on_new_output(&mut self, paused: bool, position: Duration, out_rate: u32) {
        let generation = self.current.as_ref().map_or(0, |c| c.epoch);
        if let Some(cur) = &mut self.current {
            match resampler_for(cur.source_rate, out_rate) {
                Ok(r) => cur.resampler = r,
                Err(e) => {
                    let msg = e.to_string();
                    self.stop_playback();
                    self.emit(generation, PlayerEvent::Error(msg));
                    return;
                }
            }
            cur.eq.set_sample_rate(out_rate);
        }
        // The old ring buffer and pending samples are gone: re-seek so nothing is skipped.
        // Live streams cannot seek; they just continue with the clock carried over.
        if !self.seek(position) {
            if let Some(cur) = &mut self.current {
                cur.pending.clear();
                cur.pending_pos = 0;
                cur.eq.reset();
            }
            self.shared.flush(FLUSH_TIMEOUT);
            self.status.set_base(position);
        }
        self.set_paused(paused);
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
        // A flush timed out and the output has not performed it yet: it would discard what we
        // push now (the start of a track, or audio after a seek), so wait for it.
        if self.shared.flush_pending() {
            return Step::Wait;
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
                    let rate = cur.decoder.sample_rate();
                    let channels = cur.decoder.info().channels;
                    let format_changed = rate != cur.source_rate || channels != cur.channels;
                    cur.channels = channels;
                    if rate != cur.source_rate {
                        // A chained stream (e.g. Ogg radio) switched sample rate: finish the
                        // old conversion and convert the new segment at its own rate.
                        if let Some(r) = &mut cur.resampler {
                            r.finish(&mut cur.pending);
                        }
                        match resampler_for(rate, self.output.sample_rate()) {
                            Ok(r) => cur.resampler = r,
                            Err(e) => {
                                let (msg, generation) = (e.to_string(), cur.epoch);
                                self.stop_playback();
                                self.emit(generation, PlayerEvent::Error(msg));
                                return Step::Idle;
                            }
                        }
                        cur.source_rate = rate;
                    }
                    match &mut cur.resampler {
                        Some(r) => r.process(&cur.decoded, &mut cur.pending),
                        None => cur.pending.extend_from_slice(&cur.decoded),
                    }
                    cur.eq.process(&mut cur.pending);
                    if format_changed {
                        let (info, generation) = (cur.decoder.info().clone(), cur.epoch);
                        self.emit(generation, PlayerEvent::FormatChanged(info));
                    }
                }
                Ok(false) => {
                    cur.eof = true;
                    if let Some(r) = &mut cur.resampler {
                        r.finish(&mut cur.pending);
                        cur.eq.process(&mut cur.pending);
                    }
                }
                Err(e) => {
                    let (msg, generation) = (e.to_string(), cur.epoch);
                    self.stop_playback();
                    if !self.superseded(generation) {
                        self.emit(generation, PlayerEvent::Error(msg));
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
        let generation = cur.epoch;
        self.current = None;
        self.status.set_state(PlaybackState::Stopped);
        self.status.set_base(Duration::ZERO);
        self.emit(generation, PlayerEvent::Finished);
        Step::Idle
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_lost_output_stream_is_reopened() {
        let engine = Engine::start(
            &OutputSpec::Null {
                sample_rate: 44_100,
                realtime: true,
            },
            Arc::new(|| {}),
        );
        assert_eq!(engine.status.outputs_opened.load(Ordering::Acquire), 1);
        // What the cpal error callback does when the device disappears.
        engine.shared.report_stream_lost();
        // The engine notices on its next command (or step while playing).
        engine.set_eq(&EqSettings::default());
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while engine.status.outputs_opened.load(Ordering::Acquire) < 2 {
            assert!(std::time::Instant::now() < deadline, "output not reopened");
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(engine.output_rate(), 44_100);
        assert!(engine.output_error().is_none());
    }

    #[test]
    fn pausing_a_finished_track_settles_on_stopped() {
        let engine = Engine::start(
            &OutputSpec::Null {
                sample_rate: 44_100,
                realtime: false,
            },
            Arc::new(|| {}),
        );
        // As if the handle marked a track paused just as it ended.
        engine.status.set_state(PlaybackState::Paused);
        engine.shared.set_paused(true);
        engine.pause();
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while engine.state() != PlaybackState::Stopped {
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(2));
        }
        assert!(!engine.shared.is_paused());
    }
}
