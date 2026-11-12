//! The application controller: everything the player does, independent of how it is drawn.
//!
//! The UI calls methods on [`Controller`] and reads its state; the controller talks to the
//! audio engine, tag scanner and OS media controls through the traits in [`crate::backend`],
//! which makes all of this testable with fakes.

use std::path::{Path, PathBuf};
use std::time::Duration;

use eleven_core::analyzer::{Analyzer, oscilloscope};
use eleven_core::eq::{BANDS, EqPreset, MAX_GAIN_DB, builtin_presets};
use eleven_core::format::{ClockDisplay, Marquee, marquee_text};
use eleven_core::playlist::{Entry, SortKey};
use eleven_core::playlist_io::{self, PlaylistItem};
use eleven_core::{
    Config, Location, PlaybackState, PlayerEvent, Playlist, SelectMode, TrackInfo, VisMode,
    is_audio_path,
};

use crate::backend::{Backend, MediaCommand, MediaControls, TagReader};

/// How long transient messages stay in the title display, in seconds.
const MESSAGE_SECS: f32 = 3.0;
/// Number of oscilloscope points (one per visualiser pixel column).
pub const SCOPE_POINTS: usize = 76;
/// Maximum directory depth when adding folders (guards against symlink loops).
const MAX_DEPTH: usize = 32;
/// Seconds moved by the seek keys.
pub const SEEK_STEP: Duration = Duration::from_secs(5);
/// Volume change per key press.
pub const VOLUME_STEP: f32 = 0.05;

/// Where the controller persists state. `None` fields disable persistence (tests).
#[derive(Debug, Clone, Default)]
pub struct Persistence {
    /// The TOML config file.
    pub config_file: Option<PathBuf>,
    /// The auto-saved playlist (M3U).
    pub playlist_file: Option<PathBuf>,
}

/// The player.
pub struct Controller {
    config: Config,
    playlist: Playlist,
    backend: Box<dyn Backend>,
    tags: Box<dyn TagReader>,
    media: Box<dyn MediaControls>,
    persistence: Persistence,
    analyzer: Analyzer,
    vis_buffer: Vec<f32>,
    scope: Vec<f32>,
    marquee: Marquee,
    track: Option<TrackInfo>,
    stream_title: Option<String>,
    buffering: Option<u8>,
    message: Option<(String, f32)>,
    consecutive_errors: usize,
    last_published: Option<(PlaybackState, String)>,
}

impl std::fmt::Debug for Controller {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Controller")
            .field("entries", &self.playlist.len())
            .field("state", &self.state())
            .finish_non_exhaustive()
    }
}

impl Controller {
    /// Creates the controller, restores the saved playlist and pushes the saved volume,
    /// balance and EQ to the backend.
    pub fn new(
        config: Config,
        backend: Box<dyn Backend>,
        tags: Box<dyn TagReader>,
        media: Box<dyn MediaControls>,
        persistence: Persistence,
    ) -> Self {
        let mut this = Self {
            config,
            playlist: Playlist::new(),
            backend,
            tags,
            media,
            persistence,
            analyzer: Analyzer::default(),
            vis_buffer: Vec::new(),
            scope: vec![0.0; SCOPE_POINTS],
            marquee: Marquee::new(""),
            track: None,
            stream_title: None,
            buffering: None,
            message: None,
            consecutive_errors: 0,
            last_published: None,
        };
        this.playlist.set_shuffle(this.config.shuffle);
        this.playlist.set_repeat(this.config.repeat);
        this.backend.set_volume(this.config.volume);
        this.backend.set_balance(this.config.balance);
        this.backend.set_eq(&this.config.equalizer);
        if let Some(path) = this.persistence.playlist_file.clone()
            && path.exists()
        {
            this.load_playlist(&path);
            if let Some(i) = this.config.current_track {
                this.playlist.set_current(i);
            }
        }
        if let Some(err) = this.backend.output_error() {
            this.show_message(&format!("No audio device ({err}); playing silently"));
        }
        this.refresh_marquee();
        this
    }

    // ----------------------------------------------------------------------------------------
    // Read-only state for the UI

    /// The configuration (window toggles, volume, EQ, ...).
    #[must_use]
    pub fn config(&self) -> &Config {
        &self.config
    }

    /// The playlist.
    #[must_use]
    pub fn playlist(&self) -> &Playlist {
        &self.playlist
    }

    /// Mutable playlist access for editing commands (selection, sorting, removal...).
    pub fn playlist_mut(&mut self) -> &mut Playlist {
        &mut self.playlist
    }

    /// Transport state.
    #[must_use]
    pub fn state(&self) -> PlaybackState {
        self.backend.state()
    }

    /// Position within the current track.
    #[must_use]
    pub fn position(&self) -> Duration {
        self.backend.position()
    }

    /// Length of the current track, if known.
    #[must_use]
    pub fn duration(&self) -> Option<Duration> {
        self.track
            .as_ref()
            .and_then(|t| t.duration)
            .or_else(|| self.playlist.current().and_then(|e| e.duration))
    }

    /// Whether the current track can be seeked.
    #[must_use]
    pub fn seekable(&self) -> bool {
        self.state().is_active()
            && self.track.as_ref().is_some_and(|t| t.seekable)
            && self.duration().is_some()
    }

