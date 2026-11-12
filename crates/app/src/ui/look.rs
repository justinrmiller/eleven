//! The drawing interface shared by the built-in look and `.wsz` skins.
//!
//! UI code decides *what* is shown and where (see [`super::layout`]); a [`Look`] decides how it
//! is painted. Both looks paint into the same rectangles, so hit-testing is identical.

use egui::{Color32, Painter, Pos2, Rect};
use eleven_core::format::ClockDisplay;
use eleven_core::{EqSettings, PlaybackState};

/// Transport button glyphs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Glyph {
    /// Previous track.
    Previous,
    /// Play.
    Play,
    /// Pause.
    Pause,
    /// Stop.
    Stop,
    /// Next track.
    Next,
    /// Eject (open).
    Eject,
}

/// Which title bar is being drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TitleKind {
    /// Main window.
    Main,
    /// Main window in shade mode.
    Shade,
    /// Equaliser.
    Equalizer,
}

/// Small buttons in title bars.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TitleButton {
    /// Main menu.
    Menu,
    /// Minimise.
    Minimize,
    /// Shade / unshade.
    Shade,
    /// Close.
    Close,
}

/// Two-state buttons.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Toggle {
    /// Shuffle.
    Shuffle,
    /// Repeat.
    Repeat,
    /// Equaliser section visibility.
    Equalizer,
    /// Playlist section visibility.
    Playlist,
    /// Equaliser on/off.
    EqOn,
    /// Equaliser auto-load (shown disabled).
    EqAuto,
}

/// Clutterbar buttons, top to bottom.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Clutter {
    /// "O": the options menu.
    Options,
    /// "A": always on top.
    AlwaysOnTop,
    /// "I": file info.
    FileInfo,
    /// "D": double size.
    DoubleSize,
    /// "V": visualisation mode.
    Visualization,
}

impl Clutter {
    /// All buttons in display order.
    pub const ALL: [Self; 5] = [
        Self::Options,
        Self::AlwaysOnTop,
        Self::FileInfo,
        Self::DoubleSize,
        Self::Visualization,
    ];

    /// The button's letter.
    #[must_use]
    pub fn letter(self) -> &'static str {
        match self {
            Self::Options => "O",
            Self::AlwaysOnTop => "A",
            Self::FileInfo => "I",
            Self::DoubleSize => "D",
            Self::Visualization => "V",
        }
    }

    /// Accessible label.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Options => "Options (O)",
            Self::AlwaysOnTop => "Always on top (A)",
            Self::FileInfo => "File info (I)",
            Self::DoubleSize => "Double size (D)",
            Self::Visualization => "Visualization (V)",
        }
    }
}

/// Colours of the playlist rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlaylistStyle {
    /// Normal row text.
    pub normal: Color32,
    /// The current track's text.
    pub current: Color32,
    /// List background.
    pub background: Color32,
    /// Selected row background.
    pub selected: Color32,
}

/// Visualiser colours.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VisColors {
    /// Background.
    pub background: Color32,
    /// Bar colours from the bottom row (index 0) to the top row (index 15).
    pub bars: [Color32; 16],
    /// Peak markers.
    pub peak: Color32,
    /// Oscilloscope trace.
    pub scope: Color32,
}

/// Everything needed to paint the player.
pub trait Look {
    /// Short name for menus.
    fn name(&self) -> String;
    /// Main section background (everything that is not a control).
    fn main_background(&self, p: &Painter, origin: Pos2);
    /// A title bar.
    fn title_bar(&self, p: &Painter, rect: Rect, kind: TitleKind, focused: bool);
    /// A title-bar button.
    fn title_button(&self, p: &Painter, rect: Rect, which: TitleButton, pressed: bool);
    /// A transport button.
    fn transport(&self, p: &Painter, rect: Rect, glyph: Glyph, pressed: bool);
    /// A toggle button.
    fn toggle(&self, p: &Painter, rect: Rect, which: Toggle, on: bool, pressed: bool);
    /// The four-digit clock; `blank` hides the digits (blinking while paused).
    fn clock(&self, p: &Painter, origin: Pos2, clock: &ClockDisplay, blank: bool);
    /// Play/pause/stop indicator.
    fn status(&self, p: &Painter, rect: Rect, state: PlaybackState);
    /// The clutterbar; `lit[i]` shows button `Clutter::ALL[i]` pressed or active.
    fn clutterbar(&self, p: &Painter, origin: Pos2, lit: [bool; 5]);
    /// LCD text (title, kbps, kHz).
    fn lcd_text(&self, p: &Painter, rect: Rect, text: &str);
    /// Mono/stereo lights: `None` = nothing loaded.
    fn channels(&self, p: &Painter, mono: Rect, stereo: Rect, is_stereo: Option<bool>);
    /// Seek bar; `None` hides the knob (nothing seekable).
    fn posbar(&self, p: &Painter, rect: Rect, progress: Option<f32>, pressed: bool);
    /// Volume slider (`0.0..=1.0`).
    fn volume(&self, p: &Painter, rect: Rect, value: f32, pressed: bool);
    /// Balance slider (`-1.0..=1.0`).
    fn balance(&self, p: &Painter, rect: Rect, value: f32, pressed: bool);
    /// Visualiser palette.
    fn vis_colors(&self) -> VisColors;
    /// Equaliser section background.
    fn eq_background(&self, p: &Painter, origin: Pos2);
    /// An equaliser slider showing `db` (`-12..=12`).
    fn eq_slider(&self, p: &Painter, rect: Rect, db: f32, pressed: bool);
    /// The response graph.
    fn eq_graph(&self, p: &Painter, rect: Rect, eq: &EqSettings);
    /// A text button (EQ presets, playlist footer).
    fn text_button(&self, p: &Painter, rect: Rect, label: &str, pressed: bool);
    /// Playlist frame, title bar and footer.
    fn playlist_frame(&self, p: &Painter, section: Rect, focused: bool);
    /// Playlist row colours.
    fn playlist_style(&self) -> PlaylistStyle;
    /// Playlist scrollbar thumb at `fraction` (`0.0..=1.0`).
    fn scrollbar(&self, p: &Painter, track: Rect, fraction: f32, pressed: bool);
}
