//! The egui user interface.
//!
//! [`ElevenApp::tick`] runs the player (engine events, media keys, timers) and
//! [`ElevenApp::show`] draws the docked main / equaliser / playlist sections, runs dialogs,
//! maps keyboard shortcuts and drag-and-drop, and keeps the native window sized to fit.
//! eframe calls `tick` even while the window is minimised or hidden and `show` only while it is
//! visible, so playback keeps advancing in the background.

pub mod actions;
pub mod builtin;
pub mod dialogs;
pub mod equalizer;
pub mod layout;
pub mod look;
pub mod main_window;
pub mod playlist;
pub mod skin_look;
pub mod widgets;

use std::path::PathBuf;
use std::time::Duration;

use egui::{Color32, Context, Event, Ui, Vec2, ViewportCommand, WindowLevel};
use eleven_core::{EntryId, PlaybackState};

use crate::backend::FileChooser;
use crate::controller::{Controller, SEEK_STEP, VOLUME_STEP};
use actions::{Action, shortcut};
use dialogs::JumpDialog;
use layout::{Frame, Sections};
use look::Look;

/// Transient UI state (not persisted).
#[derive(Debug, Default)]
pub struct UiState {
    /// Seek-bar position while dragging.
    pub seek_drag: Option<f32>,
    /// First visible playlist row.
    pub pl_scroll: usize,
    /// Fractional mouse-wheel scrolling.
    pub scroll_accum: f32,
    /// The current track last scrolled into view.
    pub followed: Option<EntryId>,
    /// Jump-to-file dialog, when open.
    pub jump: Option<JumpDialog>,
    /// Open-location dialog text, when open.
    pub location: Option<String>,
    /// Save-preset dialog text, when open.
    pub preset_name: Option<String>,
    /// Preferences window open.
    pub show_prefs: bool,
    /// Skins listed in preferences, read when it opens.
    pub skins: Option<Vec<PathBuf>>,
    /// Output devices listed in preferences, queried when it opens (enumerating devices can
    /// take a while, e.g. with ALSA).
    pub devices: Option<Vec<String>>,
    /// About window open.
    pub show_about: bool,
    /// File-info window for this playlist index.
    pub info: Option<usize>,
    /// Blink phase for the paused clock, in seconds.
    pub blink: f32,
    /// Skin chosen in preferences, waiting to be loaded (`Some(None)` = built-in).
    pub pending_skin: Option<Option<PathBuf>>,
    /// Last skin loading error.
    pub skin_error: Option<String>,
    /// Files are being dragged over the window.
    pub drop_hover: bool,
    /// Last window size sent, with the zoom it was sent at.
    last_size: Option<(Vec2, f32)>,
    last_on_top: Option<bool>,
    last_decorations: Option<bool>,
}

/// The whole application UI.
pub struct ElevenApp {
    /// The player.
    pub controller: Controller,
    /// Transient UI state.
    pub state: UiState,
    chooser: Box<dyn FileChooser>,
    look: Box<dyn Look>,
    skins_dir: Option<PathBuf>,
    last_time: Option<f64>,
    /// Set when the user asked to quit (tests read it; the window is closed via a command).
    pub quit_requested: bool,
}

impl std::fmt::Debug for ElevenApp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ElevenApp")
            .field("look", &self.look.name())
            .finish_non_exhaustive()
    }
}

impl ElevenApp {
    /// Creates the UI around a controller. The configured skin is loaded on the first frame.
    #[must_use]
    pub fn new(
        controller: Controller,
        chooser: Box<dyn FileChooser>,
        skins_dir: Option<PathBuf>,
    ) -> Self {
        let pending = controller.config().skin.clone().map(Some);
        Self {
            controller,
            state: UiState {
                pending_skin: pending,
                ..UiState::default()
            },
            chooser,
            look: Box::new(builtin::Builtin),
            skins_dir,
            last_time: None,
            quit_requested: false,
        }
    }

    /// Name of the active look.
    #[must_use]
    pub fn look_name(&self) -> String {
        self.look.name()
    }

