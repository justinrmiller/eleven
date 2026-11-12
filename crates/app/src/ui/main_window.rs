//! The main section: title bar, clock, visualiser, title display, sliders and transport.

use egui::{Id, Pos2, Rect, Sense, Stroke, Ui, ViewportCommand, pos2, vec2};
use eleven_core::format::{balance_tooltip, format_duration, kbps_text, khz_text, volume_tooltip};
use eleven_core::{PlaybackState, VisMode};

use super::UiState;
use super::actions::Action;
use super::layout::{self, main as l};
use super::look::{Glyph, Look, TitleButton, TitleKind, Toggle};
use super::widgets::{self, Axis};
use crate::controller::Controller;

/// Characters visible in the title display.
pub const TITLE_CHARS: usize = 31;

/// Makes a title bar draggable (moves the OS window) and returns its response.
pub fn drag_area(ui: &Ui, id: Id, rect: Rect) -> egui::Response {
    let response = ui.interact(rect, id, Sense::click_and_drag());
    if response.drag_started() {
        ui.ctx().send_viewport_cmd(ViewportCommand::StartDrag);
    }
    response
}

fn item(ui: &mut Ui, label: &str, action: Action, actions: &mut Vec<Action>) {
    if ui.button(label).clicked() {
        actions.push(action);
        ui.close();
    }
}

fn check(ui: &mut Ui, on: bool, label: &str, action: Action, actions: &mut Vec<Action>) {
    let mut v = on;
    if ui.checkbox(&mut v, label).clicked() {
        actions.push(action);
        ui.close();
    }
}

/// The main menu (title-bar menu button and right-click).
pub fn main_menu(ui: &mut Ui, ctl: &mut Controller, actions: &mut Vec<Action>) {
    item(ui, "Add files...", Action::AddFiles, actions);
    item(ui, "Add folder...", Action::AddFolder, actions);
    item(ui, "Open location...", Action::OpenLocation, actions);
    ui.separator();
    ui.menu_button("Playback", |ui| {
        item(ui, "Previous", Action::Previous, actions);
        item(ui, "Play", Action::Play, actions);
        item(ui, "Pause", Action::Pause, actions);
        item(ui, "Stop", Action::Stop, actions);
        item(ui, "Next", Action::Next, actions);
        item(ui, "Back 5 seconds", Action::SeekBack, actions);
        item(ui, "Forward 5 seconds", Action::SeekForward, actions);
        item(ui, "Jump to file...", Action::JumpToFile, actions);
    });
    ui.menu_button("Options", |ui| {
        let cfg = ctl.config().clone();
        let remaining = cfg.time_mode == eleven_core::TimeMode::Remaining;
        check(
            ui,
            remaining,
            "Time remaining",
            Action::ToggleTimeMode,
            actions,
        );
        check(ui, cfg.shuffle, "Shuffle", Action::ToggleShuffle, actions);
        check(ui, cfg.repeat, "Repeat", Action::ToggleRepeat, actions);
        check(
            ui,
            cfg.double_size,
            "Double size",
            Action::ToggleDoubleSize,
            actions,
        );
        check(ui, cfg.shaded, "Shade mode", Action::ToggleShade, actions);
        check(
            ui,
            cfg.always_on_top,
            "Always on top",
            Action::ToggleAlwaysOnTop,
            actions,
        );
        check(
            ui,
            cfg.show_equalizer,
            "Equalizer",
            Action::ToggleEqualizer,
            actions,
        );
        check(
            ui,
            cfg.show_playlist,
            "Playlist",
            Action::TogglePlaylist,
            actions,
        );
    });
    ui.menu_button("Visualization", |ui| {
        for (mode, label) in [
            (VisMode::Spectrum, "Spectrum analyzer"),
            (VisMode::Oscilloscope, "Oscilloscope"),
            (VisMode::Off, "Off"),
        ] {
            if ui.radio(ctl.config().vis_mode == mode, label).clicked() {
                ctl.set_vis_mode(mode);
                ui.close();
            }
        }
    });
    ui.separator();
    item(ui, "Preferences...", Action::Preferences, actions);
    item(ui, "About eleven", Action::About, actions);
    item(ui, "Exit", Action::Quit, actions);
}

