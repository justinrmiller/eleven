//! Modal dialogs and tool windows: jump to file, open location, save preset, skins,
//! preferences, file info and about.

use std::path::{MAIN_SEPARATOR, Path};

use egui::{Context, Id, Key, Modal, ScrollArea, TextEdit, Window};
use egui::{Ui, ViewportBuilder, ViewportClass, ViewportId};
use eleven_core::format::{format_optional_duration, playlist_time_text};

use super::UiState;
use super::actions::{Action, SHORTCUTS};
use crate::controller::Controller;

/// State of the jump-to-file dialog.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct JumpDialog {
    /// Search text.
    pub query: String,
    /// Highlighted result (index into the result list).
    pub selected: usize,
}

/// Maximum results listed by the jump dialog.
const MAX_RESULTS: usize = 200;

/// Shows every open dialog. Returns actions for the app to perform.
pub fn show(ctx: &Context, ctl: &mut Controller, st: &mut UiState) -> Vec<Action> {
    let mut actions = Vec::new();
    jump(ctx, ctl, st);
    location(ctx, ctl, st);
    preset_name(ctx, ctl, st);
    skins(ctx, ctl, st, &mut actions);
    preferences(ctx, ctl, st, &mut actions);
    file_info(ctx, ctl, st);
    about(ctx, st);
    actions
}

fn jump(ctx: &Context, ctl: &mut Controller, st: &mut UiState) {
    let Some(dialog) = st.jump.as_mut() else {
        return;
    };
    let results = ctl.playlist().search(&dialog.query);
    let (up, down, enter) = ctx.input(|i| {
        (
            i.key_pressed(Key::ArrowUp),
            i.key_pressed(Key::ArrowDown),
            i.key_pressed(Key::Enter),
        )
    });
    // Only the first MAX_RESULTS are listed, so the highlight (and Enter) stays within them.
    let last = results.len().min(MAX_RESULTS).saturating_sub(1);
    if down {
        dialog.selected = (dialog.selected + 1).min(last);
    }
    if up {
        dialog.selected = dialog.selected.saturating_sub(1);
    }
    dialog.selected = dialog.selected.min(last);
    let mut chosen = None;
    let modal = Modal::new(Id::unique("jump_dialog")).show(ctx, |ui| {
        ui.set_width(240.0);
        ui.heading("Jump to file");
        let edit = ui.add(
            TextEdit::singleline(&mut dialog.query)
                .hint_text("Type to search")
                .id(Id::unique("jump_query")),
        );
        edit.request_focus();
        if edit.changed() {
            dialog.selected = 0;
        }
        ScrollArea::vertical().max_height(180.0).show(ui, |ui| {
            for (n, &index) in results.iter().take(MAX_RESULTS).enumerate() {
                let Some(entry) = ctl.playlist().get(index) else {
                    continue;
                };
                let label = format!("{}. {}", index + 1, entry.display_title());
                let r = ui.selectable_label(n == dialog.selected, label);
                if n == dialog.selected {
                    r.scroll_to_me(None);
                }
                if r.clicked() {
                    dialog.selected = n;
                }
                if r.double_clicked() {
                    chosen = Some(index);
                }
            }
        });
        ui.label(format!(
            "{} of {} match",
            results.len(),
            ctl.playlist().len()
        ));
        ui.horizontal(|ui| {
            if ui.button("Jump").clicked() {
                chosen = results.get(dialog.selected).copied();
            }
            if ui.button("Close").clicked() {
                chosen = Some(usize::MAX);
            }
        });
    });
    if enter {
        chosen = results.get(dialog.selected).copied().or(chosen);
    }
    match chosen {
        Some(usize::MAX) => st.jump = None,
        Some(index) => {
            ctl.jump_to(index);
            st.jump = None;
        }
        None if modal.should_close() => st.jump = None,
        None => {}
    }
}

fn location(ctx: &Context, ctl: &mut Controller, st: &mut UiState) {
    let Some(text) = st.location.as_mut() else {
        return;
    };
    // Read Enter before the text field: it surrenders focus on Enter, and requesting focus
    // again below would hide that from `lost_focus`.
    let mut done = ctx.input(|i| i.key_pressed(Key::Enter)).then_some(true);
    let modal = Modal::new(Id::unique("location_dialog")).show(ctx, |ui| {
        ui.set_width(240.0);
        ui.heading("Open location");
        ui.label("Stream URL (http/https) or file path:");
        let edit = ui.add(
            TextEdit::singleline(text)
                .hint_text("http://...")
                .id(Id::unique("location_text")),
        );
        edit.request_focus();
        ui.horizontal(|ui| {
            if ui.button("Play location").clicked() {
                done = Some(true);
            }
            if ui.button("Cancel").clicked() {
                done = Some(false);
            }
        });
    });
    match done {
        Some(true) => {
            let text = st.location.take().unwrap_or_default();
            if ctl.add_location(&text) {
                ctl.play_index(ctl.playlist().len() - 1);
            }
        }
        Some(false) => st.location = None,
        None if modal.should_close() => st.location = None,
        None => {}
    }
}

