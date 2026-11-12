//! The built-in look: original vector art in a classic gunmetal-and-green-LCD style.

use egui::{Align2, Color32, CornerRadius, FontId, Painter, Pos2, Rect, Stroke, pos2, vec2};
use eleven_core::eq::MAX_GAIN_DB;
use eleven_core::format::{ClockDisplay, seven_segment};
use eleven_core::{EqSettings, PlaybackState};

use super::layout::{self, PL_FOOTER, PL_LEFT, PL_RIGHT, PL_TITLE};
use super::look::{Clutter, Glyph, Look, PlaylistStyle, TitleButton, TitleKind, Toggle, VisColors};

/// Colours of the built-in look.
pub mod palette {
    use egui::Color32;

    /// Window body.
    pub const FRAME: Color32 = Color32::from_rgb(0x2a, 0x2d, 0x3a);
    /// Bevel highlight.
    pub const LIGHT: Color32 = Color32::from_rgb(0x4a, 0x50, 0x64);
    /// Bevel shadow.
    pub const DARK: Color32 = Color32::from_rgb(0x12, 0x14, 0x1a);
    /// Active title bar, top.
    pub const TITLE_TOP: Color32 = Color32::from_rgb(0x3b, 0x4a, 0x80);
    /// Active title bar, bottom.
    pub const TITLE_BOTTOM: Color32 = Color32::from_rgb(0x1c, 0x23, 0x42);
    /// Inactive title bar.
    pub const TITLE_INACTIVE: Color32 = Color32::from_rgb(0x30, 0x33, 0x40);
    /// Title text.
    pub const TITLE_TEXT: Color32 = Color32::from_rgb(0xd6, 0xdc, 0xff);
    /// LCD background.
    pub const LCD: Color32 = Color32::from_rgb(0x04, 0x07, 0x06);
    /// Lit LCD segments and text.
    pub const LCD_ON: Color32 = Color32::from_rgb(0x3c, 0xf0, 0x60);
    /// Unlit LCD segments.
    pub const LCD_OFF: Color32 = Color32::from_rgb(0x0d, 0x26, 0x14);
    /// Button face.
    pub const BUTTON: Color32 = Color32::from_rgb(0x3d, 0x42, 0x54);
    /// Pressed button face.
    pub const BUTTON_DOWN: Color32 = Color32::from_rgb(0x26, 0x29, 0x34);
    /// Button glyphs.
    pub const ICON: Color32 = Color32::from_rgb(0xdc, 0xe0, 0xee);
    /// Slider track.
    pub const TRACK: Color32 = Color32::from_rgb(0x0e, 0x10, 0x16);
    /// Slider knob.
    pub const KNOB: Color32 = Color32::from_rgb(0xc4, 0xc8, 0xd6);
    /// Warm end of the volume ramp ("one louder").
    pub const HOT: Color32 = Color32::from_rgb(0xf0, 0x5a, 0x28);
    /// Current track in the playlist.
    pub const CURRENT: Color32 = Color32::WHITE;
    /// Selected playlist row.
    pub const SELECTED: Color32 = Color32::from_rgb(0x1f, 0x3d, 0x7a);
}

use palette as c;

/// Linear interpolation between two colours.
#[must_use]
pub fn lerp(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    // Channel values stay within 0..=255, so the casts are exact enough.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let mix = |x: u8, y: u8| (f32::from(x) + (f32::from(y) - f32::from(x)) * t).round() as u8;
    Color32::from_rgb(mix(a.r(), b.r()), mix(a.g(), b.g()), mix(a.b(), b.b()))
}

/// Raised (or sunken) bevelled box.
fn bevel(p: &Painter, rect: Rect, fill: Color32, raised: bool) {
    p.rect_filled(rect, CornerRadius::ZERO, fill);
    let (tl, br) = if raised {
        (c::LIGHT, c::DARK)
    } else {
        (c::DARK, c::LIGHT)
    };
    let s = |col| Stroke::new(1.0, col);
    p.line_segment([rect.left_top(), rect.right_top()], s(tl));
    p.line_segment([rect.left_top(), rect.left_bottom()], s(tl));
    p.line_segment([rect.left_bottom(), rect.right_bottom()], s(br));
    p.line_segment([rect.right_top(), rect.right_bottom()], s(br));
}

