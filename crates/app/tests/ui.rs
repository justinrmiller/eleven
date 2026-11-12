//! UI tests: drive the real egui UI headlessly with egui_kittest and fake backends.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use egui::{Event, Key, Modifiers, PointerButton, Pos2, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use eleven_app::backend::fake::{FakeBackend, FakeChooser, FakeMediaControls, FakeState, FakeTags};
use eleven_app::{Controller, ElevenApp, Persistence};
use eleven_core::{Config, Location, PlaybackState, PlayerEvent, TrackInfo};

struct Rig {
    harness: Harness<'static, ElevenApp>,
    backend: Arc<Mutex<FakeState>>,
}

fn app(
    config: Config,
    tracks: usize,
    chooser: FakeChooser,
    skins: Option<PathBuf>,
) -> (ElevenApp, Arc<Mutex<FakeState>>) {
    let (backend, state) = FakeBackend::new();
    let mut controller = Controller::new(
        config,
        Box::new(backend),
        Box::new(FakeTags::default()),
        Box::new(FakeMediaControls::default()),
        Persistence::default(),
    );
    for i in 0..tracks {
        controller.add_location(&format!("/music/{i:02}.mp3"));
    }
    controller.update(0.0);
    (ElevenApp::new(controller, Box::new(chooser), skins), state)
}

fn rig_with(config: Config, tracks: usize, chooser: FakeChooser, skins: Option<PathBuf>) -> Rig {
    let (app, backend) = app(config, tracks, chooser, skins);
    let mut harness = Harness::builder()
        .with_size(vec2(275.0, 600.0))
        .with_step_dt(1.0 / 60.0)
        .build_ui_state(|ui, app: &mut ElevenApp| app.show(ui), app);
    harness.run_steps(2);
    Rig { harness, backend }
}

fn rig(tracks: usize) -> Rig {
    rig_with(Config::default(), tracks, FakeChooser::default(), None)
}

fn file(i: usize) -> Location {
    Location::File(PathBuf::from(format!("/music/{i:02}.mp3")))
}

impl Rig {
    fn click(&mut self, label: &str) {
        self.harness.get_by_label(label).click();
        self.harness.run_steps(2);
    }

    fn key(&mut self, key: Key) {
        self.harness.key_press(key);
        self.harness.run_steps(2);
    }

    fn key_mod(&mut self, modifiers: Modifiers, key: Key) {
        self.harness.key_press_modifiers(modifiers, key);
        self.harness.run_steps(2);
    }

    fn press_at(&mut self, pos: Pos2) {
        for pressed in [true, false] {
            self.harness.event(Event::PointerMoved(pos));
            self.harness.event(Event::PointerButton {
                pos,
                button: PointerButton::Primary,
                pressed,
                modifiers: Modifiers::NONE,
            });
            self.harness.step();
        }
        self.harness.run_steps(1);
    }

    fn plays(&self) -> Vec<Location> {
        self.backend.lock().unwrap().plays()
    }

    fn state(&self) -> PlaybackState {
        self.backend.lock().unwrap().state
    }

    fn app(&self) -> &ElevenApp {
        self.harness.state()
    }

    fn exists(&self, label: &str) -> bool {
        self.harness.query_by_label(label).is_some()
    }
}

fn cmd() -> Modifiers {
    Modifiers {
        command: true,
        ctrl: true,
        ..Modifiers::NONE
    }
}

#[test]
fn transport_buttons_drive_the_backend() {
    let mut r = rig(3);
    r.click("Play");
    assert_eq!(r.plays(), [file(0)]);
    assert_eq!(r.state(), PlaybackState::Playing);
    r.click("Pause");
    assert_eq!(r.state(), PlaybackState::Paused);
    r.click("Play");
    assert_eq!(r.state(), PlaybackState::Playing, "play resumes");
    r.click("Next");
    r.click("Previous");
    assert_eq!(r.plays(), [file(0), file(1), file(0)]);
    r.click("Stop");
    assert_eq!(r.state(), PlaybackState::Stopped);
}

#[test]
fn xmms_keyboard_shortcuts() {
    let mut r = rig(3);
    r.key(Key::X);
    r.key(Key::B);
    r.key(Key::C);
    assert_eq!(r.state(), PlaybackState::Paused);
    r.key(Key::V);
    assert_eq!(r.state(), PlaybackState::Stopped);
    r.key(Key::Z);
    assert_eq!(r.plays(), [file(0), file(1)]);
    r.key(Key::S);
    r.key(Key::R);
    assert!(r.app().controller.config().shuffle && r.app().controller.config().repeat);
    let before = r.app().controller.config().volume;
    r.key(Key::ArrowUp);
    assert!(r.app().controller.config().volume > before);
    r.key(Key::ArrowDown);
    r.key(Key::ArrowDown);
    assert!(r.app().controller.config().volume < before);
}

