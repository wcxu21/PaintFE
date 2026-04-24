# PaintFE Bugfix & Feature Implementation Plan

## Overview

This plan addresses 7 user-reported bugs/features. Each section below documents the root cause, the fix approach, and the files requiring changes.

---

## Bug 1: Paste overlay remains when layer is deleted

### Root Cause
When a layer is deleted (via [`src/components/layers/operations.rs`](src/components/layers/operations.rs:126) `delete_layer()`), the code checks for text editing state cleanup but **never checks if a paste overlay is active** (`self.paste_overlay` on the app struct). The paste overlay lives on [`PaintFEApp`](src/app/types.rs:136) as `paste_overlay: Option<PasteOverlay>`, not on the layer itself, so deleting the layer doesn't affect it.

The layer deletion flow in [`src/app/panels.rs`](src/app/panels.rs:174-188) handles text editing cleanup after deletion but has no paste overlay handling.

### Fix
In [`src/app/panels.rs`](src/app/panels.rs:174), after the text editing cleanup block, add a check: if `self.paste_overlay.is_some()`, call `self.cancel_paste_overlay()` (which exists at [`src/app/ops/helpers.rs`](src/app/ops/helpers.rs:187)).

### Files to modify
- [`src/app/panels.rs`](src/app/panels.rs:174) — Add paste overlay cancellation after layer deletion

### Implementation steps
1. In `show_floating_layers_panel()`, after the `pending_deleted_layer` text editing cleanup block (line ~188), add:
   ```rust
   // Cancel paste overlay if the layer it was on was deleted
   if self.paste_overlay.is_some() && self.layers_panel.pending_deleted_layer.is_some() {
       self.cancel_paste_overlay();
   }
   ```

---

## Bug 2: Selection vanishes when switching project tabs

### Root Cause
In [`src/app/bootstrap.rs`](src/app/bootstrap.rs:410), `switch_to_project()` explicitly calls `project.canvas_state.clear_selection()` when leaving a project. The comment says "so it doesn't linger as an unremovable ghost when we come back."

### Fix
**Option A (Recommended):** Remove the `clear_selection()` call. The "ghost" concern can be addressed by ensuring the selection is properly invalidated when switching back (e.g., if the canvas was resized). This is the simplest fix and preserves user selection state across tab switches.

**Option B:** Save the selection mask before switching and restore it when returning. This is more complex but safer if Option A causes issues.

### Files to modify
- [`src/app/bootstrap.rs`](src/app/bootstrap.rs:410) — Remove or comment out `project.canvas_state.clear_selection()`

### Implementation steps
1. In `switch_to_project()`, remove line 410: `project.canvas_state.clear_selection();`
2. Test that switching tabs preserves selection and that returning to a project doesn't show a "ghost" selection.

---

## Bug 3: Project tab text is selectable

### Root Cause
In [`src/app/runtime/update/dialogs_menu.rs`](src/app/runtime/update/dialogs_menu.rs:2070-2072), project tabs use `egui::Label::new(text).sense(egui::Sense::click_and_drag())`. `egui::Label` by default allows text selection (click-drag highlights text), which shows the I-beam cursor.

### Fix
Replace `egui::Label` with `egui::Button` for the tab label, or use a different approach that doesn't allow text selection. Since the tab needs both `click` (switch tab) and `drag` (reorder) interactions, using `egui::Button::new(text).sense(egui::Sense::click_and_drag())` is the cleanest fix.

### Files to modify
- [`src/app/runtime/update/dialogs_menu.rs`](src/app/runtime/update/dialogs_menu.rs:2070-2072) — Replace `egui::Label::new(text)` with `egui::Button::new(text).frame(false).sense(egui::Sense::click_and_drag())`

### Implementation steps
1. Change line 2070-2072 from:
   ```rust
   let label_resp = ui.add(
       egui::Label::new(text)
           .sense(egui::Sense::click_and_drag()),
   );
   ```
   to:
   ```rust
   let label_resp = ui.add(
       egui::Button::new(text)
           .frame(false)
           .sense(egui::Sense::click_and_drag()),
   );
   ```