/// Draws and handles the main section at `origin`.
pub fn show(
    ui: &mut Ui,
    ctl: &mut Controller,
    st: &mut UiState,
    look: &dyn Look,
    origin: Pos2,
) -> Vec<Action> {
    let mut actions = Vec::new();
    if ctl.config().shaded {
        show_shaded(ui, ctl, look, origin, &mut actions);
        return actions;
    }
    let painter = ui.painter().clone();
    let section = Rect::from_min_size(origin, vec2(layout::WIDTH, layout::MAIN_HEIGHT));

    // Background (lowest layer) carries the right-click menu.
    let bg = ui.interact(section, Id::unique("main_bg"), Sense::click());
    egui::Popup::context_menu(&bg).show(|ui| main_menu(ui, ctl, &mut actions));
    look.main_background(&painter, origin);

    // Title bar.
    look.title_bar(&painter, l::title_bar(origin), TitleKind::Main, true);
    let title = drag_area(ui, Id::unique("main_title"), l::title_bar(origin));
    if title.double_clicked() {
        actions.push(Action::ToggleShade);
    }
    title_buttons(ui, look, origin, &mut actions, ctl);

    // Clock (click toggles elapsed/remaining) and status light.
    let clock_resp = widgets::button(ui, Id::unique("clock"), l::clock(origin), "Time display");
    if clock_resp.clicked() {
        ctl.toggle_time_mode();
    }
    let clock_text = format_duration(ctl.position());
    clock_resp.on_hover_text(format!("{clock_text} (click: elapsed/remaining)"));
    let state = ctl.state();
    // While paused the digits blink, as in XMMS.
    let blank = state == PlaybackState::Paused && (st.blink % 1.0) > 0.5;
    if state.is_active() {
        look.clock(&painter, origin, &ctl.clock(), blank);
    } else {
        look.clock(
            &painter,
            origin,
            &eleven_core::format::ClockDisplay {
                negative: false,
                digits: [0; 4],
            },
            true,
        );
    }
    look.status(&painter, l::status(origin), state);

    // Visualiser (click cycles mode).
    let vis_rect = l::vis(origin);
    if widgets::button(ui, Id::unique("vis"), vis_rect, "Visualizer")
        .on_hover_text("Click to change visualization")
        .clicked()
    {
        ctl.cycle_vis_mode();
    }
    draw_vis(&painter, vis_rect, ctl, look);

    // Title display, bitrate and sample rate.
    let title_rect = l::title(origin);
    look.lcd_text(&painter, title_rect, &ctl.marquee_visible(TITLE_CHARS));
    let title_resp = ui.interact(title_rect, Id::unique("title_text"), Sense::click());
    let full_title = ctl.title_text();
    title_resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, &full_title));
    if title_resp.double_clicked() {
        actions.push(Action::FileInfo);
    }
    let info = ctl.track_info().cloned();
    look.lcd_text(
        &painter,
        l::kbps(origin).expand2(vec2(2.0, 2.0)),
        &kbps_text(info.as_ref().and_then(|i| i.bitrate_kbps)),
    );
    look.lcd_text(
        &painter,
        l::khz(origin).expand2(vec2(2.0, 2.0)),
        &khz_text(info.as_ref().map(|i| i.sample_rate)),
    );
    look.channels(
        &painter,
        l::mono(origin),
        l::stereo(origin),
        info.as_ref().map(eleven_core::TrackInfo::is_stereo),
    );

    // Volume and balance.
    let vol = ctl.config().volume;
    let v = widgets::slider(
        ui,
        Id::unique("volume"),
        l::volume(origin),
        "Volume",
        Axis::Horizontal,
        14.0,
        f64::from(vol),
    );
    if let Some(value) = v.value {
        ctl.set_volume(value);
    }
    let pressed = v.response.is_pointer_button_down_on();
    v.response
        .on_hover_text(volume_tooltip(ctl.config().volume));
    look.volume(&painter, l::volume(origin), ctl.config().volume, pressed);

    let bal = ctl.config().balance;
    let b = widgets::slider(
        ui,
        Id::unique("balance"),
        l::balance(origin),
        "Balance",
        Axis::Horizontal,
        14.0,
        f64::from(bal),
    );
    if let Some(value) = b.value {
        ctl.set_balance(value * 2.0 - 1.0);
    }
    let pressed = b.response.is_pointer_button_down_on();
    b.response
        .on_hover_text(balance_tooltip(ctl.config().balance));
    look.balance(&painter, l::balance(origin), ctl.config().balance, pressed);

    // EQ / PL toggles.
    let cfg = ctl.config().clone();
    for (rect, which, on, label, action) in [
        (
            l::eq_toggle(origin),
            Toggle::Equalizer,
            cfg.show_equalizer,
            "Equalizer",
            Action::ToggleEqualizer,
        ),
        (
            l::pl_toggle(origin),
            Toggle::Playlist,
            cfg.show_playlist,
            "Playlist",
            Action::TogglePlaylist,
        ),
    ] {
        let r = widgets::toggle(ui, Id::unique(label), rect, label, on);
        look.toggle(&painter, rect, which, on, r.is_pointer_button_down_on());
        if r.clicked() {
            actions.push(action);
        }
    }

    // Seek bar.
    let pos_rect = l::posbar(origin);
    let seekable = ctl.seekable();
    let progress = ctl.progress();
    let s = widgets::slider(
        ui,
        Id::unique("seek"),
        pos_rect,
        "Seek",
        Axis::Horizontal,
        29.0,
        f64::from(progress.unwrap_or(0.0)),
    );
    if seekable {
        if let Some(value) = s.value {
            st.seek_drag = Some(value);
        }
        if s.released
            && let Some(f) = st.seek_drag.take()
        {
            ctl.seek_fraction(f);
        }
    } else {
        st.seek_drag = None;
    }
    let shown = st.seek_drag.or(progress).filter(|_| seekable);
    if let (Some(f), Some(d)) = (st.seek_drag, ctl.duration()) {
        s.response
            .on_hover_text(format!("Seek to {}", format_duration(d.mul_f32(f))));
    }
    look.posbar(&painter, pos_rect, shown, st.seek_drag.is_some());

    // Transport.
    let glyphs = [
        (Glyph::Previous, "Previous", Action::Previous),
        (Glyph::Play, "Play", Action::Play),
        (Glyph::Pause, "Pause", Action::Pause),
        (Glyph::Stop, "Stop", Action::Stop),
        (Glyph::Next, "Next", Action::Next),
    ];
    for (rect, (glyph, label, action)) in l::transport(origin).into_iter().zip(glyphs) {
        let r = widgets::button(ui, Id::unique(label), rect, label);
        look.transport(&painter, rect, glyph, r.is_pointer_button_down_on());
        if r.clicked() {
            actions.push(action);
        }
    }
    let eject = widgets::button(ui, Id::unique("Open"), l::eject(origin), "Open");
    look.transport(
        &painter,
        l::eject(origin),
        Glyph::Eject,
        eject.is_pointer_button_down_on(),
    );
    if eject.on_hover_text("Add files (L)").clicked() {
        actions.push(Action::AddFiles);
    }

    // Shuffle / repeat.
    for (rect, which, on, label, action) in [
        (
            l::shuffle(origin),
            Toggle::Shuffle,
            cfg.shuffle,
            "Shuffle",
            Action::ToggleShuffle,
        ),
        (
            l::repeat(origin),
            Toggle::Repeat,
            cfg.repeat,
            "Repeat",
            Action::ToggleRepeat,
        ),
    ] {
        let r = widgets::toggle(ui, Id::unique(label), rect, label, on);
        look.toggle(&painter, rect, which, on, r.is_pointer_button_down_on());
        if r.clicked() {
            actions.push(action);
        }
    }
    actions
}

