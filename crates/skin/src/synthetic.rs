//! Builds placeholder `.wsz` archives in memory, for tests and demos.
//!
//! Each sheet is filled with its own flat colour (see [`sheet_color`]) with a darker border, so
//! tests can tell which sheet a pixel came from. No third-party artwork is involved.

use std::io::{Cursor, Write};

use crate::sprites::Sheet;
use crate::textfiles::Rgb;

/// The fill colour used for `sheet` in synthetic skins.
#[must_use]
pub fn sheet_color(sheet: Sheet) -> Rgb {
    match sheet {
        Sheet::Main => Rgb(0x40, 0x30, 0x60),
        Sheet::Titlebar => Rgb(0x80, 0x20, 0x20),
        Sheet::Cbuttons => Rgb(0x20, 0x70, 0x90),
        Sheet::Numbers => Rgb(0x10, 0x10, 0x10),
        Sheet::NumsEx => Rgb(0x18, 0x18, 0x18),
        Sheet::Text => Rgb(0x00, 0x30, 0x00),
        Sheet::Playpaus => Rgb(0x90, 0x90, 0x20),
        Sheet::Monoster => Rgb(0x50, 0x50, 0x50),
        Sheet::Posbar => Rgb(0x30, 0x60, 0x30),
        Sheet::Volume => Rgb(0x60, 0x40, 0x20),
        Sheet::Balance => Rgb(0x20, 0x40, 0x60),
        Sheet::Shufrep => Rgb(0x70, 0x30, 0x70),
        Sheet::Eqmain => Rgb(0x30, 0x50, 0x70),
        Sheet::Pledit => Rgb(0x50, 0x70, 0x30),
    }
}

/// Options for a synthetic skin archive.
#[derive(Debug, Clone)]
pub struct SyntheticSkin {
    /// Sheets to leave out.
    pub omit: Vec<Sheet>,
    /// Store file names in upper case (`MAIN.BMP`).
    pub upper_case: bool,
    /// Put every file under this folder prefix (e.g. `"MySkin/"`).
    pub folder: Option<String>,
    /// Include `pledit.txt` and `viscolor.txt`.
    pub text_files: bool,
    /// Replace this sheet's bytes with garbage.
    pub corrupt: Option<Sheet>,
}

impl Default for SyntheticSkin {
    fn default() -> Self {
        Self {
            omit: Vec::new(),
            upper_case: false,
            folder: None,
            text_files: true,
            corrupt: None,
        }
    }
}

fn bmp(sheet: Sheet) -> Vec<u8> {
    let (w, h) = sheet.nominal_size();
    let Rgb(r, g, b) = sheet_color(sheet);
    let border = image::Rgb([r / 2, g / 2, b / 2]);
    let img = image::RgbImage::from_fn(w, h, |x, y| {
        if x == 0 || y == 0 || x == w - 1 || y == h - 1 {
            border
        } else {
            image::Rgb([r, g, b])
        }
    });
    let mut out = Vec::new();
    image::DynamicImage::ImageRgb8(img)
        .write_to(&mut Cursor::new(&mut out), image::ImageFormat::Bmp)
        .expect("encoding a BMP in memory cannot fail");
    out
}

impl SyntheticSkin {
    fn name(&self, file: &str) -> String {
        let file = if self.upper_case {
            file.to_ascii_uppercase()
        } else {
            file.to_owned()
        };
        format!("{}{file}", self.folder.as_deref().unwrap_or(""))
    }

    /// Builds the zip archive.
    ///
    /// # Panics
    /// Never in practice: writing to an in-memory buffer cannot fail.
    #[must_use]
    pub fn build(&self) -> Vec<u8> {
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let opts = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        if let Some(folder) = &self.folder {
            zip.add_directory(folder.as_str(), opts)
                .expect("in-memory zip");
        }
        for sheet in Sheet::ALL {
            if self.omit.contains(&sheet) {
                continue;
            }
            let data = if self.corrupt == Some(sheet) {
                b"garbage".to_vec()
            } else {
                bmp(sheet)
            };
            zip.start_file(self.name(sheet.file_name()), opts)
                .expect("in-memory zip");
            zip.write_all(&data).expect("in-memory zip");
        }
        if self.text_files {
            zip.start_file(self.name("pledit.txt"), opts)
                .expect("in-memory zip");
            zip.write_all(b"[Text]\r\nNormal=#10E010\r\nCurrent=#FFFFFF\r\nNormalBG=#000000\r\nSelectedBG=#203060\r\nFont=Arial\r\n")
                .expect("in-memory zip");
            zip.start_file(self.name("viscolor.txt"), opts)
                .expect("in-memory zip");
            let mut vis = String::new();
            for i in 0..24u32 {
                vis.push_str(&format!(
                    "{},{},{}, // colour {i}\n",
                    i * 10,
                    255 - i * 10,
                    64
                ));
            }
            zip.write_all(vis.as_bytes()).expect("in-memory zip");
        }
        zip.finish().expect("in-memory zip").into_inner()
    }
}
