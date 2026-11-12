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
    let hex = text.trim().trim_start_matches('#');
    if hex.len() < 6 || !hex.is_char_boundary(6) {
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

/// Parses `viscolor.txt`: up to 24 lines of `r,g,b` with optional `// comments`.
/// Returns `None` if fewer than 24 colours could be read.
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
    (n == VIS_COLORS).then_some(out)
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
        assert_eq!(parse_hex_color("#\u{e9}\u{e9}\u{e9}\u{e9}"), None);
    }

    #[test]
    fn pledit_parsing() {
        let c = parse_pledit(
            "[Text]\r\nNormal=#112233\r\nCurrent= #FFFFFF \r\nNormalBG=#000000\r\nselectedbg=#0000FF\r\nFont=Arial\r\nbogus\r\nNormal=oops\r\n",
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
        // Space-separated and tab-separated values work too.
        let spaced: String = (0..24).map(|_| "10 20\t30\n").collect();
        assert_eq!(parse_viscolor(&spaced).unwrap()[5], Rgb(10, 20, 30));
    }
}
