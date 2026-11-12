//! Window geometry, in logical points at 1x (double size uses egui's zoom factor).
//!
//! The rectangles follow the classic 275-pixel-wide skin layout, so the built-in look and
//! `.wsz` skins share hit-testing.

use egui::{Pos2, Rect, Vec2, pos2, vec2};

/// Width of every section.
pub const WIDTH: f32 = 275.0;
/// Height of the main section.
pub const MAIN_HEIGHT: f32 = 116.0;
/// Height of the equaliser section.
pub const EQ_HEIGHT: f32 = 116.0;
/// Height of the main section when shaded.
pub const SHADE_HEIGHT: f32 = 14.0;
/// Playlist title bar height.
pub const PL_TITLE: f32 = 20.0;
/// Playlist footer height.
pub const PL_FOOTER: f32 = 38.0;
/// Height of one playlist row.
pub const PL_ROW: f32 = 13.0;
/// Playlist side margins (left frame, right frame with scrollbar).
pub const PL_LEFT: f32 = 12.0;
/// Right playlist margin (contains the scrollbar).
pub const PL_RIGHT: f32 = 20.0;

/// `Rect` from x, y, width, height relative to `origin`.
#[must_use]
pub fn r(origin: Pos2, x: f32, y: f32, w: f32, h: f32) -> Rect {
    Rect::from_min_size(origin + vec2(x, y), vec2(w, h))
}

/// Height of the playlist section for `rows` visible rows.
#[must_use]
pub fn playlist_height(rows: usize) -> f32 {
    #[allow(clippy::cast_precision_loss)]
    let rows = rows as f32;
    PL_TITLE + rows * PL_ROW + 4.0 + PL_FOOTER
}

/// Which sections are visible.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sections {
    /// Main window shaded to a strip.
    pub shaded: bool,
    /// Equaliser shown.
    pub equalizer: bool,
    /// Playlist shown.
    pub playlist: bool,
    /// Visible playlist rows.
    pub rows: usize,
}

/// Where each section sits in the window.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Frame {
    /// The main (or shaded) section.
    pub main: Rect,
    /// The equaliser, if shown.
    pub equalizer: Option<Rect>,
    /// The playlist, if shown.
    pub playlist: Option<Rect>,
}

impl Frame {
    /// Stacks the sections from the top-left `origin`.
    #[must_use]
    pub fn compute(origin: Pos2, s: Sections) -> Self {
        let main_h = if s.shaded { SHADE_HEIGHT } else { MAIN_HEIGHT };
        let main = Rect::from_min_size(origin, vec2(WIDTH, main_h));
        let mut y = main.bottom();
        let equalizer = s.equalizer.then(|| {
            let rect = Rect::from_min_size(pos2(origin.x, y), vec2(WIDTH, EQ_HEIGHT));
            y += EQ_HEIGHT;
            rect
        });
        let playlist = s
            .playlist
            .then(|| Rect::from_min_size(pos2(origin.x, y), vec2(WIDTH, playlist_height(s.rows))));
        Self {
            main,
            equalizer,
            playlist,
        }
    }

    /// Total window size.
    #[must_use]
    pub fn size(&self) -> Vec2 {
        let bottom = self
            .playlist
            .or(self.equalizer)
            .unwrap_or(self.main)
            .bottom();
        vec2(WIDTH, bottom - self.main.top())
    }
}

/// Main-window element rectangles relative to the section origin.
pub mod main {
    use super::{Pos2, Rect, r};

