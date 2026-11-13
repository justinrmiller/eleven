# Notes for agents

## Workarounds to remove once upstream fixes land

When you upgrade `eframe` / `egui` / `egui-winit`, check each item below. If the upstream fix
is in, delete the workaround, its test and its entry here.

### macOS: eframe keeps a restored window "minimized" (eframe 0.37.0)

**Symptom without the workaround:** on macOS, after the title bar's minimize button and a
restore from the Dock, the window stays frozen and ignores clicks while playback carries on.

**Cause:**
- `ViewportCommand::Minimized(true)` sets `ViewportInfo::minimized = Some(true)` (egui-winit,
  `process_viewport_command`).
- On macOS, `egui_winit::update_viewport_info` reads the real minimized state only when the
  window is created. It skips it at runtime because of a deadlock
  ([egui#3494](https://github.com/emilk/egui/issues/3494)), so nothing ever clears the flag.
- eframe 0.37 runs only `App::logic`, with no UI pass and no input, for viewports whose
  `ViewportInfo::visible()` is false. That check includes `minimized`
  (`run_ui_and_paint` in `eframe/src/native/glow_integration.rs`).

**Workaround:** `ElevenApp::clear_stale_minimized` in `crates/app/src/ui/mod.rs`, called from
`tick`, with `UiState::minimized_lost_focus` set by `Action::Minimize`. After eleven
minimizes the window, it waits for the window to lose focus and regain it (restored), then
sends `ViewportCommand::Minimized(false)` to clear the stale flag. Test:
`restoring_a_minimized_window_brings_the_ui_back` in `crates/app/tests/ui.rs`.

**How to tell whether a new release fixes it:**
1. Read the new `egui-winit` source: does `update_viewport_info` refresh
   `viewport_info.minimized` on macOS at runtime, or does winit report minimize/restore some
   other way that updates the flag?
2. Read the new eframe `run_ui_and_paint`: does it still skip the UI when
   `info.visible()` is false because of `minimized`?
3. Search the egui changelog and issues for "minimized" together with "macOS".
4. Confirm on a Mac: comment out the `clear_stale_minimized` call, run the app, click the
   title bar's minimize button, restore the window from the Dock, and check that it redraws
   and responds to clicks.

If the fix has landed, remove `clear_stale_minimized`, the `minimized_lost_focus` field, the
extra line in the `Action::Minimize` arm, the test above, and this section.