    /// The `.wsz` files available in the skins directory.
    #[must_use]
    pub fn available_skins(&self) -> Vec<PathBuf> {
        let Some(dir) = &self.skins_dir else {
            return Vec::new();
        };
        let mut skins: Vec<PathBuf> = std::fs::read_dir(dir)
            .map(|rd| {
                rd.filter_map(Result::ok)
                    .map(|e| e.path())
                    .filter(|p| {
                        p.extension().is_some_and(|e| {
                            e.eq_ignore_ascii_case("wsz") || e.eq_ignore_ascii_case("zip")
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        skins.sort();
        skins
    }

    /// Switches to a `.wsz` skin, or back to the built-in look with `None`.
    pub fn set_skin(&mut self, ctx: &Context, path: Option<PathBuf>) {
        match path {
            None => {
                self.look = Box::new(builtin::Builtin);
                self.controller.config_mut().skin = None;
                self.state.skin_error = None;
            }
            Some(path) => match skin_look::SkinLook::load(ctx, &path) {
                Ok(look) => {
                    self.controller
                        .show_message(&format!("Skin: {}", look.name()));
                    self.look = Box::new(look);
                    self.controller.config_mut().skin = Some(path);
                    self.state.skin_error = None;
                }
                Err(e) => {
                    let msg = format!("Cannot load skin {}: {e}", path.display());
                    log::warn!("{msg}");
                    self.controller.show_message("Skin failed to load");
                    self.state.skin_error = Some(msg);
                }
            },
        }
    }

    fn sections(&self) -> Sections {
        let c = self.controller.config();
        Sections {
            shaded: c.shaded,
            equalizer: c.show_equalizer,
            playlist: c.show_playlist,
            rows: c.playlist_rows,
        }
    }

    /// Runs the player: engine events (advancing to the next track), tag results, media keys
    /// and timers. Call it once per frame before [`ElevenApp::show`], and also while the window
    /// is hidden (eframe's `App::logic`).
    pub fn tick(&mut self, ctx: &Context) {
        // While the window is hidden, the input time stays at the last shown frame, so dt is
        // zero: timers pause, but events are still handled.
        let now = ctx.input(|i| i.time);
        #[allow(clippy::cast_possible_truncation)]
        let dt = self
            .last_time
            .map_or(0.0, |t| (now - t).clamp(0.0, 0.25) as f32);
        self.last_time = Some(now);
        self.controller.update(dt);
        self.state.blink = (self.state.blink + dt) % 1.0;
        let visible = ctx.input(|i| i.viewport().visible()).unwrap_or(true);
        let interval = match self.controller.state() {
            // Hidden: engine and media-key events wake the app; this is only a backstop.
            _ if !visible => Duration::from_millis(500),
            PlaybackState::Playing => Duration::from_millis(16),
            _ => Duration::from_millis(100),
        };
        ctx.request_repaint_after(interval);
    }

    /// Draws one frame and handles input. [`ElevenApp::tick`] runs the player itself.
    pub fn show(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        if let Some(skin) = self.state.pending_skin.take() {
            self.set_skin(&ctx, skin);
        }
        self.sync_window(&ctx);

        let origin = ui.max_rect().min;
        let frame = Frame::compute(origin, self.sections());
        ui.painter().rect_filled(ui.max_rect(), 0.0, Color32::BLACK);

        let look: &dyn Look = self.look.as_ref();
        let mut actions = main_window::show(
            ui,
            &mut self.controller,
            &mut self.state,
            look,
            frame.main.min,
        );
        if let Some(eq) = frame.equalizer {
            actions.extend(equalizer::show(
                ui,
                &mut self.controller,
                &mut self.state,
                look,
                eq.min,
            ));
        }
        if let Some(pl) = frame.playlist {
            actions.extend(playlist::show(
                ui,
                &mut self.controller,
                &mut self.state,
                look,
                pl,
            ));
        }
        if self.state.show_prefs {
            if self.state.skins.is_none() {
                self.state.skins = Some(self.available_skins());
            }
        } else {
            self.state.skins = None;
            self.state.devices = None;
        }
        actions.extend(dialogs::show(&ctx, &mut self.controller, &mut self.state));
        actions.extend(self.keyboard(&ctx));
        self.drag_and_drop(&ctx, ui);
        for action in actions {
            self.apply(&ctx, action);
        }

        if ctx.input(|i| i.viewport().close_requested()) {
            self.persist();
        }
    }

    fn keyboard(&mut self, ctx: &Context) -> Vec<Action> {
        if ctx.egui_wants_keyboard_input()
            || self.state.jump.is_some()
            || self.state.location.is_some()
        {
            return Vec::new();
        }
        ctx.input(|i| {
            i.events
                .iter()
                .filter_map(|e| match e {
                    Event::Key {
                        key,
                        pressed: true,
                        repeat,
                        modifiers,
                        ..
                    } => {
                        let action = shortcut(*key, *modifiers)?;
                        let repeatable = matches!(
                            action,
                            Action::SeekBack
                                | Action::SeekForward
                                | Action::VolumeUp
                                | Action::VolumeDown
                        );
                        (!*repeat || repeatable).then_some(action)
                    }
                    _ => None,
                })
                .collect()
        })
    }

    fn drag_and_drop(&mut self, ctx: &Context, ui: &Ui) {
        self.state.drop_hover = ctx.input(|i| !i.raw.hovered_files.is_empty());
        let dropped: Vec<PathBuf> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .map(|f| f.path().to_path_buf())
                .filter(|p| !p.as_os_str().is_empty())
                .collect()
        });
        if !dropped.is_empty() {
            let n = self.controller.add_paths(&dropped);
            self.controller.show_message(&format!("Added {n} item(s)"));
        }
        if self.state.drop_hover {
            ui.painter().rect_stroke(
                ui.max_rect().shrink(1.0),
                0.0,
                egui::Stroke::new(2.0, Color32::from_rgb(0x3c, 0xf0, 0x60)),
                egui::StrokeKind::Inside,
            );
        }
    }

    /// Keeps the native window in step with the configuration.
    fn sync_window(&mut self, ctx: &Context) {
        let cfg = self.controller.config();
        let zoom = if cfg.double_size { 2.0 } else { 1.0 };
        if (ctx.zoom_factor() - zoom).abs() > f32::EPSILON {
            // A zoom change only takes effect at the end of the pass, and window sizes are
            // converted with the zoom in force then: resize on the next pass instead.
            ctx.set_zoom_factor(zoom);
            return;
        }
        let size = Frame::compute(egui::Pos2::ZERO, self.sections()).size();
        if self.state.last_size != Some((size, zoom)) {
            ctx.send_viewport_cmd(ViewportCommand::InnerSize(size));
            ctx.send_viewport_cmd(ViewportCommand::MinInnerSize(size));
            ctx.send_viewport_cmd(ViewportCommand::MaxInnerSize(size));
            self.state.last_size = Some((size, zoom));
        }
        let cfg = self.controller.config();
        if self.state.last_on_top != Some(cfg.always_on_top) {
            let level = if cfg.always_on_top {
                WindowLevel::AlwaysOnTop
            } else {
                WindowLevel::Normal
            };
            ctx.send_viewport_cmd(ViewportCommand::WindowLevel(level));
            self.state.last_on_top = Some(cfg.always_on_top);
        }
        if self.state.last_decorations != Some(cfg.decorations) {
            ctx.send_viewport_cmd(ViewportCommand::Decorations(cfg.decorations));
            self.state.last_decorations = Some(cfg.decorations);
        }
    }

    fn start_dir(&self) -> Option<PathBuf> {
        self.controller.config().last_directory.clone()
    }

    fn first_selected(&self) -> Option<usize> {
        self.controller
            .playlist()
            .selected_indices()
            .first()
            .copied()
    }

    /// Performs an action.
    pub fn apply(&mut self, ctx: &Context, action: Action) {
        let c = &mut self.controller;
        match action {
            Action::Previous => c.previous(),
            Action::Play => c.play(),
            Action::Pause => c.pause(),
            Action::Stop => c.stop(),
            Action::Next => c.next(),
            Action::SeekBack => c.seek_relative(SEEK_STEP, false),
            Action::SeekForward => c.seek_relative(SEEK_STEP, true),
            Action::VolumeUp => c.adjust_volume(VOLUME_STEP),
            Action::VolumeDown => c.adjust_volume(-VOLUME_STEP),
            Action::ToggleShuffle => c.toggle_shuffle(),
            Action::ToggleRepeat => c.toggle_repeat(),
            Action::ToggleTimeMode => c.toggle_time_mode(),
            Action::CycleVis => c.cycle_vis_mode(),
            Action::ToggleDoubleSize => c.config_mut().double_size ^= true,
            Action::ToggleEqualizer => c.config_mut().show_equalizer ^= true,
            Action::TogglePlaylist => c.config_mut().show_playlist ^= true,
            Action::ToggleShade => c.config_mut().shaded ^= true,
            Action::ToggleAlwaysOnTop => c.config_mut().always_on_top ^= true,
            Action::SelectAll => c.playlist_mut().select_all(),
            Action::RemoveSelected => c.remove_selected(),
            Action::PlaySelected => {
                if let Some(i) = self.first_selected() {
                    self.controller.play_index(i);
                }
            }
            Action::JumpToFile => self.state.jump = Some(JumpDialog::default()),
            Action::OpenLocation => self.state.location = Some(String::new()),
            Action::Preferences => self.state.show_prefs = true,
            Action::About => self.state.show_about = true,
            Action::FileInfo => {
                self.state.info = self
                    .first_selected()
                    .or_else(|| self.controller.playlist().current_index());
            }
            Action::AddFiles => {
                let start = self.start_dir();
                if let Some(files) = self.chooser.pick_files(start.as_deref()) {
                    let was_empty = self.controller.playlist().is_empty();
                    self.controller.add_paths(&files);
                    if was_empty && self.controller.state() == PlaybackState::Stopped {
                        self.controller.play();
                    }
                }
            }
            Action::AddFolder => {
                let start = self.start_dir();
                if let Some(dir) = self.chooser.pick_folder(start.as_deref()) {
                    let n = self.controller.add_paths(&[dir]);
                    self.controller.show_message(&format!("Added {n} file(s)"));
                }
            }
            Action::LoadPlaylist => {
                let start = self.start_dir();
                if let Some(files) = self.chooser.pick_files(start.as_deref()) {
                    self.controller.playlist_mut().clear();
                    self.controller.add_paths(&files);
                }
            }
            Action::SavePlaylist => {
                let start = self.start_dir();
                if let Some(path) = self.chooser.save_playlist(start.as_deref()) {
                    let msg = match self.controller.save_playlist(&path) {
                        Ok(()) => "Playlist saved".to_owned(),
                        Err(e) => format!("Cannot save playlist: {e}"),
                    };
                    self.controller.show_message(&msg);
                }
            }
            Action::LoadSkin => {
                let start = self.skins_dir.clone();
                if let Some(path) = self.chooser.pick_skin(start.as_deref()) {
                    self.set_skin(ctx, Some(path));
                }
            }
            Action::Minimize => ctx.send_viewport_cmd(ViewportCommand::Minimized(true)),
            Action::Quit => {
                self.persist();
                self.quit_requested = true;
                ctx.send_viewport_cmd(ViewportCommand::Close);
            }
        }
    }

    /// Saves config and playlist, logging failures.
    pub fn persist(&mut self) {
        if let Err(e) = self.controller.save_state() {
            log::warn!("could not save state: {e}");
        }
    }
}

impl eframe::App for ElevenApp {
    fn logic(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        self.tick(ctx);
    }

    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        self.show(ui);
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.persist();
    }
}
