//! User intents produced by buttons, menus and keyboard shortcuts, and the key map.

use egui::{Key, Modifiers};

/// Something the user asked for that the app (not just the controller) must handle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Previous track.
    Previous,
    /// Play / restart / resume.
    Play,
    /// Toggle pause.
    Pause,
    /// Stop.
    Stop,
    /// Next track.
    Next,
    /// Add files via the file dialog.
    AddFiles,
    /// Add a folder via the folder dialog.
    AddFolder,
    /// Ask for a URL (or path) to play.
    OpenLocation,
    /// Open the jump-to-file dialog.
    JumpToFile,
    /// Seek back [`crate::controller::SEEK_STEP`].
    SeekBack,
    /// Seek forward [`crate::controller::SEEK_STEP`].
    SeekForward,
    /// Volume up a step.
    VolumeUp,
    /// Volume down a step.
    VolumeDown,
    /// Toggle shuffle.
    ToggleShuffle,
    /// Toggle repeat.
    ToggleRepeat,
    /// Toggle double size.
    ToggleDoubleSize,
    /// Show/hide the equaliser.
    ToggleEqualizer,
    /// Show/hide the playlist.
    TogglePlaylist,
    /// Shade/unshade the main window.
    ToggleShade,
    /// Toggle always-on-top.
    ToggleAlwaysOnTop,
    /// Elapsed/remaining time.
    ToggleTimeMode,
    /// Cycle the visualiser mode.
    CycleVis,
    /// Open preferences.
    Preferences,
    /// Open the skin browser.
    Skins,
    /// Remove selected playlist entries.
    RemoveSelected,
    /// Play the first selected entry.
    PlaySelected,
    /// Select all playlist entries.
    SelectAll,
    /// Load a playlist file.
    LoadPlaylist,
    /// Save the playlist to a file.
    SavePlaylist,
    /// Show file info for the first selected entry.
    FileInfo,
    /// Show the about box.
    About,
    /// Pick a `.wsz` skin file.
    LoadSkin,
    /// Minimise the window.
    Minimize,
    /// Quit.
    Quit,
}

/// Maps a key press to an action, following XMMS's shortcuts where they exist.
///
/// `command` is Ctrl on Linux and Cmd on macOS (egui's `Modifiers::command`).
#[must_use]
pub fn shortcut(key: Key, m: Modifiers) -> Option<Action> {
    let plain = !m.command && !m.alt && !m.shift;
    let cmd = m.command && !m.alt;
    Some(match key {
        Key::Z if plain => Action::Previous,
        Key::X if plain => Action::Play,
        Key::C if plain => Action::Pause,
        Key::V if plain => Action::Stop,
        Key::B if plain => Action::Next,
        Key::L if plain => Action::AddFiles,
        Key::L if cmd && m.shift => Action::AddFolder,
        Key::L if cmd => Action::OpenLocation,
        Key::J if plain => Action::JumpToFile,
        Key::S if plain => Action::ToggleShuffle,
        Key::R if plain => Action::ToggleRepeat,
        Key::ArrowLeft if plain => Action::SeekBack,
        Key::ArrowRight if plain => Action::SeekForward,
        Key::ArrowUp if plain => Action::VolumeUp,
        Key::ArrowDown if plain => Action::VolumeDown,
        Key::D if cmd => Action::ToggleDoubleSize,
        Key::W if cmd => Action::ToggleShade,
        Key::T if cmd => Action::ToggleAlwaysOnTop,
        Key::P if cmd => Action::Preferences,
        Key::A if cmd => Action::SelectAll,
        Key::O if cmd => Action::LoadPlaylist,
        Key::S if cmd => Action::SavePlaylist,
        Key::I if cmd => Action::FileInfo,
        Key::Q if cmd => Action::Quit,
        Key::S if m.alt && !m.command => Action::Skins,
        Key::G if m.alt => Action::ToggleEqualizer,
        Key::E if m.alt => Action::TogglePlaylist,
        Key::Delete | Key::Backspace if plain => Action::RemoveSelected,
        Key::Enter if plain => Action::PlaySelected,
        _ => return None,
    })
}