fn vertical_gradient(p: &Painter, rect: Rect, top: Color32, bottom: Color32) {
    // Truncation: the number of 1-point bands in a small rect.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let n = rect.height().ceil().max(1.0) as usize;
    for i in 0..n {
        #[allow(clippy::cast_precision_loss)]
        let t = i as f32 / n as f32;
        #[allow(clippy::cast_precision_loss)]
        let y = rect.top() + i as f32;
        let band = Rect::from_min_max(
            pos2(rect.left(), y),
            pos2(rect.right(), (y + 1.0).min(rect.bottom())),
        );
        p.rect_filled(band, CornerRadius::ZERO, lerp(top, bottom, t));
    }
}

fn triangle(p: &Painter, points: [Pos2; 3], color: Color32) {
    p.add(egui::Shape::convex_polygon(
        points.to_vec(),
        color,
        Stroke::NONE,
    ));
}

fn draw_glyph(p: &Painter, rect: Rect, glyph: Glyph, color: Color32) {
    let c0 = rect.center();
    let s = rect.height().min(rect.width()) * 0.28;
    match glyph {
        Glyph::Play => triangle(
            p,
            [
                c0 + vec2(-s * 0.8, -s),
                c0 + vec2(-s * 0.8, s),
                c0 + vec2(s, 0.0),
            ],
            color,
        ),
        Glyph::Pause => {
            p.rect_filled(
                Rect::from_center_size(c0 - vec2(s * 0.5, 0.0), vec2(s * 0.55, 2.0 * s)),
                0.0,
                color,
            );
            p.rect_filled(
                Rect::from_center_size(c0 + vec2(s * 0.5, 0.0), vec2(s * 0.55, 2.0 * s)),
                0.0,
                color,
            );
        }
        Glyph::Stop => {
            p.rect_filled(
                Rect::from_center_size(c0, vec2(1.7 * s, 1.7 * s)),
                0.0,
                color,
            );
        }
        Glyph::Previous | Glyph::Next => {
            let dir = if glyph == Glyph::Next { 1.0 } else { -1.0 };
            let tip = c0 + vec2(dir * s * 0.6, 0.0);
            triangle(
                p,
                [tip, tip + vec2(-dir * s, -s), tip + vec2(-dir * s, s)],
                color,
            );
            let tip2 = tip - vec2(dir * s, 0.0);
            triangle(
                p,
                [
                    tip2,
                    tip2 + vec2(-dir * s * 0.8, -s * 0.8),
                    tip2 + vec2(-dir * s * 0.8, s * 0.8),
                ],
                color,
            );
            let bar_x = tip.x + dir * 1.0;
            p.rect_filled(
                Rect::from_center_size(pos2(bar_x, c0.y), vec2(1.5, 2.0 * s)),
                0.0,
                color,
            );
        }
        Glyph::Eject => {
            triangle(
                p,
                [
                    c0 + vec2(0.0, -s),
                    c0 + vec2(-s, s * 0.2),
                    c0 + vec2(s, s * 0.2),
                ],
                color,
            );
            p.rect_filled(
                Rect::from_center_size(c0 + vec2(0.0, s * 0.75), vec2(2.0 * s, 1.5)),
                0.0,
                color,
            );
        }
    }
}

/// Paints a seven-segment digit into a 9×13 cell.
fn seven_seg(p: &Painter, cell: Rect, digit: Option<u8>) {
    let mask = digit.map_or(0, seven_segment);
    let (x0, x1) = (cell.left() + 1.0, cell.right() - 1.0);
    let (y0, ym, y1) = (cell.top(), cell.center().y, cell.bottom());
    let t = 1.6;
    let segs = [
        Rect::from_min_max(pos2(x0 + 1.0, y0), pos2(x1 - 1.0, y0 + t)), // a
        Rect::from_min_max(pos2(x1 - t, y0 + 1.0), pos2(x1, ym - 0.5)), // b
        Rect::from_min_max(pos2(x1 - t, ym + 0.5), pos2(x1, y1 - 1.0)), // c
        Rect::from_min_max(pos2(x0 + 1.0, y1 - t), pos2(x1 - 1.0, y1)), // d
        Rect::from_min_max(pos2(x0, ym + 0.5), pos2(x0 + t, y1 - 1.0)), // e
        Rect::from_min_max(pos2(x0, y0 + 1.0), pos2(x0 + t, ym - 0.5)), // f
        Rect::from_min_max(pos2(x0 + 1.0, ym - t / 2.0), pos2(x1 - 1.0, ym + t / 2.0)), // g
    ];
    for (i, seg) in segs.iter().enumerate() {
        let on = mask & (1 << i) != 0;
        p.rect_filled(*seg, 0.0, if on { c::LCD_ON } else { c::LCD_OFF });
    }
}

