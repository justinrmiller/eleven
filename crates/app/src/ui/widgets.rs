//! Interaction helpers for custom-painted controls, with accessibility labels so the UI is usable
//! with screen readers and testable with egui_kittest.

use egui::{Id, Rect, Response, Role, Sense, Ui, WidgetInfo};

/// A clickable region announced as a button called `label`.
pub fn button(ui: &Ui, id: Id, rect: Rect, label: &str) -> Response {
    let response = ui.interact(rect, id, Sense::click());
    let label = label.to_owned();
    response.widget_info(|| WidgetInfo::labeled(Role::Button, true, &label));
    response
}

/// A two-state button announced with its `selected` state.
pub fn toggle(ui: &Ui, id: Id, rect: Rect, label: &str, on: bool) -> Response {
    let response = ui.interact(rect, id, Sense::click());
    let label = label.to_owned();
    response.widget_info(|| WidgetInfo::selected(Role::CheckBox, true, on, &label));
    response
}

/// Orientation of a [`slider`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Axis {
    /// Left (0) to right (1).
    Horizontal,
    /// Bottom (0) to top (1).
    Vertical,
}

/// Result of a slider interaction.
#[derive(Debug, Clone)]
pub struct SliderResponse {
    /// The underlying response.
    pub response: Response,
    /// The new normalised value while the pointer is pressed or dragging.
    pub value: Option<f32>,
    /// `true` on the frame the pointer was released (commit seeks here).
    pub released: bool,
}

/// A drag/click slider over `rect` with a knob of `knob` points along the axis.
///
/// `display` is the value announced to assistive technology.
pub fn slider(
    ui: &Ui,
    id: Id,
    rect: Rect,
    label: &str,
    axis: Axis,
    knob: f32,
    display: f64,
) -> SliderResponse {
    let response = ui.interact(rect, id, Sense::click_and_drag());
    let label = label.to_owned();
    response.widget_info(|| WidgetInfo::slider(true, display, &label));
    let active = response.is_pointer_button_down_on() || response.dragged() || response.clicked();
    let value = if active {
        response.interact_pointer_pos().map(|pos| match axis {
            Axis::Horizontal => ((pos.x - rect.left() - knob / 2.0)
                / (rect.width() - knob).max(1.0))
            .clamp(0.0, 1.0),
            Axis::Vertical => {
                1.0 - ((pos.y - rect.top() - knob / 2.0) / (rect.height() - knob).max(1.0))
                    .clamp(0.0, 1.0)
            }
        })
    } else {
        None
    };
    let released = response.drag_stopped() || response.clicked();
    SliderResponse {
        response,
        value,
        released,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{pos2, vec2};
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;

    #[test]
    fn buttons_and_toggles_are_labelled() {
        let mut clicks = 0;
        let mut harness = Harness::new_ui(|ui| {
            let r = Rect::from_min_size(pos2(10.0, 10.0), vec2(40.0, 20.0));
            if button(ui, Id::unique("b"), r, "Press me").clicked() {
                clicks += 1;
            }
            let r2 = Rect::from_min_size(pos2(60.0, 10.0), vec2(40.0, 20.0));
            toggle(ui, Id::unique("t"), r2, "Switch", true);
        });
        harness.get_by_label("Press me").click();
        harness.run();
        assert!(harness.query_by_label("Switch").is_some());
        drop(harness);
        assert_eq!(clicks, 1);
    }

    #[test]
    fn horizontal_slider_maps_pointer_to_value() {
        let mut got = None;
        let mut harness = Harness::new_ui(|ui| {
            let r = Rect::from_min_size(pos2(0.0, 0.0), vec2(110.0, 10.0));
            let s = slider(ui, Id::unique("s"), r, "Level", Axis::Horizontal, 10.0, 0.0);
            if let Some(v) = s.value {
                got = Some(v);
            }
        });
        harness.get_by_label("Level").click();
        harness.run();
        drop(harness);
        assert!((got.unwrap() - 0.5).abs() < 0.01, "{got:?}");
    }

    #[test]
    fn vertical_slider_is_bottom_up() {
        let mut got = None;
        let mut harness = Harness::new_ui(|ui| {
            let r = Rect::from_min_size(pos2(0.0, 0.0), vec2(10.0, 50.0));
            let s = slider(ui, Id::unique("v"), r, "Gain", Axis::Vertical, 10.0, 0.0);
            if let Some(v) = s.value {
                got = Some(v);
            }
        });
        harness.get_by_label("Gain").click();
        harness.run();
        drop(harness);
        assert!((got.unwrap() - 0.5).abs() < 0.01);
    }
}
