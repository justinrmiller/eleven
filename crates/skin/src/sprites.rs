//! Sprite coordinates inside the classic skin bitmaps.
//!
//! These follow the de-facto layout of Winamp 2 / XMMS skins (as documented by the skinning
//! community). Coordinates are in source-bitmap pixels.

/// The bitmaps a classic skin can contain.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Sheet {
    /// `main.bmp` – main window background.
    Main,
    /// `titlebar.bmp` – title bars and their buttons.
    Titlebar,
    /// `cbuttons.bmp` – transport buttons.
    Cbuttons,
    /// `numbers.bmp` – clock digits.
    Numbers,
    /// `nums_ex.bmp` – extended clock digits (with a real minus sign).
    NumsEx,
    /// `text.bmp` – the 5×6 bitmap font.
    Text,
    /// `playpaus.bmp` – play/pause/stop indicator.
    Playpaus,
    /// `monoster.bmp` – mono/stereo lights.
    Monoster,
    /// `posbar.bmp` – seek bar.
    Posbar,
    /// `volume.bmp` – volume slider frames.
    Volume,
    /// `balance.bmp` – balance slider frames.
    Balance,
    /// `shufrep.bmp` – shuffle/repeat/EQ/PL buttons.
    Shufrep,
    /// `eqmain.bmp` – equaliser window.
    Eqmain,
    /// `pledit.bmp` – playlist frame.
    Pledit,
}

impl Sheet {
    /// Every sheet.
    pub const ALL: [Self; 14] = [
        Self::Main,
        Self::Titlebar,
        Self::Cbuttons,
        Self::Numbers,
        Self::NumsEx,
        Self::Text,
        Self::Playpaus,
        Self::Monoster,
        Self::Posbar,
        Self::Volume,
        Self::Balance,
        Self::Shufrep,
        Self::Eqmain,
        Self::Pledit,
    ];

    /// Sheets the main window cannot be drawn without (digits come from either numbers sheet).
    pub const REQUIRED: [Self; 9] = [
        Self::Main,
        Self::Titlebar,
        Self::Cbuttons,
        Self::Text,
        Self::Playpaus,
        Self::Monoster,
        Self::Posbar,
        Self::Volume,
        Self::Shufrep,
    ];

    /// The lower-case file name inside the archive.
    #[must_use]
    pub fn file_name(self) -> &'static str {
        match self {
            Self::Main => "main.bmp",
            Self::Titlebar => "titlebar.bmp",
            Self::Cbuttons => "cbuttons.bmp",
            Self::Numbers => "numbers.bmp",
            Self::NumsEx => "nums_ex.bmp",
            Self::Text => "text.bmp",
            Self::Playpaus => "playpaus.bmp",
            Self::Monoster => "monoster.bmp",
            Self::Posbar => "posbar.bmp",
            Self::Volume => "volume.bmp",
            Self::Balance => "balance.bmp",
            Self::Shufrep => "shufrep.bmp",
            Self::Eqmain => "eqmain.bmp",
            Self::Pledit => "pledit.bmp",
        }
    }

    /// The size a well-formed sheet has (used to build synthetic skins and sanity checks).
    #[must_use]
    pub fn nominal_size(self) -> (u32, u32) {
        match self {
            Self::Main => (275, 116),
            Self::Titlebar => (344, 87),
            Self::Cbuttons => (136, 36),
            Self::Numbers => (99, 13),
            Self::NumsEx => (108, 13),
            Self::Text => (155, 18),
            Self::Playpaus => (42, 9),
            Self::Monoster => (58, 24),
            Self::Posbar => (307, 10),
            Self::Volume | Self::Balance => (68, 433),
            Self::Shufrep => (92, 85),
            Self::Eqmain => (275, 315),
            Self::Pledit => (280, 186),
        }
    }
}

/// A rectangle inside a sheet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sprite {
    /// The source sheet.
    pub sheet: Sheet,
    /// Left edge.
    pub x: u32,
    /// Top edge.
    pub y: u32,
    /// Width.
    pub w: u32,
    /// Height.
    pub h: u32,
}

const fn s(sheet: Sheet, x: u32, y: u32, w: u32, h: u32) -> Sprite {
    Sprite { sheet, x, y, w, h }
}

/// The main window background.
pub const MAIN: Sprite = s(Sheet::Main, 0, 0, 275, 116);