---

## Bug 4: Add pixel grid color/alpha option in interface settings

### Root Cause
The pixel grid is rendered in [`src/canvas/view/overlay.rs`](src/canvas/view/overlay.rs:35-36) with hardcoded colors:
```rust
let grid_outline = Color32::from_black_alpha(90);
let grid_center = Color32::from_white_alpha(100);
```
The [`AppSettings`](src/config/settings.rs:30) struct has no fields for pixel grid color or alpha. The settings UI in [`src/ui/panels/settings_window.rs`](src/ui/panels/settings_window.rs:833-877) has a "Canvas Rendering" section but no pixel grid color/alpha controls.

### Fix
1. Add `pixel_grid_outline_color: Color32` and `pixel_grid_center_color: Color32` fields to `AppSettings`
2. Add save/load serialization for these fields in `save()` and `load()` methods
3. Add UI controls in the Interface tab's Canvas Rendering section
4. Wire the settings into the pixel grid rendering code in `overlay.rs`

### Files to modify
- [`src/config/settings.rs`](src/config/settings.rs:30) — Add new fields, defaults, save/load
- [`src/ui/panels/settings_window.rs`](src/ui/panels/settings_window.rs:833) — Add color picker controls
- [`src/canvas/view/overlay.rs`](src/canvas/view/overlay.rs:35-36) — Use settings values instead of hardcoded colors
- [`src/canvas/view/core.rs`](src/canvas/view/core.rs:1007-1008) — Pass settings to `draw_pixel_grid()`

### Implementation steps
1. Add fields to `AppSettings`:
   ```rust
   pub pixel_grid_outline_color: Color32,  // default: Color32::from_black_alpha(90)
   pub pixel_grid_center_color: Color32,   // default: Color32::from_white_alpha(100)
   ```
2. Add save/load in `save()` and `load()` methods
3. Add UI in `show_interface_tab()` Canvas Rendering section with two color pickers
4. Modify `draw_pixel_grid()` signature to accept `&AppSettings` and use the settings values
5. Update the call site in `core.rs` to pass settings

---

## Bug 5: Keybinds don't persist between restarts

### Root Cause
The save/load mechanism **does exist** and works correctly:
- [`settings.save()`](src/config/settings.rs:751) writes keybindings via `self.keybindings.to_config_lines()` at line 1042
- [`settings.load()`](src/config/settings.rs:1120) parses keybindings at line 1523-1525
- The Keybinds tab in [`settings_window.rs`](src/ui/panels/settings_window.rs:1998-2000) calls `settings.save()` when "Apply" is clicked

However, the issue is likely that **settings are not loaded on startup** or the **config file path** is not being found. Need to verify:
1. That `AppSettings::load()` is called during app initialization
2. That the config file path resolves correctly on the user's system

### Fix
1. Verify that [`AppSettings::load()`](src/config/settings.rs:1120) is called during app startup (check [`src/app/bootstrap.rs`](src/app/bootstrap.rs:2) `PaintFEApp::new()`)
2. Verify the config file path resolution in [`settings_path()`](src/config/settings.rs:356)
3. If the path is correct but loading fails silently, add a debug log to show whether keybindings were loaded

### Files to investigate
- [`src/app/bootstrap.rs`](src/app/bootstrap.rs:2) — Check `new()` for settings loading
- [`src/config/settings.rs`](src/config/settings.rs:356) — Check `settings_path()` resolution
- [`src/config/settings.rs`](src/config/settings.rs:1120) — Check `load()` error handling

### Implementation steps
1. Read `bootstrap.rs` to confirm `AppSettings::load()` is called
2. If not, add the call
3. Add a debug log in `load()` to confirm keybindings are being deserialized
4. If the config file doesn't exist, `load()` returns defaults — ensure `save()` is called at least once after first launch

---

## Bug 6: Can't bind Ctrl+Shift+Z

