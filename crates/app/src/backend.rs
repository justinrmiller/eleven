//! Seams between the controller and the outside world: the audio engine, the tag scanner,
//! OS media controls and file dialogs. Each has a real implementation and a fake for tests.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use eleven_audio::{Engine, MetadataScanner, OutputSpec, TagInfo};
use eleven_core::{EntryId, EqSettings, Location, PlaybackState, PlayerEvent};

/// The playback engine as seen by the controller.
pub trait Backend {
    /// Starts playing `location`.
    fn play(&mut self, location: &Location);
    /// Pauses playback.
    fn pause(&mut self);
    /// Resumes paused playback.
    fn resume(&mut self);
    /// Stops playback.
    fn stop(&mut self);
    /// Seeks within the current track.
    fn seek(&mut self, position: Duration);
    /// Sets the volume (`0.0..=1.0`).
    fn set_volume(&mut self, volume: f32);
    /// Sets the balance (`-1.0..=1.0`).
    fn set_balance(&mut self, balance: f32);
    /// Updates the equaliser.
    fn set_eq(&mut self, eq: &EqSettings);
    /// Switches the output device (`None` = system default).
    fn set_output_device(&mut self, name: Option<String>);
    /// The transport state.
    fn state(&self) -> PlaybackState;
    /// Position within the current track.
    fn position(&self) -> Duration;
    /// Takes pending engine events.
    fn poll_events(&mut self) -> Vec<PlayerEvent>;
    /// Appends new mono visualiser samples.
    fn drain_vis(&mut self, out: &mut Vec<f32>);
    /// Name of the active output device.
    fn output_name(&self) -> String;
    /// Why the requested output is unavailable, if a fallback is in use.
    fn output_error(&self) -> Option<String>;
    /// Available output device names.
    fn output_devices(&self) -> Vec<String>;
}

/// [`Backend`] backed by the real [`Engine`].
#[derive(Debug)]
pub struct EngineBackend {
    engine: Engine,
}

impl EngineBackend {
    /// Starts the engine on `device` (or the default device).
    #[must_use]
    pub fn start(device: Option<String>, notify: eleven_audio::Notify) -> Self {
        Self {
            engine: Engine::start(&OutputSpec::Device(device), notify),
        }
    }

    /// Wraps an existing engine.
    #[must_use]
    pub fn from_engine(engine: Engine) -> Self {
        Self { engine }
    }
}

impl Backend for EngineBackend {
    fn play(&mut self, location: &Location) {
        self.engine.play(location.clone());
    }
    fn pause(&mut self) {
        self.engine.pause();
    }
    fn resume(&mut self) {
        self.engine.resume();
    }
    fn stop(&mut self) {
        self.engine.stop();
    }
    fn seek(&mut self, position: Duration) {
        self.engine.seek(position);
    }
    fn set_volume(&mut self, volume: f32) {
        self.engine.set_volume(volume);
    }
    fn set_balance(&mut self, balance: f32) {
        self.engine.set_balance(balance);
    }
    fn set_eq(&mut self, eq: &EqSettings) {
        self.engine.set_eq(eq);
    }
    fn set_output_device(&mut self, name: Option<String>) {
        self.engine.set_output(OutputSpec::Device(name));
    }
    fn state(&self) -> PlaybackState {
        self.engine.state()
    }
    fn position(&self) -> Duration {
        self.engine.position()
    }
    fn poll_events(&mut self) -> Vec<PlayerEvent> {
        self.engine.poll_events()
    }
    fn drain_vis(&mut self, out: &mut Vec<f32>) {
        self.engine.drain_vis(out);
    }
    fn output_name(&self) -> String {
        self.engine.output_name()
    }
    fn output_error(&self) -> Option<String> {
        self.engine.output_error()
    }
    fn output_devices(&self) -> Vec<String> {
        eleven_audio::list_devices()
    }
}

