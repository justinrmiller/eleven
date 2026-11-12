//! A [`Look`] that paints classic `.wsz` skin sprites.
//!
//! Each sheet becomes a nearest-filtered texture so pixel art stays crisp at double size.
//! Sections the skin lacks (equaliser, playlist) and tiny shade-mode buttons fall back to the
//! built-in look.

use std::collections::HashMap;
use std::path::Path;

use egui::{
    Color32, ColorImage, Context, Painter, Pos2, Rect, Stroke, TextureHandle, TextureOptions, pos2,
    vec2,
};
use eleven_core::eq::MAX_GAIN_DB;
use eleven_core::format::ClockDisplay;
use eleven_core::{EqSettings, PlaybackState};
use eleven_skin::sprites::{
    self, Sheet, Sprite, cbuttons, eqmain, monoster, playpaus, pledit, posbar, shufrep, title,
};
use eleven_skin::{Rgb, Skin};

use super::builtin::Builtin;
use super::layout::{self, main as l};
use super::look::{Glyph, Look, PlaylistStyle, TitleButton, TitleKind, Toggle, VisColors};

fn color(c: Rgb) -> Color32 {
    Color32::from_rgb(c.0, c.1, c.2)
}

/// A loaded skin with its textures.
pub struct SkinLook {
    skin: Skin,
    textures: HashMap<Sheet, (TextureHandle, [f32; 2])>,
    fallback: Builtin,
    graph_color: Color32,
}

impl std::fmt::Debug for SkinLook {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SkinLook")
            .field("name", &self.skin.name)
            .finish_non_exhaustive()
    }
}

impl SkinLook {
    /// Loads a skin file and uploads its sheets.
    ///
    /// # Errors
    /// Returns the loader's error message.
    pub fn load(ctx: &Context, path: &Path) -> Result<Self, String> {
        let skin = Skin::load(path).map_err(|e| e.to_string())?;
        Ok(Self::from_skin(ctx, skin))
    }

    /// Uploads an already-parsed skin.
    #[must_use]
    pub fn from_skin(ctx: &Context, skin: Skin) -> Self {
        let mut textures = HashMap::new();
        for (sheet, bmp) in skin.sheets() {
            let image = ColorImage::from_rgba_unmultiplied(
                [bmp.width as usize, bmp.height as usize],
                &bmp.rgba,
            );
            let handle = ctx.load_texture(
                format!("skin-{}-{sheet:?}", skin.name),
                image,
                TextureOptions::NEAREST,
            );
            #[allow(clippy::cast_precision_loss)]
            let size = [bmp.width as f32, bmp.height as f32];
            textures.insert(sheet, (handle, size));
        }
        // The middle of the EQ graph colour ramp.
        let graph_color = skin
            .sheet(Sheet::Eqmain)
            .and_then(|b| b.pixel(eqmain::GRAPH_COLORS.x, eqmain::GRAPH_COLORS.y + 9))
            .map_or(Color32::from_rgb(0xe0, 0xc0, 0x40), |[r, g, b, _]| {
                Color32::from_rgb(r, g, b)
            });
        Self {
            skin,
            textures,
            fallback: Builtin,
            graph_color,
        }
    }

    /// Paints `sprite` into `dest`; returns `false` if the sheet is missing or too small.
    fn draw(&self, p: &Painter, sprite: Sprite, dest: Rect) -> bool {
        let Some((tex, [w, h])) = self.textures.get(&sprite.sheet) else {
            return false;
        };
        let bmp_ok = self
            .skin
            .sheet(sprite.sheet)
            .is_some_and(|b| b.contains(&sprite));
        if !bmp_ok {
            return false;
        }
        #[allow(clippy::cast_precision_loss)]
        let uv = Rect::from_min_max(
            pos2(sprite.x as f32 / w, sprite.y as f32 / h),
            pos2(
                (sprite.x + sprite.w) as f32 / w,
                (sprite.y + sprite.h) as f32 / h,
            ),
        );
        p.image(tex.id(), dest, uv, Color32::WHITE);
        true
    }

    /// Paints `sprite` at its natural size with its top-left at `at`.
    fn blit(&self, p: &Painter, sprite: Sprite, at: Pos2) -> bool {
        #[allow(clippy::cast_precision_loss)]
        let size = vec2(sprite.w as f32, sprite.h as f32);
        self.draw(p, sprite, Rect::from_min_size(at, size))
    }

    fn pressed_overlay(p: &Painter, rect: Rect, pressed: bool) {
        if pressed {
            p.rect_filled(rect, 0.0, Color32::from_black_alpha(80));
        }
    }
}

impl Look for SkinLook {
    fn name(&self) -> String {
        self.skin.name.clone()
    }