    /// Title bar.
    #[must_use]
    pub fn title_bar(o: Pos2) -> Rect {
        r(o, 0.0, 0.0, 275.0, 14.0)
    }
    /// Main menu button (top left).
    #[must_use]
    pub fn menu(o: Pos2) -> Rect {
        r(o, 6.0, 3.0, 9.0, 9.0)
    }
    /// Minimise button.
    #[must_use]
    pub fn minimize(o: Pos2) -> Rect {
        r(o, 244.0, 3.0, 9.0, 9.0)
    }
    /// Shade button.
    #[must_use]
    pub fn shade(o: Pos2) -> Rect {
        r(o, 254.0, 3.0, 9.0, 9.0)
    }
    /// Close button.
    #[must_use]
    pub fn close(o: Pos2) -> Rect {
        r(o, 264.0, 3.0, 9.0, 9.0)
    }
    /// The LCD panel behind clock and visualiser.
    #[must_use]
    pub fn lcd(o: Pos2) -> Rect {
        r(o, 9.0, 20.0, 95.0, 45.0)
    }
    /// Play/pause/stop indicator.
    #[must_use]
    pub fn status(o: Pos2) -> Rect {
        r(o, 24.0, 28.0, 9.0, 9.0)
    }
    /// Minus sign of the clock.
    #[must_use]
    pub fn minus(o: Pos2) -> Rect {
        r(o, 36.0, 32.0, 7.0, 2.0)
    }
    /// The whole clock (click target).
    #[must_use]
    pub fn clock(o: Pos2) -> Rect {
        r(o, 36.0, 26.0, 63.0, 13.0)
    }
    /// X positions of the four clock digits.
    pub const DIGIT_X: [f32; 4] = [48.0, 60.0, 78.0, 90.0];
    /// Y position and size of a clock digit.
    pub const DIGIT_Y: f32 = 26.0;
    /// Digit size.
    pub const DIGIT_W: f32 = 9.0;
    /// Digit height.
    pub const DIGIT_H: f32 = 13.0;
    /// Visualiser.
    #[must_use]
    pub fn vis(o: Pos2) -> Rect {
        r(o, 24.0, 43.0, 76.0, 16.0)
    }
    /// Scrolling title.
    #[must_use]
    pub fn title(o: Pos2) -> Rect {
        r(o, 111.0, 24.0, 154.0, 12.0)
    }
    /// Bitrate digits.
    #[must_use]
    pub fn kbps(o: Pos2) -> Rect {
        r(o, 111.0, 43.0, 15.0, 6.0)
    }
    /// Sample-rate digits.
    #[must_use]
    pub fn khz(o: Pos2) -> Rect {
        r(o, 156.0, 43.0, 10.0, 6.0)
    }
    /// "mono" indicator.
    #[must_use]
    pub fn mono(o: Pos2) -> Rect {
        r(o, 212.0, 41.0, 27.0, 12.0)
    }
    /// "stereo" indicator.
    #[must_use]
    pub fn stereo(o: Pos2) -> Rect {
        r(o, 239.0, 41.0, 29.0, 12.0)
    }
    /// Volume slider.
    #[must_use]
    pub fn volume(o: Pos2) -> Rect {
        r(o, 107.0, 57.0, 68.0, 13.0)
    }
    /// Balance slider.
    #[must_use]
    pub fn balance(o: Pos2) -> Rect {
        r(o, 177.0, 57.0, 38.0, 13.0)
    }
    /// EQ window toggle.
    #[must_use]
    pub fn eq_toggle(o: Pos2) -> Rect {
        r(o, 219.0, 58.0, 23.0, 12.0)
    }
    /// Playlist toggle.
    #[must_use]
    pub fn pl_toggle(o: Pos2) -> Rect {
        r(o, 242.0, 58.0, 23.0, 12.0)
    }
    /// Seek bar.
    #[must_use]
    pub fn posbar(o: Pos2) -> Rect {
        r(o, 16.0, 72.0, 248.0, 10.0)
    }
    /// Transport buttons: previous, play, pause, stop, next.
    #[must_use]
    pub fn transport(o: Pos2) -> [Rect; 5] {
        [
            r(o, 16.0, 88.0, 23.0, 18.0),
            r(o, 39.0, 88.0, 23.0, 18.0),
            r(o, 62.0, 88.0, 23.0, 18.0),
            r(o, 85.0, 88.0, 23.0, 18.0),
            r(o, 108.0, 88.0, 22.0, 18.0),
        ]
    }
    /// Eject (open files).
    #[must_use]
    pub fn eject(o: Pos2) -> Rect {
        r(o, 136.0, 89.0, 22.0, 16.0)
    }
    /// Shuffle toggle.
    #[must_use]
    pub fn shuffle(o: Pos2) -> Rect {
        r(o, 164.0, 89.0, 47.0, 15.0)
    }
    /// Repeat toggle.
    #[must_use]
    pub fn repeat(o: Pos2) -> Rect {
        r(o, 210.0, 89.0, 28.0, 15.0)
    }
}

/// Equaliser element rectangles relative to the section origin.
pub mod eq {
    use super::{Pos2, Rect, r};

    /// Title bar.
    #[must_use]
    pub fn title_bar(o: Pos2) -> Rect {
        r(o, 0.0, 0.0, 275.0, 14.0)
    }
    /// Close (hide) button.
    #[must_use]
    pub fn close(o: Pos2) -> Rect {
        r(o, 264.0, 3.0, 9.0, 9.0)
    }
    /// "ON" toggle.
    #[must_use]
    pub fn on(o: Pos2) -> Rect {
        r(o, 14.0, 18.0, 26.0, 12.0)
    }
    /// "AUTO" toggle.
    #[must_use]
    pub fn auto(o: Pos2) -> Rect {
        r(o, 40.0, 18.0, 32.0, 12.0)
    }
    /// Response graph.
    #[must_use]
    pub fn graph(o: Pos2) -> Rect {
        r(o, 86.0, 17.0, 113.0, 19.0)
    }
    /// "PRESETS" button.
    #[must_use]
    pub fn presets(o: Pos2) -> Rect {
        r(o, 217.0, 18.0, 44.0, 12.0)
    }
    /// Preamp slider.
    #[must_use]
    pub fn preamp(o: Pos2) -> Rect {
        r(o, 21.0, 38.0, 14.0, 63.0)
    }
    /// Band slider `i` (0..10).
    #[must_use]
    pub fn band(o: Pos2, i: usize) -> Rect {
        #[allow(clippy::cast_precision_loss)]
        let x = 78.0 + 18.0 * i as f32;
        r(o, x, 38.0, 14.0, 63.0)
    }
}

