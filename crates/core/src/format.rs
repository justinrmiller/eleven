//! Text formatting for the displays: clocks, durations, the scrolling title and indicators.

use std::time::Duration;

/// Whether the main clock shows elapsed or remaining time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TimeMode {
    /// Time since the start of the track.
    #[default]
    Elapsed,
    /// Time until the end of the track (shown with a leading minus).
    Remaining,
}

impl TimeMode {
    /// The other mode.
    #[must_use]
    pub fn toggled(self) -> Self {
        match self {
            Self::Elapsed => Self::Remaining,
            Self::Remaining => Self::Elapsed,
        }
    }
}

/// What the four-digit main clock should show.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClockDisplay {
    /// Show a leading minus sign (remaining-time mode).
    pub negative: bool,
    /// The four digits, as `MM:SS` or, past 99:59, `HH:MM`.
    pub digits: [u8; 4],
}

impl ClockDisplay {
    /// Computes the clock for `position` within a track of length `duration`.
    ///
    /// Remaining mode falls back to elapsed time when the duration is unknown (streams).
    #[must_use]
    pub fn new(position: Duration, duration: Option<Duration>, mode: TimeMode) -> Self {
        let (secs, negative) = match (mode, duration) {
            (TimeMode::Remaining, Some(total)) => (total.saturating_sub(position).as_secs(), true),
            _ => (position.as_secs(), false),
        };
        let (major, minor) = if secs < 100 * 60 {
            (secs / 60, secs % 60)
        } else {
            ((secs / 3600).min(99), (secs / 60) % 60)
        };
        // Both halves are < 100, so the casts are lossless.
        #[allow(clippy::cast_possible_truncation)]
        let digits = [
            (major / 10) as u8,
            (major % 10) as u8,
            (minor / 10) as u8,
            (minor % 10) as u8,
        ];
        Self { negative, digits }
    }
}

