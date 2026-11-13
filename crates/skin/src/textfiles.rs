//! Parsers for the text files inside a skin: `pledit.txt` and `viscolor.txt`.

/// An RGB colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Rgb(pub u8, pub u8, pub u8);

/// Playlist colours from `pledit.txt`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaylistColors {
    /// Normal entry text.
    pub normal: Rgb,
    /// Current entry text.
    pub current: Rgb,
    /// List background.
    pub normal_bg: Rgb,
    /// Selected entry background.
    pub selected_bg: Rgb,
    /// Requested font family (informational).
    pub font: Option<String>,
}

impl Default for PlaylistColors {
    fn default() -> Self {
        Self {
            normal: Rgb(0x00, 0xff, 0x00),
            current: Rgb(0xff, 0xff, 0xff),
            normal_bg: Rgb(0x00, 0x00, 0x00),
            selected_bg: Rgb(0x00, 0x00, 0xc6),
            font: None,
        }
    }
}

/// Parses `#rrggbb` (the `#` is optional).
#[must_use]
pub fn parse_hex_color(text: &str) -> Option<Rgb> {
    // Only ASCII hex digits are valid; checking first also keeps the byte slicing below on
    // character boundaries (`#aé1234` used to panic).
    let hex = text.trim().trim_start_matches('#').get(..6)?;
    if !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let byte = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok();
    Some(Rgb(byte(0)?, byte(2)?, byte(4)?))
}

/// Parses `pledit.txt`. Missing or malformed keys keep their defaults.
#[must_use]
pub fn parse_pledit(text: &str) -> PlaylistColors {
    let mut colors = PlaylistColors::default();
    for line in text.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value = value.trim();
        match key.trim().to_ascii_lowercase().as_str() {
            "normal" => colors.normal = parse_hex_color(value).unwrap_or(colors.normal),
            "current" => colors.current = parse_hex_color(value).unwrap_or(colors.current),
            "normalbg" => colors.normal_bg = parse_hex_color(value).unwrap_or(colors.normal_bg),
            "selectedbg" => {
                colors.selected_bg = parse_hex_color(value).unwrap_or(colors.selected_bg);
            }
            "font" if !value.is_empty() => colors.font = Some(value.to_owned()),
            _ => {}
        }
    }
    colors
}

/// Number of colours in `viscolor.txt`.
pub const VIS_COLORS: usize = 24;

/// Colours needed for a usable palette: background, dots and the 16 spectrum rows.
pub const MIN_VIS_COLORS: usize = 18;

/// Peak-dot colour used when a palette omits it (the 24th entry).
pub const DEFAULT_PEAK: Rgb = Rgb(150, 150, 150);

/// Parses `viscolor.txt`: up to 24 lines of `r,g,b` with optional `// comments`.
///
/// Many skins ship short palettes (23 lines, without the peak colour, is common). As in
/// Winamp, missing entries get defaults: oscilloscope colours reuse the top spectrum colours
/// and the peak dots are grey. Returns `None` if fewer than [`MIN_VIS_COLORS`] colours are
/// present.
#[must_use]
pub fn parse_viscolor(text: &str) -> Option<[Rgb; VIS_COLORS]> {
    let mut out = [Rgb::default(); VIS_COLORS];
    let mut n = 0;
    for line in text.lines() {
        if n == VIS_COLORS {
            break;
        }
        let data = line.split("//").next().unwrap_or("");
        let nums: Vec<u8> = data
            .split(|c: char| !c.is_ascii_digit())
            .filter(|t| !t.is_empty())
            .filter_map(|t| t.parse::<u16>().ok())
            // Values above 255 are clamped rather than rejected.
            .map(|v| u8::try_from(v.min(255)).unwrap_or(255))
            .collect();
        if nums.len() >= 3 {
            out[n] = Rgb(nums[0], nums[1], nums[2]);
            n += 1;
        }
    }
    if n < MIN_VIS_COLORS {
        return None;
    }
    for i in n..VIS_COLORS {
        out[i] = if i == VIS_COLORS - 1 {
            DEFAULT_PEAK
        } else {
            out[2 + (i - MIN_VIS_COLORS)]
        };
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_colors() {
        assert_eq!(parse_hex_color("#00FF7f"), Some(Rgb(0, 255, 127)));
        assert_eq!(parse_hex_color("102030"), Some(Rgb(16, 32, 48)));
        assert_eq!(parse_hex_color("#12345"), None);
        assert_eq!(parse_hex_color("#zz0000"), None);
        assert_eq!(
            parse_hex_color("#a\u{e9}1234"),
            None,
            "non-ASCII must not panic"
        );
        assert_eq!(parse_hex_color("\u{e9}\u{e9}\u{e9}"), None);
        assert_eq!(
            parse_hex_color("#+12345"),
            None,
            "from_str_radix would accept a sign"
        );
        assert_eq!(
            parse_hex_color("#102030ff"),
            Some(Rgb(16, 32, 48)),
            "extra digits ignored"
        );
        assert_eq!(parse_hex_color("#\u{e9}\u{e9}\u{e9}\u{e9}"), None);
    }

    #[test]
    fn pledit_parsing() {
        let c = parse_pledit(
            "[Text]\r\nSelectedBG=#\u{e9}\u{e9}\u{e9}\r\nNormal=#112233\r\nCurrent= #FFFFFF \r\nNormalBG=#000000\r\nselectedbg=#0000FF\r\nFont=Arial\r\nbogus\r\nNormal=oops\r\n",
        );
        assert_eq!(
            c.normal,
            Rgb(0x11, 0x22, 0x33),
            "malformed later values are ignored"
        );
        assert_eq!(c.selected_bg, Rgb(0, 0, 255));
        assert_eq!(c.font.as_deref(), Some("Arial"));
        assert_eq!(parse_pledit(""), PlaylistColors::default());
    }

    #[test]
    fn viscolor_parsing() {
        let mut text = String::new();
        for i in 0..24 {
            text.push_str(&format!("{i},{},{} // colour {i}\n", i * 2, 300));
        }
        text.push_str("1,2,3\n");
        let colors = parse_viscolor(&text).unwrap();
        assert_eq!(colors[0], Rgb(0, 0, 255));
        assert_eq!(colors[23], Rgb(23, 46, 255));
        assert_eq!(parse_viscolor("1,2,3\n// only one\n4 5\n"), None);
        // Short palettes (23 lines is common) are completed with defaults.
        let short: String = (0..23).map(|i| format!("{i},{i},{i}\n")).collect();
        let c = parse_viscolor(&short).unwrap();
        assert_eq!(c[22], Rgb(22, 22, 22), "parsed entries are kept");
        assert_eq!(c[23], DEFAULT_PEAK);
        let minimal: String = (0..18).map(|i| format!("{i},0,0\n")).collect();
        let c = parse_viscolor(&minimal).unwrap();
        assert_eq!(
            c[18],
            Rgb(2, 0, 0),
            "scope colours reuse the top spectrum colours"
        );
        assert_eq!(c[22], Rgb(6, 0, 0));
        let too_short: String = (0..17).map(|i| format!("{i},0,0\n")).collect();
        assert_eq!(parse_viscolor(&too_short), None);
        // Space-separated and tab-separated values work too.
        let spaced: String = (0..24).map(|_| "10 20\t30\n").collect();
        assert_eq!(parse_viscolor(&spaced).unwrap()[5], Rgb(10, 20, 30));
    }
}