    /// Technical info about the playing track.
    #[must_use]
    pub fn track_info(&self) -> Option<&TrackInfo> {
        self.track.as_ref().filter(|_| self.state().is_active())
    }

    /// The four-digit clock.
    #[must_use]
    pub fn clock(&self) -> ClockDisplay {
        ClockDisplay::new(self.position(), self.duration(), self.config.time_mode)
    }

    /// Playback progress in `0.0..=1.0`, if the length is known.
    #[must_use]
    pub fn progress(&self) -> Option<f32> {
        let d = self.duration()?.as_secs_f32();
        (d > 0.0).then(|| (self.position().as_secs_f32() / d).clamp(0.0, 1.0))
    }

    /// The visible title text, `width` characters wide.
    #[must_use]
    pub fn marquee_visible(&self, width: usize) -> String {
        self.marquee.visible(width, self.config.scroll_title)
    }

    /// The full title text (for tooltips and accessibility).
    #[must_use]
    pub fn title_text(&self) -> String {
        self.marquee.text()
    }

    /// Spectrum bar levels and peak markers.
    #[must_use]
    pub fn spectrum(&self) -> (&[f32], &[f32]) {
        (self.analyzer.levels(), self.analyzer.peaks())
    }

    /// Oscilloscope samples (`-1.0..=1.0`).
    #[must_use]
    pub fn scope(&self) -> &[f32] {
        &self.scope
    }

    /// The name of the output device.
    #[must_use]
    pub fn output_name(&self) -> String {
        self.backend.output_name()
    }

    /// Available output devices.
    #[must_use]
    pub fn output_devices(&self) -> Vec<String> {
        self.backend.output_devices()
    }

    /// All presets: built-in followed by the user's.
    #[must_use]
    pub fn presets(&self) -> Vec<EqPreset> {
        let mut all = builtin_presets();
        all.extend(self.config.eq_presets.iter().cloned());
        all
    }

    // ----------------------------------------------------------------------------------------
    // Periodic update

    /// Processes engine/tag/media events and advances animations by `dt` seconds.
    pub fn update(&mut self, dt: f32) {
        for event in self.backend.poll_events() {
            self.handle_event(event);
        }
        for (id, tags) in self.tags.poll() {
            self.playlist
                .update_metadata(id, tags.title, tags.artist, tags.duration);
            if self.playlist.current_id() == Some(id) && self.stream_title.is_none() {
                self.refresh_marquee();
            }
        }
        for cmd in self.media.poll() {
            self.handle_media(cmd);
        }
        self.update_vis(dt);
        if let Some((_, left)) = &mut self.message {
            *left -= dt;
            if *left <= 0.0 {
                self.message = None;
                self.refresh_marquee();
            }
        }
        self.marquee.tick(dt);
        self.publish_media_state();
    }

    fn update_vis(&mut self, dt: f32) {
        self.vis_buffer.clear();
        self.backend.drain_vis(&mut self.vis_buffer);
        let playing = self.state() == PlaybackState::Playing;
        if playing && !self.vis_buffer.is_empty() {
            if let Some(rate) = self.track.as_ref().map(|t| t.sample_rate) {
                // The tap runs at the device rate; the track rate is a close-enough proxy.
                self.analyzer.set_sample_rate(rate);
            }
            self.analyzer.update(&self.vis_buffer, dt);
            let tail = self.vis_buffer.len().saturating_sub(SCOPE_POINTS * 4);
            self.scope = oscilloscope(&self.vis_buffer[tail..], SCOPE_POINTS);
        } else {
            self.analyzer.decay(dt);
            for s in &mut self.scope {
                *s *= 0.8;
            }
        }
    }

    fn handle_event(&mut self, event: PlayerEvent) {
        match event {
            PlayerEvent::Started(info) => {
                self.track = Some(info);
                self.stream_title = None;
                self.buffering = None;
                self.consecutive_errors = 0;
                self.analyzer.reset();
                self.refresh_marquee();
            }
            PlayerEvent::Buffering(pct) => {
                self.buffering = (pct < 100).then_some(pct);
                self.refresh_marquee();
            }
            PlayerEvent::StreamTitle(title) => {
                if let Some(id) = self.playlist.current_id() {
                    self.playlist
                        .update_metadata(id, Some(title.clone()), None, None);
                }
                self.stream_title = Some(title);
                self.refresh_marquee();
            }
            PlayerEvent::Finished => {
                if self.playlist.next_track().is_some() {
                    self.play_current();
                } else {
                    self.refresh_marquee();
                }
            }
            PlayerEvent::Error(msg) => {
                self.consecutive_errors += 1;
                log::warn!("playback error: {msg}");
                self.show_message(&format!("Error: {msg}"));
                // Skip unplayable entries, but give up once every entry has failed.
                if self.consecutive_errors < self.playlist.len()
                    && self.playlist.next_track().is_some()
                {
                    self.play_current();
                }
            }
        }
    }

    fn handle_media(&mut self, cmd: MediaCommand) {
        match cmd {
            MediaCommand::Play => self.play(),
            MediaCommand::Pause => {
                if self.state() == PlaybackState::Playing {
                    self.pause();
                }
            }
            MediaCommand::Toggle => {
                if self.state() == PlaybackState::Playing {
                    self.pause();
                } else {
                    self.play();
                }
            }
            MediaCommand::Stop => self.stop(),
            MediaCommand::Next => self.next(),
            MediaCommand::Previous => self.previous(),
        }
    }