    fn main_background(&self, p: &Painter, origin: Pos2) {
        if !self.blit(p, sprites::MAIN, origin) {
            self.fallback.main_background(p, origin);
        }
    }

    fn title_bar(&self, p: &Painter, rect: Rect, kind: TitleKind, focused: bool) {
        let sprite = match kind {
            TitleKind::Main if focused => title::ACTIVE,
            TitleKind::Main => title::INACTIVE,
            TitleKind::Shade if focused => title::SHADE_ACTIVE,
            TitleKind::Shade => title::SHADE_INACTIVE,
            TitleKind::Equalizer => eqmain::TITLE[usize::from(!focused)],
        };
        if !self.draw(p, sprite, rect) {
            self.fallback.title_bar(p, rect, kind, focused);
        }
    }

    fn title_button(&self, p: &Painter, rect: Rect, which: TitleButton, pressed: bool) {
        let set = match which {
            TitleButton::Menu => title::MENU,
            TitleButton::Minimize => title::MINIMIZE,
            TitleButton::Shade => title::SHADE,
            TitleButton::Close => title::CLOSE,
        };
        if !self.draw(p, set[usize::from(pressed)], rect) {
            self.fallback.title_button(p, rect, which, pressed);
        }
    }

    fn transport(&self, p: &Painter, rect: Rect, glyph: Glyph, pressed: bool) {
        // Shade-mode mini buttons are far smaller than the sprites; draw them as vectors.
        if rect.width() < 15.0 {
            self.fallback.transport(p, rect, glyph, pressed);
            return;
        }
        let set = match glyph {
            Glyph::Previous => cbuttons::PREVIOUS,
            Glyph::Play => cbuttons::PLAY,
            Glyph::Pause => cbuttons::PAUSE,
            Glyph::Stop => cbuttons::STOP,
            Glyph::Next => cbuttons::NEXT,
            Glyph::Eject => cbuttons::EJECT,
        };
        if !self.draw(p, set[usize::from(pressed)], rect) {
            self.fallback.transport(p, rect, glyph, pressed);
        }
    }

    fn toggle(&self, p: &Painter, rect: Rect, which: Toggle, on: bool, pressed: bool) {
        let set = match which {
            Toggle::Shuffle => shufrep::SHUFFLE,
            Toggle::Repeat => shufrep::REPEAT,
            Toggle::Equalizer => shufrep::EQ,
            Toggle::Playlist => shufrep::PL,
            Toggle::EqOn => eqmain::ON,
            Toggle::EqAuto => eqmain::AUTO,
        };
        if !self.draw(p, set[usize::from(on)][usize::from(pressed)], rect) {
            self.fallback.toggle(p, rect, which, on, pressed);
        }
    }

    fn clock(&self, p: &Painter, origin: Pos2, clock: &ClockDisplay, blank: bool) {
        let sheet = self.skin.digits_sheet();
        for (i, x) in l::DIGIT_X.iter().enumerate() {
            let d = if blank { 10 } else { clock.digits[i] };
            self.blit(p, sprites::digit(sheet, d), origin + vec2(*x, l::DIGIT_Y));
        }
        if clock.negative && !blank {
            let at = if sheet == Sheet::NumsEx {
                origin + vec2(36.0, 26.0)
            } else {
                origin + vec2(38.0, 32.0)
            };
            self.blit(p, sprites::minus(sheet), at);
        }
    }

    fn status(&self, p: &Painter, rect: Rect, state: PlaybackState) {
        let sprite = match state {
            PlaybackState::Playing => playpaus::PLAYING,
            PlaybackState::Paused => playpaus::PAUSED,
            PlaybackState::Stopped => playpaus::STOPPED,
        };
        if !self.draw(p, sprite, rect) {
            self.fallback.status(p, rect, state);
        }
    }

    fn lcd_text(&self, p: &Painter, rect: Rect, text: &str) {
        let clipped = p.with_clip_rect(rect);
        let y = rect.center().y - 3.0;
        for (i, ch) in text.chars().enumerate() {
            #[allow(clippy::cast_precision_loss)]
            let x = rect.left() + 5.0 * i as f32;
            if x >= rect.right() {
                break;
            }
            self.blit(&clipped, sprites::text_glyph(ch), pos2(x, y));
        }
    }

    fn channels(&self, p: &Painter, mono: Rect, stereo: Rect, is_stereo: Option<bool>) {
        let off = |b: bool| usize::from(!b);
        let ok = self.draw(p, monoster::MONO[off(is_stereo == Some(false))], mono)
            & self.draw(p, monoster::STEREO[off(is_stereo == Some(true))], stereo);
        if !ok {
            self.fallback.channels(p, mono, stereo, is_stereo);
        }
    }

