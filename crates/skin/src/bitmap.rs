//! Lenient decoding of run-length-encoded (RLE4 / RLE8) BMP files.
//!
//! Skins were saved by many old tools whose RLE encoders produced slightly malformed data:
//! runs that overrun odd-width rows by a pixel, or a missing end-of-bitmap marker. Winamp and
//! browsers clip such data; the `image` crate rejects the whole file. This decoder takes the
//! forgiving route and is used when `image` refuses an RLE bitmap.

use crate::Bitmap;

const BI_RLE8: u32 = 1;
const BI_RLE4: u32 = 2;
/// Refuse absurd dimensions instead of allocating for them.
const MAX_SIDE: u32 = 4096;

fn u16_at(d: &[u8], i: usize) -> Option<u16> {
    d.get(i..i + 2).map(|b| u16::from_le_bytes([b[0], b[1]]))
}

fn u32_at(d: &[u8], i: usize) -> Option<u32> {
    d.get(i..i + 4)
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

fn i32_at(d: &[u8], i: usize) -> Option<i32> {
    d.get(i..i + 4)
        .map(|b| i32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

/// The `k`-th 4- or 8-bit index inside packed `bytes`.
fn nibble_or_byte(bytes: &[u8], k: usize, rle4: bool) -> u8 {
    if rle4 {
        let byte = bytes[k / 2];
        if k.is_multiple_of(2) {
            byte >> 4
        } else {
            byte & 0x0f
        }
    } else {
        bytes[k]
    }
}

/// Decodes an RLE4 or RLE8 BMP, clipping pixels that fall outside the image and stopping
/// quietly at the end of the data. Returns `None` for anything that is not an RLE BMP with a
/// usable header.
#[must_use]
pub fn decode_rle_bmp(data: &[u8]) -> Option<Bitmap> {
    if data.get(..2)? != b"BM" {
        return None;
    }
    let pixel_offset = usize::try_from(u32_at(data, 10)?).ok()?;
    let header_size = usize::try_from(u32_at(data, 14)?).ok()?;
    let width = i32_at(data, 18)?;
    let height = i32_at(data, 22)?;
    let bpp = u16_at(data, 28)?;
    let rle4 = match (u32_at(data, 30)?, bpp) {
        (BI_RLE8, 8) => false,
        (BI_RLE4, 4) => true,
        _ => return None,
    };
    let w = u32::try_from(width)
        .ok()
        .filter(|&w| (1..=MAX_SIDE).contains(&w))?;
    let h = Some(height.unsigned_abs()).filter(|&h| (1..=MAX_SIDE).contains(&h))?;
    let (wu, hu) = (usize::try_from(w).ok()?, usize::try_from(h).ok()?);
    let bottom_up = height > 0;

    // Palette: BGRx entries after the DIB header.
    let colors_used = u32_at(data, 46)
        .and_then(|c| usize::try_from(c).ok())
        .unwrap_or(0);
    let palette_len = if colors_used == 0 {
        1 << bpp
    } else {
        colors_used.min(256)
    };
    let palette_start = 14 + header_size;
    let palette: Vec<[u8; 4]> = (0..palette_len)
        .map(|i| {
            let at = palette_start + 4 * i;
            data.get(at..at + 3)
                .map_or([0, 0, 0, 255], |c| [c[2], c[1], c[0], 255])
        })
        .collect();

    // Decode into palette indices; row 0 is the first row in the file.
    let mut idx = vec![0u8; wu * hu];
    let mut put = |x: usize, row: usize, v: u8| {
        if x < wu && row < hu {
            idx[row * wu + x] = v;
        }
    };
    let stream = data.get(pixel_offset..)?;
    let (mut i, mut x, mut row) = (0usize, 0usize, 0usize);
    while i + 1 < stream.len() && row < hu {
        let (count, value) = (usize::from(stream[i]), stream[i + 1]);
        i += 2;
        if count > 0 {
            // Encoded run: `count` pixels of one byte (RLE8) or alternating nibbles (RLE4).
            let pair = [value];
            for k in 0..count {
                put(
                    x,
                    row,
                    nibble_or_byte(&pair, if rle4 { k % 2 } else { 0 }, rle4),
                );
                x += 1;
            }
            continue;
        }
        match value {
            0 => {
                x = 0;
                row += 1;
            }
            1 => break,
            2 => {
                let (Some(&dx), Some(&dy)) = (stream.get(i), stream.get(i + 1)) else {
                    break;
                };
                i += 2;
                x += usize::from(dx);
                row += usize::from(dy);
            }
            n => {
                // Absolute run of `n` literal pixels, padded to a 16-bit boundary.
                let n = usize::from(n);
                let bytes = if rle4 { n.div_ceil(2) } else { n };
                let Some(run) = stream.get(i..i + bytes) else {
                    break;
                };
                for k in 0..n {
                    put(x, row, nibble_or_byte(run, k, rle4));
                    x += 1;
                }
                i += bytes + (bytes & 1);
            }
        }
    }

    let mut rgba = Vec::with_capacity(wu * hu * 4);
    for out_row in 0..hu {
        let src = if bottom_up { hu - 1 - out_row } else { out_row };
        for px in &idx[src * wu..(src + 1) * wu] {
            rgba.extend_from_slice(
                &palette
                    .get(usize::from(*px))
                    .copied()
                    .unwrap_or([0, 0, 0, 255]),
            );
        }
    }
    Some(Bitmap {
        width: w,
        height: h,
        rgba,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds a BMP with the given compression, palette and pixel stream.
    fn bmp(
        width: i32,
        height: i32,
        bpp: u16,
        compression: u32,
        palette: &[[u8; 3]],
        stream: &[u8],
    ) -> Vec<u8> {
        let header = 40u32;
        let offset = 14 + header + 4 * u32::try_from(palette.len()).unwrap();
        let mut d = Vec::new();
        d.extend_from_slice(b"BM");
        d.extend_from_slice(&(offset + u32::try_from(stream.len()).unwrap()).to_le_bytes());
        d.extend_from_slice(&[0; 4]);
        d.extend_from_slice(&offset.to_le_bytes());
        d.extend_from_slice(&header.to_le_bytes());
        d.extend_from_slice(&width.to_le_bytes());
        d.extend_from_slice(&height.to_le_bytes());
        d.extend_from_slice(&1u16.to_le_bytes());
        d.extend_from_slice(&bpp.to_le_bytes());
        d.extend_from_slice(&compression.to_le_bytes());
        d.extend_from_slice(&u32::try_from(stream.len()).unwrap().to_le_bytes());
        d.extend_from_slice(&[0; 8]);
        d.extend_from_slice(&u32::try_from(palette.len()).unwrap().to_le_bytes());
        d.extend_from_slice(&[0; 4]);
        for [r, g, b] in palette {
            d.extend_from_slice(&[*b, *g, *r, 0]);
        }
        d.extend_from_slice(stream);
        d
    }

    const PAL: [[u8; 3]; 3] = [[0, 0, 0], [255, 0, 0], [0, 0, 255]];

    fn pixel(b: &Bitmap, x: u32, y: u32) -> [u8; 4] {
        b.pixel(x, y).unwrap()
    }

    #[test]
    fn rle4_with_rows_that_overrun_an_odd_width() {
        // 3 pixels wide, 2 rows, bottom-up. Each row is written as two 2-pixel runs, so the
        // last pixel spills past the row end, exactly like the Windows XP skin's text.bmp.
        let stream = [2, 0x12, 2, 0x12, 0, 0, 2, 0x21, 2, 0x21, 0, 1];
        let data = bmp(3, 2, 4, BI_RLE4, &PAL, &stream);
        assert!(
            image::load_from_memory(&data).is_err(),
            "the image crate rejects this file"
        );
        let b = decode_rle_bmp(&data).unwrap();
        assert_eq!((b.width, b.height), (3, 2));
        // File row 0 is the bottom row.
        assert_eq!(pixel(&b, 0, 1), [255, 0, 0, 255]);
        assert_eq!(pixel(&b, 1, 1), [0, 0, 255, 255]);
        assert_eq!(pixel(&b, 2, 1), [255, 0, 0, 255]);
        assert_eq!(pixel(&b, 0, 0), [0, 0, 255, 255]);
        assert_eq!(pixel(&b, 1, 0), [255, 0, 0, 255]);
    }

    #[test]
    fn rle8_absolute_runs_deltas_and_missing_end_marker() {
        // 4x3, top-down (negative height). Row 0: absolute run [1,2,1] (padded) + an encoded run
        // of one 2. End of line, a delta of (+1, +1) to (1, 2), and a run of two 1s; the stream
        // ends without an end-of-bitmap marker.
        let stream = [0, 3, 1, 2, 1, 0, 1, 2, 0, 0, 0, 2, 1, 1, 2, 1];
        let data = bmp(4, -3, 8, BI_RLE8, &PAL, &stream);
        let b = decode_rle_bmp(&data).unwrap();
        assert_eq!(pixel(&b, 0, 0), [255, 0, 0, 255]);
        assert_eq!(pixel(&b, 1, 0), [0, 0, 255, 255]);
        assert_eq!(pixel(&b, 3, 0), [0, 0, 255, 255]);
        assert_eq!(pixel(&b, 0, 2), [0, 0, 0, 255], "skipped by the delta");
        assert_eq!(pixel(&b, 1, 2), [255, 0, 0, 255]);
        assert_eq!(pixel(&b, 2, 2), [255, 0, 0, 255]);
    }

    #[test]
    fn rle4_absolute_run_and_truncated_data() {
        // Absolute run of 3 nibbles (2 bytes, already word aligned), then a truncated run.
        let stream = [0, 3, 0x12, 0x10, 0, 5, 0x11];
        let data = bmp(4, 1, 4, BI_RLE4, &PAL, &stream);
        let b = decode_rle_bmp(&data).unwrap();
        assert_eq!(pixel(&b, 0, 0), [255, 0, 0, 255]);
        assert_eq!(pixel(&b, 1, 0), [0, 0, 255, 255]);
        assert_eq!(pixel(&b, 2, 0), [255, 0, 0, 255]);
        assert_eq!(pixel(&b, 3, 0), [0, 0, 0, 255]);
    }

    #[test]
    fn non_rle_and_garbage_are_not_handled() {
        assert!(decode_rle_bmp(b"not a bitmap").is_none());
        assert!(decode_rle_bmp(b"BM").is_none());
        assert!(
            decode_rle_bmp(&bmp(2, 2, 24, 0, &[], &[0; 16])).is_none(),
            "uncompressed"
        );
        assert!(
            decode_rle_bmp(&bmp(2, 2, 8, BI_RLE4, &PAL, &[0, 1])).is_none(),
            "bpp mismatch"
        );
        assert!(
            decode_rle_bmp(&bmp(0, 2, 8, BI_RLE8, &PAL, &[0, 1])).is_none(),
            "zero width"
        );
        assert!(
            decode_rle_bmp(&bmp(9000, 2, 8, BI_RLE8, &PAL, &[0, 1])).is_none(),
            "too large"
        );
        let b = decode_rle_bmp(&bmp(2, 1, 8, BI_RLE8, &[], &[2, 7, 0, 2])).unwrap();
        assert_eq!(
            pixel(&b, 0, 0),
            [0, 0, 0, 255],
            "missing palette entries are black"
        );
        let b = decode_rle_bmp(&bmp(2, 1, 8, BI_RLE8, &PAL, &[0, 2, 9])).unwrap();
        assert_eq!(
            pixel(&b, 1, 0),
            [0, 0, 0, 255],
            "truncated delta stops cleanly"
        );
    }
}