/// Reads tags for playlist entries, asynchronously.
pub trait TagReader {
    /// Queues `path` for entry `id`.
    fn request(&mut self, id: EntryId, path: &Path);
    /// Takes finished results.
    fn poll(&mut self) -> Vec<(EntryId, TagInfo)>;
}

impl TagReader for MetadataScanner {
    fn request(&mut self, id: EntryId, path: &Path) {
        MetadataScanner::request(self, id, path.to_owned());
    }
    fn poll(&mut self) -> Vec<(EntryId, TagInfo)> {
        MetadataScanner::poll(self)
    }
}

/// Commands from OS media keys / Now Playing widgets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaCommand {
    /// Start or resume.
    Play,
    /// Pause.
    Pause,
    /// Toggle play/pause.
    Toggle,
    /// Stop.
    Stop,
    /// Next track.
    Next,
    /// Previous track.
    Previous,
}

/// OS integration for media keys and "Now Playing" information.
pub trait MediaControls {
    /// Takes pending commands.
    fn poll(&mut self) -> Vec<MediaCommand>;
    /// Publishes the current track.
    fn set_metadata(&mut self, title: &str, artist: Option<&str>, duration: Option<Duration>);
    /// Publishes the transport state.
    fn set_playback(&mut self, state: PlaybackState, position: Duration);
}

/// Media controls that do nothing (unsupported platform or initialisation failure).
#[derive(Debug, Default)]
pub struct NoMediaControls;

impl MediaControls for NoMediaControls {
    fn poll(&mut self) -> Vec<MediaCommand> {
        Vec::new()
    }
    fn set_metadata(&mut self, _: &str, _: Option<&str>, _: Option<Duration>) {}
    fn set_playback(&mut self, _: PlaybackState, _: Duration) {}
}

/// Native file dialogs.
pub trait FileChooser {
    /// Asks for audio and playlist files to add.
    fn pick_files(&mut self, start: Option<&Path>) -> Option<Vec<PathBuf>>;
    /// Asks for a folder to add.
    fn pick_folder(&mut self, start: Option<&Path>) -> Option<PathBuf>;
    /// Asks where to save a playlist.
    fn save_playlist(&mut self, start: Option<&Path>) -> Option<PathBuf>;
    /// Asks for a `.wsz` skin.
    fn pick_skin(&mut self, start: Option<&Path>) -> Option<PathBuf>;
}

/// [`FileChooser`] using the platform's native dialogs (via rfd).
#[derive(Debug, Default)]
pub struct NativeFileChooser;

fn dialog(start: Option<&Path>) -> rfd::FileDialog {
    let d = rfd::FileDialog::new();
    match start {
        Some(dir) if dir.is_dir() => d.set_directory(dir),
        _ => d,
    }
}

impl FileChooser for NativeFileChooser {
    fn pick_files(&mut self, start: Option<&Path>) -> Option<Vec<PathBuf>> {
        dialog(start)
            .set_title("Add files")
            .add_filter(
                "Audio and playlists",
                &[eleven_core::AUDIO_EXTENSIONS, &["m3u", "m3u8", "pls"]].concat(),
            )
            .add_filter("All files", &["*"])
            .pick_files()
    }
    fn pick_folder(&mut self, start: Option<&Path>) -> Option<PathBuf> {
        dialog(start).set_title("Add folder").pick_folder()
    }
    fn save_playlist(&mut self, start: Option<&Path>) -> Option<PathBuf> {
        dialog(start)
            .set_title("Save playlist")
            .set_file_name("playlist.m3u")
            .add_filter("M3U playlist", &["m3u", "m3u8"])
            .add_filter("PLS playlist", &["pls"])
            .save_file()
    }
    fn pick_skin(&mut self, start: Option<&Path>) -> Option<PathBuf> {
        dialog(start)
            .set_title("Load skin")
            .add_filter("Classic skins", &["wsz", "zip"])
            .pick_file()
    }
}