fn preset_name(ctx: &Context, ctl: &mut Controller, st: &mut UiState) {
    let Some(name) = st.preset_name.as_mut() else {
        return;
    };
    let mut done = ctx.input(|i| i.key_pressed(Key::Enter)).then_some(true);
    let modal = Modal::new(Id::unique("preset_dialog")).show(ctx, |ui| {
        ui.set_width(220.0);
        ui.heading("Save preset");
        let edit = ui.add(
            TextEdit::singleline(name)
                .hint_text("Preset name")
                .id(Id::unique("preset_name")),
        );
        edit.request_focus();
        ui.horizontal(|ui| {
            if ui.button("Save").clicked() {
                done = Some(true);
            }
            if ui.button("Cancel").clicked() {
                done = Some(false);
            }
        });
    });
    match done {
        Some(true) => {
            let name = st.preset_name.take().unwrap_or_default();
            ctl.save_preset(&name);
        }
        Some(false) => st.preset_name = None,
        None if modal.should_close() => st.preset_name = None,
        None => {}
    }
}

fn preferences(ctx: &Context, ctl: &mut Controller, st: &mut UiState, actions: &mut Vec<Action>) {
    if !st.show_prefs {
        return;
    }
    let devices: &[String] = st.devices.get_or_insert_with(|| ctl.output_devices());
    let mut open = true;
    Window::new("Preferences")
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .default_width(250.0)
        .show(ctx, |ui| {
            ui.heading("Audio");
            let current = ctl.config().output_device.clone();
            let mut choice = current.clone();
            egui::ComboBox::from_id_salt("output_device")
                .selected_text(
                    current
                        .clone()
                        .unwrap_or_else(|| "System default".to_owned()),
                )
                .width(220.0)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut choice, None, "System default");
                    for d in devices {
                        ui.selectable_value(&mut choice, Some(d.clone()), d);
                    }
                });
            if choice != current {
                ctl.set_output_device(choice);
            }
            ui.label(format!("Playing on: {}", ctl.output_name()));
            ui.separator();
            ui.heading("Display");
            let cfg = ctl.config_mut();
            ui.checkbox(&mut cfg.scroll_title, "Scroll long titles");
            ui.checkbox(&mut cfg.decorations, "Use the system window frame");
            let mut on_top = cfg.always_on_top;
            if ui.checkbox(&mut on_top, "Always on top").changed() {
                actions.push(Action::ToggleAlwaysOnTop);
            }
            let mut double = cfg.double_size;
            if ui.checkbox(&mut double, "Double size").changed() {
                actions.push(Action::ToggleDoubleSize);
            }
            ui.horizontal(|ui| {
                ui.label("Playlist rows");
                ui.add(egui::DragValue::new(&mut cfg.playlist_rows).range(3..=40));
            });
        });
    st.show_prefs = open;
}

/// Whether one of this file's modal dialogs is open (they own the keyboard). egui's own record
/// of modals lags a frame behind, so the app checks this too.
#[must_use]
pub fn modal_open(st: &UiState) -> bool {
    st.jump.is_some() || st.location.is_some() || st.preset_name.is_some()
}

/// The skin browser, in its own window next to the player: the built-in look, the skins
/// folder's `.wsz` files and any skin loaded from elsewhere. Choosing a skin applies it at once
/// and the browser stays open, so skins can be compared on the player beside it.
///
/// A separate window rather than a dialog inside the player: the player can be as small as
/// 275x116 (or 14 high when shaded), too small to hold the list.
fn skins(ctx: &Context, ctl: &Controller, st: &mut UiState, actions: &mut Vec<Action>) {
    if !st.show_skins {
        return;
    }
    let mut builder = ViewportBuilder::default()
        .with_title("Skins")
        .with_inner_size([250.0, 300.0])
        .with_min_inner_size([200.0, 170.0]);
    if let Some(pos) = st.skins_window_pos {
        builder = builder.with_position(pos);
    }
    let close = ctx.show_viewport_immediate(
        ViewportId::from_hash_of("eleven-skins"),
        builder,
        |ui, class| {
            let mut close = false;
            if class == ViewportClass::EmbeddedWindow {
                skins_content(ui, ctl, st, actions, &mut close);
            } else {
                egui::Frame::central_panel(ui.style())
                    .show(ui, |ui| skins_content(ui, ctl, st, actions, &mut close));
            }
            close || ui.input(|i| i.viewport().close_requested() || i.key_pressed(Key::Escape))
        },
    );
    if close {
        st.show_skins = false;
    }
}