#[test]
fn seek_keys_and_seek_bar() {
    let mut r = rig(1);
    r.click("Play");
    r.backend
        .lock()
        .unwrap()
        .events
        .push(PlayerEvent::Started(TrackInfo {
            sample_rate: 44_100,
            channels: 2,
            duration: Some(Duration::from_secs(100)),
            seekable: true,
            codec: "mp3".into(),
            ..TrackInfo::default()
        }));
    r.harness.run_steps(2);
    r.key(Key::ArrowRight);
    assert_eq!(r.backend.lock().unwrap().position, Duration::from_secs(5));
    r.key(Key::ArrowLeft);
    assert_eq!(r.backend.lock().unwrap().position, Duration::ZERO);
    // Clicking the middle of the seek bar seeks to ~50 %.
    r.click("Seek");
    let pos = r.backend.lock().unwrap().position.as_secs_f32();
    assert!((pos - 50.0).abs() < 2.0, "{pos}");
}

#[test]
fn toggles_show_and_hide_sections() {
    let mut r = rig(2);
    assert!(r.exists("Equalizer on"));
    assert!(r.exists("1. Fake Artist - Title of 00"));
    r.click("Equalizer");
    assert!(!r.exists("Equalizer on"));
    r.click("Playlist");
    assert!(!r.exists("1. Fake Artist - Title of 00"));
    r.key_mod(Modifiers::ALT, Key::G);
    r.key_mod(Modifiers::ALT, Key::E);
    assert!(r.exists("Equalizer on") && r.exists("2. Fake Artist - Title of 01"));
    r.click("Shuffle");
    r.click("Repeat");
    let cfg = r.app().controller.config().clone();
    assert!(cfg.shuffle && cfg.repeat);
}

#[test]
fn shade_and_double_size() {
    let mut r = rig(1);
    r.key_mod(cmd(), Key::W);
    assert!(r.app().controller.config().shaded);
    assert!(!r.exists("Seek"), "the full main window is hidden");
    r.click("Play");
    assert_eq!(
        r.state(),
        PlaybackState::Playing,
        "mini transport works when shaded"
    );
    r.click("Shade");
    assert!(!r.app().controller.config().shaded);
    r.key_mod(cmd(), Key::D);
    assert!((r.harness.ctx.zoom_factor() - 2.0).abs() < f32::EPSILON);
    r.key_mod(cmd(), Key::D);
    assert!((r.harness.ctx.zoom_factor() - 1.0).abs() < f32::EPSILON);
    r.key_mod(cmd(), Key::T);
    assert!(r.app().controller.config().always_on_top);
}

#[test]
fn clock_and_visualizer_clicks() {
    let mut r = rig(1);
    r.click("Time display");
    assert_eq!(
        r.app().controller.config().time_mode,
        eleven_core::TimeMode::Remaining
    );
    r.click("Visualizer");
    assert_eq!(
        r.app().controller.config().vis_mode,
        eleven_core::VisMode::Oscilloscope
    );
    r.click("Play");
    r.backend.lock().unwrap().vis = (0..2048).map(|i| ((i as f32) * 0.3).sin()).collect();
    r.harness.run_steps(3);
    r.click("Visualizer");
    r.backend.lock().unwrap().vis = (0..2048).map(|i| ((i as f32) * 0.3).sin()).collect();
    r.harness.run_steps(3);
    assert_eq!(
        r.app().controller.config().vis_mode,
        eleven_core::VisMode::Off
    );
}

#[test]
fn volume_and_balance_sliders() {
    let mut r = rig(0);
    r.click("Volume");
    let v = r.app().controller.config().volume;
    assert!((v - 0.5).abs() < 0.05, "{v}");
    let rect = r.harness.get_by_label("Balance").rect();
    r.press_at(rect.right_center() - vec2(1.0, 0.0));
    assert!(r.app().controller.config().balance > 0.9);
}

