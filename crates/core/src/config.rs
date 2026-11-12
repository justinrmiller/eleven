//! Persistent user configuration, stored as TOML.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::eq::{EqPreset, EqSettings};
use crate::format::TimeMode;

/// What the main-window visualiser draws.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VisMode {
    /// Spectrum analyser bars.
    #[default]
    Spectrum,
    /// Waveform.
    Oscilloscope,
    /// Nothing.
    Off,
}

impl VisMode {
    /// The next mode in the click-to-cycle order.
    #[must_use]
    pub fn cycled(self) -> Self {
        match self {
            Self::Spectrum => Self::Oscilloscope,
            Self::Oscilloscope => Self::Off,
            Self::Off => Self::Spectrum,
        }
    }
}

/// All persisted settings. Unknown keys are ignored and missing keys take their defaults, so
/// configs from older or newer versions still load.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Output volume, `0.0..=1.0`.
    pub volume: f32,
    /// Stereo balance, `-1.0` (left) ..= `1.0` (right).
    pub balance: f32,
    /// Shuffle play order.
    pub shuffle: bool,
    /// Wrap around at the end of the playlist.
    pub repeat: bool,
    /// Elapsed or remaining time on the main clock.
    pub time_mode: TimeMode,
    /// Visualiser mode.
    pub vis_mode: VisMode,
    /// Show the equaliser section.
    pub show_equalizer: bool,
    /// Show the playlist section.
    pub show_playlist: bool,
    /// Render everything at 2x.
    pub double_size: bool,
    /// Collapse the main window to its title bar ("windowshade").
    pub shaded: bool,
    /// Keep the window above others.
    pub always_on_top: bool,
    /// Use the OS window frame instead of the custom title bar.
    pub decorations: bool,
    /// Scroll long titles in the main display.
    pub scroll_title: bool,
    /// Preferred output device name; `None` uses the system default.
    pub output_device: Option<String>,
    /// Active `.wsz` skin; `None` uses the built-in look.
    pub skin: Option<PathBuf>,
    /// Directory the file dialogs open in.
    pub last_directory: Option<PathBuf>,
    /// Index of the current track in the saved playlist.
    pub current_track: Option<usize>,
    /// Number of visible playlist rows.
    pub playlist_rows: usize,
    /// Equaliser state.
    pub equalizer: EqSettings,
    /// User-saved equaliser presets.
    pub eq_presets: Vec<EqPreset>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            volume: 0.8,
            balance: 0.0,
            shuffle: false,
            repeat: false,
            time_mode: TimeMode::Elapsed,
            vis_mode: VisMode::Spectrum,
            show_equalizer: true,
            show_playlist: true,
            double_size: false,
            shaded: false,
            always_on_top: false,
            decorations: false,
            scroll_title: true,
            output_device: None,
            skin: None,
            last_directory: None,
            current_track: None,
            playlist_rows: 8,
            equalizer: EqSettings::default(),
            eq_presets: Vec::new(),
        }
    }
}