    fn publish_media_state(&mut self) {
        let state = self.state();
        let title = self.current_title();
        if self.last_published.as_ref() != Some(&(state, title.clone())) {
            let artist = self.playlist.current().and_then(|e| e.artist.clone());
            self.media
                .set_metadata(&title, artist.as_deref(), self.duration());
            self.media.set_playback(state, self.position());
            self.last_published = Some((state, title));
        }
    }

    /// Shows a transient message in the title display.
    pub fn show_message(&mut self, text: &str) {
        self.message = Some((text.to_owned(), MESSAGE_SECS));
        self.refresh_marquee();
    }

    /// The entry the title display describes: the current track, or else the first entry
    /// (what Play would start).
    fn display_entry(&self) -> Option<(usize, &Entry)> {
        match self.playlist.current_index() {
            Some(i) => self.playlist.get(i).map(|e| (i, e)),
            None => self.playlist.get(0).map(|e| (0, e)),
        }
    }

    fn current_title(&self) -> String {
        if let Some(t) = &self.stream_title {
            return t.clone();
        }
        match self.display_entry() {
            Some((_, e)) => e.display_title(),
            None => self
                .track
                .as_ref()
                .and_then(|t| t.title.clone())
                .unwrap_or_default(),
        }
    }

    fn refresh_marquee(&mut self) {
        let text = if let Some((msg, _)) = &self.message {
            msg.clone()
        } else if let Some(pct) = self.buffering {
            format!("Buffering... {pct}%")
        } else if let Some((index, entry)) = self.display_entry() {
            let duration = if self.stream_title.is_some() {
                None
            } else if self.playlist.current_index() == Some(index) {
                self.duration()
            } else {
                entry.duration
            };
            marquee_text(Some(index + 1), &self.current_title(), duration)
        } else {
            "eleven - it goes to eleven".to_owned()
        };
        self.marquee.set_text(&text);
    }

    // ----------------------------------------------------------------------------------------
    // Transport

    fn play_current(&mut self) {
        let Some(entry) = self.playlist.current() else {
            return;
        };
        let location = entry.location.clone();
        self.track = None;
        self.stream_title = None;
        self.buffering = location.is_url().then_some(0);
        self.backend.play(&location);
        self.refresh_marquee();
    }

    /// XMMS "Play": resume if paused, restart the current track if playing, otherwise start the
    /// current (or first) track.
    pub fn play(&mut self) {
        match self.state() {
            PlaybackState::Paused => self.backend.resume(),
            _ => {
                if self.playlist.current().is_none() && self.playlist.next_track().is_none() {
                    return;
                }
                self.consecutive_errors = 0;
                self.play_current();
            }
        }
    }

    /// Plays the entry at `index` (double-click in the playlist).
    pub fn play_index(&mut self, index: usize) {
        if self.playlist.set_current(index).is_some() {
            self.consecutive_errors = 0;
            self.play_current();
        }
    }

    /// Toggles pause while a track is loaded.
    pub fn pause(&mut self) {
        match self.state() {
            PlaybackState::Playing => self.backend.pause(),
            PlaybackState::Paused => self.backend.resume(),
            PlaybackState::Stopped => {}
        }
    }

    /// Stops playback.
    pub fn stop(&mut self) {
        self.backend.stop();
        self.buffering = None;
        self.refresh_marquee();
    }

    /// Next track; starts it if something is playing.
    pub fn next(&mut self) {
        let active = self.state().is_active();
        if self.playlist.next_track().is_some() {
            if active {
                self.play_current();
            } else {
                self.refresh_marquee();
            }
        }
    }

    /// Previous track; starts it if something is playing.
    pub fn previous(&mut self) {
        let active = self.state().is_active();
        if self.playlist.prev_track().is_some() {
            if active {
                self.play_current();
            } else {
                self.refresh_marquee();
            }
        }
    }

    /// Seeks to a fraction (`0.0..=1.0`) of the track.
    pub fn seek_fraction(&mut self, fraction: f32) {
        if let Some(d) = self.duration().filter(|_| self.seekable()) {
            self.backend.seek(d.mul_f32(fraction.clamp(0.0, 1.0)));
        }
    }

    /// Seeks forwards (`forward = true`) or backwards by `step`.
    pub fn seek_relative(&mut self, step: Duration, forward: bool) {
        if !self.seekable() {
            return;
        }
        let pos = self.position();
        let target = if forward {
            pos + step
        } else {
            pos.saturating_sub(step)
        };
        let target = self.duration().map_or(target, |d| target.min(d));
        self.backend.seek(target);
    }

    // ----------------------------------------------------------------------------------------
    // Volume, balance, toggles

    /// Sets the volume (`0.0..=1.0`).
    pub fn set_volume(&mut self, volume: f32) {
        self.config.volume = volume.clamp(0.0, 1.0);
        self.backend.set_volume(self.config.volume);
    }

    /// Nudges the volume by `delta`.
    pub fn adjust_volume(&mut self, delta: f32) {
        self.set_volume(self.config.volume + delta);
    }