/// Title bars.
pub mod title {
    use super::{Sheet::Titlebar as T, Sprite, s};
    /// Active main title bar.
    pub const ACTIVE: Sprite = s(T, 27, 0, 275, 14);
    /// Inactive main title bar.
    pub const INACTIVE: Sprite = s(T, 27, 15, 275, 14);
    /// Active shade-mode bar.
    pub const SHADE_ACTIVE: Sprite = s(T, 27, 29, 275, 14);
    /// Inactive shade-mode bar.
    pub const SHADE_INACTIVE: Sprite = s(T, 27, 42, 275, 14);
    /// Menu button (normal, pressed).
    pub const MENU: [Sprite; 2] = [s(T, 0, 0, 9, 9), s(T, 0, 9, 9, 9)];
    /// Minimise button.
    pub const MINIMIZE: [Sprite; 2] = [s(T, 9, 0, 9, 9), s(T, 9, 9, 9, 9)];
    /// Close button.
    pub const CLOSE: [Sprite; 2] = [s(T, 18, 0, 9, 9), s(T, 18, 9, 9, 9)];
    /// Shade button.
    pub const SHADE: [Sprite; 2] = [s(T, 0, 18, 9, 9), s(T, 9, 18, 9, 9)];
    /// Unshade button (shown in shade mode).
    pub const UNSHADE: [Sprite; 2] = [s(T, 0, 27, 9, 9), s(T, 9, 27, 9, 9)];
}

/// Transport buttons, `[normal, pressed]`.
pub mod cbuttons {
    use super::{Sheet::Cbuttons as C, Sprite, s};
    /// Previous.
    pub const PREVIOUS: [Sprite; 2] = [s(C, 0, 0, 23, 18), s(C, 0, 18, 23, 18)];
    /// Play.
    pub const PLAY: [Sprite; 2] = [s(C, 23, 0, 23, 18), s(C, 23, 18, 23, 18)];
    /// Pause.
    pub const PAUSE: [Sprite; 2] = [s(C, 46, 0, 23, 18), s(C, 46, 18, 23, 18)];
    /// Stop.
    pub const STOP: [Sprite; 2] = [s(C, 69, 0, 23, 18), s(C, 69, 18, 23, 18)];
    /// Next.
    pub const NEXT: [Sprite; 2] = [s(C, 92, 0, 22, 18), s(C, 92, 18, 22, 18)];
    /// Eject.
    pub const EJECT: [Sprite; 2] = [s(C, 114, 0, 22, 16), s(C, 114, 16, 22, 16)];
}

/// A clock digit (`0..=9`; anything else is the blank cell).
#[must_use]
pub fn digit(sheet: Sheet, n: u8) -> Sprite {
    let index = u32::from(n.min(10));
    s(sheet, index * 9, 0, 9, 13)
}

/// The clock's minus sign: a full cell in `nums_ex.bmp`, a dash taken from `numbers.bmp`.
#[must_use]
pub fn minus(sheet: Sheet) -> Sprite {
    match sheet {
        Sheet::NumsEx => s(sheet, 99, 0, 9, 13),
        _ => s(sheet, 20, 6, 5, 1),
    }
}

/// Play/pause/stop indicator.
pub mod playpaus {
    use super::{Sheet::Playpaus as P, Sprite, s};
    /// Playing.
    pub const PLAYING: Sprite = s(P, 0, 0, 9, 9);
    /// Paused.
    pub const PAUSED: Sprite = s(P, 9, 0, 9, 9);
    /// Stopped.
    pub const STOPPED: Sprite = s(P, 18, 0, 9, 9);
}

/// Mono/stereo lights, `[on, off]`.
pub mod monoster {
    use super::{Sheet::Monoster as M, Sprite, s};
    /// Stereo.
    pub const STEREO: [Sprite; 2] = [s(M, 0, 0, 29, 12), s(M, 0, 12, 29, 12)];
    /// Mono.
    pub const MONO: [Sprite; 2] = [s(M, 29, 0, 27, 12), s(M, 29, 12, 27, 12)];
}

/// Seek bar.
pub mod posbar {
    use super::{Sheet::Posbar as P, Sprite, s};
    /// Background.
    pub const BACKGROUND: Sprite = s(P, 0, 0, 248, 10);
    /// Thumb, `[normal, pressed]`.
    pub const THUMB: [Sprite; 2] = [s(P, 248, 0, 29, 10), s(P, 278, 0, 29, 10)];
}

