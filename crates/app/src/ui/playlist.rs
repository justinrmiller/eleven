//! The playlist editor section.

use egui::{Align2, FontId, Id, Rect, Sense, Ui, pos2, vec2};
use eleven_core::SelectMode;
use eleven_core::format::{format_optional_duration, playlist_time_text};
use eleven_core::playlist::SortKey;

use super::UiState;
use super::actions::Action;
use super::layout::{PL_ROW, playlist as l};
use super::look::{Look, TitleButton};
use super::main_window::drag_area;
use super::widgets;
use crate::controller::Controller;

/// Accessible label of a playlist row.
#[must_use]
pub fn row_label(index: usize, title: &str) -> String {
    format!("{}. {title}", index + 1)
}

fn menu_item(ui: &mut Ui, label: &str, actions: &mut Vec<Action>, action: Action) {
    if ui.button(label).clicked() {
        actions.push(action);
        ui.close();
    }
}

fn select_mode(ui: &Ui) -> SelectMode {
    let m = ui.input(|i| i.modifiers);
    if m.shift {
        SelectMode::Range
    } else if m.command {
        SelectMode::Toggle
    } else {
        SelectMode::Single
    }
}

/// Keeps the current track visible when it changes.
fn follow_current(ctl: &Controller, st: &mut UiState, rows: usize) {
    let current = ctl.playlist().current_id();
    if current != st.followed {
        st.followed = current;
        if let Some(i) = ctl.playlist().current_index()
            && (i < st.pl_scroll || i >= st.pl_scroll + rows)
        {
            st.pl_scroll = i.saturating_sub(rows / 2);
        }
    }
}

