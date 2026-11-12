//! Where eleven keeps its files.

use std::path::PathBuf;

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
        self.dir.join(eleven_core::Config::FILE_NAME)
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

#[cfg(test)]
mod tests {
    use super::*;

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