/// Number of background frames in the volume and balance sheets.
pub const SLIDER_FRAMES: u32 = 28;

/// Volume background frame for `value` in `0.0..=1.0`.
#[must_use]
pub fn volume_frame(value: f32) -> Sprite {
    s(Sheet::Volume, 0, frame_index(value) * 15, 68, 13)
}

/// Balance background frame for `value` in `-1.0..=1.0` (frames are symmetric).
#[must_use]
pub fn balance_frame(sheet: Sheet, value: f32) -> Sprite {
    let x = if sheet == Sheet::Balance { 9 } else { 15 };
    s(sheet, x, frame_index(value.abs()) * 15, 38, 13)
}

fn frame_index(value: f32) -> u32 {
    let v = if value.is_finite() {
        value.clamp(0.0, 1.0)
    } else {
        0.0
    };
    // The product is within 0..=27.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let i = (v * (SLIDER_FRAMES - 1) as f32).round() as u32;
    i
}

/// Volume/balance thumb, `[normal, pressed]`.
#[must_use]
pub fn slider_thumb(sheet: Sheet) -> [Sprite; 2] {
    [s(sheet, 15, 422, 14, 11), s(sheet, 0, 422, 14, 11)]
}

/// Shuffle/repeat/EQ/PL buttons, indexed `[on as usize][pressed as usize]`.
pub mod shufrep {
    use super::{Sheet::Shufrep as S, Sprite, s};
    /// Repeat.
    pub const REPEAT: [[Sprite; 2]; 2] = [
        [s(S, 0, 0, 28, 15), s(S, 0, 15, 28, 15)],
        [s(S, 0, 30, 28, 15), s(S, 0, 45, 28, 15)],
    ];
    /// Shuffle.
    pub const SHUFFLE: [[Sprite; 2]; 2] = [
        [s(S, 28, 0, 47, 15), s(S, 28, 15, 47, 15)],
        [s(S, 28, 30, 47, 15), s(S, 28, 45, 47, 15)],
    ];
    /// EQ window toggle.
    pub const EQ: [[Sprite; 2]; 2] = [
        [s(S, 0, 61, 23, 12), s(S, 46, 61, 23, 12)],
        [s(S, 0, 73, 23, 12), s(S, 46, 73, 23, 12)],
    ];
    /// Playlist toggle.
    pub const PL: [[Sprite; 2]; 2] = [
        [s(S, 23, 61, 23, 12), s(S, 69, 61, 23, 12)],
        [s(S, 23, 73, 23, 12), s(S, 69, 73, 23, 12)],
    ];
}

/// The equaliser window.
pub mod eqmain {
    use super::{SLIDER_FRAMES, Sheet::Eqmain as E, Sprite, s};
    /// Background.
    pub const BACKGROUND: Sprite = s(E, 0, 0, 275, 116);
    /// Title bar, `[active, inactive]`.
    pub const TITLE: [Sprite; 2] = [s(E, 0, 134, 275, 14), s(E, 0, 149, 275, 14)];
    /// "ON" button, `[on as usize][pressed as usize]`.
    pub const ON: [[Sprite; 2]; 2] = [
        [s(E, 10, 119, 26, 12), s(E, 128, 119, 26, 12)],
        [s(E, 69, 119, 26, 12), s(E, 187, 119, 26, 12)],
    ];
    /// "AUTO" button.
    pub const AUTO: [[Sprite; 2]; 2] = [
        [s(E, 36, 119, 32, 12), s(E, 154, 119, 32, 12)],
        [s(E, 95, 119, 32, 12), s(E, 213, 119, 32, 12)],
    ];
    /// "PRESETS" button, `[normal, pressed]`.
    pub const PRESETS: [Sprite; 2] = [s(E, 224, 164, 44, 12), s(E, 224, 176, 44, 12)];
    /// Slider thumb, `[normal, pressed]`.
    pub const THUMB: [Sprite; 2] = [s(E, 0, 164, 11, 11), s(E, 0, 176, 11, 11)];
    /// Response graph background.
    pub const GRAPH: Sprite = s(E, 0, 294, 113, 19);
    /// The 19-pixel colour ramp used for the graph line (top to bottom).
    pub const GRAPH_COLORS: Sprite = s(E, 115, 294, 1, 19);