    /// Sets the balance (`-1.0..=1.0`); values near the centre snap to it, as in XMMS.
    pub fn set_balance(&mut self, balance: f32) {
        let b = balance.clamp(-1.0, 1.0);
        self.config.balance = if b.abs() < 0.08 { 0.0 } else { b };
        self.backend.set_balance(self.config.balance);
    }

    /// Toggles shuffle.
    pub fn toggle_shuffle(&mut self) {
        self.config.shuffle = !self.config.shuffle;
        self.playlist.set_shuffle(self.config.shuffle);
    }

    /// Toggles repeat.
    pub fn toggle_repeat(&mut self) {
        self.config.repeat = !self.config.repeat;
        self.playlist.set_repeat(self.config.repeat);
    }

    /// Switches between elapsed and remaining time.
    pub fn toggle_time_mode(&mut self) {
        self.config.time_mode = self.config.time_mode.toggled();
    }

    /// Cycles spectrum → oscilloscope → off.
    pub fn cycle_vis_mode(&mut self) {
        self.config.vis_mode = self.config.vis_mode.cycled();
    }

    /// Sets the visualiser mode.
    pub fn set_vis_mode(&mut self, mode: VisMode) {
        self.config.vis_mode = mode;
    }

    /// Mutable access to window/display options (EQ/PL visibility, double size, ...).
    pub fn config_mut(&mut self) -> &mut Config {
        &mut self.config
    }

    /// Switches the output device.
    pub fn set_output_device(&mut self, name: Option<String>) {
        self.config.output_device.clone_from(&name);
        self.backend.set_output_device(name);
    }

    // ----------------------------------------------------------------------------------------
    // Equaliser

    fn push_eq(&mut self) {
        self.config.equalizer = self.config.equalizer.sanitized();
        self.backend.set_eq(&self.config.equalizer);
    }

    /// Turns the equaliser on or off.
    pub fn set_eq_enabled(&mut self, on: bool) {
        self.config.equalizer.enabled = on;
        self.push_eq();
    }

    /// Sets one band's gain in dB.
    pub fn set_eq_band(&mut self, band: usize, db: f32) {
        if band < BANDS {
            self.config.equalizer.bands_db[band] = db.clamp(-MAX_GAIN_DB, MAX_GAIN_DB);
            self.push_eq();
        }
    }

    /// Sets the preamp gain in dB.
    pub fn set_preamp(&mut self, db: f32) {
        self.config.equalizer.preamp_db = db.clamp(-MAX_GAIN_DB, MAX_GAIN_DB);
        self.push_eq();
    }

    /// Loads a preset's gains.
    pub fn apply_preset(&mut self, preset: &EqPreset) {
        self.config.equalizer.apply_preset(preset);
        self.push_eq();
        self.show_message(&format!("EQ preset: {}", preset.name));
    }

    /// Saves the current gains as a user preset.
    pub fn save_preset(&mut self, name: &str) {
        let name = name.trim();
        if !name.is_empty() {
            self.config
                .save_preset(EqPreset::from_settings(name, &self.config.equalizer));
        }
    }

    /// Deletes a user preset.
    pub fn delete_preset(&mut self, name: &str) -> bool {
        self.config.delete_preset(name)
    }

    /// Resets all bands and the preamp to 0 dB.
    pub fn reset_eq(&mut self) {
        self.config.equalizer.preamp_db = 0.0;
        self.config.equalizer.bands_db = [0.0; BANDS];
        self.push_eq();
    }

    // ----------------------------------------------------------------------------------------
    // Playlist contents

    /// Adds files, folders (recursively) and playlist files. Returns the number of entries
    /// added. Tags are requested for new local files.
    pub fn add_paths(&mut self, paths: &[PathBuf]) -> usize {
        let mut added = 0;
        for path in paths {
            if path.is_dir() {
                let mut files = Vec::new();
                collect_audio_files(path, 0, &mut files);
                for f in &files {
                    self.add_file(f);
                    added += 1;
                }
            } else if playlist_io::is_playlist_path(path) {
                added += self.load_playlist(path);
            } else if path.is_file() && is_audio_path(path) {
                self.add_file(path);
                added += 1;
            }
        }
        if let Some(parent) = paths.first().and_then(|p| {
            if p.is_dir() {
                Some(p.as_path())
            } else {
                p.parent()
            }
        }) {
            self.config.last_directory = Some(parent.to_owned());
        }
        self.refresh_marquee();
        added
    }

    fn add_file(&mut self, path: &Path) {
        let id = self.playlist.push(Location::File(path.to_owned()));
        self.tags.request(id, path);
    }

    /// Adds a stream URL (or a path typed by hand).
    pub fn add_location(&mut self, text: &str) -> bool {
        let text = text.trim();
        if text.is_empty() {
            return false;
        }
        let location = Location::parse(text, None);
        let id = self.playlist.push(location.clone());
        if let Location::File(p) = &location {
            self.tags.request(id, p);
        }
        self.refresh_marquee();
        true
    }

    /// Appends a playlist file's entries; returns how many were added.
    pub fn load_playlist(&mut self, path: &Path) -> usize {
        match playlist_io::load(path) {
            Ok(items) => {
                let n = items.len();
                for item in items {
                    let id =
                        self.playlist
                            .push_with(item.location.clone(), item.title, item.duration);
                    if let Location::File(p) = &item.location {
                        self.tags.request(id, p);
                    }
                }
                n
            }
            Err(e) => {
                self.show_message(&format!("Cannot load playlist: {e}"));
                0
            }
        }
    }