    fn posbar(&self, p: &Painter, rect: Rect, progress: Option<f32>, pressed: bool) {
        if !self.draw(p, posbar::BACKGROUND, rect) {
            self.fallback.posbar(p, rect, progress, pressed);
            return;
        }
        if let Some(f) = progress {
            let x = rect.left() + (rect.width() - 29.0) * f;
            self.blit(p, posbar::THUMB[usize::from(pressed)], pos2(x, rect.top()));
        }
    }

    fn volume(&self, p: &Painter, rect: Rect, value: f32, pressed: bool) {
        if !self.draw(p, sprites::volume_frame(value), rect) {
            self.fallback.volume(p, rect, value, pressed);
            return;
        }
        let x = rect.left() + (rect.width() - 14.0) * value;
        self.blit(
            p,
            sprites::slider_thumb(Sheet::Volume)[usize::from(pressed)],
            pos2(x, rect.top() + 1.0),
        );
    }

    fn balance(&self, p: &Painter, rect: Rect, value: f32, pressed: bool) {
        let sheet = self.skin.balance_sheet();
        if !self.draw(p, sprites::balance_frame(sheet, value), rect) {
            self.fallback.balance(p, rect, value, pressed);
            return;
        }
        let x = rect.left() + (rect.width() - 14.0) * (value + 1.0) / 2.0;
        self.blit(
            p,
            sprites::slider_thumb(sheet)[usize::from(pressed)],
            pos2(x, rect.top() + 1.0),
        );
    }

    fn vis_colors(&self) -> VisColors {
        let Some(c) = self.skin.vis_colors else {
            return self.fallback.vis_colors();
        };
        let mut bars = [Color32::BLACK; 16];
        for (i, b) in bars.iter_mut().enumerate() {
            // viscolor.txt lists the spectrum top (index 2) to bottom (index 17).
            *b = color(c[17 - i]);
        }
        VisColors {
            background: color(c[0]),
            bars,
            peak: color(c[23]),
            scope: color(c[18]),
        }
    }

    fn eq_background(&self, p: &Painter, origin: Pos2) {
        if !self.blit(p, eqmain::BACKGROUND, origin) {
            self.fallback.eq_background(p, origin);
        }
    }

    fn eq_slider(&self, p: &Painter, rect: Rect, db: f32, pressed: bool) {
        if !self.draw(p, eqmain::slider_frame(db), rect) {
            self.fallback.eq_slider(p, rect, db, pressed);
            return;
        }
        let t = (MAX_GAIN_DB - db.clamp(-MAX_GAIN_DB, MAX_GAIN_DB)) / (2.0 * MAX_GAIN_DB);
        let y = rect.top() + (rect.height() - 11.0) * t;
        self.blit(
            p,
            eqmain::THUMB[usize::from(pressed)],
            pos2(rect.left() + 1.0, y),
        );
    }

    fn eq_graph(&self, p: &Painter, rect: Rect, eq: &EqSettings) {
        if !self.draw(p, eqmain::GRAPH, rect) {
            self.fallback.eq_graph(p, rect, eq);
            return;
        }
        let mut settings = eq.clone();
        settings.enabled = true;
        let mid = rect.center().y;
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let n = rect.width() as usize;
        let points: Vec<Pos2> = (0..n)
            .map(|i| {
                #[allow(clippy::cast_precision_loss)]
                let t = i as f64 / (n.max(2) - 1) as f64;
                #[allow(clippy::cast_possible_truncation)]
                let db = settings.response_db(20.0 * 1_000f64.powf(t), 44_100) as f32;
                #[allow(clippy::cast_precision_loss)]
                let x = rect.left() + i as f32;
                pos2(
                    x,
                    mid - db.clamp(-MAX_GAIN_DB, MAX_GAIN_DB) / MAX_GAIN_DB
                        * (rect.height() / 2.0 - 1.0),
                )
            })
            .collect();
        p.line(points, Stroke::new(1.0, self.graph_color));
    }

    fn text_button(&self, p: &Painter, rect: Rect, label: &str, pressed: bool) {
        if label == "PRESETS" {
            if !self.draw(p, eqmain::PRESETS[usize::from(pressed)], rect) {
                self.fallback.text_button(p, rect, label, pressed);
            }
        } else if self.skin.has_playlist() {
            // Playlist footer buttons are part of the frame artwork.
            Self::pressed_overlay(p, rect, pressed);
        } else {
            self.fallback.text_button(p, rect, label, pressed);
        }
    }