/// Playlist element rectangles relative to the section rect.
pub mod playlist {
    use super::{PL_FOOTER, PL_LEFT, PL_RIGHT, PL_TITLE, Rect, pos2, r};

    /// Title bar.
    #[must_use]
    pub fn title_bar(s: Rect) -> Rect {
        r(s.min, 0.0, 0.0, s.width(), PL_TITLE)
    }
    /// Close (hide) button.
    #[must_use]
    pub fn close(s: Rect) -> Rect {
        r(s.min, s.width() - 11.0, 3.0, 9.0, 9.0)
    }
    /// The rows area.
    #[must_use]
    pub fn list(s: Rect) -> Rect {
        Rect::from_min_max(
            pos2(s.left() + PL_LEFT, s.top() + PL_TITLE),
            pos2(s.right() - PL_RIGHT, s.bottom() - PL_FOOTER),
        )
    }
    /// The scrollbar track.
    #[must_use]
    pub fn scrollbar(s: Rect) -> Rect {
        let list = list(s);
        Rect::from_min_max(
            pos2(s.right() - 15.0, list.top()),
            pos2(s.right() - 7.0, list.bottom()),
        )
    }
    /// Footer buttons: add, remove, select, misc, list.
    #[must_use]
    pub fn buttons(s: Rect) -> [Rect; 5] {
        let y = s.height() - 30.0;
        [
            r(s.min, 14.0, y, 25.0, 18.0),
            r(s.min, 43.0, y, 25.0, 18.0),
            r(s.min, 72.0, y, 25.0, 18.0),
            r(s.min, 101.0, y, 25.0, 18.0),
            r(s.min, s.width() - 46.0, y, 25.0, 18.0),
        ]
    }
    /// The time readout in the footer.
    #[must_use]
    pub fn time(s: Rect) -> Rect {
        r(s.min, 132.0, s.height() - 30.0, 92.0, 18.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sections_stack() {
        let all = Sections {
            shaded: false,
            equalizer: true,
            playlist: true,
            rows: 8,
        };
        let f = Frame::compute(Pos2::ZERO, all);
        assert_eq!(f.main.height(), MAIN_HEIGHT);
        assert_eq!(f.equalizer.unwrap().top(), MAIN_HEIGHT);
        assert_eq!(f.playlist.unwrap().top(), MAIN_HEIGHT + EQ_HEIGHT);
        assert_eq!(
            f.size(),
            vec2(WIDTH, MAIN_HEIGHT + EQ_HEIGHT + playlist_height(8))
        );
        let shaded = Frame::compute(
            Pos2::ZERO,
            Sections {
                shaded: true,
                equalizer: false,
                playlist: false,
                rows: 8,
            },
        );
        assert_eq!(shaded.size(), vec2(WIDTH, SHADE_HEIGHT));
        let pl_only = Frame::compute(
            Pos2::ZERO,
            Sections {
                shaded: false,
                equalizer: false,
                playlist: true,
                rows: 3,
            },
        );
        assert_eq!(pl_only.playlist.unwrap().top(), MAIN_HEIGHT);
    }

    #[test]
    fn playlist_geometry_fits_its_section() {
        let s = Rect::from_min_size(pos2(0.0, 232.0), vec2(WIDTH, playlist_height(8)));
        let list = playlist::list(s);
        assert_eq!(list.height(), 8.0 * PL_ROW + 4.0);
        assert!(s.contains_rect(list) && s.contains_rect(playlist::scrollbar(s)));
        for b in playlist::buttons(s) {
            assert!(s.contains_rect(b));
            assert!(!b.intersects(list));
        }
        assert!(s.contains_rect(playlist::time(s)));
        assert!(s.contains_rect(playlist::close(s)));
        assert!(s.contains_rect(playlist::title_bar(s)));
    }

    #[test]
    fn main_and_eq_elements_fit() {
        let main_rect = Rect::from_min_size(Pos2::ZERO, vec2(WIDTH, MAIN_HEIGHT));
        let o = Pos2::ZERO;
        let mut rects = vec![
            main::title_bar(o),
            main::menu(o),
            main::minimize(o),
            main::shade(o),
            main::close(o),
            main::lcd(o),
            main::status(o),
            main::minus(o),
            main::clock(o),
            main::vis(o),
            main::title(o),
            main::kbps(o),
            main::khz(o),
            main::mono(o),
            main::stereo(o),
            main::volume(o),
            main::balance(o),
            main::eq_toggle(o),
            main::pl_toggle(o),
            main::posbar(o),
            main::eject(o),
            main::shuffle(o),
            main::repeat(o),
        ];
        rects.extend(main::transport(o));
        assert!(
            rects.iter().all(|r| main_rect.contains_rect(*r)),
            "main elements inside"
        );
        let mut eqs = vec![
            eq::title_bar(o),
            eq::close(o),
            eq::on(o),
            eq::auto(o),
            eq::graph(o),
        ];
        eqs.extend([eq::presets(o), eq::preamp(o)]);
        eqs.extend((0..10).map(|i| eq::band(o, i)));
        assert!(
            eqs.iter().all(|r| main_rect.contains_rect(*r)),
            "eq elements inside"
        );
    }
}