/// Errors from reading or writing the config file.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    /// The file exists but is not valid TOML for this schema.
    #[error("invalid config file {path}: {source}")]
    Parse {
        /// The offending file.
        path: PathBuf,
        /// The underlying TOML error.
        source: toml::de::Error,
    },
    /// The config could not be serialised.
    #[error("could not serialise config: {0}")]
    Serialize(#[from] toml::ser::Error),
    /// Reading or writing failed.
    #[error("config I/O error: {0}")]
    Io(#[from] std::io::Error),
}

impl Config {
    /// Name of the config file inside the config directory.
    pub const FILE_NAME: &'static str = "config.toml";

    /// Parses TOML text and sanitises the values.
    ///
    /// # Errors
    /// Returns the TOML error for malformed input.
    pub fn from_toml(text: &str) -> Result<Self, toml::de::Error> {
        toml::from_str::<Self>(text).map(Self::sanitized)
    }

    /// Serialises to TOML.
    ///
    /// # Errors
    /// Fails only if a value cannot be represented in TOML.
    pub fn to_toml(&self) -> Result<String, toml::ser::Error> {
        toml::to_string_pretty(self)
    }

    /// Loads the config at `path`; a missing file yields the defaults.
    ///
    /// # Errors
    /// Returns an error if the file exists but cannot be read or parsed.
    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        match std::fs::read_to_string(path) {
            Ok(text) => Self::from_toml(&text).map_err(|source| ConfigError::Parse {
                path: path.to_owned(),
                source,
            }),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e.into()),
        }
    }

    /// Writes the config to `path` atomically (temp file + rename), creating parent dirs.
    ///
    /// # Errors
    /// Returns an error if serialisation or any file operation fails.
    pub fn save(&self, path: &Path) -> Result<(), ConfigError> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("toml.tmp");
        std::fs::write(&tmp, self.to_toml()?)?;
        std::fs::rename(&tmp, path)?;
        Ok(())
    }

    /// Clamps every numeric field into its valid range.
    #[must_use]
    pub fn sanitized(mut self) -> Self {
        let finite_or = |v: f32, d: f32| if v.is_finite() { v } else { d };
        self.volume = finite_or(self.volume, 0.8).clamp(0.0, 1.0);
        self.balance = finite_or(self.balance, 0.0).clamp(-1.0, 1.0);
        self.playlist_rows = self.playlist_rows.clamp(3, 40);
        self.equalizer = self.equalizer.sanitized();
        self.eq_presets.retain(|p| !p.name.trim().is_empty());
        self
    }

    /// Adds or replaces (by name) a user preset.
    pub fn save_preset(&mut self, preset: EqPreset) {
        match self.eq_presets.iter_mut().find(|p| p.name == preset.name) {
            Some(existing) => *existing = preset,
            None => self.eq_presets.push(preset),
        }
    }

    /// Removes a user preset by name; returns whether it existed.
    pub fn delete_preset(&mut self, name: &str) -> bool {
        let before = self.eq_presets.len();
        self.eq_presets.retain(|p| p.name != name);
        self.eq_presets.len() != before
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eq::BANDS;

    #[test]
    fn defaults_round_trip() {
        let c = Config::default();
        let text = c.to_toml().unwrap();
        assert_eq!(Config::from_toml(&text).unwrap(), c);
    }

    #[test]
    fn full_round_trip() {
        let mut c = Config {
            volume: 0.25,
            balance: -0.5,
            shuffle: true,
            repeat: true,
            time_mode: TimeMode::Remaining,
            vis_mode: VisMode::Oscilloscope,
            double_size: true,
            output_device: Some("Speakers".into()),
            skin: Some("/skins/base.wsz".into()),
            current_track: Some(3),
            ..Config::default()
        };
        c.equalizer.enabled = true;
        c.equalizer.bands_db[3] = 4.5;
        c.save_preset(EqPreset {
            name: "Mine".into(),
            preamp_db: 1.0,
            bands_db: [2.0; BANDS],
        });
        let parsed = Config::from_toml(&c.to_toml().unwrap()).unwrap();
        assert_eq!(parsed, c);
    }

    #[test]
    fn missing_and_unknown_keys_are_tolerated() {
        let c =
            Config::from_toml("volume = 0.5\nfuture_option = true\n[equalizer]\nenabled = true\n")
                .unwrap();
        assert!((c.volume - 0.5).abs() < f32::EPSILON);
        assert!(c.equalizer.enabled);
        assert_eq!(c.equalizer.bands_db, [0.0; BANDS]);
        assert_eq!(c.playlist_rows, Config::default().playlist_rows);
    }

    #[test]
    fn values_are_sanitised() {
        let c = Config::from_toml(
            "volume = 7.0\nbalance = -3.0\nplaylist_rows = 1000\n\
             [equalizer]\npreamp_db = 40.0\n\
             [[eq_presets]]\nname = \" \"\npreamp_db = 0.0\nbands_db = [0,0,0,0,0,0,0,0,0,0]\n",
        )
        .unwrap();
        assert!((c.volume - 1.0).abs() < f32::EPSILON);
        assert!((c.balance + 1.0).abs() < f32::EPSILON);
        assert_eq!(c.playlist_rows, 40);
        assert!((c.equalizer.preamp_db - 12.0).abs() < f32::EPSILON);
        assert!(c.eq_presets.is_empty(), "blank preset names are dropped");
        let nan = Config {
            volume: f32::NAN,
            balance: f32::INFINITY,
            ..Config::default()
        };
        let s = nan.sanitized();
        assert!((s.volume - 0.8).abs() < f32::EPSILON);
        assert!(s.balance.abs() < f32::EPSILON);
    }

    #[test]
    fn invalid_toml_is_an_error() {
        assert!(Config::from_toml("volume = = 1").is_err());
        assert!(Config::from_toml("volume = \"loud\"").is_err());
    }

    #[test]
    fn load_and_save() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join(Config::FILE_NAME);
        assert_eq!(
            Config::load(&path).unwrap(),
            Config::default(),
            "missing file -> defaults"
        );
        let c = Config {
            volume: 0.11,
            ..Config::default()
        };
        c.save(&path).unwrap();
        assert_eq!(Config::load(&path).unwrap(), c);
        assert!(!path.with_extension("toml.tmp").exists());
        std::fs::write(&path, "volume = [").unwrap();
        let err = Config::load(&path).unwrap_err();
        assert!(matches!(err, ConfigError::Parse { .. }));
        assert!(err.to_string().contains("invalid config"));
        // A directory where the file should be is an I/O error.
        let dir_path = dir.path().join("adir");
        std::fs::create_dir(&dir_path).unwrap();
        assert!(matches!(Config::load(&dir_path), Err(ConfigError::Io(_))));
    }

    #[test]
    fn presets_are_saved_replaced_and_deleted() {
        let mut c = Config::default();
        let p = |name: &str, g: f32| EqPreset {
            name: name.into(),
            preamp_db: 0.0,
            bands_db: [g; BANDS],
        };
        c.save_preset(p("A", 1.0));
        c.save_preset(p("B", 2.0));
        c.save_preset(p("A", 3.0));
        assert_eq!(c.eq_presets.len(), 2);
        assert!((c.eq_presets[0].bands_db[0] - 3.0).abs() < f32::EPSILON);
        assert!(c.delete_preset("A"));
        assert!(!c.delete_preset("A"));
        assert_eq!(c.eq_presets.len(), 1);
    }

    #[test]
    fn vis_mode_cycles() {
        let m = VisMode::default();
        assert_eq!(m.cycled(), VisMode::Oscilloscope);
        assert_eq!(m.cycled().cycled(), VisMode::Off);
        assert_eq!(m.cycled().cycled().cycled(), VisMode::Spectrum);
    }
}