    fn playlist_items(&self) -> Vec<PlaylistItem> {
        self.playlist
            .entries()
            .iter()
            .map(|e: &Entry| PlaylistItem {
                location: e.location.clone(),
                title: Some(e.display_title()),
                duration: e.duration,
            })
            .collect()
    }

    /// Writes the playlist to `path` (format chosen by extension).
    ///
    /// # Errors
    /// Returns a description of the failure.
    pub fn save_playlist(&mut self, path: &Path) -> Result<(), String> {
        playlist_io::save(path, &self.playlist_items()).map_err(|e| e.to_string())
    }

    /// Sorts the playlist.
    pub fn sort(&mut self, key: SortKey) {
        self.playlist.sort_by(key);
        self.refresh_marquee();
    }

    /// Plays the first search result (jump-to-file).
    pub fn jump_to(&mut self, index: usize) {
        self.playlist.select(index, SelectMode::Single);
        self.play_index(index);
    }

    /// Removes the selected entries.
    pub fn remove_selected(&mut self) {
        self.playlist.remove_selected();
        self.refresh_marquee();
    }

    // ----------------------------------------------------------------------------------------
    // Persistence

    /// Saves the config and the playlist (if persistence is configured).
    ///
    /// # Errors
    /// Returns a description of the first failure.
    pub fn save_state(&mut self) -> Result<(), String> {
        self.config.current_track = self.playlist.current_index();
        if let Some(path) = self.persistence.playlist_file.clone() {
            if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
            }
            self.save_playlist(&path)?;
        }
        if let Some(path) = &self.persistence.config_file {
            self.config.save(path).map_err(|e| e.to_string())?;
        }
        Ok(())
    }
}