fn skins_content(
    ui: &mut Ui,
    ctl: &Controller,
    st: &mut UiState,
    actions: &mut Vec<Action>,
    close: &mut bool,
) {
    let skins: &[std::path::PathBuf] = st.skins.as_deref().unwrap_or_default();
    let active = ctl.config().skin.as_deref();
    // Keep the buttons in view: the list scrolls in whatever height is left.
    let reserve = 90.0;
    ScrollArea::vertical()
        .max_height((ui.available_height() - reserve).max(48.0))
        .auto_shrink([false, true])
        .show(ui, |ui| {
            if ui.radio(active.is_none(), "Built-in").clicked() {
                st.pending_skin = Some(None);
            }
            for path in skins {
                let name = path
                    .file_stem()
                    .map_or_else(String::new, |s| s.to_string_lossy().into_owned());
                if ui.radio(active == Some(path.as_path()), name).clicked() {
                    st.pending_skin = Some(Some(path.clone()));
                }
            }
            // A skin loaded from outside the skins folder.
            if let Some(path) = active.filter(|p| !skins.iter().any(|s| s == p)) {
                let _ = ui.radio(true, path.display().to_string());
            }
        });
    if let Some(err) = &st.skin_error {
        ui.colored_label(egui::Color32::LIGHT_RED, err);
    }
    if let Some(hint) = &st.skins_hint {
        ui.weak(hint);
    }
    ui.horizontal(|ui| {
        if ui.button("Load skin file...").clicked() {
            actions.push(Action::LoadSkin);
        }
        if ui.button("Close").clicked() {
            *close = true;
        }
    });
}

/// `path` with `home` shown as `~`, the way people know their config folders.
#[must_use]
pub fn home_relative(path: &Path, home: Option<&Path>) -> String {
    match home.and_then(|home| path.strip_prefix(home).ok()) {
        Some(rest) => format!("~{MAIN_SEPARATOR}{}", rest.display()),
        None => path.display().to_string(),
    }
}

fn file_info(ctx: &Context, ctl: &Controller, st: &mut UiState) {
    let Some(id) = st.info else {
        return;
    };
    let Some((index, entry)) = ctl
        .playlist()
        .index_of(id)
        .and_then(|i| ctl.playlist().get(i).map(|e| (i, e)))
    else {
        // The entry was removed.
        st.info = None;
        return;
    };
    let mut open = true;
    Window::new("File info")
        .open(&mut open)
        .collapsible(false)
        .default_width(250.0)
        .show(ctx, |ui| {
            egui::Grid::new("info_grid")
                .num_columns(2)
                .striped(true)
                .show(ui, |ui| {
                    ui.label("Location");
                    ui.label(entry.location.as_playlist_string());
                    ui.end_row();
                    ui.label("Title");
                    ui.label(entry.title.clone().unwrap_or_default());
                    ui.end_row();
                    ui.label("Artist");
                    ui.label(entry.artist.clone().unwrap_or_default());
                    ui.end_row();
                    ui.label("Length");
                    ui.label(format_optional_duration(entry.duration));
                    ui.end_row();
                    if ctl.playlist().current_index() == Some(index)
                        && let Some(info) = ctl.track_info()
                    {
                        ui.label("Codec");
                        ui.label(&info.codec);
                        ui.end_row();
                        ui.label("Sample rate");
                        ui.label(format!("{} Hz", info.sample_rate));
                        ui.end_row();
                        ui.label("Channels");
                        ui.label(info.channels.to_string());
                        ui.end_row();
                        ui.label("Bitrate");
                        ui.label(
                            info.bitrate_kbps
                                .map_or_else(|| "unknown".to_owned(), |k| format!("{k} kbit/s")),
                        );
                        ui.end_row();
                    }
                });
        });
    if !open {
        st.info = None;
    }
}

fn about(ctx: &Context, st: &mut UiState) {
    if !st.show_about {
        return;
    }
    let mut open = true;
    Window::new("About eleven")
        .open(&mut open)
        .collapsible(false)
        .default_width(250.0)
        .show(ctx, |ui| {
            ui.heading(format!("eleven {}", env!("CARGO_PKG_VERSION")));
            ui.label("An XMMS-style audio player written in Rust and egui.");
            ui.label("These go to eleven.");
            ui.separator();
            ScrollArea::vertical().max_height(200.0).show(ui, |ui| {
                egui::Grid::new("shortcuts").num_columns(2).show(ui, |ui| {
                    for (keys, what) in SHORTCUTS {
                        ui.monospace(*keys);
                        ui.label(*what);
                        ui.end_row();
                    }
                });
            });
        });
    st.show_about = open;
}

/// The footer text, exposed for tests.
#[must_use]
pub fn playlist_footer(ctl: &Controller) -> String {
    playlist_time_text(
        ctl.playlist().selected_duration(),
        ctl.playlist().total_duration(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_under_home_use_a_tilde() {
        let home = Path::new("/home/someone");
        let skins = home.join(".config").join("eleven").join("skins");
        assert_eq!(
            home_relative(&skins, Some(home)),
            format!(
                "~{MAIN_SEPARATOR}{}",
                Path::new(".config/eleven/skins").display()
            )
        );
        assert_eq!(
            home_relative(Path::new("/elsewhere/skins"), Some(home)),
            "/elsewhere/skins"
        );
        assert_eq!(home_relative(&skins, None), skins.display().to_string());
    }
}