/// Draws and handles the playlist section in `section`.
#[allow(clippy::too_many_lines)]
pub fn show(
    ui: &mut Ui,
    ctl: &mut Controller,
    st: &mut UiState,
    look: &dyn Look,
    section: Rect,
) -> Vec<Action> {
    let mut actions = Vec::new();
    let painter = ui.painter().clone();
    look.playlist_frame(&painter, section, true);
    drag_area(ui, Id::unique("pl_title"), l::title_bar(section));
    let close = widgets::button(
        ui,
        Id::unique("pl_close"),
        l::close(section),
        "Close playlist",
    );
    look.title_button(
        &painter,
        l::close(section),
        TitleButton::Close,
        close.is_pointer_button_down_on(),
    );
    if close.clicked() {
        actions.push(Action::TogglePlaylist);
    }

    let list = l::list(section);
    let rows = ctl.config().playlist_rows;
    let len = ctl.playlist().len();
    let max_scroll = len.saturating_sub(rows);
    follow_current(ctl, st, rows);

    // Mouse-wheel scrolling over the list.
    let list_resp = ui.interact(list, Id::unique("pl_list"), Sense::hover());
    if list_resp.hovered() {
        let dy = ui.input(|i| i.smooth_scroll_delta.y);
        st.scroll_accum -= dy;
        while st.scroll_accum >= PL_ROW {
            st.pl_scroll += 1;
            st.scroll_accum -= PL_ROW;
        }
        while st.scroll_accum <= -PL_ROW {
            st.pl_scroll = st.pl_scroll.saturating_sub(1);
            st.scroll_accum += PL_ROW;
        }
    }
    st.pl_scroll = st.pl_scroll.min(max_scroll);

    // Rows.
    let style = look.playlist_style();
    let clip = painter.with_clip_rect(list);
    let font = FontId::proportional(9.0);
    let current = ctl.playlist().current_id();
    let mut clicked: Option<(usize, SelectMode)> = None;
    let mut double: Option<usize> = None;
    for row in 0..rows {
        let index = st.pl_scroll + row;
        let Some(entry) = ctl.playlist().get(index) else {
            break;
        };
        #[allow(clippy::cast_precision_loss)]
        let y = list.top() + 2.0 + row as f32 * PL_ROW;
        let rect = Rect::from_min_size(pos2(list.left(), y), vec2(list.width(), PL_ROW));
        let title = entry.display_title();
        let label = row_label(index, &title);
        let resp = ui.interact(rect, Id::unique(("pl_row", entry.id())), Sense::click());
        let selected = entry.selected;
        resp.widget_info(|| {
            egui::WidgetInfo::selected(egui::Role::ListItem, true, selected, &label)
        });
        if entry.selected {
            clip.rect_filled(rect, 0.0, style.selected);
        }
        let color = if Some(entry.id()) == current {
            style.current
        } else {
            style.normal
        };
        let duration = format_optional_duration(entry.duration);
        let dur_galley = clip.layout_no_wrap(duration, font.clone(), color);
        let dur_w = dur_galley.size().x;
        clip.galley(
            pos2(rect.right() - dur_w - 2.0, rect.top()),
            dur_galley,
            color,
        );
        let text_clip = clip.with_clip_rect(Rect::from_min_max(
            rect.min,
            pos2(rect.right() - dur_w - 6.0, rect.bottom()),
        ));
        text_clip.text(
            rect.left_center() + vec2(2.0, 0.0),
            Align2::LEFT_CENTER,
            &label,
            font.clone(),
            color,
        );
        if resp.double_clicked() {
            double = Some(index);
        } else if resp.clicked() {
            clicked = Some((index, select_mode(ui)));
        }
        if resp.secondary_clicked() && !entry.selected {
            clicked = Some((index, SelectMode::Single));
        }
        egui::Popup::context_menu(&resp).show(|ui| {
            if ui.button("Play").clicked() {
                double = Some(index);
                ui.close();
            }
            menu_item(ui, "File info", &mut actions, Action::FileInfo);
            menu_item(ui, "Remove selected", &mut actions, Action::RemoveSelected);
        });
    }
    if let Some((index, mode)) = clicked {
        ctl.playlist_mut().select(index, mode);
    }
    if let Some(index) = double {
        ctl.playlist_mut().select(index, SelectMode::Single);
        ctl.play_index(index);
    }

    // Scrollbar.
    let track = l::scrollbar(section);
    let fraction = if max_scroll == 0 {
        0.0
    } else {
        #[allow(clippy::cast_precision_loss)]
        let f = st.pl_scroll as f32 / max_scroll as f32;
        f
    };
    let sb = widgets::slider(
        ui,
        Id::unique("pl_scroll"),
        track,
        "Playlist scroll",
        widgets::Axis::Vertical,
        18.0,
        f64::from(fraction),
    );
    if let Some(v) = sb.value {
        // Slider is bottom-up; the list scrolls top-down.
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            clippy::cast_precision_loss
        )]
        let target = ((1.0 - v) * max_scroll as f32).round() as usize;
        st.pl_scroll = target.min(max_scroll);
    }
    look.scrollbar(
        &painter,
        track,
        fraction,
        sb.response.is_pointer_button_down_on(),
    );

    // Footer buttons with menus.
    let labels = ["ADD", "REM", "SEL", "MISC", "LIST"];
    let names = ["Add", "Remove", "Select", "Misc", "List"];
    for (i, rect) in l::buttons(section).into_iter().enumerate() {
        let resp = widgets::button(ui, Id::unique(("pl_btn", i)), rect, names[i]);
        look.text_button(&painter, rect, labels[i], resp.is_pointer_button_down_on());
        egui::Popup::menu(&resp).show(|ui| match i {
            0 => {
                menu_item(ui, "Add files...", &mut actions, Action::AddFiles);
                menu_item(ui, "Add folder...", &mut actions, Action::AddFolder);
                menu_item(ui, "Add URL...", &mut actions, Action::OpenLocation);
            }
            1 => {
                menu_item(ui, "Remove selected", &mut actions, Action::RemoveSelected);
                if ui.button("Crop (keep selected)").clicked() {
                    ctl.playlist_mut().crop();
                    ui.close();
                }
                if ui.button("Clear playlist").clicked() {
                    ctl.playlist_mut().clear();
                    ui.close();
                }
            }
            2 => {
                menu_item(ui, "Select all", &mut actions, Action::SelectAll);
                if ui.button("Select none").clicked() {
                    ctl.playlist_mut().select_none();
                    ui.close();
                }
                if ui.button("Invert selection").clicked() {
                    ctl.playlist_mut().invert_selection();
                    ui.close();
                }
            }
            3 => {
                for (label, key) in [
                    ("Sort by title", SortKey::Title),
                    ("Sort by filename", SortKey::Filename),
                    ("Sort by path", SortKey::Path),
                    ("Sort by length", SortKey::Duration),
                ] {
                    if ui.button(label).clicked() {
                        ctl.sort(key);
                        ui.close();
                    }
                }
                if ui.button("Reverse").clicked() {
                    ctl.playlist_mut().reverse();
                    ui.close();
                }
                if ui.button("Randomize").clicked() {
                    ctl.playlist_mut().randomize();
                    ui.close();
                }
                menu_item(ui, "File info...", &mut actions, Action::FileInfo);
                menu_item(ui, "Jump to file...", &mut actions, Action::JumpToFile);
            }
            _ => {
                menu_item(ui, "Load playlist...", &mut actions, Action::LoadPlaylist);
                menu_item(ui, "Save playlist...", &mut actions, Action::SavePlaylist);
                if ui.button("New playlist").clicked() {
                    ctl.playlist_mut().clear();
                    ui.close();
                }
            }
        });
    }

    // Time readout: selected/total.
    let time = playlist_time_text(
        ctl.playlist().selected_duration(),
        ctl.playlist().total_duration(),
    );
    look.lcd_text(&painter, l::time(section), &time);
    let time_resp = ui.interact(l::time(section), Id::unique("pl_time"), Sense::hover());
    time_resp.on_hover_text("Selected / total playing time");
    actions
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_are_one_based() {
        assert_eq!(row_label(0, "Tonight"), "1. Tonight");
    }
}