    fn playlist_frame(&self, p: &Painter, section: Rect, focused: bool) {
        if !self.skin.has_playlist() {
            self.fallback.playlist_frame(p, section, focused);
            return;
        }
        let state = usize::from(!focused);
        let (x0, y0, w, h) = (
            section.left(),
            section.top(),
            section.width(),
            section.height(),
        );
        // Background of the list.
        p.rect_filled(section, 0.0, self.playlist_style().background);
        // Top edge: corners, tiles, centred title.
        let mut x = x0 + 25.0;
        while x < x0 + w - 25.0 {
            self.blit(p, pledit::TOP_TILE[state], pos2(x, y0));
            x += 25.0;
        }
        self.blit(p, pledit::TOP_LEFT[state], pos2(x0, y0));
        self.blit(
            p,
            pledit::TITLE[state],
            pos2(x0 + ((w - 100.0) / 2.0).round(), y0),
        );
        self.blit(p, pledit::TOP_RIGHT[state], pos2(x0 + w - 25.0, y0));
        // Sides.
        let bottom = y0 + h - layout::PL_FOOTER;
        let mut y = y0 + layout::PL_TITLE;
        let side_clip = p.with_clip_rect(Rect::from_min_max(
            pos2(x0, y0 + layout::PL_TITLE),
            pos2(x0 + w, bottom),
        ));
        while y < bottom {
            self.blit(&side_clip, pledit::LEFT_TILE, pos2(x0, y));
            self.blit(&side_clip, pledit::RIGHT_TILE, pos2(x0 + w - 20.0, y));
            y += 29.0;
        }
        // Bottom edge.
        let mut x = x0 + 125.0;
        while x < x0 + w - 150.0 {
            self.blit(p, pledit::BOTTOM_TILE, pos2(x, bottom));
            x += 25.0;
        }
        self.blit(p, pledit::BOTTOM_LEFT, pos2(x0, bottom));
        self.blit(p, pledit::BOTTOM_RIGHT, pos2(x0 + w - 150.0, bottom));
    }

    fn playlist_style(&self) -> PlaylistStyle {
        if !self.skin.has_playlist() {
            return self.fallback.playlist_style();
        }
        let c = &self.skin.playlist_colors;
        PlaylistStyle {
            normal: color(c.normal),
            current: color(c.current),
            background: color(c.normal_bg),
            selected: color(c.selected_bg),
        }
    }

    fn scrollbar(&self, p: &Painter, track: Rect, fraction: f32, pressed: bool) {
        if !self.skin.has_playlist() {
            self.fallback.scrollbar(p, track, fraction, pressed);
            return;
        }
        let y = track.top() + (track.height() - 18.0).max(0.0) * fraction.clamp(0.0, 1.0);
        self.blit(
            p,
            pledit::THUMB[usize::from(pressed)],
            pos2(track.left(), y),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eleven_skin::synthetic::SyntheticSkin;

    fn look(spec: &SyntheticSkin) -> SkinLook {
        let ctx = Context::default();
        let skin = Skin::from_bytes("Synthetic", &spec.build()).unwrap();
        SkinLook::from_skin(&ctx, skin)
    }

    #[test]
    fn colours_come_from_the_skin() {
        let l = look(&SyntheticSkin::default());
        assert_eq!(l.name(), "Synthetic");
        assert_eq!(
            l.playlist_style().normal,
            Color32::from_rgb(0x10, 0xe0, 0x10)
        );
        let vis = l.vis_colors();
        assert_eq!(vis.background, Color32::from_rgb(0, 255, 64));
        assert_eq!(
            vis.bars[15],
            Color32::from_rgb(20, 235, 64),
            "top bar is viscolor index 2"
        );
        assert!(format!("{l:?}").contains("Synthetic"));
    }

    #[test]
    fn missing_sections_fall_back_to_builtin() {
        let l = look(&SyntheticSkin {
            omit: vec![Sheet::Pledit, Sheet::Eqmain],
            text_files: false,
            ..Default::default()
        });
        assert_eq!(l.playlist_style(), Builtin.playlist_style());
        assert_eq!(l.vis_colors(), Builtin.vis_colors());
    }

    #[test]
    fn load_reports_errors() {
        let ctx = Context::default();
        let dir = tempfile::tempdir().unwrap();
        let bad = dir.path().join("bad.wsz");
        std::fs::write(&bad, b"nope").unwrap();
        assert!(
            SkinLook::load(&ctx, &bad)
                .unwrap_err()
                .contains("not a skin archive")
        );
        let good = dir.path().join("Good.wsz");
        std::fs::write(&good, SyntheticSkin::default().build()).unwrap();
        assert_eq!(SkinLook::load(&ctx, &good).unwrap().name(), "Good");
    }
}