/// Test doubles. Public so the UI tests in `tests/` can use them too.
pub mod fake {
    use super::{
        Arc, Backend, Duration, EntryId, EqSettings, FileChooser, Location, MediaCommand,
        MediaControls, Mutex, Path, PathBuf, PlaybackState, PlayerEvent, PoisonError, TagInfo,
        TagReader,
    };

    /// A recorded backend call.
    #[derive(Debug, Clone, PartialEq)]
    pub enum Call {
        /// `play(location)`
        Play(Location),
        /// `pause()`
        Pause,
        /// `resume()`
        Resume,
        /// `stop()`
        Stop,
        /// `seek(position)`
        Seek(Duration),
        /// `set_volume(v)`
        Volume(f32),
        /// `set_balance(b)`
        Balance(f32),
        /// `set_eq(settings)`
        Eq(EqSettings),
        /// `set_output_device(name)`
        Output(Option<String>),
    }

    /// Shared, inspectable state of a [`FakeBackend`].
    #[derive(Debug, Default)]
    pub struct FakeState {
        /// Every call, in order.
        pub calls: Vec<Call>,
        /// What `state()` returns.
        pub state: PlaybackState,
        /// What `position()` returns.
        pub position: Duration,
        /// Events returned by the next `poll_events()`.
        pub events: Vec<PlayerEvent>,
        /// Samples returned by the next `drain_vis()`.
        pub vis: Vec<f32>,
        /// What `output_error()` returns.
        pub output_error: Option<String>,
    }

    impl FakeState {
        /// The `Play` calls so far.
        #[must_use]
        pub fn plays(&self) -> Vec<Location> {
            self.calls
                .iter()
                .filter_map(|c| match c {
                    Call::Play(l) => Some(l.clone()),
                    _ => None,
                })
                .collect()
        }
    }

    /// A backend that records calls and mimics the engine's state changes.
    #[derive(Debug, Clone, Default)]
    pub struct FakeBackend {
        /// The shared state; clone the `Arc` to inspect it from a test.
        pub state: Arc<Mutex<FakeState>>,
    }

    impl FakeBackend {
        /// Creates a fake and returns it with a handle to its state.
        #[must_use]
        pub fn new() -> (Self, Arc<Mutex<FakeState>>) {
            let b = Self::default();
            let s = Arc::clone(&b.state);
            (b, s)
        }

        fn with<R>(&self, f: impl FnOnce(&mut FakeState) -> R) -> R {
            f(&mut self.state.lock().unwrap_or_else(PoisonError::into_inner))
        }
    }

    impl Backend for FakeBackend {
        fn play(&mut self, location: &Location) {
            self.with(|s| {
                s.calls.push(Call::Play(location.clone()));
                s.state = PlaybackState::Playing;
                s.position = Duration::ZERO;
            });
        }
        fn pause(&mut self) {
            self.with(|s| {
                s.calls.push(Call::Pause);
                if s.state == PlaybackState::Playing {
                    s.state = PlaybackState::Paused;
                }
            });
        }
        fn resume(&mut self) {
            self.with(|s| {
                s.calls.push(Call::Resume);
                if s.state == PlaybackState::Paused {
                    s.state = PlaybackState::Playing;
                }
            });
        }
        fn stop(&mut self) {
            self.with(|s| {
                s.calls.push(Call::Stop);
                s.state = PlaybackState::Stopped;
                s.position = Duration::ZERO;
            });
        }
        fn seek(&mut self, position: Duration) {
            self.with(|s| {
                s.calls.push(Call::Seek(position));
                s.position = position;
            });
        }
        fn set_volume(&mut self, volume: f32) {
            self.with(|s| s.calls.push(Call::Volume(volume)));
        }
        fn set_balance(&mut self, balance: f32) {
            self.with(|s| s.calls.push(Call::Balance(balance)));
        }
        fn set_eq(&mut self, eq: &EqSettings) {
            self.with(|s| s.calls.push(Call::Eq(eq.clone())));
        }
        fn set_output_device(&mut self, name: Option<String>) {
            self.with(|s| s.calls.push(Call::Output(name)));
        }
        fn state(&self) -> PlaybackState {
            self.with(|s| s.state)
        }
        fn position(&self) -> Duration {
            self.with(|s| s.position)
        }
        fn poll_events(&mut self) -> Vec<PlayerEvent> {
            self.with(|s| std::mem::take(&mut s.events))
        }
        fn drain_vis(&mut self, out: &mut Vec<f32>) {
            self.with(|s| out.append(&mut s.vis));
        }
        fn output_name(&self) -> String {
            "Fake output".to_owned()
        }
        fn output_error(&self) -> Option<String> {
            self.with(|s| s.output_error.clone())
        }
        fn output_devices(&self) -> Vec<String> {
            vec!["Fake output".to_owned(), "Other output".to_owned()]
        }
    }