/// Human-readable shortcut list for the about box and README.
pub const SHORTCUTS: &[(&str, &str)] = &[
    ("Z / X / C / V / B", "Previous / Play / Pause / Stop / Next"),
    ("L", "Add files"),
    ("Ctrl+L", "Open location (URL)"),
    ("Ctrl+Shift+L", "Add folder"),
    ("J", "Jump to file"),
    ("Left / Right", "Seek 5 s"),
    ("Up / Down", "Volume"),
    ("S / R", "Shuffle / Repeat"),
    ("Alt+G / Alt+E", "Equalizer / Playlist"),
    ("Ctrl+D", "Double size"),
    ("Ctrl+W", "Shade"),
    ("Ctrl+T", "Always on top"),
    ("Ctrl+P", "Preferences"),
    ("Alt+S", "Skins"),
    ("Ctrl+A", "Select all"),
    ("Ctrl+O / Ctrl+S", "Load / Save playlist"),
    ("Ctrl+I", "File info"),
    ("Delete", "Remove selected"),
    ("Enter", "Play selected"),
    ("Ctrl+Q", "Quit"),
];

#[cfg(test)]
mod tests {
    use super::*;

    const NONE: Modifiers = Modifiers::NONE;

    fn cmd() -> Modifiers {
        Modifiers {
            command: true,
            ctrl: true,
            ..Modifiers::NONE
        }
    }

    #[test]
    fn xmms_transport_keys() {
        assert_eq!(shortcut(Key::Z, NONE), Some(Action::Previous));
        assert_eq!(shortcut(Key::X, NONE), Some(Action::Play));
        assert_eq!(shortcut(Key::C, NONE), Some(Action::Pause));
        assert_eq!(shortcut(Key::V, NONE), Some(Action::Stop));
        assert_eq!(shortcut(Key::B, NONE), Some(Action::Next));
        assert_eq!(shortcut(Key::J, NONE), Some(Action::JumpToFile));
        assert_eq!(shortcut(Key::L, NONE), Some(Action::AddFiles));
    }

    #[test]
    fn modifiers_change_meaning() {
        assert_eq!(shortcut(Key::L, cmd()), Some(Action::OpenLocation));
        assert_eq!(
            shortcut(
                Key::L,
                Modifiers {
                    shift: true,
                    ..cmd()
                }
            ),
            Some(Action::AddFolder)
        );
        assert_eq!(shortcut(Key::S, NONE), Some(Action::ToggleShuffle));
        assert_eq!(shortcut(Key::S, cmd()), Some(Action::SavePlaylist));
        assert_eq!(shortcut(Key::Z, cmd()), None, "Ctrl+Z is not previous");
        assert_eq!(
            shortcut(Key::G, Modifiers::ALT),
            Some(Action::ToggleEqualizer)
        );
        assert_eq!(
            shortcut(Key::E, Modifiers::ALT),
            Some(Action::TogglePlaylist)
        );
        assert_eq!(shortcut(Key::S, Modifiers::ALT), Some(Action::Skins));
        assert_eq!(shortcut(Key::D, cmd()), Some(Action::ToggleDoubleSize));
        assert_eq!(shortcut(Key::Q, cmd()), Some(Action::Quit));
        assert_eq!(shortcut(Key::X, Modifiers::SHIFT), None);
    }

    #[test]
    fn every_documented_action_is_reachable() {
        let keys = [
            Key::ArrowLeft,
            Key::ArrowRight,
            Key::ArrowUp,
            Key::ArrowDown,
            Key::R,
            Key::Delete,
            Key::Backspace,
            Key::Enter,
        ];
        for k in keys {
            assert!(shortcut(k, NONE).is_some(), "{k:?}");
        }
        for k in [Key::W, Key::T, Key::P, Key::A, Key::O, Key::I] {
            assert!(shortcut(k, cmd()).is_some(), "Ctrl+{k:?}");
        }
        assert!(SHORTCUTS.len() >= 15);
        assert_eq!(shortcut(Key::F1, NONE), None);
    }
}
