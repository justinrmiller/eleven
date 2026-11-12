//! The equaliser section: on/off, presets, response graph, preamp and ten band sliders.

use egui::{Id, Pos2, Ui};
use eleven_core::eq::{BAND_LABELS, BANDS, MAX_GAIN_DB};

use super::UiState;
use super::actions::Action;
use super::layout::eq as l;
use super::look::{Look, TitleButton, TitleKind, Toggle};
use super::main_window::drag_area;
use super::widgets::{self, Axis};
use crate::controller::Controller;

/// Knob height of the vertical sliders.
const KNOB: f32 = 11.0;

#[cfg(test)]
fn db_to_unit(db: f32) -> f32 {
    (db + MAX_GAIN_DB) / (2.0 * MAX_GAIN_DB)
}

fn unit_to_db(v: f32) -> f32 {
    // Snap to whole dB near zero so the "flat" position is easy to hit.
    let db = v * 2.0 * MAX_GAIN_DB - MAX_GAIN_DB;
    if db.abs() < 0.75 {
        0.0
    } else {
        (db * 2.0).round() / 2.0
    }
}

/// Accessible name of a band slider, e.g. `"1K Hz"`.
#[must_use]
pub fn band_label(i: usize) -> String {
    format!("{} Hz", BAND_LABELS[i])
}

/// The presets popup.
fn presets_menu(ui: &mut Ui, ctl: &mut Controller, st: &mut UiState) {
    ui.label("Load preset");
    for preset in ctl.presets() {
        if ui.button(&preset.name).clicked() {
            ctl.apply_preset(&preset);
            ui.close();
        }
    }
    ui.separator();
    if ui.button("Save preset...").clicked() {
        st.preset_name = Some(String::new());
        ui.close();
    }
    let user: Vec<String> = ctl
        .config()
        .eq_presets
        .iter()
        .map(|p| p.name.clone())
        .collect();
    if !user.is_empty() {
        ui.menu_button("Delete preset", |ui| {
            for name in user {
                if ui.button(&name).clicked() {
                    ctl.delete_preset(&name);
                    ui.close();
                }
            }
        });
    }
    if ui.button("Reset to flat").clicked() {
        ctl.reset_eq();
        ui.close();
    }
}

/// Draws and handles the equaliser section at `origin`.
pub fn show(
    ui: &mut Ui,
    ctl: &mut Controller,
    st: &mut UiState,
    look: &dyn Look,
    origin: Pos2,
) -> Vec<Action> {
    let mut actions = Vec::new();
    let painter = ui.painter().clone();
    look.eq_background(&painter, origin);
    look.title_bar(&painter, l::title_bar(origin), TitleKind::Equalizer, true);
    drag_area(ui, Id::unique("eq_title"), l::title_bar(origin));
    let close = widgets::button(
        ui,
        Id::unique("eq_close"),
        l::close(origin),
        "Close equalizer",
    );
    look.title_button(
        &painter,
        l::close(origin),
        TitleButton::Close,
        close.is_pointer_button_down_on(),
    );
    if close.clicked() {
        actions.push(Action::ToggleEqualizer);
    }

    let enabled = ctl.config().equalizer.enabled;
    let on = widgets::toggle(
        ui,
        Id::unique("eq_on"),
        l::on(origin),
        "Equalizer on",
        enabled,
    );
    look.toggle(
        &painter,
        l::on(origin),
        Toggle::EqOn,
        enabled,
        on.is_pointer_button_down_on(),
    );
    if on.clicked() {
        ctl.set_eq_enabled(!enabled);
    }

    look.eq_graph(&painter, l::graph(origin), &ctl.config().equalizer);

    let presets = widgets::button(ui, Id::unique("eq_presets"), l::presets(origin), "Presets");
    look.text_button(
        &painter,
        l::presets(origin),
        "PRESETS",
        presets.is_pointer_button_down_on(),
    );
    egui::Popup::menu(&presets).show(|ui| presets_menu(ui, ctl, st));

    // Preamp.
    let pre = ctl.config().equalizer.preamp_db;
    let s = widgets::slider(
        ui,
        Id::unique("preamp"),
        l::preamp(origin),
        "Preamp",
        Axis::Vertical,
        KNOB,
        f64::from(pre),
    );
    if let Some(v) = s.value {
        ctl.set_preamp(unit_to_db(v));
    }
    if s.response.double_clicked() {
        ctl.set_preamp(0.0);
    }
    let pressed = s.response.is_pointer_button_down_on();
    s.response.on_hover_text(format!(
        "Preamp {:+.1} dB",
        ctl.config().equalizer.preamp_db
    ));
    look.eq_slider(
        &painter,
        l::preamp(origin),
        ctl.config().equalizer.preamp_db,
        pressed,
    );

    // Bands.
    for i in 0..BANDS {
        let rect = l::band(origin, i);
        let db = ctl.config().equalizer.bands_db[i];
        let label = band_label(i);
        let s = widgets::slider(
            ui,
            Id::unique(("band", i)),
            rect,
            &label,
            Axis::Vertical,
            KNOB,
            f64::from(db),
        );
        if let Some(v) = s.value {
            ctl.set_eq_band(i, unit_to_db(v));
        }
        if s.response.double_clicked() {
            ctl.set_eq_band(i, 0.0);
        }
        let pressed = s.response.is_pointer_button_down_on();
        let db = ctl.config().equalizer.bands_db[i];
        s.response
            .on_hover_text(format!("{label}: {db:+.1} dB (double-click to reset)"));
        look.eq_slider(&painter, rect, db, pressed);
    }
    actions
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unit_db_mapping() {
        assert!((unit_to_db(1.0) - 12.0).abs() < f32::EPSILON);
        assert!((unit_to_db(0.0) + 12.0).abs() < f32::EPSILON);
        assert!(unit_to_db(0.51).abs() < f32::EPSILON, "snaps to flat");
        assert!((unit_to_db(0.75) - 6.0).abs() < f32::EPSILON);
        assert!((db_to_unit(0.0) - 0.5).abs() < f32::EPSILON);
        assert_eq!(band_label(4), "1K Hz");
    }
}