/// Formats a duration as `m:ss`, or `h:mm:ss` from one hour.
#[must_use]
pub fn format_duration(duration: Duration) -> String {
    let secs = duration.as_secs();
    let (h, m, s) = (secs / 3600, (secs / 60) % 60, secs % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

/// Formats an optional duration, using `--:--` when unknown.
#[must_use]
pub fn format_optional_duration(duration: Option<Duration>) -> String {
    duration.map_or_else(|| "--:--".to_owned(), format_duration)
}

/// The text the main window's marquee scrolls: `"1. Title (3:45)"`.
#[must_use]
pub fn marquee_text(number: Option<usize>, title: &str, duration: Option<Duration>) -> String {
    let mut text = match number {
        Some(n) => format!("{n}. {title}"),
        None => title.to_owned(),
    };
    if let Some(d) = duration {
        text.push_str(&format!(" ({})", format_duration(d)));
    }
    text
}

/// Separator inserted between repetitions of a scrolling title.
pub const MARQUEE_SEPARATOR: &str = "  ***  ";

/// A character-based scrolling text window, like the title display in XMMS.
#[derive(Debug, Clone, PartialEq)]
pub struct Marquee {
    chars: Vec<char>,
    offset: f32,
}

impl Marquee {
    /// Characters scrolled per second.
    pub const SPEED: f32 = 6.0;

    /// Creates a marquee for `text`, starting at its first character.
    #[must_use]
    pub fn new(text: &str) -> Self {
        Self {
            chars: text.chars().collect(),
            offset: 0.0,
        }
    }

    /// Replaces the text if it changed, restarting the scroll.
    pub fn set_text(&mut self, text: &str) {
        if !self.chars.iter().copied().eq(text.chars()) {
            *self = Self::new(text);
        }
    }

    /// The full (unscrolled) text.
    #[must_use]
    pub fn text(&self) -> String {
        self.chars.iter().collect()
    }

    /// Advances the scroll by `dt` seconds.
    pub fn tick(&mut self, dt: f32) {
        let cycle = self.cycle_len();
        if cycle == 0 {
            return;
        }
        #[allow(clippy::cast_precision_loss)]
        let cycle = cycle as f32;
        self.offset = (self.offset + dt.max(0.0) * Self::SPEED) % cycle;
    }

    fn cycle_len(&self) -> usize {
        self.chars.len() + MARQUEE_SEPARATOR.chars().count()
    }

    /// The `width` characters currently visible. Text that fits is shown unscrolled.
    #[must_use]
    pub fn visible(&self, width: usize, scrolling: bool) -> String {
        if !scrolling || self.chars.len() <= width {
            return self.chars.iter().take(width).collect();
        }
        let cycle: Vec<char> = self
            .chars
            .iter()
            .copied()
            .chain(MARQUEE_SEPARATOR.chars())
            .collect();
        // `offset` is always in `0..cycle.len()`, so truncation is the intent.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let start = (self.offset + 1e-3) as usize % cycle.len();
        cycle.iter().cycle().skip(start).take(width).collect()
    }
}

/// Seven-segment masks for the digits 0–9. Bit 0 is segment `a` (top), then clockwise
/// `b`, `c`, `d` (bottom), `e`, `f`, and bit 6 is `g` (middle).
pub const SEVEN_SEGMENT: [u8; 10] = [
    0b011_1111, // 0
    0b000_0110, // 1
    0b101_1011, // 2
    0b100_1111, // 3
    0b110_0110, // 4
    0b110_1101, // 5
    0b111_1101, // 6
    0b000_0111, // 7
    0b111_1111, // 8
    0b110_1111, // 9
];

/// The segment mask for `digit` (values above 9 render blank).
#[must_use]
pub fn seven_segment(digit: u8) -> u8 {
    SEVEN_SEGMENT.get(usize::from(digit)).copied().unwrap_or(0)
}

/// Bitrate indicator text: right-aligned, at most three digits (`"128"`, `" 96"`), or blank.
#[must_use]
pub fn kbps_text(kbps: Option<u32>) -> String {
    kbps.map_or_else(|| "   ".to_owned(), |k| format!("{:>3}", k.min(999)))
}

/// Sample-rate indicator text in kHz, two characters (`"44"`, `"48"`, `" 8"`), or blank.
#[must_use]
pub fn khz_text(sample_rate: Option<u32>) -> String {
    sample_rate.map_or_else(|| "  ".to_owned(), |r| format!("{:>2}", (r / 1000).min(99)))
}

/// Tooltip text for the volume slider, on a scale that goes to eleven.
#[must_use]
pub fn volume_tooltip(volume: f32) -> String {
    let eleven = (volume.clamp(0.0, 1.0) * 11.0 * 10.0).round() / 10.0;
    if eleven >= 11.0 {
        "Volume: 11 (one louder)".to_owned()
    } else {
        format!("Volume: {eleven:.1} / 11")
    }
}

/// Tooltip text for the balance slider.
#[must_use]
pub fn balance_tooltip(balance: f32) -> String {
    let b = balance.clamp(-1.0, 1.0);
    // The value is clamped to ±100, so the cast is exact enough.
    #[allow(clippy::cast_possible_truncation)]
    let pct = (b.abs() * 100.0).round() as i32;
    if pct == 0 {
        "Balance: center".to_owned()
    } else if b < 0.0 {
        format!("Balance: {pct}% left")
    } else {
        format!("Balance: {pct}% right")
    }
}

/// Playlist footer: `"selected/total"` time, e.g. `"3:45/1:02:03"`, with `+` for unknowns.
#[must_use]
pub fn playlist_time_text(
    selected: crate::playlist::TotalDuration,
    total: crate::playlist::TotalDuration,
) -> String {
    let part = |t: crate::playlist::TotalDuration| {
        let mut s = format_duration(t.known);
        if t.has_unknown {
            s.push('+');
        }
        s
    };
    format!("{}/{}", part(selected), part(total))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::playlist::TotalDuration;
    use proptest::prelude::*;

    fn s(secs: u64) -> Duration {
        Duration::from_secs(secs)
    }

    #[test]
    fn durations() {
        assert_eq!(format_duration(s(0)), "0:00");
        assert_eq!(format_duration(s(65)), "1:05");
        assert_eq!(format_duration(s(3599)), "59:59");
        assert_eq!(format_duration(s(3600)), "1:00:00");
        assert_eq!(format_duration(s(11 * 3600 + 11 * 60 + 11)), "11:11:11");
        assert_eq!(format_optional_duration(None), "--:--");
        assert_eq!(format_optional_duration(Some(s(9))), "0:09");
    }

    #[test]
    fn clock_elapsed_and_remaining() {
        let c = ClockDisplay::new(s(83), Some(s(200)), TimeMode::Elapsed);
        assert_eq!(
            c,
            ClockDisplay {
                negative: false,
                digits: [0, 1, 2, 3]
            }
        );
        let c = ClockDisplay::new(s(83), Some(s(200)), TimeMode::Remaining);
        assert_eq!(
            c,
            ClockDisplay {
                negative: true,
                digits: [0, 1, 5, 7]
            }
        );
        // Remaining without a known duration falls back to elapsed.
        let c = ClockDisplay::new(s(83), None, TimeMode::Remaining);
        assert!(!c.negative);
        // Past 99:59 the clock switches to hours:minutes.
        let c = ClockDisplay::new(s(100 * 60), None, TimeMode::Elapsed);
        assert_eq!(c.digits, [0, 1, 4, 0]);
        let c = ClockDisplay::new(s(1_000 * 3600), None, TimeMode::Elapsed);
        assert_eq!(c.digits[..2], [9, 9]);
        // Positions past the end never underflow.
        let c = ClockDisplay::new(s(300), Some(s(200)), TimeMode::Remaining);
        assert_eq!(c.digits, [0, 0, 0, 0]);
    }

    #[test]
    fn time_mode_toggles() {
        assert_eq!(TimeMode::Elapsed.toggled(), TimeMode::Remaining);
        assert_eq!(TimeMode::Remaining.toggled(), TimeMode::Elapsed);
    }

    #[test]
    fn marquee_text_formats() {
        assert_eq!(
            marquee_text(Some(3), "Tonight", Some(s(225))),
            "3. Tonight (3:45)"
        );
        assert_eq!(marquee_text(None, "Radio", None), "Radio");
    }

    #[test]
    fn marquee_scrolls_and_wraps() {
        let mut m = Marquee::new("ABCDEFGH");
        assert_eq!(
            m.visible(20, true),
            "ABCDEFGH",
            "short text does not scroll"
        );
        assert_eq!(m.visible(4, true), "ABCD");
        assert_eq!(m.visible(4, false), "ABCD");
        m.tick(1.0 / Marquee::SPEED);
        assert_eq!(m.visible(4, true), "BCDE");
        m.tick(6.0 / Marquee::SPEED);
        assert_eq!(m.visible(4, true), "H  *");
        // One full cycle returns to the start.
        let cycle = 8 + MARQUEE_SEPARATOR.chars().count();
        #[allow(clippy::cast_precision_loss)]
        m.tick((cycle - 7) as f32 / Marquee::SPEED);
        assert_eq!(m.visible(4, true), "ABCD");
        m.tick(-5.0);
        assert_eq!(m.visible(4, true), "ABCD", "negative dt is ignored");
    }

    #[test]
    fn marquee_set_text_restarts_only_on_change() {
        let mut m = Marquee::new("Hello world");
        m.tick(1.0);
        let before = m.visible(4, true);
        m.set_text("Hello world");
        assert_eq!(m.visible(4, true), before);
        m.set_text("Other");
        assert_eq!(m.text(), "Other");
        assert_eq!(m.visible(3, true), "Oth");
        let mut empty = Marquee::new("");
        empty.tick(1.0);
        assert_eq!(empty.visible(5, true), "");
    }

    #[test]
    fn seven_segment_digits() {
        assert_eq!(seven_segment(8), 0b111_1111);
        assert_eq!(seven_segment(1), 0b000_0110);
        assert_eq!(seven_segment(10), 0);
        // Each digit's segment count matches a real seven-segment display.
        let counts: Vec<u32> = SEVEN_SEGMENT.iter().map(|m| m.count_ones()).collect();
        assert_eq!(counts, [6, 2, 5, 5, 4, 5, 6, 3, 7, 6]);
    }

    #[test]
    fn indicator_texts() {
        assert_eq!(kbps_text(Some(128)), "128");
        assert_eq!(kbps_text(Some(96)), " 96");
        assert_eq!(kbps_text(Some(1411)), "999");
        assert_eq!(kbps_text(None), "   ");
        assert_eq!(khz_text(Some(44_100)), "44");
        assert_eq!(khz_text(Some(8_000)), " 8");
        assert_eq!(khz_text(Some(192_000)), "99");
        assert_eq!(khz_text(None), "  ");
    }

    #[test]
    fn volume_goes_to_eleven() {
        assert_eq!(volume_tooltip(1.0), "Volume: 11 (one louder)");
        assert_eq!(volume_tooltip(2.0), "Volume: 11 (one louder)");
        assert_eq!(volume_tooltip(0.0), "Volume: 0.0 / 11");
        assert_eq!(volume_tooltip(0.5), "Volume: 5.5 / 11");
    }

    #[test]
    fn balance_tooltips() {
        assert_eq!(balance_tooltip(0.0), "Balance: center");
        assert_eq!(balance_tooltip(-0.25), "Balance: 25% left");
        assert_eq!(balance_tooltip(3.0), "Balance: 100% right");
    }

    #[test]
    fn playlist_footer() {
        let sel = TotalDuration {
            known: s(225),
            has_unknown: false,
        };
        let total = TotalDuration {
            known: s(3723),
            has_unknown: true,
        };
        assert_eq!(playlist_time_text(sel, total), "3:45/1:02:03+");
    }

    proptest! {
        #[test]
        fn clock_digits_are_decimal(pos in 0u64..10_000_000, dur in proptest::option::of(0u64..10_000_000)) {
            for mode in [TimeMode::Elapsed, TimeMode::Remaining] {
                let c = ClockDisplay::new(s(pos), dur.map(s), mode);
                prop_assert!(c.digits.iter().all(|&d| d <= 9));
                prop_assert!(c.digits[2] <= 5, "minor unit tens digit is at most 5");
            }
        }

        #[test]
        fn duration_formatting_round_trips(secs in 0u64..1_000_000) {
            let text = format_duration(s(secs));
            let parsed = text.split(':').fold(0u64, |acc, part| acc * 60 + part.parse::<u64>().unwrap());
            prop_assert_eq!(parsed, secs);
        }

        #[test]
        fn marquee_visible_has_requested_width(text in "[a-z ]{0,40}", width in 0usize..30, t in 0.0f32..100.0) {
            let mut m = Marquee::new(&text);
            m.tick(t);
            let v = m.visible(width, true);
            prop_assert_eq!(v.chars().count(), width.min(text.chars().count()));
        }
    }
}
