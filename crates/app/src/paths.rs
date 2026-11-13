//! Where eleven keeps its files.

use std::path::{Path, PathBuf};

use eleven_core::Config;
use eleven_core::config::ConfigError;

/// Environment variable that overrides the config directory (portable installs, tests).
pub const CONFIG_DIR_ENV: &str = "ELEVEN_CONFIG_DIR";

/// Locations of eleven's persistent files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppPaths {
    /// Base directory: `~/.config/eleven` on Linux, `~/Library/Application Support/eleven`
    /// on macOS.
    pub dir: PathBuf,
}

impl AppPaths {
    /// Resolves the directory from [`CONFIG_DIR_ENV`] or the platform default.
    #[must_use]
    pub fn detect() -> Option<Self> {
        if let Some(dir) = std::env::var_os(CONFIG_DIR_ENV).filter(|v| !v.is_empty()) {
            return Some(Self {
                dir: PathBuf::from(dir),
            });
        }
        directories::BaseDirs::new().map(|b| Self {
            dir: b.config_dir().join("eleven"),
        })
    }

    /// The TOML config file.
    #[must_use]
    pub fn config_file(&self) -> PathBuf {
        self.dir.join(Config::FILE_NAME)
    }

    /// The auto-saved playlist.
    #[must_use]
    pub fn playlist_file(&self) -> PathBuf {
        self.dir.join("playlist.m3u")
    }

    /// Where the skin browser looks for `.wsz` files.
    #[must_use]
    pub fn skins_dir(&self) -> PathBuf {
        self.dir.join("skins")
    }

    /// Persistence settings for the controller.
    #[must_use]
    pub fn persistence(&self) -> crate::Persistence {
        crate::Persistence {
            config_file: Some(self.config_file()),
            playlist_file: Some(self.playlist_file()),
        }
    }
}

/// The result of [`load_config`].
#[derive(Debug)]
pub struct LoadedConfig {
    /// The settings to use (the defaults if the file could not be used).
    pub config: Config,
    /// Something the user should be told about the settings file.
    pub warning: Option<String>,
    /// Whether saving may write the file. `false` when an unreadable file could not be moved
    /// aside, so quitting does not replace the user's settings with the defaults.
    pub writable: bool,
}

/// Loads the settings file at `path`.
///
/// A file that cannot be parsed (a typo, or a newer version's format) is renamed to
/// `<name>.bad` before the defaults are used, so the user's settings survive the save on exit.
#[must_use]
pub fn load_config(path: &Path) -> LoadedConfig {
    let fallback = |warning: String, writable: bool| {
        log::warn!("{warning}");
        LoadedConfig {
            config: Config::default(),
            warning: Some(warning),
            writable,
        }
    };
    match Config::load(path) {
        Ok(config) => LoadedConfig {
            config,
            warning: None,
            writable: true,
        },
        Err(ConfigError::Parse { source, .. }) => {
            let mut backup = path.as_os_str().to_owned();
            backup.push(".bad");
            let backup = PathBuf::from(backup);
            log::warn!("cannot parse {}: {source}", path.display());
            match std::fs::rename(path, &backup) {
                Ok(()) => fallback(
                    format!(
                        "Settings unreadable - kept as {}",
                        backup.file_name().unwrap_or_default().to_string_lossy()
                    ),
                    true,
                ),
                Err(e) => fallback(format!("Settings unreadable and not saved ({e})"), false),
            }
        }
        Err(e) => fallback(format!("Settings not loaded: {e}"), false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn broken_settings_are_kept_aside() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(Config::FILE_NAME);
        let broken = "volume = \"loud\"\n";
        std::fs::write(&path, broken).unwrap();
        let loaded = load_config(&path);
        assert!(loaded.writable);
        assert!(loaded.warning.unwrap().contains("config.toml.bad"));
        assert_eq!(loaded.config, Config::default());
        assert_eq!(
            std::fs::read_to_string(dir.path().join("config.toml.bad")).unwrap(),
            broken
        );
        // Saving now creates a fresh file without touching the backup.
        loaded.config.save(&path).unwrap();
        assert!(dir.path().join("config.toml.bad").exists());
    }

    #[test]
    fn good_and_missing_settings_load_quietly() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(Config::FILE_NAME);
        let missing = load_config(&path);
        assert!(missing.warning.is_none() && missing.writable);
        let config = Config {
            volume: 0.25,
            ..Config::default()
        };
        config.save(&path).unwrap();
        let loaded = load_config(&path);
        assert!(loaded.warning.is_none());
        assert!((loaded.config.volume - 0.25).abs() < 1e-6);
    }

    #[test]
    fn unreadable_settings_are_never_overwritten() {
        // A directory where the file should be: reading fails with an I/O error.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(Config::FILE_NAME);
        std::fs::create_dir(&path).unwrap();
        let loaded = load_config(&path);
        assert!(!loaded.writable);
        assert!(loaded.warning.is_some());
    }

    #[test]
    fn file_names() {
        let p = AppPaths {
            dir: PathBuf::from("/cfg"),
        };
        assert_eq!(p.config_file(), PathBuf::from("/cfg/config.toml"));
        assert_eq!(p.playlist_file(), PathBuf::from("/cfg/playlist.m3u"));
        assert_eq!(p.skins_dir(), PathBuf::from("/cfg/skins"));
        let per = p.persistence();
        assert_eq!(per.config_file, Some(PathBuf::from("/cfg/config.toml")));
    }

    #[test]
    fn detect_finds_a_directory() {
        // Either the override or the platform default must resolve on Linux and macOS.
        let p = AppPaths::detect().expect("a config dir");
        assert!(p.dir.ends_with("eleven") || std::env::var_os(CONFIG_DIR_ENV).is_some());
    }
}