#[test]
fn playlist_selection_and_double_click() {
    let mut r = rig(4);
    r.click("2. Fake Artist - Title of 01");
    assert_eq!(r.app().controller.playlist().selected_indices(), [1]);
    r.harness
        .get_by_label("4. Fake Artist - Title of 03")
        .click_modifiers(Modifiers::SHIFT);
    r.harness.run_steps(2);
    assert_eq!(r.app().controller.playlist().selected_indices(), [1, 2, 3]);
    // Double-click plays (wait out the double-click window after the previous clicks).
    r.harness.run_steps(20);
    let row = r.harness.get_by_label("3. Fake Artist - Title of 02");
    row.click();
    r.harness.step();
    r.harness
        .get_by_label("3. Fake Artist - Title of 02")
        .click();
    r.harness.run_steps(2);
    assert_eq!(r.plays(), [file(2)]);
    // Delete removes the selection; Enter plays the selection.
    r.harness.run_steps(20);
    r.click("1. Fake Artist - Title of 00");
    r.key(Key::Delete);
    assert_eq!(r.app().controller.playlist().len(), 3);
    r.harness.run_steps(20);
    r.click("1. Fake Artist - Title of 01");
    r.key(Key::Enter);
    assert_eq!(r.plays(), [file(2), file(1)]);
    r.key_mod(cmd(), Key::A);
    assert_eq!(r.app().controller.playlist().selected_indices().len(), 3);
}

#[test]
fn playlist_menus() {
    let mut r = rig(3);
    r.click("Select");
    r.click("Select all");
    assert_eq!(r.app().controller.playlist().selected_indices().len(), 3);
    r.click("Select");
    r.click("Invert selection");
    assert!(r.app().controller.playlist().selected_indices().is_empty());
    r.click("Misc");
    r.click("Reverse");
    assert_eq!(
        r.app().controller.playlist().get(0).unwrap().location,
        file(2)
    );
    r.click("Misc");
    r.click("Sort by path");
    assert_eq!(
        r.app().controller.playlist().get(0).unwrap().location,
        file(0)
    );
    r.click("Remove");
    r.click("Clear playlist");
    assert!(r.app().controller.playlist().is_empty());
}

#[test]
fn jump_to_file_dialog() {
    let mut r = rig(5);
    r.key(Key::J);
    assert!(r.exists("Jump to file"));
    // Type into the (auto-focused) search field.
    r.harness.event(Event::Text("of 03".into()));
    r.harness.run_steps(2);
    assert!(r.exists("1 of 5 match"));
    r.key(Key::Enter);
    assert_eq!(r.plays(), [file(3)]);
    assert!(!r.exists("Jump to file"));
}

#[test]
fn open_location_dialog_plays_a_url() {
    let mut r = rig(0);
    r.key_mod(cmd(), Key::L);
    assert!(r.exists("Open location"));
    r.harness
        .event(Event::Text("http://radio.example/live".into()));
    r.harness.run_steps(2);
    r.click("Play location");
    assert_eq!(
        r.plays(),
        [Location::Url("http://radio.example/live".into())]
    );
}

#[test]
fn equalizer_controls_and_presets() {
    let mut r = rig(0);
    r.click("Equalizer on");
    assert!(r.app().controller.config().equalizer.enabled);
    // Press near the top of the 1 kHz slider: a big boost.
    let rect = r.harness.get_by_label("1K Hz").rect();
    r.press_at(rect.center_top() + vec2(0.0, 3.0));
    assert!(r.app().controller.config().equalizer.bands_db[4] > 10.0);
    let rect = r.harness.get_by_label("Preamp").rect();
    r.press_at(rect.center_bottom() - vec2(0.0, 3.0));
    assert!(r.app().controller.config().equalizer.preamp_db < -10.0);
    r.click("Presets");
    r.click("Rock");
    let rock = eleven_core::eq::builtin_presets()
        .into_iter()
        .find(|p| p.name == "Rock")
        .unwrap();
    assert_eq!(
        r.app().controller.config().equalizer.bands_db,
        rock.bands_db
    );
    // Save a user preset through the dialog.
    r.click("Presets");
    r.click("Save preset...");
    r.harness.event(Event::Text("Mine".into()));
    r.harness.run_steps(2);
    r.click("Save");
    assert!(
        r.app()
            .controller
            .config()
            .eq_presets
            .iter()
            .any(|p| p.name == "Mine")
    );
    r.click("Presets");
    r.click("Reset to flat");
    assert_eq!(r.app().controller.config().equalizer.bands_db, [0.0; 10]);
}

#[test]
fn windows_open_from_shortcuts_and_menus() {
    let mut r = rig(2);
    r.click("Menu");
    r.click("About eleven");
    assert!(r.app().state.show_about, "about requested");
    r.harness.run_steps(2);
    assert!(r.exists("These go to eleven."));
    r.key_mod(cmd(), Key::P);
    assert!(r.app().state.show_prefs);
    r.harness.run_steps(2);
    assert!(r.exists("Scroll long titles"));
    r.click("1. Fake Artist - Title of 00");
    r.key_mod(cmd(), Key::I);
    assert!(r.exists("File info"));
}