    /// Slider background frame for `db` in `-12..=12` (28 frames, top = +12).
    #[must_use]
    pub fn slider_frame(db: f32) -> Sprite {
        let t = if db.is_finite() {
            ((db + 12.0) / 24.0).clamp(0.0, 1.0)
        } else {
            0.5
        };
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let i = (t * (SLIDER_FRAMES - 1) as f32).round() as u32;
        s(E, 13 + (i % 14) * 15, 164 + (i / 14) * 65, 14, 63)
    }
}

/// The playlist frame pieces.
pub mod pledit {
    use super::{Sheet::Pledit as P, Sprite, s};
    /// Top-left corner, `[active, inactive]`.
    pub const TOP_LEFT: [Sprite; 2] = [s(P, 0, 0, 25, 20), s(P, 0, 21, 25, 20)];
    /// Title piece.
    pub const TITLE: [Sprite; 2] = [s(P, 26, 0, 100, 20), s(P, 26, 21, 100, 20)];
    /// Top filler tile.
    pub const TOP_TILE: [Sprite; 2] = [s(P, 127, 0, 25, 20), s(P, 127, 21, 25, 20)];
    /// Top-right corner.
    pub const TOP_RIGHT: [Sprite; 2] = [s(P, 153, 0, 25, 20), s(P, 153, 21, 25, 20)];
    /// Left edge tile.
    pub const LEFT_TILE: Sprite = s(P, 0, 42, 12, 29);
    /// Right edge tile (with the scrollbar groove).
    pub const RIGHT_TILE: Sprite = s(P, 31, 42, 20, 29);
    /// Bottom-left corner (with the ADD/REM/SEL/MISC buttons).
    pub const BOTTOM_LEFT: Sprite = s(P, 0, 72, 125, 38);
    /// Bottom-right corner (with the LIST button and time display).
    pub const BOTTOM_RIGHT: Sprite = s(P, 126, 72, 150, 38);
    /// Bottom filler tile.
    pub const BOTTOM_TILE: Sprite = s(P, 179, 0, 25, 38);
    /// Scrollbar thumb, `[normal, pressed]`.
    pub const THUMB: [Sprite; 2] = [s(P, 52, 53, 8, 18), s(P, 61, 53, 8, 18)];
}