fn knob(p: &Painter, rect: Rect, pressed: bool) {
    bevel(p, rect, if pressed { c::ICON } else { c::KNOB }, !pressed);
}

/// The built-in look.
#[derive(Debug, Default, Clone, Copy)]
pub struct Builtin;

impl Look for Builtin {
    fn name(&self) -> String {
        "Built-in".to_owned()
    }

    fn main_background(&self, p: &Painter, origin: Pos2) {
        let rect = Rect::from_min_size(origin, vec2(layout::WIDTH, layout::MAIN_HEIGHT));
        bevel(p, rect, c::FRAME, true);
        // LCD panel and the title/info strip.
        bevel(p, layout::main::lcd(origin), c::LCD, false);
        let info = Rect::from_min_max(origin + vec2(108.0, 21.0), origin + vec2(268.0, 53.0));
        bevel(p, info, c::LCD, false);
        p.text(
            origin + vec2(129.0, 46.0),
            Align2::LEFT_CENTER,
            "kbps",
            FontId::proportional(6.0),
            c::LCD_OFF,
        );
        p.text(
            origin + vec2(168.0, 46.0),
            Align2::LEFT_CENTER,
            "khz",
            FontId::proportional(6.0),
            c::LCD_OFF,
        );
        // A subtle maker's mark.
        p.text(
            origin + vec2(262.0, 98.0),
            Align2::RIGHT_CENTER,
            "eleven",
            FontId::proportional(8.0),
            c::LIGHT,
        );
    }

    fn title_bar(&self, p: &Painter, rect: Rect, kind: TitleKind, focused: bool) {
        if focused {
            vertical_gradient(p, rect, c::TITLE_TOP, c::TITLE_BOTTOM);
        } else {
            p.rect_filled(rect, 0.0, c::TITLE_INACTIVE);
        }
        let text = match kind {
            TitleKind::Main | TitleKind::Shade => "ELEVEN",
            TitleKind::Equalizer => "ELEVEN EQUALIZER",
        };
        if kind != TitleKind::Shade {
            p.text(
                rect.center(),
                Align2::CENTER_CENTER,
                text,
                FontId::monospace(8.0),
                c::TITLE_TEXT,
            );
        }
    }

    fn title_button(&self, p: &Painter, rect: Rect, which: TitleButton, pressed: bool) {
        bevel(
            p,
            rect,
            if pressed { c::BUTTON_DOWN } else { c::BUTTON },
            !pressed,
        );
        let s = Stroke::new(1.0, c::ICON);
        let r = rect.shrink(2.0);
        match which {
            TitleButton::Menu => {
                for i in 0..3 {
                    #[allow(clippy::cast_precision_loss)]
                    let y = r.top() + 0.5 + i as f32 * 2.0;
                    p.line_segment([pos2(r.left(), y), pos2(r.right(), y)], s);
                }
            }
            TitleButton::Minimize => {
                p.line_segment([r.left_bottom(), r.right_bottom()], s);
            }
            TitleButton::Shade => {
                p.line_segment(
                    [pos2(r.left(), r.center().y), pos2(r.right(), r.center().y)],
                    s,
                );
            }
            TitleButton::Close => {
                p.line_segment([r.left_top(), r.right_bottom()], s);
                p.line_segment([r.right_top(), r.left_bottom()], s);
            }
        }
    }

    fn transport(&self, p: &Painter, rect: Rect, glyph: Glyph, pressed: bool) {
        bevel(
            p,
            rect,
            if pressed { c::BUTTON_DOWN } else { c::BUTTON },
            !pressed,
        );
        let offset = if pressed {
            vec2(0.5, 0.5)
        } else {
            vec2(0.0, 0.0)
        };
        draw_glyph(p, rect.translate(offset), glyph, c::ICON);
    }