fn title_buttons(
    ui: &mut Ui,
    look: &dyn Look,
    origin: Pos2,
    actions: &mut Vec<Action>,
    ctl: &mut Controller,
) {
    let painter = ui.painter().clone();
    let menu = widgets::button(ui, Id::unique("Menu"), l::menu(origin), "Menu");
    look.title_button(
        &painter,
        l::menu(origin),
        TitleButton::Menu,
        menu.is_pointer_button_down_on(),
    );
    egui::Popup::menu(&menu).show(|ui| main_menu(ui, ctl, actions));
    for (rect, which, label, action) in [
        (
            l::minimize(origin),
            TitleButton::Minimize,
            "Minimize",
            Action::Minimize,
        ),
        (
            l::shade(origin),
            TitleButton::Shade,
            "Shade",
            Action::ToggleShade,
        ),
        (l::close(origin), TitleButton::Close, "Close", Action::Quit),
    ] {
        let r = widgets::button(ui, Id::unique(label), rect, label);
        look.title_button(&painter, rect, which, r.is_pointer_button_down_on());
        if r.clicked() {
            actions.push(action);
        }
    }
}

fn show_shaded(
    ui: &mut Ui,
    ctl: &mut Controller,
    look: &dyn Look,
    origin: Pos2,
    actions: &mut Vec<Action>,
) {
    let painter = ui.painter().clone();
    let bar = Rect::from_min_size(origin, vec2(layout::WIDTH, layout::SHADE_HEIGHT));
    look.title_bar(&painter, bar, TitleKind::Shade, true);
    let drag = drag_area(ui, Id::unique("main_title"), bar);
    egui::Popup::context_menu(&drag).show(|ui| main_menu(ui, ctl, actions));
    if drag.double_clicked() {
        actions.push(Action::ToggleShade);
    }
    title_buttons(ui, look, origin, actions, ctl);
    // Mini display: time and a slice of the title.
    let time = if ctl.state().is_active() {
        format_duration(ctl.position())
    } else {
        String::new()
    };
    look.lcd_text(
        &painter,
        Rect::from_min_size(origin + vec2(20.0, 2.0), vec2(40.0, 10.0)),
        &time,
    );
    look.lcd_text(
        &painter,
        Rect::from_min_size(origin + vec2(62.0, 2.0), vec2(100.0, 10.0)),
        &ctl.marquee_visible(20),
    );
    let mini = [
        (Glyph::Previous, "Previous", Action::Previous),
        (Glyph::Play, "Play", Action::Play),
        (Glyph::Pause, "Pause", Action::Pause),
        (Glyph::Stop, "Stop", Action::Stop),
        (Glyph::Next, "Next", Action::Next),
    ];
    for (i, (glyph, label, action)) in mini.into_iter().enumerate() {
        #[allow(clippy::cast_precision_loss)]
        let rect = Rect::from_min_size(origin + vec2(168.0 + 10.0 * i as f32, 3.0), vec2(9.0, 9.0));
        let r = widgets::button(ui, Id::unique(label), rect, label);
        look.transport(&painter, rect, glyph, r.is_pointer_button_down_on());
        if r.clicked() {
            actions.push(action);
        }
    }
}