/// Recursively collects audio files under `dir`, sorted by name, skipping hidden entries.
fn collect_audio_files(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    if depth > MAX_DEPTH {
        return;
    }
    let Ok(read) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<PathBuf> = read
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| {
            !p.file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with('.'))
        })
        .collect();
    entries.sort_by_key(|p| p.file_name().map(|n| n.to_string_lossy().to_lowercase()));
    for path in entries {
        if path.is_dir() {
            collect_audio_files(&path, depth + 1, out);
        } else if is_audio_path(&path) {
            out.push(path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::fake::{
        Call, FakeBackend, FakeMediaControls, FakeMediaState, FakeState, FakeTags,
    };
    use eleven_core::TimeMode;
    use std::sync::{Arc, Mutex};

    struct Rig {
        c: Controller,
        backend: Arc<Mutex<FakeState>>,
        media: Arc<Mutex<FakeMediaState>>,
    }

    fn rig_with(config: Config, persistence: Persistence) -> Rig {
        let (backend, state) = FakeBackend::new();
        let media = FakeMediaControls::default();
        let media_state = Arc::clone(&media.state);
        let c = Controller::new(
            config,
            Box::new(backend),
            Box::new(FakeTags::default()),
            Box::new(media),
            persistence,
        );
        Rig {
            c,
            backend: state,
            media: media_state,
        }
    }

    fn rig() -> Rig {
        rig_with(Config::default(), Persistence::default())
    }

    fn with_tracks(n: usize) -> Rig {
        let mut r = rig();
        for i in 0..n {
            r.c.add_location(&format!("/music/{i:02}.mp3"));
        }
        r.c.update(0.0);
        r
    }

    fn file(i: usize) -> Location {
        Location::File(PathBuf::from(format!("/music/{i:02}.mp3")))
    }

    fn push_event(r: &Rig, e: PlayerEvent) {
        r.backend.lock().unwrap().events.push(e);
    }

    fn info(seekable: bool, secs: Option<u64>) -> TrackInfo {
        TrackInfo {
            title: None,
            sample_rate: 44_100,
            channels: 2,
            bitrate_kbps: Some(128),
            duration: secs.map(Duration::from_secs),
            seekable,
            codec: "mp3".into(),
        }
    }

    #[test]
    fn startup_pushes_settings_to_backend() {
        let config = Config {
            volume: 0.3,
            balance: 0.5,
            shuffle: true,
            repeat: true,
            ..Config::default()
        };
        let r = rig_with(config, Persistence::default());
        let calls = r.backend.lock().unwrap().calls.clone();
        assert!(calls.contains(&Call::Volume(0.3)));
        assert!(calls.contains(&Call::Balance(0.5)));
        assert!(calls.iter().any(|c| matches!(c, Call::Eq(_))));
        assert!(r.c.playlist().shuffle() && r.c.playlist().repeat());
        assert_eq!(r.c.title_text(), "eleven - it goes to eleven");
        assert!(format!("{:?}", r.c).contains("Controller"));
    }

    #[test]
    fn play_starts_first_track_and_tags_fill_titles() {
        let mut r = with_tracks(3);
        assert_eq!(
            r.c.playlist().get(0).unwrap().display_title(),
            "Fake Artist - Title of 00"
        );
        r.c.play();
        assert_eq!(r.backend.lock().unwrap().plays(), [file(0)]);
        assert_eq!(r.c.state(), PlaybackState::Playing);
        assert!(
            r.c.title_text()
                .starts_with("1. Fake Artist - Title of 00 (1:00)")
        );
    }

    #[test]
    fn play_on_empty_playlist_does_nothing() {
        let mut r = rig();
        r.c.play();
        r.c.next();
        r.c.previous();
        assert!(r.backend.lock().unwrap().plays().is_empty());
    }

    #[test]
    fn xmms_play_pause_semantics() {
        let mut r = with_tracks(2);
        r.c.play();
        r.c.pause();
        assert_eq!(r.c.state(), PlaybackState::Paused);
        r.c.play(); // resumes
        assert_eq!(r.c.state(), PlaybackState::Playing);
        r.c.pause();
        r.c.pause(); // toggles back
        assert_eq!(r.c.state(), PlaybackState::Playing);
        r.c.play(); // restarts the current track
        assert_eq!(r.backend.lock().unwrap().plays(), [file(0), file(0)]);
        r.c.stop();
        assert_eq!(r.c.state(), PlaybackState::Stopped);
        r.c.pause();
        assert_eq!(
            r.c.state(),
            PlaybackState::Stopped,
            "pause does nothing when stopped"
        );
    }

    #[test]
    fn finished_advances_and_stops_at_end() {
        let mut r = with_tracks(2);
        r.c.play();
        push_event(&r, PlayerEvent::Finished);
        r.c.update(0.016);
        assert_eq!(r.backend.lock().unwrap().plays(), [file(0), file(1)]);
        r.backend.lock().unwrap().state = PlaybackState::Stopped;
        push_event(&r, PlayerEvent::Finished);
        r.c.update(0.016);
        assert_eq!(
            r.backend.lock().unwrap().plays().len(),
            2,
            "no repeat: stop at the end"
        );
        r.c.toggle_repeat();
        push_event(&r, PlayerEvent::Finished);
        r.c.update(0.016);
        assert_eq!(r.backend.lock().unwrap().plays().last(), Some(&file(0)));
    }

    #[test]
    fn errors_skip_tracks_but_give_up_eventually() {
        let mut r = with_tracks(3);
        r.c.toggle_repeat();
        r.c.play();
        for _ in 0..10 {
            push_event(&r, PlayerEvent::Error("bad".into()));
            r.c.update(0.016);
        }
        // Three entries: the first plus two skips, then it stops trying.
        assert_eq!(
            r.backend.lock().unwrap().plays(),
            [file(0), file(1), file(2)]
        );
        assert!(r.c.title_text().contains("Error: bad"));
        // A successful start resets the counter.
        push_event(&r, PlayerEvent::Started(info(true, Some(10))));
        r.c.update(0.016);
        push_event(&r, PlayerEvent::Error("bad".into()));
        r.c.update(0.016);
        assert_eq!(r.backend.lock().unwrap().plays().len(), 4);
    }

    #[test]
    fn next_and_previous_respect_transport_state() {
        let mut r = with_tracks(3);
        r.c.next(); // stopped: select only
        assert_eq!(r.c.playlist().current_index(), Some(0));
        assert!(r.backend.lock().unwrap().plays().is_empty());
        r.c.next();
        r.c.play();
        r.c.next();
        r.c.previous();
        assert_eq!(
            r.backend.lock().unwrap().plays(),
            [file(1), file(2), file(1)]
        );
        r.c.play_index(0);
        assert_eq!(r.backend.lock().unwrap().plays().last(), Some(&file(0)));
        r.c.play_index(99);
        assert_eq!(r.backend.lock().unwrap().plays().len(), 4);
        r.c.stop();
        r.c.previous();
        assert_eq!(
            r.c.playlist().current_index(),
            Some(0),
            "already at the start"
        );
    }

    #[test]
    fn seeking_requires_a_seekable_track() {
        let mut r = with_tracks(1);
        r.c.seek_fraction(0.5);
        assert!(
            !r.backend
                .lock()
                .unwrap()
                .calls
                .iter()
                .any(|c| matches!(c, Call::Seek(_)))
        );
        r.c.play();
        push_event(&r, PlayerEvent::Started(info(true, Some(100))));
        r.c.update(0.0);
        assert!(r.c.seekable());
        r.c.seek_fraction(0.5);
        r.c.seek_relative(SEEK_STEP, true);
        r.c.seek_relative(Duration::from_secs(500), true);
        r.c.seek_relative(Duration::from_secs(500), false);
        let seeks: Vec<Call> = r
            .backend
            .lock()
            .unwrap()
            .calls
            .iter()
            .filter(|c| matches!(c, Call::Seek(_)))
            .cloned()
            .collect();
        assert_eq!(
            seeks,
            [
                Call::Seek(Duration::from_secs(50)),
                Call::Seek(Duration::from_secs(55)),
                Call::Seek(Duration::from_secs(100)),
                Call::Seek(Duration::ZERO),
            ]
        );
        assert!((r.c.progress().unwrap() - 0.0).abs() < f32::EPSILON);
        // Streams are not seekable.
        push_event(&r, PlayerEvent::Started(info(false, None)));
        r.c.update(0.0);
        assert!(!r.c.seekable());
    }

    #[test]
    fn clock_and_progress() {
        let mut r = with_tracks(1);
        r.c.play();
        push_event(&r, PlayerEvent::Started(info(true, Some(200))));
        r.c.update(0.0);
        r.backend.lock().unwrap().position = Duration::from_secs(83);
        assert_eq!(r.c.clock().digits, [0, 1, 2, 3]);
        r.c.toggle_time_mode();
        assert_eq!(r.c.config().time_mode, TimeMode::Remaining);
        assert_eq!(r.c.clock().digits, [0, 1, 5, 7]);
        assert!(r.c.clock().negative);
        assert!((r.c.progress().unwrap() - 0.415).abs() < 0.001);
        assert_eq!(r.c.track_info().unwrap().bitrate_kbps, Some(128));
    }

    #[test]
    fn volume_balance_and_toggles() {
        let mut r = rig();
        r.c.set_volume(2.0);
        assert!((r.c.config().volume - 1.0).abs() < f32::EPSILON);
        r.c.adjust_volume(-0.25);
        assert!((r.c.config().volume - 0.75).abs() < f32::EPSILON);
        r.c.set_balance(0.05);
        assert!(r.c.config().balance.abs() < f32::EPSILON, "snaps to centre");
        r.c.set_balance(-0.6);
        assert!((r.c.config().balance + 0.6).abs() < f32::EPSILON);
        let calls = r.backend.lock().unwrap().calls.clone();
        assert!(calls.contains(&Call::Volume(0.75)));
        assert!(calls.contains(&Call::Balance(-0.6)));
        r.c.toggle_shuffle();
        assert!(r.c.config().shuffle && r.c.playlist().shuffle());
        r.c.cycle_vis_mode();
        assert_eq!(r.c.config().vis_mode, VisMode::Oscilloscope);
        r.c.set_vis_mode(VisMode::Off);
        assert_eq!(r.c.config().vis_mode, VisMode::Off);
        r.c.config_mut().double_size = true;
        assert!(r.c.config().double_size);
        r.c.set_output_device(Some("Other output".into()));
        assert_eq!(r.c.config().output_device.as_deref(), Some("Other output"));
        assert_eq!(r.c.output_name(), "Fake output");
        assert_eq!(r.c.output_devices().len(), 2);
    }

    #[test]
    fn equalizer_changes_reach_the_backend() {
        let mut r = rig();
        r.c.set_eq_enabled(true);
        r.c.set_eq_band(3, 20.0);
        r.c.set_eq_band(99, 1.0);
        r.c.set_preamp(-30.0);
        let eq = &r.c.config().equalizer;
        assert!(eq.enabled);
        assert!((eq.bands_db[3] - 12.0).abs() < f32::EPSILON);
        assert!((eq.preamp_db + 12.0).abs() < f32::EPSILON);
        let last_eq = r
            .backend
            .lock()
            .unwrap()
            .calls
            .iter()
            .rev()
            .find_map(|c| match c {
                Call::Eq(e) => Some(e.clone()),
                _ => None,
            });
        assert_eq!(last_eq.as_ref(), Some(&r.c.config().equalizer));
        let rock =
            r.c.presets()
                .into_iter()
                .find(|p| p.name == "Rock")
                .unwrap();
        r.c.apply_preset(&rock);
        assert_eq!(r.c.config().equalizer.bands_db, rock.bands_db);
        assert!(r.c.title_text().contains("EQ preset: Rock"));
        r.c.save_preset("  Mine ");
        r.c.save_preset("   ");
        assert!(r.c.presets().iter().any(|p| p.name == "Mine"));
        assert!(r.c.delete_preset("Mine"));
        r.c.reset_eq();
        assert_eq!(r.c.config().equalizer.bands_db, [0.0; BANDS]);
    }

    #[test]
    fn messages_expire() {
        let mut r = with_tracks(1);
        r.c.show_message("Hello");
        assert_eq!(r.c.title_text(), "Hello");
        r.c.update(MESSAGE_SECS + 0.1);
        assert!(r.c.title_text().contains("Title of 00"));
    }

    #[test]
    fn stream_events_update_title_and_buffering() {
        let mut r = rig();
        r.c.add_location("http://radio.example/live");
        r.c.play();
        assert_eq!(r.c.title_text(), "Buffering... 0%");
        push_event(&r, PlayerEvent::Buffering(40));
        r.c.update(0.0);
        assert_eq!(r.c.title_text(), "Buffering... 40%");
        push_event(&r, PlayerEvent::Buffering(100));
        push_event(&r, PlayerEvent::Started(info(false, None)));
        push_event(
            &r,
            PlayerEvent::StreamTitle("Spinal Tap - Gimme Some Money".into()),
        );
        r.c.update(0.0);
        assert_eq!(r.c.title_text(), "1. Spinal Tap - Gimme Some Money");
        assert_eq!(
            r.c.playlist().get(0).unwrap().title.as_deref(),
            Some("Spinal Tap - Gimme Some Money")
        );
        assert!(!r.c.add_location("  "));
    }

    #[test]
    fn media_keys_drive_the_transport() {
        let mut r = with_tracks(3);
        r.media.lock().unwrap().incoming =
            vec![MediaCommand::Play, MediaCommand::Next, MediaCommand::Toggle];
        r.c.update(0.0);
        assert_eq!(r.c.state(), PlaybackState::Paused);
        assert_eq!(r.backend.lock().unwrap().plays(), [file(0), file(1)]);
        r.media.lock().unwrap().incoming = vec![
            MediaCommand::Toggle,
            MediaCommand::Pause,
            MediaCommand::Pause,
            MediaCommand::Previous,
            MediaCommand::Stop,
        ];
        r.c.update(0.0);
        assert_eq!(r.c.state(), PlaybackState::Stopped);
        // The current state and title were published.
        let m = r.media.lock().unwrap();
        assert_eq!(m.state, Some(PlaybackState::Stopped));
        assert!(m.title.as_deref().is_some_and(|t| t.contains("Title of")));
    }

    #[test]
    fn visualiser_updates_while_playing_and_decays_when_stopped() {
        let mut r = with_tracks(1);
        r.c.play();
        push_event(&r, PlayerEvent::Started(info(true, Some(10))));
        #[allow(clippy::cast_precision_loss)]
        let tone: Vec<f32> = (0..2048)
            .map(|i| (i as f32 * 2.0 * std::f32::consts::PI * 1000.0 / 44_100.0).sin())
            .collect();
        r.backend.lock().unwrap().vis.clone_from(&tone);
        r.c.update(0.016);
        assert!(r.c.spectrum().0.iter().any(|&l| l > 0.5));
        assert!(r.c.scope().iter().any(|&s| s.abs() > 0.1));
        assert_eq!(r.c.scope().len(), SCOPE_POINTS);
        r.c.stop();
        for _ in 0..50 {
            r.c.update(0.05);
        }
        assert!(r.c.spectrum().0.iter().all(|&l| l == 0.0));
        assert!(r.c.scope().iter().all(|&s| s.abs() < 1e-3));
    }

    #[test]
    fn adding_folders_and_playlists() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("b_album")).unwrap();
        std::fs::create_dir_all(root.join(".hidden")).unwrap();
        for f in [
            "a.mp3",
            "b_album/2.flac",
            "b_album/1.ogg",
            "cover.jpg",
            ".hidden/x.mp3",
        ] {
            std::fs::write(root.join(f), b"").unwrap();
        }
        std::fs::write(
            root.join("list.m3u"),
            "#EXTM3U\n#EXTINF:5,Five\nhttp://s/1\n",
        )
        .unwrap();
        let mut r = rig();
        let n = r.c.add_paths(&[root.to_owned()]);
        assert_eq!(n, 3);
        let names: Vec<String> =
            r.c.playlist()
                .entries()
                .iter()
                .map(|e| e.location.display_name())
                .collect();
        assert_eq!(names, ["a", "1", "2"]);
        assert_eq!(r.c.config().last_directory.as_deref(), Some(root));
        let n = r.c.add_paths(&[
            root.join("list.m3u"),
            root.join("cover.jpg"),
            root.join("a.mp3"),
        ]);
        assert_eq!(n, 2);
        assert_eq!(
            r.c.playlist().get(3).unwrap().title.as_deref(),
            Some("Five")
        );
        assert_eq!(r.c.load_playlist(&root.join("missing.m3u")), 0);
        assert!(r.c.title_text().contains("Cannot load playlist"));
    }

    #[test]
    fn editing_helpers() {
        let mut r = with_tracks(3);
        r.c.playlist_mut().select(1, SelectMode::Single);
        r.c.remove_selected();
        assert_eq!(r.c.playlist().len(), 2);
        r.c.sort(SortKey::Path);
        r.c.jump_to(1);
        assert_eq!(r.backend.lock().unwrap().plays(), [file(2)]);
        assert_eq!(r.c.playlist().selected_indices(), [1]);
    }

    #[test]
    fn state_is_persisted_and_restored() {
        let dir = tempfile::tempdir().unwrap();
        let persistence = Persistence {
            config_file: Some(dir.path().join("config.toml")),
            playlist_file: Some(dir.path().join("sub/playlist.m3u")),
        };
        {
            let mut r = rig_with(Config::default(), persistence.clone());
            r.c.add_location("/music/a.mp3");
            r.c.add_location("http://radio.example/s");
            r.c.update(0.0);
            r.c.next();
            r.c.next();
            r.c.set_volume(0.42);
            r.c.save_state().unwrap();
        }
        let config = Config::load(persistence.config_file.as_ref().unwrap()).unwrap();
        assert!((config.volume - 0.42).abs() < 1e-6);
        assert_eq!(config.current_track, Some(1));
        let r = rig_with(config, persistence);
        assert_eq!(r.c.playlist().len(), 2);
        assert_eq!(r.c.playlist().current_index(), Some(1));
        assert!(r.c.playlist().get(1).unwrap().location.is_url());
        let out = tempfile::tempdir().unwrap();
        let mut r = r;
        assert!(r.c.save_playlist(&out.path().join("x.pls")).is_ok());
        assert!(r.c.save_playlist(&out.path().join("x.txt")).is_err());
    }

    #[test]
    fn output_fallback_is_announced() {
        let (backend, state) = FakeBackend::new();
        state.lock().unwrap().output_error = Some("no device".into());
        let c = Controller::new(
            Config::default(),
            Box::new(backend),
            Box::new(FakeTags::default()),
            Box::new(crate::backend::NoMediaControls),
            Persistence::default(),
        );
        assert!(c.title_text().contains("No audio device"));
    }
}
