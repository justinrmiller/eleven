//! Loader for classic `.wsz` skins (the Winamp 2 format that XMMS also used).
//!
//! A `.wsz` file is a zip archive of BMP sprite sheets plus a few text files. File names are
//! matched case-insensitively and may sit inside a sub-folder. Only the sheets needed for the
//! main window are mandatory; the equaliser and playlist fall back to the host's own look when
//! `eqmain.bmp` or `pledit.bmp` is missing.
//!
//! No skin artwork ships with eleven; [`synthetic`] builds placeholder skins for tests.

pub mod bitmap;
pub mod sprites;
pub mod synthetic;
pub mod textfiles;

use std::collections::HashMap;
use std::io::{Cursor, Read};
use std::path::Path;

pub use sprites::{Sheet, Sprite};
pub use textfiles::{PlaylistColors, Rgb};

/// Largest archive member eleven will read (guards against zip bombs).
const MAX_MEMBER_BYTES: u64 = 16 * 1024 * 1024;

/// An RGBA bitmap.
#[derive(Clone, PartialEq, Eq)]
pub struct Bitmap {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Row-major RGBA8 pixels.
    pub rgba: Vec<u8>,
}

impl std::fmt::Debug for Bitmap {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Bitmap({}x{})", self.width, self.height)
    }
}

impl Bitmap {
    /// The pixel at (`x`, `y`), if inside the bitmap.
    #[must_use]
    pub fn pixel(&self, x: u32, y: u32) -> Option<[u8; 4]> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let i = ((y * self.width + x) * 4) as usize;
        self.rgba.get(i..i + 4).map(|p| [p[0], p[1], p[2], p[3]])
    }

    /// Whether `sprite` lies entirely inside this bitmap.
    #[must_use]
    pub fn contains(&self, sprite: &Sprite) -> bool {
        sprite.x + sprite.w <= self.width && sprite.y + sprite.h <= self.height
    }
}