/// Paints the visualiser into `rect`.
pub fn draw_vis(p: &egui::Painter, rect: Rect, ctl: &Controller, look: &dyn Look) {
    let colors = look.vis_colors();
    p.rect_filled(rect, 0.0, colors.background);
    match ctl.config().vis_mode {
        VisMode::Off => {}
        VisMode::Spectrum => {
            let (levels, peaks) = ctl.spectrum();
            #[allow(clippy::cast_precision_loss)]
            let bar_w = rect.width() / levels.len().max(1) as f32;
            for (i, (&level, &peak)) in levels.iter().zip(peaks).enumerate() {
                #[allow(clippy::cast_precision_loss)]
                let x = rect.left() + i as f32 * bar_w;
                // Truncation: number of lit 1-point rows.
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let lit = (level * rect.height()).round() as usize;
                for row in 0..lit.min(16) {
                    #[allow(clippy::cast_precision_loss)]
                    let y = rect.bottom() - row as f32 - 1.0;
                    let cell = Rect::from_min_size(pos2(x, y), vec2((bar_w - 1.0).max(1.0), 1.0));
                    p.rect_filled(cell, 0.0, colors.bars[row.min(15)]);
                }
                if peak > 0.01 {
                    let y = (rect.bottom() - peak * rect.height()).max(rect.top());
                    let cell = Rect::from_min_size(pos2(x, y), vec2((bar_w - 1.0).max(1.0), 1.0));
                    p.rect_filled(cell, 0.0, colors.peak);
                }
            }
        }
        VisMode::Oscilloscope => {
            let scope = ctl.scope();
            #[allow(clippy::cast_precision_loss)]
            let step = rect.width() / scope.len().max(1) as f32;
            let points: Vec<Pos2> = scope
                .iter()
                .enumerate()
                .map(|(i, &s)| {
                    #[allow(clippy::cast_precision_loss)]
                    let x = rect.left() + i as f32 * step;
                    pos2(x, rect.center().y - s * (rect.height() / 2.0 - 1.0))
                })
                .collect();
            p.line(points, Stroke::new(1.0, colors.scope));
        }
    }
}