    fn toggle(&self, p: &Painter, rect: Rect, which: Toggle, on: bool, pressed: bool) {
        bevel(
            p,
            rect,
            if pressed { c::BUTTON_DOWN } else { c::BUTTON },
            !pressed,
        );
        let label = match which {
            Toggle::Shuffle => "SHUFFLE",
            Toggle::Repeat => "REP",
            Toggle::Equalizer => "EQ",
            Toggle::Playlist => "PL",
            Toggle::EqOn => "ON",
            Toggle::EqAuto => "AUTO",
        };
        let led = Rect::from_min_size(rect.left_center() + vec2(2.5, -1.5), vec2(3.0, 3.0));
        p.rect_filled(led, 1.0, if on { c::LCD_ON } else { c::LCD_OFF });
        let text_pos = rect.center() + vec2(2.0, 0.0);
        let color = c::ICON;
        p.text(
            text_pos,
            Align2::CENTER_CENTER,
            label,
            FontId::proportional(7.0),
            color,
        );
    }

    fn clock(&self, p: &Painter, origin: Pos2, clock: &ClockDisplay, blank: bool) {
        use layout::main::{DIGIT_H, DIGIT_W, DIGIT_X, DIGIT_Y};
        for (i, x) in DIGIT_X.iter().enumerate() {
            let cell = Rect::from_min_size(origin + vec2(*x, DIGIT_Y), vec2(DIGIT_W, DIGIT_H));
            seven_seg(p, cell, (!blank).then_some(clock.digits[i]));
        }
        // Colon.
        let colon = if blank { c::LCD_OFF } else { c::LCD_ON };
        p.rect_filled(
            Rect::from_min_size(origin + vec2(72.0, 29.0), vec2(2.0, 2.0)),
            0.0,
            colon,
        );
        p.rect_filled(
            Rect::from_min_size(origin + vec2(72.0, 34.0), vec2(2.0, 2.0)),
            0.0,
            colon,
        );
        let minus = if clock.negative && !blank {
            c::LCD_ON
        } else {
            c::LCD_OFF
        };
        p.rect_filled(layout::main::minus(origin), 0.0, minus);
    }

    fn status(&self, p: &Painter, rect: Rect, state: PlaybackState) {
        match state {
            PlaybackState::Playing => draw_glyph(p, rect.expand(3.0), Glyph::Play, c::LCD_ON),
            PlaybackState::Paused => draw_glyph(p, rect.expand(3.0), Glyph::Pause, c::LCD_ON),
            PlaybackState::Stopped => draw_glyph(p, rect.expand(3.0), Glyph::Stop, c::LCD_OFF),
        }
    }

    fn clutterbar(&self, p: &Painter, origin: Pos2, lit: [bool; 5]) {
        let font = FontId::monospace(6.5);
        for ((rect, which), on) in layout::main::clutter_buttons(origin)
            .iter()
            .zip(Clutter::ALL)
            .zip(lit)
        {
            let color = if on { c::LCD_ON } else { c::LCD_OFF };
            p.text(
                rect.center(),
                Align2::CENTER_CENTER,
                which.letter(),
                font.clone(),
                color,
            );
        }
    }

    fn lcd_text(&self, p: &Painter, rect: Rect, text: &str) {
        let clipped = p.with_clip_rect(rect);
        clipped.text(
            rect.left_center(),
            Align2::LEFT_CENTER,
            text,
            FontId::monospace(8.0),
            c::LCD_ON,
        );
    }

    fn channels(&self, p: &Painter, mono: Rect, stereo: Rect, is_stereo: Option<bool>) {
        let font = FontId::proportional(7.0);
        let on = |b: bool| if b { c::LCD_ON } else { c::LCD_OFF };
        p.text(
            mono.center(),
            Align2::CENTER_CENTER,
            "mono",
            font.clone(),
            on(is_stereo == Some(false)),
        );
        p.text(
            stereo.center(),
            Align2::CENTER_CENTER,
            "stereo",
            font,
            on(is_stereo == Some(true)),
        );
    }

    fn posbar(&self, p: &Painter, rect: Rect, progress: Option<f32>, pressed: bool) {
        bevel(p, rect, c::TRACK, false);
        if let Some(f) = progress {
            let fill = Rect::from_min_max(
                rect.min + vec2(1.0, 3.0),
                pos2(
                    rect.left() + 1.0 + (rect.width() - 2.0) * f,
                    rect.bottom() - 3.0,
                ),
            );
            p.rect_filled(fill, 0.0, c::LCD_OFF);
            let kw = 29.0;
            let x = rect.left() + (rect.width() - kw) * f;
            knob(
                p,
                Rect::from_min_size(pos2(x, rect.top()), vec2(kw, rect.height())),
                pressed,
            );
        }
    }