/// Errors from loading a skin.
#[derive(Debug, thiserror::Error)]
pub enum SkinError {
    /// The file could not be read.
    #[error("cannot read skin: {0}")]
    Io(#[from] std::io::Error),
    /// The file is not a valid zip archive.
    #[error("not a skin archive: {0}")]
    Archive(String),
    /// A bitmap could not be decoded.
    #[error("cannot decode {name}: {reason}")]
    Image {
        /// The bitmap's file name.
        name: String,
        /// Decoder message.
        reason: String,
    },
    /// A sheet needed for the main window is missing.
    #[error("skin is missing {0}")]
    Missing(&'static str),
}

/// A loaded skin.
#[derive(Debug, Clone)]
pub struct Skin {
    /// Display name (the archive's file stem).
    pub name: String,
    sheets: HashMap<Sheet, Bitmap>,
    /// Playlist colours (`pledit.txt`), or defaults.
    pub playlist_colors: PlaylistColors,
    /// Visualiser palette (`viscolor.txt`), if present and complete.
    pub vis_colors: Option<[Rgb; textfiles::VIS_COLORS]>,
}

impl Skin {
    /// Loads a `.wsz`/zip file from disk.
    ///
    /// # Errors
    /// See [`Skin::from_bytes`].
    pub fn load(path: &Path) -> Result<Self, SkinError> {
        let bytes = std::fs::read(path)?;
        let name = path
            .file_stem()
            .map_or_else(|| "skin".to_owned(), |s| s.to_string_lossy().into_owned());
        Self::from_bytes(&name, &bytes)
    }

    /// Loads a skin from the bytes of a zip archive.
    ///
    /// # Errors
    /// Fails if the archive is invalid, a bitmap cannot be decoded, or a sheet required for the
    /// main window is missing.
    pub fn from_bytes(name: &str, bytes: &[u8]) -> Result<Self, SkinError> {
        let files = read_archive(bytes)?;
        let mut sheets = HashMap::new();
        for sheet in Sheet::ALL {
            if let Some(data) = files.get(sheet.file_name()) {
                sheets.insert(sheet, decode_bitmap(sheet.file_name(), data)?);
            }
        }
        for sheet in Sheet::REQUIRED {
            if !sheets.contains_key(&sheet) {
                return Err(SkinError::Missing(sheet.file_name()));
            }
        }
        if !sheets.contains_key(&Sheet::Numbers) && !sheets.contains_key(&Sheet::NumsEx) {
            return Err(SkinError::Missing(Sheet::Numbers.file_name()));
        }
        let text = |file: &str| {
            files
                .get(file)
                .map(|b| String::from_utf8_lossy(b).into_owned())
        };
        Ok(Self {
            name: name.to_owned(),
            sheets,
            playlist_colors: text("pledit.txt")
                .map(|t| textfiles::parse_pledit(&t))
                .unwrap_or_default(),
            vis_colors: text("viscolor.txt").and_then(|t| textfiles::parse_viscolor(&t)),
        })
    }

    /// A sheet, if the skin has it.
    #[must_use]
    pub fn sheet(&self, sheet: Sheet) -> Option<&Bitmap> {
        self.sheets.get(&sheet)
    }

    /// All loaded sheets.
    pub fn sheets(&self) -> impl Iterator<Item = (Sheet, &Bitmap)> {
        self.sheets.iter().map(|(s, b)| (*s, b))
    }

    /// The sheet clock digits come from (`nums_ex.bmp` preferred).
    #[must_use]
    pub fn digits_sheet(&self) -> Sheet {
        if self.sheets.contains_key(&Sheet::NumsEx) {
            Sheet::NumsEx
        } else {
            Sheet::Numbers
        }
    }

    /// The sheet the balance slider comes from (`volume.bmp` if `balance.bmp` is missing).
    #[must_use]
    pub fn balance_sheet(&self) -> Sheet {
        if self.sheets.contains_key(&Sheet::Balance) {
            Sheet::Balance
        } else {
            Sheet::Volume
        }
    }

    /// Whether the equaliser can be drawn from this skin.
    #[must_use]
    pub fn has_equalizer(&self) -> bool {
        self.sheets.contains_key(&Sheet::Eqmain)
    }

    /// Whether the playlist can be drawn from this skin.
    #[must_use]
    pub fn has_playlist(&self) -> bool {
        self.sheets.contains_key(&Sheet::Pledit)
    }
}

/// Reads every member of the archive into a map keyed by lower-case file name (directories are
/// ignored; the shallowest match wins when names repeat).
fn read_archive(bytes: &[u8]) -> Result<HashMap<String, Vec<u8>>, SkinError> {
    let mut zip =
        zip::ZipArchive::new(Cursor::new(bytes)).map_err(|e| SkinError::Archive(e.to_string()))?;
    let mut files: HashMap<String, (usize, Vec<u8>)> = HashMap::new();
    for i in 0..zip.len() {
        let mut member = zip
            .by_index(i)
            .map_err(|e| SkinError::Archive(e.to_string()))?;
        if member.is_dir() || member.size() > MAX_MEMBER_BYTES {
            continue;
        }
        let Ok(full) = member.name().map(|n| n.replace('\\', "/")) else {
            continue;
        };
        let depth = full.matches('/').count();
        let Some(base) = full.rsplit('/').next().map(str::to_ascii_lowercase) else {
            continue;
        };
        if base.is_empty() || files.get(&base).is_some_and(|(d, _)| *d <= depth) {
            continue;
        }
        let mut data = Vec::new();
        member
            .by_ref()
            .take(MAX_MEMBER_BYTES)
            .read_to_end(&mut data)?;
        files.insert(base, (depth, data));
    }
    Ok(files.into_iter().map(|(k, (_, v))| (k, v)).collect())
}

fn decode_bitmap(name: &str, data: &[u8]) -> Result<Bitmap, SkinError> {
    match image::load_from_memory(data) {
        Ok(img) => {
            let img = img.to_rgba8();
            Ok(Bitmap {
                width: img.width(),
                height: img.height(),
                rgba: img.into_raw(),
            })
        }
        // Old skin tools wrote slightly malformed RLE bitmaps that `image` rejects.
        Err(e) => bitmap::decode_rle_bmp(data).ok_or_else(|| SkinError::Image {
            name: name.to_owned(),
            reason: e.to_string(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use synthetic::{SyntheticSkin, sheet_color};

    #[test]
    fn loads_a_complete_synthetic_skin() {
        let bytes = SyntheticSkin::default().build();
        let skin = Skin::from_bytes("Test", &bytes).unwrap();
        assert_eq!(skin.name, "Test");
        assert!(skin.has_equalizer() && skin.has_playlist());
        assert_eq!(skin.digits_sheet(), Sheet::NumsEx);
        assert_eq!(skin.balance_sheet(), Sheet::Balance);
        assert_eq!(skin.sheets().count(), Sheet::ALL.len());
        // Every sheet decoded to its nominal size with its marker colour.
        for sheet in Sheet::ALL {
            let bmp = skin.sheet(sheet).unwrap();
            assert_eq!((bmp.width, bmp.height), sheet.nominal_size(), "{sheet:?}");
            let [r, g, b, _] = bmp.pixel(1, 1).unwrap();
            assert_eq!(Rgb(r, g, b), sheet_color(sheet), "{sheet:?}");
            assert!(bmp.contains(&Sprite {
                sheet,
                x: 0,
                y: 0,
                w: bmp.width,
                h: bmp.height
            }));
        }
        assert_eq!(skin.playlist_colors.normal, Rgb(0x10, 0xe0, 0x10));
        assert!(skin.vis_colors.is_some());
        assert!(format!("{:?}", skin.sheet(Sheet::Main).unwrap()).contains("275x116"));
    }

    #[test]
    fn names_are_case_insensitive_and_may_be_nested() {
        let bytes = SyntheticSkin {
            upper_case: true,
            folder: Some("MySkin/".into()),
            ..Default::default()
        }
        .build();
        let skin = Skin::from_bytes("Nested", &bytes).unwrap();
        assert!(skin.sheet(Sheet::Cbuttons).is_some());
    }

    #[test]
    fn optional_sheets_fall_back() {
        let bytes = SyntheticSkin {
            omit: vec![Sheet::Eqmain, Sheet::Pledit, Sheet::NumsEx, Sheet::Balance],
            text_files: false,
            ..Default::default()
        }
        .build();
        let skin = Skin::from_bytes("Minimal", &bytes).unwrap();
        assert!(!skin.has_equalizer());
        assert!(!skin.has_playlist());
        assert_eq!(skin.digits_sheet(), Sheet::Numbers);
        assert_eq!(skin.balance_sheet(), Sheet::Volume);
        assert_eq!(skin.playlist_colors, PlaylistColors::default());
        assert!(skin.vis_colors.is_none());
    }

    #[test]
    fn missing_required_sheets_are_errors() {
        let bytes = SyntheticSkin {
            omit: vec![Sheet::Cbuttons],
            ..Default::default()
        }
        .build();
        let err = Skin::from_bytes("x", &bytes).unwrap_err();
        assert!(matches!(err, SkinError::Missing("cbuttons.bmp")), "{err}");
        let bytes = SyntheticSkin {
            omit: vec![Sheet::Numbers, Sheet::NumsEx],
            ..Default::default()
        }
        .build();
        assert!(matches!(
            Skin::from_bytes("x", &bytes),
            Err(SkinError::Missing("numbers.bmp"))
        ));
    }

    #[test]
    fn corrupt_input_is_rejected() {
        assert!(matches!(
            Skin::from_bytes("x", b"not a zip"),
            Err(SkinError::Archive(_))
        ));
        let bytes = SyntheticSkin {
            corrupt: Some(Sheet::Main),
            ..Default::default()
        }
        .build();
        let err = Skin::from_bytes("x", &bytes).unwrap_err();
        assert!(
            matches!(&err, SkinError::Image { name, .. } if name == "main.bmp"),
            "{err}"
        );
        let dir = tempfile::tempdir().unwrap();
        assert!(matches!(
            Skin::load(&dir.path().join("missing.wsz")),
            Err(SkinError::Io(_))
        ));
    }

    #[test]
    fn loads_from_disk_using_the_file_stem_as_name() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("Gunmetal.wsz");
        std::fs::write(&path, SyntheticSkin::default().build()).unwrap();
        assert_eq!(Skin::load(&path).unwrap().name, "Gunmetal");
    }

    #[test]
    fn pixel_bounds() {
        let b = Bitmap {
            width: 2,
            height: 1,
            rgba: vec![1, 2, 3, 4, 5, 6, 7, 8],
        };
        assert_eq!(b.pixel(1, 0), Some([5, 6, 7, 8]));
        assert_eq!(b.pixel(2, 0), None);
        assert_eq!(b.pixel(0, 1), None);
    }
}