/// Position of a character in `text.bmp` as `(column, row)`; unknown characters map to space.
#[must_use]
pub fn text_glyph(ch: char) -> Sprite {
    const ROW1: &str = "0123456789\u{2026}.:()-'!_+\\/[]^&%,=$#";
    let lower = ch.to_ascii_lowercase();
    let (col, row) = if lower.is_ascii_lowercase() {
        (u32::from(lower as u8 - b'a'), 0)
    } else if let Some(i) = ROW1.chars().position(|c| c == ch) {
        // `position` is < 31.
        #[allow(clippy::cast_possible_truncation)]
        let i = i as u32;
        (i, 1)
    } else {
        match ch {
            '"' => (26, 0),
            '@' => (27, 0),
            '\u{c5}' | '\u{e5}' => (0, 2),
            '\u{d6}' | '\u{f6}' => (1, 2),
            '\u{c4}' | '\u{e4}' => (2, 2),
            '?' => (3, 2),
            '*' => (4, 2),
            '<' | '{' => (13, 1),
            '>' | '}' => (14, 1),
            '~' => (15, 1),
            '|' => (21, 1),
            ';' => (12, 1),
            _ => (30, 0),
        }
    };
    s(Sheet::Text, col * 5, row * 6, 5, 6)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inside(sp: Sprite) -> bool {
        let (w, h) = sp.sheet.nominal_size();
        sp.x + sp.w <= w && sp.y + sp.h <= h
    }

    #[test]
    fn all_sprites_fit_their_nominal_sheets() {
        let mut all = vec![
            MAIN,
            title::ACTIVE,
            title::INACTIVE,
            title::SHADE_ACTIVE,
            title::SHADE_INACTIVE,
        ];
        all.extend(title::MENU);
        all.extend(title::MINIMIZE);
        all.extend(title::CLOSE);
        all.extend(title::SHADE);
        all.extend(title::UNSHADE);
        for b in [
            cbuttons::PREVIOUS,
            cbuttons::PLAY,
            cbuttons::PAUSE,
            cbuttons::STOP,
            cbuttons::NEXT,
            cbuttons::EJECT,
        ] {
            all.extend(b);
        }
        for d in 0..=10 {
            all.push(digit(Sheet::Numbers, d));
            all.push(digit(Sheet::NumsEx, d));
        }
        all.extend([minus(Sheet::Numbers), minus(Sheet::NumsEx)]);
        all.extend([playpaus::PLAYING, playpaus::PAUSED, playpaus::STOPPED]);
        all.extend(monoster::STEREO);
        all.extend(monoster::MONO);
        all.push(posbar::BACKGROUND);
        all.extend(posbar::THUMB);
        for v in [0.0, 0.5, 1.0, 7.0, f32::NAN] {
            all.push(volume_frame(v));
            all.push(balance_frame(Sheet::Balance, v - 0.5));
            all.push(balance_frame(Sheet::Volume, v));
            all.push(eqmain::slider_frame(v * 24.0 - 12.0));
        }
        all.extend(slider_thumb(Sheet::Volume));
        all.extend(slider_thumb(Sheet::Balance));
        for b in [
            shufrep::REPEAT,
            shufrep::SHUFFLE,
            shufrep::EQ,
            shufrep::PL,
            eqmain::ON,
            eqmain::AUTO,
        ] {
            all.extend(b.into_iter().flatten());
        }
        all.extend([eqmain::BACKGROUND, eqmain::GRAPH, eqmain::GRAPH_COLORS]);
        all.extend(eqmain::TITLE);
        all.extend(eqmain::PRESETS);
        all.extend(eqmain::THUMB);
        for p in [
            pledit::TOP_LEFT,
            pledit::TITLE,
            pledit::TOP_TILE,
            pledit::TOP_RIGHT,
            pledit::THUMB,
        ] {
            all.extend(p);
        }
        all.extend([
            pledit::LEFT_TILE,
            pledit::RIGHT_TILE,
            pledit::BOTTOM_LEFT,
            pledit::BOTTOM_RIGHT,
            pledit::BOTTOM_TILE,
        ]);
        for ch in "The quick brown fox: 0123456789 (it goes to eleven!) <>?*@\"~|;\u{e5}\u{f6}\u{e4}\u{263a}".chars() {
            all.push(text_glyph(ch));
        }
        for sp in all {
            assert!(inside(sp), "{sp:?} outside {:?}", sp.sheet.nominal_size());
        }
    }

    #[test]
    fn glyph_lookup() {
        assert_eq!((text_glyph('a').x, text_glyph('a').y), (0, 0));
        assert_eq!(text_glyph('Z'), text_glyph('z'));
        assert_eq!((text_glyph('0').x, text_glyph('0').y), (0, 6));
        assert_eq!((text_glyph(':').x, text_glyph(':').y), (60, 6));
        assert_eq!(
            text_glyph('\u{263a}'),
            text_glyph(' '),
            "unknown glyphs are spaces"
        );
        assert_eq!(text_glyph(' ').x, 150);
    }

    #[test]
    fn frames_follow_values() {
        assert_eq!(volume_frame(0.0).y, 0);
        assert_eq!(volume_frame(1.0).y, 27 * 15);
        assert_eq!(
            balance_frame(Sheet::Balance, -1.0),
            balance_frame(Sheet::Balance, 1.0)
        );
        assert_eq!(
            eqmain::slider_frame(-12.0),
            s(Sheet::Eqmain, 13, 164, 14, 63)
        );
        assert_eq!(
            eqmain::slider_frame(12.0),
            s(Sheet::Eqmain, 13 + 13 * 15, 164 + 65, 14, 63)
        );
        assert_eq!(digit(Sheet::Numbers, 3).x, 27);
        assert_eq!(digit(Sheet::Numbers, 99).x, 90, "out of range -> blank");
    }

    #[test]
    fn sheet_metadata() {
        assert_eq!(Sheet::ALL.len(), 14);
        assert!(Sheet::REQUIRED.iter().all(|s| Sheet::ALL.contains(s)));
        let names: std::collections::HashSet<_> =
            Sheet::ALL.iter().map(|s| s.file_name()).collect();
        assert_eq!(names.len(), 14);
    }
}
