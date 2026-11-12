//! Reading and writing playlist files: M3U / extended M3U and PLS.

use std::fmt::Write as _;
use std::path::Path;
use std::time::Duration;

use crate::playlist::Location;

/// One entry read from (or to be written to) a playlist file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaylistItem {
    /// The file or stream.
    pub location: Location,
    /// Title from `#EXTINF` / `TitleN`, if present.
    pub title: Option<String>,
    /// Length from `#EXTINF` / `LengthN`, if known (whole seconds).
    pub duration: Option<Duration>,
}

impl PlaylistItem {
    /// An item with only a location.
    #[must_use]
    pub fn new(location: Location) -> Self {
        Self {
            location,
            title: None,
            duration: None,
        }
    }
}

/// Supported playlist file formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaylistFormat {
    /// `.m3u` / `.m3u8`, written as extended M3U.
    M3u,
    /// `.pls` (Winamp/Shoutcast style INI).
    Pls,
}

impl PlaylistFormat {
    /// Picks the format from a file extension (case-insensitive).
    #[must_use]
    pub fn from_path(path: &Path) -> Option<Self> {
        match path
            .extension()?
            .to_string_lossy()
            .to_ascii_lowercase()
            .as_str()
        {
            "m3u" | "m3u8" => Some(Self::M3u),
            "pls" => Some(Self::Pls),
            _ => None,
        }
    }
}