    /// A tag reader that answers immediately with tags derived from the file name.
    #[derive(Debug, Default)]
    pub struct FakeTags {
        pending: Vec<(EntryId, TagInfo)>,
        /// Paths requested so far.
        pub requested: Vec<PathBuf>,
    }

    impl TagReader for FakeTags {
        fn request(&mut self, id: EntryId, path: &Path) {
            self.requested.push(path.to_owned());
            let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned());
            self.pending.push((
                id,
                TagInfo {
                    title: stem.map(|s| format!("Title of {s}")),
                    artist: Some("Fake Artist".into()),
                    album: None,
                    duration: Some(Duration::from_secs(60)),
                },
            ));
        }
        fn poll(&mut self) -> Vec<(EntryId, TagInfo)> {
            std::mem::take(&mut self.pending)
        }
    }

    /// Shared state of [`FakeMediaControls`].
    #[derive(Debug, Default)]
    pub struct FakeMediaState {
        /// Commands to deliver on the next poll.
        pub incoming: Vec<MediaCommand>,
        /// Last published title.
        pub title: Option<String>,
        /// Last published state.
        pub state: Option<PlaybackState>,
    }

    /// Media controls whose commands are injected by the test.
    #[derive(Debug, Clone, Default)]
    pub struct FakeMediaControls {
        /// Shared state.
        pub state: Arc<Mutex<FakeMediaState>>,
    }

    impl MediaControls for FakeMediaControls {
        fn poll(&mut self) -> Vec<MediaCommand> {
            std::mem::take(
                &mut self
                    .state
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .incoming,
            )
        }
        fn set_metadata(&mut self, title: &str, _: Option<&str>, _: Option<Duration>) {
            self.state
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .title = Some(title.to_owned());
        }
        fn set_playback(&mut self, state: PlaybackState, _: Duration) {
            self.state
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .state = Some(state);
        }
    }

    /// A file chooser returning scripted answers.
    #[derive(Debug, Default, Clone)]
    pub struct FakeChooser {
        /// Returned by `pick_files`.
        pub files: Option<Vec<PathBuf>>,
        /// Returned by `pick_folder`.
        pub folder: Option<PathBuf>,
        /// Returned by `save_playlist`.
        pub save: Option<PathBuf>,
        /// Returned by `pick_skin`.
        pub skin: Option<PathBuf>,
    }

    impl FileChooser for FakeChooser {
        fn pick_files(&mut self, _: Option<&Path>) -> Option<Vec<PathBuf>> {
            self.files.clone()
        }
        fn pick_folder(&mut self, _: Option<&Path>) -> Option<PathBuf> {
            self.folder.clone()
        }
        fn save_playlist(&mut self, _: Option<&Path>) -> Option<PathBuf> {
            self.save.clone()
        }
        fn pick_skin(&mut self, _: Option<&Path>) -> Option<PathBuf> {
            self.skin.clone()
        }
    }
}