    fn volume(&self, p: &Painter, rect: Rect, value: f32, pressed: bool) {
        let track = rect.shrink2(vec2(0.0, 3.5));
        bevel(p, track, c::TRACK, false);
        let fill = Rect::from_min_max(
            track.min + vec2(1.0, 1.0),
            pos2(
                track.left() + (track.width() - 2.0) * value + 1.0,
                track.bottom() - 1.0,
            ),
        );
        p.rect_filled(fill, 0.0, lerp(c::LCD_ON, c::HOT, value));
        let kw = 14.0;
        let x = rect.left() + (rect.width() - kw) * value;
        knob(
            p,
            Rect::from_min_size(pos2(x, rect.top() + 1.0), vec2(kw, rect.height() - 2.0)),
            pressed,
        );
    }

    fn balance(&self, p: &Painter, rect: Rect, value: f32, pressed: bool) {
        let track = rect.shrink2(vec2(0.0, 3.5));
        bevel(p, track, c::TRACK, false);
        let centre = track.center().x;
        let off = (track.width() / 2.0 - 1.0) * value;
        let fill = Rect::from_x_y_ranges(
            centre.min(centre + off)..=centre.max(centre + off),
            track.top() + 1.0..=track.bottom() - 1.0,
        );
        p.rect_filled(fill, 0.0, lerp(c::LCD_ON, c::HOT, value.abs()));
        let kw = 14.0;
        let x = rect.left() + (rect.width() - kw) * (value + 1.0) / 2.0;
        knob(
            p,
            Rect::from_min_size(pos2(x, rect.top() + 1.0), vec2(kw, rect.height() - 2.0)),
            pressed,
        );
    }

    fn vis_colors(&self) -> VisColors {
        let mut bars = [Color32::BLACK; 16];
        for (i, b) in bars.iter_mut().enumerate() {
            #[allow(clippy::cast_precision_loss)]
            let t = i as f32 / 15.0;
            *b = if t < 0.6 {
                lerp(Color32::from_rgb(0x18, 0xa0, 0x30), c::LCD_ON, t / 0.6)
            } else {
                lerp(Color32::from_rgb(0xe8, 0xe0, 0x30), c::HOT, (t - 0.6) / 0.4)
            };
        }
        VisColors {
            background: c::LCD,
            bars,
            peak: Color32::from_rgb(0xc8, 0xc8, 0xd8),
            scope: c::LCD_ON,
        }
    }

    fn eq_background(&self, p: &Painter, origin: Pos2) {
        let rect = Rect::from_min_size(origin, vec2(layout::WIDTH, layout::EQ_HEIGHT));
        bevel(p, rect, c::FRAME, true);
        let font = FontId::proportional(6.0);
        p.text(
            origin + vec2(28.0, 106.0),
            Align2::CENTER_CENTER,
            "PREAMP",
            font.clone(),
            c::ICON,
        );
        for (i, label) in eleven_core::eq::BAND_LABELS.iter().enumerate() {
            let band = layout::eq::band(origin, i);
            p.text(
                pos2(band.center().x, origin.y + 106.0),
                Align2::CENTER_CENTER,
                *label,
                font.clone(),
                c::ICON,
            );
        }
        for (y, label) in [(40.0, "+12db"), (69.0, "+0db"), (98.0, "-12db")] {
            p.text(
                origin + vec2(42.0, y),
                Align2::LEFT_CENTER,
                label,
                font.clone(),
                c::LIGHT,
            );
        }
    }

    fn eq_slider(&self, p: &Painter, rect: Rect, db: f32, pressed: bool) {
        let track = rect.shrink2(vec2(4.5, 0.0));
        bevel(p, track, c::TRACK, false);
        let t = (MAX_GAIN_DB - db.clamp(-MAX_GAIN_DB, MAX_GAIN_DB)) / (2.0 * MAX_GAIN_DB);
        let kh = 11.0;
        let y = rect.top() + (rect.height() - kh) * t;
        let centre_y = rect.center().y;
        let fill = Rect::from_x_y_ranges(
            track.left() + 1.0..=track.right() - 1.0,
            centre_y.min(y + kh / 2.0)..=centre_y.max(y + kh / 2.0),
        );
        p.rect_filled(fill, 0.0, lerp(c::LCD_ON, c::HOT, db.abs() / MAX_GAIN_DB));
        knob(
            p,
            Rect::from_min_size(pos2(rect.left(), y), vec2(rect.width(), kh)),
            pressed,
        );
    }