### Root Cause
The issue is in [`is_pressed()`](src/config/keybindings.rs:733) in `keybindings.rs`. The method has three detection paths:
1. **Windows VK probe** (line 768-783) — Uses `windows_key_probe::is_vk_down()` for edge detection
2. **State-based edge detection** (line 787-799) — Uses `ctx.input(|i| i.key_down(key))` 
3. **Event consumption** (line 801-863) — Uses `consume_key()` fallback

The problem: **Ctrl+Shift+Z** conflicts with the **Undo** binding (Ctrl+Z). When `is_pressed()` is called for `BindableAction::Undo` (Ctrl+Z), the event consumption path at line 801-863 checks `combo.shift == modifiers.shift`. For Undo, `combo.shift = false`, so if Shift is held, the event is NOT consumed by Undo. However, the **state-based edge detection** (path 2) checks `i.key_down(Key::Z)` with `combo.shift == i.modifiers.shift` — for Undo, `combo.shift = false` and `i.modifiers.shift = true`, so this should NOT match.

The real issue is likely in the **rebinding UI** in [`settings_window.rs`](src/ui/panels/settings_window.rs:1862-1902). The key capture code checks `egui::Event::Key` events but may not properly capture Ctrl+Shift+Z because:
- On Windows, `egui` may translate Ctrl+Shift+Z into a different event (e.g., `Event::Text` with character 0x1A, which is the SUB character for Ctrl+Z)
- The `Event::Text` handler at line 1870 filters out alphanumeric characters, so `0x1A` would be caught by `!ch.is_ascii_alphanumeric()` but only if `is_symbol` is true — control characters are excluded by `!ch.is_control()`

### Fix
In the rebinding UI, add explicit handling for the Ctrl+Shift+Z case. The `Event::Key` handler at line 1887 catches `Key::Z` with `pressed: true`, and the modifiers (ctrl, shift) are captured from `i.modifiers`. This should work. The issue might be that egui on Windows doesn't emit a `Key::Z` event when Ctrl+Shift+Z is pressed (it might emit a `Text` event instead).

**Fix approach:** In the rebinding UI's event loop, add a check for `Event::Text` with control characters (0x00-0x1F) that correspond to Ctrl+letter combinations, and map them back to the appropriate `KeyCombo`.

### Files to modify
- [`src/ui/panels/settings_window.rs`](src/ui/panels/settings_window.rs:1862-1902) — Add handling for Ctrl+letter text events in the rebinding UI

### Implementation steps
1. In the `Event::Text` handler, add a branch for control characters (char < 0x20):
   ```rust
   egui::Event::Text(t) if !t.is_empty() => {
       let ch = t.chars().next().unwrap();
       if ch.is_control() && ch as u8 < 0x20 {
           // Ctrl+letter: map 0x01 (Ctrl+A) through 0x1A (Ctrl+Z)
           let letter_idx = ch as u8;
           if letter_idx >= 1 && letter_idx <= 26 {
               let letter = (b'a' + letter_idx - 1) as char;
               if let Some(key) = parse_key_name(&letter.to_string()) {
                   key_combo = Some(KeyCombo { ctrl: true, shift, alt, key: Some(key), text_char: None });
               }
           }
       } else if /* existing symbol check */ { ... }
   }
   ```

---

## Bug 7: Color and History panel positioning doesn't stay anchored

### Root Cause
The panels use offset tracking from screen edges:
- **History panel** ([`src/app/panels.rs`](src/app/panels.rs:367)): `history_panel_right_offset` stores `(right_offset, bottom_offset)` — anchored from right and bottom
- **Colors panel** ([`src/app/panels.rs`](src/app/panels.rs:439)): `colors_panel_left_offset` stores `(x_offset, bottom_offset)` — anchored from left and bottom