/// Errors from loading or saving playlist files.
#[derive(Debug, thiserror::Error)]
pub enum PlaylistIoError {
    /// The extension is not `.m3u`, `.m3u8` or `.pls`.
    #[error("unsupported playlist format: {0}")]
    UnsupportedFormat(String),
    /// Reading or writing the file failed.
    #[error("playlist I/O error: {0}")]
    Io(#[from] std::io::Error),
}

/// Returns `true` if `path` has a playlist extension.
#[must_use]
pub fn is_playlist_path(path: &Path) -> bool {
    PlaylistFormat::from_path(path).is_some()
}

fn strip_bom(text: &str) -> &str {
    text.strip_prefix('\u{feff}').unwrap_or(text)
}

fn secs_to_duration(secs: i64) -> Option<Duration> {
    u64::try_from(secs)
        .ok()
        .filter(|&s| s > 0)
        .map(Duration::from_secs)
}

fn duration_to_secs(duration: Option<Duration>) -> i64 {
    duration.map_or(-1, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

/// Parses M3U or extended M3U text. Relative paths are resolved against `base_dir`.
#[must_use]
pub fn parse_m3u(text: &str, base_dir: Option<&Path>) -> Vec<PlaylistItem> {
    let mut items = Vec::new();
    let mut pending: Option<(Option<String>, Option<Duration>)> = None;
    for raw in strip_bom(text).lines() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(info) = line.strip_prefix("#EXTINF:") {
            let (secs, title) = info.split_once(',').unwrap_or((info, ""));
            // Attributes such as `tvg-id="x"` may follow the length; only the number matters.
            let secs = secs.split_whitespace().next().unwrap_or("");
            let duration = secs.parse::<f64>().ok().and_then(|s| {
                // Saturating float-to-int is the intended behaviour for absurd lengths.
                #[allow(clippy::cast_possible_truncation)]
                secs_to_duration(s.round() as i64)
            });
            let title = Some(title.trim().to_owned()).filter(|t| !t.is_empty());
            pending = Some((title, duration));
            continue;
        }
        if line.starts_with('#') {
            continue;
        }
        let (title, duration) = pending.take().unwrap_or_default();
        items.push(PlaylistItem {
            location: Location::parse(line, base_dir),
            title,
            duration,
        });
    }
    items
}

/// Serialises items as extended M3U.
#[must_use]
pub fn write_m3u(items: &[PlaylistItem]) -> String {
    let mut out = String::from("#EXTM3U\n");
    for item in items {
        if item.title.is_some() || item.duration.is_some() {
            let title = item
                .title
                .as_deref()
                .unwrap_or("")
                .replace(['\r', '\n'], " ");
            let _ = writeln!(out, "#EXTINF:{},{title}", duration_to_secs(item.duration));
        }
        let _ = writeln!(out, "{}", item.location.as_playlist_string());
    }
    out
}

/// Parses PLS text. Keys are case-insensitive; entries without a `FileN` key are skipped.
#[must_use]
pub fn parse_pls(text: &str, base_dir: Option<&Path>) -> Vec<PlaylistItem> {
    use std::collections::BTreeMap;

    #[derive(Default)]
    struct Partial {
        file: Option<String>,
        title: Option<String>,
        length: Option<i64>,
    }

    let mut parts: BTreeMap<u32, Partial> = BTreeMap::new();
    for raw in strip_bom(text).lines() {
        let line = raw.trim();
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim().to_ascii_lowercase();
        let value = value.trim();
        let (field, num) = if let Some(n) = key.strip_prefix("file") {
            ("file", n)
        } else if let Some(n) = key.strip_prefix("title") {
            ("title", n)
        } else if let Some(n) = key.strip_prefix("length") {
            ("length", n)
        } else {
            continue;
        };
        let Ok(num) = num.parse::<u32>() else {
            continue;
        };
        let part = parts.entry(num).or_default();
        match field {
            "file" => part.file = Some(value.to_owned()),
            "title" => part.title = Some(value.to_owned()).filter(|t| !t.is_empty()),
            _ => part.length = value.parse().ok(),
        }
    }
    parts
        .into_values()
        .filter_map(|p| {
            Some(PlaylistItem {
                location: Location::parse(&p.file?, base_dir),
                title: p.title,
                duration: p.length.and_then(secs_to_duration),
            })
        })
        .collect()
}

/// Serialises items as PLS (version 2).
#[must_use]
pub fn write_pls(items: &[PlaylistItem]) -> String {
    let mut out = String::from("[playlist]\n");
    for (i, item) in items.iter().enumerate() {
        let n = i + 1;
        let _ = writeln!(out, "File{n}={}", item.location.as_playlist_string());
        if let Some(title) = &item.title {
            let _ = writeln!(out, "Title{n}={}", title.replace(['\r', '\n'], " "));
        }
        let _ = writeln!(out, "Length{n}={}", duration_to_secs(item.duration));
    }
    let _ = writeln!(out, "NumberOfEntries={}\nVersion=2", items.len());
    out
}

/// Parses playlist text in the given format.
#[must_use]
pub fn parse(format: PlaylistFormat, text: &str, base_dir: Option<&Path>) -> Vec<PlaylistItem> {
    match format {
        PlaylistFormat::M3u => parse_m3u(text, base_dir),
        PlaylistFormat::Pls => parse_pls(text, base_dir),
    }
}

/// Serialises items in the given format.
#[must_use]
pub fn write(format: PlaylistFormat, items: &[PlaylistItem]) -> String {
    match format {
        PlaylistFormat::M3u => write_m3u(items),
        PlaylistFormat::Pls => write_pls(items),
    }
}

fn format_for(path: &Path) -> Result<PlaylistFormat, PlaylistIoError> {
    PlaylistFormat::from_path(path)
        .ok_or_else(|| PlaylistIoError::UnsupportedFormat(path.display().to_string()))
}

/// Loads a playlist file, choosing the format by extension. Relative entries are resolved
/// against the playlist's directory. Invalid UTF-8 is replaced rather than rejected.
///
/// # Errors
/// Returns an error for unknown extensions or if the file cannot be read.
pub fn load(path: &Path) -> Result<Vec<PlaylistItem>, PlaylistIoError> {
    let format = format_for(path)?;
    let bytes = std::fs::read(path)?;
    let text = String::from_utf8_lossy(&bytes);
    Ok(parse(format, &text, path.parent()))
}

/// Saves a playlist file, choosing the format by extension.
///
/// # Errors
/// Returns an error for unknown extensions or if the file cannot be written.
pub fn save(path: &Path, items: &[PlaylistItem]) -> Result<(), PlaylistIoError> {
    let format = format_for(path)?;
    std::fs::write(path, write(format, items))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use std::path::PathBuf;

    fn file(p: &str) -> Location {
        Location::File(PathBuf::from(p))
    }

    #[test]
    fn parses_extended_m3u_with_bom_crlf_and_relative_paths() {
        let text = "\u{feff}#EXTM3U\r\n#EXTINF:123,Spinal Tap - Big Bottom\r\nsongs/a.mp3\r\n\r\n\
                    # a comment\r\n/abs/b.flac\r\n#EXTINF:-1,Radio, with comma\r\nhttp://r.example/live\r\n";
        let items = parse_m3u(text, Some(Path::new("/base")));
        assert_eq!(
            items,
            vec![
                PlaylistItem {
                    location: file("/base/songs/a.mp3"),
                    title: Some("Spinal Tap - Big Bottom".into()),
                    duration: Some(Duration::from_secs(123)),
                },
                PlaylistItem::new(file("/abs/b.flac")),
                PlaylistItem {
                    location: Location::Url("http://r.example/live".into()),
                    title: Some("Radio, with comma".into()),
                    duration: None,
                },
            ]
        );
    }

    #[test]
    fn extinf_with_attributes_and_fractional_length() {
        let items = parse_m3u(
            "#EXTINF:12.6 tvg-id=\"x\",T\na.ogg\n#EXTINF:junk\nb.ogg\n",
            None,
        );
        assert_eq!(items[0].duration, Some(Duration::from_secs(13)));
        assert_eq!(items[0].title.as_deref(), Some("T"));
        assert_eq!(items[1].duration, None);
        assert_eq!(items[1].title, None);
    }

    #[test]
    fn writes_extended_m3u() {
        let items = vec![
            PlaylistItem {
                location: file("/m/a.mp3"),
                title: Some("A\nB".into()),
                duration: Some(Duration::from_secs(61)),
            },
            PlaylistItem::new(file("/m/b.mp3")),
        ];
        assert_eq!(
            write_m3u(&items),
            "#EXTM3U\n#EXTINF:61,A B\n/m/a.mp3\n/m/b.mp3\n"
        );
    }

    #[test]
    fn parses_pls_case_insensitively_and_in_numeric_order() {
        let text = "[playlist]\nnumberofentries=3\nFile2=http://s.example:8000/\nTitle2=Stream\n\
                    Length2=-1\nfile1=rel/one.mp3\ntitle1=\nLENGTH1=200\nTitle3=orphan\n\
                    Filex=bad\nnot a key\nVersion=2\n";
        let items = parse_pls(text, Some(Path::new("/p")));
        assert_eq!(
            items,
            vec![
                PlaylistItem {
                    location: file("/p/rel/one.mp3"),
                    title: None,
                    duration: Some(Duration::from_secs(200)),
                },
                PlaylistItem {
                    location: Location::Url("http://s.example:8000/".into()),
                    title: Some("Stream".into()),
                    duration: None,
                },
            ]
        );
    }

    #[test]
    fn writes_pls() {
        let items = vec![
            PlaylistItem {
                location: file("/a.mp3"),
                title: Some("A".into()),
                duration: Some(Duration::from_secs(5)),
            },
            PlaylistItem::new(Location::Url("http://x".into())),
        ];
        assert_eq!(
            write_pls(&items),
            "[playlist]\nFile1=/a.mp3\nTitle1=A\nLength1=5\nFile2=http://x\nLength2=-1\n\
             NumberOfEntries=2\nVersion=2\n"
        );
    }

    #[test]
    fn format_detection() {
        assert_eq!(
            PlaylistFormat::from_path(Path::new("a.M3U8")),
            Some(PlaylistFormat::M3u)
        );
        assert_eq!(
            PlaylistFormat::from_path(Path::new("a.pls")),
            Some(PlaylistFormat::Pls)
        );
        assert_eq!(PlaylistFormat::from_path(Path::new("a.mp3")), None);
        assert_eq!(PlaylistFormat::from_path(Path::new("noext")), None);
        assert!(is_playlist_path(Path::new("x.m3u")));
        assert!(!is_playlist_path(Path::new("x.txt")));
    }

    #[test]
    fn load_and_save_files() {
        let dir = tempfile::tempdir().unwrap();
        let items = vec![
            PlaylistItem {
                location: file("/music/one.flac"),
                title: Some("One".into()),
                duration: Some(Duration::from_secs(3)),
            },
            PlaylistItem::new(Location::Url("https://radio.example/s".into())),
        ];
        for name in ["list.m3u", "list.pls"] {
            let path = dir.path().join(name);
            save(&path, &items).unwrap();
            assert_eq!(load(&path).unwrap(), items, "{name}");
        }
        // Relative entries resolve against the playlist's directory.
        let rel = dir.path().join("rel.m3u");
        std::fs::write(&rel, "x.mp3\n").unwrap();
        assert_eq!(
            load(&rel).unwrap()[0].location,
            Location::File(dir.path().join("x.mp3"))
        );

        let bad = dir.path().join("list.txt");
        assert!(matches!(
            save(&bad, &items),
            Err(PlaylistIoError::UnsupportedFormat(_))
        ));
        assert!(matches!(
            load(&bad),
            Err(PlaylistIoError::UnsupportedFormat(_))
        ));
        let missing = dir.path().join("missing.m3u");
        let err = load(&missing).unwrap_err();
        assert!(err.to_string().contains("I/O"));
    }

    fn item_strategy() -> impl Strategy<Value = PlaylistItem> {
        let location = prop_oneof![
            "/[a-z]{1,8}(/[a-zA-Z0-9 _-]{1,8}){0,3}\\.(mp3|flac)".prop_map(|p| file(&p)),
            "http://[a-z]{1,8}\\.example(/[a-z0-9]{0,6})?".prop_map(Location::Url),
        ];
        let title = proptest::option::of("[A-Za-z0-9][A-Za-z0-9 ,.'-]{0,20}[A-Za-z0-9]");
        let duration = proptest::option::of((1u64..100_000).prop_map(Duration::from_secs));
        (location, title, duration).prop_map(|(location, title, duration)| PlaylistItem {
            location,
            title,
            duration,
        })
    }

    proptest! {
        #[test]
        fn m3u_round_trips(items in proptest::collection::vec(item_strategy(), 0..20)) {
            prop_assert_eq!(parse_m3u(&write_m3u(&items), None), items);
        }

        #[test]
        fn pls_round_trips(items in proptest::collection::vec(item_strategy(), 0..20)) {
            prop_assert_eq!(parse_pls(&write_pls(&items), None), items);
        }

        #[test]
        fn parsers_never_panic(text in "\\PC{0,200}") {
            let _ = parse_m3u(&text, Some(Path::new("/b")));
            let _ = parse_pls(&text, None);
        }
    }
}