    fn eq_graph(&self, p: &Painter, rect: Rect, eq: &EqSettings) {
        bevel(p, rect, c::LCD, false);
        let mid = rect.center().y;
        p.line_segment(
            [pos2(rect.left(), mid), pos2(rect.right(), mid)],
            Stroke::new(1.0, c::LCD_OFF),
        );
        let mut settings = eq.clone();
        settings.enabled = true;
        // Truncation: number of pixel columns.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let n = rect.width() as usize;
        let points: Vec<Pos2> = (0..n)
            .map(|i| {
                #[allow(clippy::cast_precision_loss)]
                let t = f64::from(i as u32) / f64::from((n - 1).max(1) as u32);
                let freq = 20.0 * 1_000f64.powf(t);
                #[allow(clippy::cast_possible_truncation)]
                let db = settings.response_db(freq, 44_100) as f32;
                let y = mid
                    - db.clamp(-MAX_GAIN_DB, MAX_GAIN_DB) / MAX_GAIN_DB
                        * (rect.height() / 2.0 - 2.0);
                #[allow(clippy::cast_precision_loss)]
                let x = rect.left() + i as f32;
                pos2(x, y)
            })
            .collect();
        let color = if eq.enabled { c::LCD_ON } else { c::LIGHT };
        p.line(points, Stroke::new(1.0, color));
    }

    fn text_button(&self, p: &Painter, rect: Rect, label: &str, pressed: bool) {
        bevel(
            p,
            rect,
            if pressed { c::BUTTON_DOWN } else { c::BUTTON },
            !pressed,
        );
        p.text(
            rect.center(),
            Align2::CENTER_CENTER,
            label,
            FontId::proportional(7.0),
            c::ICON,
        );
    }

    fn playlist_frame(&self, p: &Painter, section: Rect, focused: bool) {
        bevel(p, section, c::FRAME, true);
        let title = Rect::from_min_size(section.min, vec2(section.width(), PL_TITLE));
        if focused {
            vertical_gradient(p, title.shrink(3.0), c::TITLE_TOP, c::TITLE_BOTTOM);
        } else {
            p.rect_filled(title.shrink(3.0), 0.0, c::TITLE_INACTIVE);
        }
        p.text(
            title.center(),
            Align2::CENTER_CENTER,
            "ELEVEN PLAYLIST",
            FontId::monospace(8.0),
            c::TITLE_TEXT,
        );
        let list = Rect::from_min_max(
            pos2(
                section.left() + PL_LEFT - 2.0,
                section.top() + PL_TITLE - 1.0,
            ),
            pos2(
                section.right() - PL_RIGHT + 2.0,
                section.bottom() - PL_FOOTER + 1.0,
            ),
        );
        bevel(p, list, self.playlist_style().background, false);
    }

    fn playlist_style(&self) -> PlaylistStyle {
        PlaylistStyle {
            normal: c::LCD_ON,
            current: c::CURRENT,
            background: c::LCD,
            selected: c::SELECTED,
        }
    }

    fn scrollbar(&self, p: &Painter, track: Rect, fraction: f32, pressed: bool) {
        bevel(p, track, c::TRACK, false);
        let kh = 18.0f32.min(track.height());
        let y = track.top() + (track.height() - kh) * fraction.clamp(0.0, 1.0);
        knob(
            p,
            Rect::from_min_size(pos2(track.left(), y), vec2(track.width(), kh)),
            pressed,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colour_lerp() {
        assert_eq!(lerp(Color32::BLACK, Color32::WHITE, 0.0), Color32::BLACK);
        assert_eq!(lerp(Color32::BLACK, Color32::WHITE, 1.0), Color32::WHITE);
        assert_eq!(lerp(Color32::BLACK, Color32::WHITE, 2.0), Color32::WHITE);
        assert_eq!(
            lerp(Color32::BLACK, Color32::WHITE, 0.5),
            Color32::from_rgb(128, 128, 128)
        );
    }

    #[test]
    fn vis_ramp_goes_green_to_hot() {
        let v = Builtin.vis_colors();
        assert!(v.bars[0].g() > v.bars[0].r());
        assert_eq!(v.bars[15], c::HOT);
        assert_eq!(Builtin.name(), "Built-in");
        assert_eq!(Builtin.playlist_style().current, Color32::WHITE);
    }
}