The offset is updated **after** the window renders (line 415-416, 485-486). When `screen_size_changed` is true, the panel is repositioned using the stored offset. However, the issue is that:
1. The offset is stored as `(screen_w - win_rect.min.x, screen_h - win_rect.min.y)` — this is the distance from the screen edge to the window's top-left corner
2. When the window resizes, `screen_w` and `screen_h` change, but the offset remains the same, so the panel should move proportionally
3. **BUT**: The `screen_size_changed` flag is computed in [`canvas_tail.rs`](src/app/runtime/update/canvas_tail.rs:232-234) by comparing `last_screen_size` with the current screen rect. If the window is resized by dragging the edge, `screen_size_changed` becomes true, and the panel is repositioned using the stored offset — which should work correctly.

The actual bug might be that **the offset is not persisted** between sessions (it's stored in memory only, not in `AppSettings`), so after restart, panels reset to default positions. But the user says "when window is resized/moved" — this is a runtime issue.

**Likely root cause:** The `screen_size_changed` flag forces repositioning using the stored offset, but the offset was calculated from the **previous** frame's window position. When the user drags a panel, the offset is updated. But when the main window is resized, the panel's `current_pos()` is set to the clamped position, which **overrides** any egui-internal window position tracking. This means the panel position is always forced to the offset-based position on resize, which is correct. However, if the user **drags the panel** and then the window is resized, the panel snaps back to the offset-based position (because `screen_size_changed` is true), which might not be where the user dragged it to.

**The real fix:** The offset should be updated **before** the window renders (not after), so that when `screen_size_changed` forces repositioning, it uses the correct offset. Or, the offset should be updated continuously (every frame, not just on resize) so that user drags are always captured.

### Fix
Update the offset **before** the window position is calculated, not after. Currently the flow is:
1. Calculate position from stored offset
2. Show window (user may drag it)
3. Update offset from actual window position

The fix: Move the offset update to happen **before** the next frame's position calculation. This can be done by updating the offset in a separate pass, or by using egui's window position tracking instead of manual offset management.

**Simpler fix:** Remove the `first_show || screen_size_changed` guard and always set `current_pos()` based on the stored offset. This way, every frame repositions the panel based on the offset, which is updated every frame after the window renders. This ensures panels always stay anchored.

### Files to modify
- [`src/app/panels.rs`](src/app/panels.rs:385, 459) — Remove the `first_show || screen_size_changed` condition, always apply `current_pos()`

### Implementation steps
1. In `show_floating_history_panel()`, change line 385 from:
   ```rust
   if first_show || screen_size_changed {
   ```
   to always apply `current_pos()`:
   ```rust
   {
   ```
2. Same for `show_floating_colors_panel()` at line 459
3. Same for `show_floating_layers_panel()` at line 135
4. Same for `show_floating_tools_panel()` at line ~82

---

## Summary of Changes

| # | Bug/Feature | Root Cause | Fix Approach | Files Changed |
|---|-------------|------------|--------------|---------------|
| 1 | Paste overlay survives layer delete | No paste overlay check in layer deletion | Cancel paste overlay when layer is deleted | `src/app/panels.rs` |
| 2 | Selection lost on tab switch | `clear_selection()` called in `switch_to_project()` | Remove the `clear_selection()` call | `src/app/bootstrap.rs` |
| 3 | Tab text selectable | `egui::Label` allows text selection | Use `egui::Button` with `frame(false)` instead | `src/app/runtime/update/dialogs_menu.rs` |
| 4 | Pixel grid color/alpha | Hardcoded colors, no settings fields | Add fields, save/load, UI controls, wire to renderer | `src/config/settings.rs`, `src/ui/panels/settings_window.rs`, `src/canvas/view/overlay.rs`, `src/canvas/view/core.rs` |
| 5 | Keybinds don't persist | Need to verify settings loading on startup | Verify `AppSettings::load()` is called, add debug logging | `src/app/bootstrap.rs`, `src/config/settings.rs` |
| 6 | Can't bind Ctrl+Shift+Z | egui translates Ctrl+letter to control chars | Handle control characters in rebinding UI | `src/ui/panels/settings_window.rs` |
| 7 | Panel anchoring broken | Offset not updated before repositioning | Always apply `current_pos()` every frame | `src/app/panels.rs` |