#[test]
fn file_dialogs_add_and_save() {
    let dir = tempfile::tempdir().unwrap();
    let song = dir.path().join("song.mp3");
    std::fs::write(&song, b"").unwrap();
    let chooser = FakeChooser {
        files: Some(vec![song.clone()]),
        folder: Some(dir.path().to_owned()),
        save: Some(dir.path().join("out.m3u")),
        skin: None,
    };
    let mut r = rig_with(Config::default(), 0, chooser, None);
    r.click("Open");
    assert_eq!(
        r.plays(),
        [Location::File(song.clone())],
        "adding to an empty list starts playing"
    );
    r.key_mod(
        Modifiers {
            shift: true,
            ..cmd()
        },
        Key::L,
    );
    assert_eq!(r.app().controller.playlist().len(), 2);
    r.key_mod(cmd(), Key::S);
    let saved = std::fs::read_to_string(dir.path().join("out.m3u")).unwrap();
    assert!(saved.contains("song.mp3"));
    r.key_mod(cmd(), Key::O);
    assert_eq!(
        r.app().controller.playlist().len(),
        1,
        "loading replaces the list"
    );
}

fn write_skin(dir: &Path, name: &str) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(
        &path,
        eleven_skin::synthetic::SyntheticSkin::default().build(),
    )
    .unwrap();
    path
}

#[test]
fn skins_load_from_config_preferences_and_dialog() {
    let dir = tempfile::tempdir().unwrap();
    let skins = dir.path().join("skins");
    std::fs::create_dir_all(&skins).unwrap();
    let a = write_skin(&skins, "Alpha.wsz");
    write_skin(&skins, "Beta.wsz");
    let other = write_skin(dir.path(), "Gamma.wsz");
    let bad = dir.path().join("Broken.wsz");
    std::fs::write(&bad, b"nope").unwrap();

    // Configured skin loads on the first frame.
    let config = Config {
        skin: Some(a.clone()),
        ..Config::default()
    };
    let chooser = FakeChooser {
        skin: Some(other.clone()),
        ..FakeChooser::default()
    };
    let mut r = rig_with(config, 2, chooser, Some(skins.clone()));
    assert_eq!(r.app().look_name(), "Alpha");
    assert_eq!(r.app().available_skins().len(), 2);
    r.click("Play");
    r.harness.run_steps(3);

    // Preferences lists the skins directory.
    r.key_mod(cmd(), Key::P);
    r.click("Beta");
    assert_eq!(r.app().look_name(), "Beta");
    r.click("Built-in");
    assert_eq!(r.app().look_name(), "Built-in");
    assert_eq!(r.app().controller.config().skin, None);

    // "Load skin file..." uses the chooser.
    r.click("Load skin file...");
    assert_eq!(r.app().look_name(), "Gamma");
    assert_eq!(
        r.app().controller.config().skin.as_deref(),
        Some(other.as_path())
    );

    // A broken skin keeps the current look and reports the error.
    let ctx = r.harness.ctx.clone();
    r.harness.state_mut().set_skin(&ctx, Some(bad));
    r.harness.run_steps(2);
    assert_eq!(r.app().look_name(), "Gamma");
    assert!(
        r.app()
            .state
            .skin_error
            .as_deref()
            .is_some_and(|e| e.contains("Broken"))
    );
}

#[test]
fn shaded_skin_and_all_sections_render() {
    let dir = tempfile::tempdir().unwrap();
    let skin = write_skin(dir.path(), "Shady.wsz");
    let config = Config {
        skin: Some(skin),
        shaded: true,
        ..Config::default()
    };
    let mut r = rig_with(config, 30, FakeChooser::default(), None);
    r.click("Play");
    r.harness.run_steps(3);
    assert_eq!(r.app().look_name(), "Shady");
    // Scrolling the playlist moves the first visible row.
    r.click("Playlist scroll");
    assert!(r.app().state.pl_scroll > 0);
}

#[test]
fn quit_persists_and_closes() {
    let mut r = rig(1);
    r.key_mod(cmd(), Key::Q);
    assert!(r.app().quit_requested);
}

#[derive(Debug)]
struct Dropped(PathBuf);

impl egui::DroppedFile for Dropped {
    fn path(&self) -> &Path {
        &self.0
    }
    fn bytes(&self) -> Result<Vec<u8>, String> {
        Ok(Vec::new())
    }
}

#[test]
fn dropping_files_adds_them() {
    let dir = tempfile::tempdir().unwrap();
    let song = dir.path().join("dropped.flac");
    std::fs::write(&song, b"").unwrap();
    let mut r = rig(0);
    r.harness
        .input_mut()
        .dropped_files
        .push(Arc::new(Dropped(song)));
    r.harness.run_steps(2);
    assert_eq!(r.app().controller.playlist().len(), 1);
    assert!(r.app().controller.title_text().contains("Added 1"));
}
