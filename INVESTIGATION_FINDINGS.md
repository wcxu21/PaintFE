# Codebase Investigation Findings

## 1. Shape/Image Paste Handles with Shift Constraint

### Location: `src/ops/clipboard.rs` — PasteOverlay handle resizing

**Key File:** [src/ops/clipboard.rs](src/ops/clipboard.rs)

**Structure Definition:** [Lines 662-710](src/ops/clipboard.rs#L662-L710)
- `PasteOverlay` struct defines all handles: `Move`, `TopLeft`, `TopRight`, `BottomLeft`, `BottomRight`, `Top`, `Bottom`, `Left`, `Right`, `Rotate`, `Anchor`
- Field: `pub shift_held: bool` (line 701) — already tracks shift state
- Field: `pub active_handle: Option<HandleKind>` (line 683) — identifies which handle is being dragged

**Handle Drag Input Processing:** [Lines 1456-1555](src/ops/clipboard.rs#L1456-L1555)
- `handle_input()` method captures shift state: `self.shift_held = ui.input(|i| i.modifiers.shift)` (line 1507)
- Detects handle presses, tracks drag state with `drag_start_mouse`, `drag_start_scale_x`, `drag_start_scale_y`
- For rotation: Shift snaps to 45° increments (line 1540-1544) — **ALREADY IMPLEMENTED for rotation**

**Handle Resize Logic:** [Lines 1590-1653](src/ops/clipboard.rs#L1590-L1653)
- Method: `handle_resize(handle: HandleKind, delta_canvas: Vec2)`
- Processes all 8 edge/corner handles independently
- **MISSING:** 1:1 aspect ratio constraint for shift+drag on corner handles

**Current Handle Implementation:**
```rust
HandleKind::Right => {
    let new_w = (start_w + local_dx).max(4.0);
    let sx = new_w / src_w;
    (sx, start_sy, local_dx / 2.0, 0.0)  // Only X scale changes
},
HandleKind::TopLeft => {
    let new_w = (start_w - local_dx).max(4.0);
    let new_h = (start_h - local_dy).max(4.0);
    (new_w / src_w, new_h / src_h, local_dx / 2.0, local_dy / 2.0)  // Both scale independently
},
// ... similar for other handles
```

**Where to Add Shift Constraint:** Line 1590 onwards in `handle_resize()` method
- After computing `new_sx` and `new_sy`, if `self.shift_held` is true, constrain to 1:1 ratio:
  ```rust
  if self.shift_held {
      let aspect_ratio = src_h / src_w;
      // For corners: maintain aspect ratio
      // For edges: constrain proportionally
  }
  ```

---

## 2. Lines Tool Preview Rendering

### Location: `src/components/tools.rs` — Line preview rendering to preview layer

**Key File:** [src/components/tools.rs](src/components/tools.rs)

**Line State Structure:** [Lines 272-280](src/components/tools.rs#L272-L280)
- `pub dragging_handle: Option<usize>` — tracks which control point being edited
- `pub pan_handle_dragging: bool` — tracks pan handle state
- Line editing stage defined at line 274+

**Line Preview Rendering Entry Point:** [Lines 5550-5650](src/components/tools.rs#L5550-L5650)
- Method: `rasterize_bezier()` — renders line to `canvas_state.preview_layer`
- Called when: starting line, editing line, or settings changed
- **Line 5559:** Comment "Start stroke tracking (line uses preview layer like Brush)"

**Preview Layer Setup:** [Lines 11201-11230](src/components/tools.rs#L11201-L11230)
- Creates/clears preview_layer: `canvas_state.preview_layer = Some(TiledImage::new(...))`
- Sets blend mode: `canvas_state.preview_blend_mode = self.properties.blending_mode`
- **Line 11205:** Sets preview_layer to use tool's blending mode
- **Lines 11215-11225:** Clears only previous bounds (optimization) for incremental updates

**Bezier Point Rendering:** [Lines 11240-11350](src/components/tools.rs#L11240-L11350)
- Bezier curve sampled with spacing = 10% of brush size (line 11246)
- Points collected in `line_points` vector with pattern detection
- **Pattern checking:** Dotted/Dashed patterns respect cumulative distance (lines 11256-11270)

**Line Dot Drawing:** [Lines 11620-11680](src/components/tools.rs#L11620-L11680)
- Method: `draw_bezier_dot()` calls `draw_bezier_circle_with_hardness()`
- Anti-aliasing toggle checked at line 11651: `let anti_alias = self.line_state.line_tool.options.anti_alias`
- Uses MAX alpha blending to avoid scalloping: `if pixel_alpha > base_a`

**Selection Masking Implementation:** ⚠️ **NOT FOUND** — Line preview does NOT currently check selection mask
- Preview rendering writes directly to preview_layer without selection mask checks
- This differs from brush tool which respects `canvas_state.selection_mask`
- **Issue location:** Lines 11240-11350 (bezier point iteration) should check selection mask before calling `line_points.push()`

**Arrow Head Rendering:** [Lines 11350-11410](src/components/tools.rs#L11350-L11410)
- Arrowheads rendered via `draw_filled_triangle()` with 1px anti-alias fade
- Respects `LineEndShape::Arrow`, `ArrowSide::Start`, `ArrowSide::End`, `ArrowSide::Both`

---

## 3. Outline Tool (Generate > Outline)

### Location: `src/ops/effects.rs` — Outline generation core algorithm

**Key File:** [src/ops/effects.rs](src/ops/effects.rs)

**Outline Mode Enum:** [Lines 2108-2110](src/ops/effects.rs#L2108-L2110)
```rust
pub enum OutlineMode {
    Outside,
    Inside,
    Center,
}
```

**Main Outline Function:** [Lines 2114-2127](src/ops/effects.rs#L2114-L2127)
- `pub fn outline()` — applies to active layer
- Calls `outline_core()` internally

**Core Algorithm:** [Lines 2147-2230](src/ops/effects.rs#L2147-L2230)
- **Dilation/Erosion approach:** Uses circular morphology operations (lines 2165-2195)
  - `dilated`: max alpha in circular neighborhood
  - `eroded`: min alpha in circular neighborhood
  - Neighborhood radius: `ow` (outline width as i32)
  
- **Edge Detection Logic:** [Lines 2224-2228](src/ops/effects.rs#L2224-L2228)
  ```rust
  OutlineMode::Outside => dilated[idx] > 0 && alpha[idx] == 0,    // Transparent pixels adjacent to opaque
  OutlineMode::Inside => alpha[idx] > 0 && eroded[idx] == 0,      // Opaque pixels at edges  
  OutlineMode::Center => dilated[idx] > 0 && eroded[idx] == 0,    // Both transparent and opaque edges
  ```

- **Gap Issue Location:** Line 2164 uses circular distance check `dx * dx + dy * dy <= ow * ow`
  - This can create gaps in outline at diagonal edges when using certain width values
  - **Potential fix:** Use Chebyshev distance (max(dx, dy)) or increase sample density

- **Selection Mask Handling:** [Lines 2209-2216](src/ops/effects.rs#L2209-L2216)
  ```rust
  if let Some(mr) = mask_raw
      && x < mask_w
      && y < mask_h
      && mr[y * mask_w + x] == 0
  {
      continue;  // Skip pixels outside selection
  }
  ```

**Anti-aliasing Toggle:** ⚠️ **NOT FOUND** in outline_core()
- Parameter `width: u32` controls outline thickness (integer pixels only)
- **Missing feature:** No anti-aliasing toggle for smooth outline edges
- **Where to add:** 
  1. Add `anti_alias: bool` parameter to `outline_core()` signature
  2. Pass it through from `OutlineDialog` (see below)
  3. Apply softmax/smoothstep to edge alpha instead of binary pixel placement

**Dialog Definition:** `src/ops/effect_dialogs.rs` [Lines 2071-2120](src/ops/effect_dialogs.rs#L2071-L2120)
- Struct: `OutlineDialog { width: f32, color: [f32; 3], mode_idx: usize, first_open: bool }`
- Method: `outline_mode()` converts `mode_idx` (0=Outside, 1=Inside, 2=Center)
- **Feature gap:** No `anti_alias: bool` field in dialog yet

**Menu Integration:** [src/app.rs](src/app.rs)
- Keyboard shortcut: `BindableAction::GenerateOutline` (line 3182)
- Menu item: Line 5301-5309 in Generate menu
- Dialog instantiation: [Lines 3185-3186](src/app.rs#L3185-L3186)
- Result handling: [Lines 10195-10296](src/app.rs#L10195-L10296) — filters result through outline_core with mode

---

## 4. Selection Cut Behavior

### Location: `src/ops/clipboard.rs` and `src/app.rs` — Cut operation

**Cut Function Definition:** [Lines 563-570](src/ops/clipboard.rs#L563-L570)
```rust
pub fn cut_selection(state: &mut CanvasState, transparent_cutout: bool) -> bool {
    if !copy_selection(state, transparent_cutout) {
        return false;
    }
    state.delete_selected_pixels();  // Deletes but doesn't clear selection mask
    state.mark_dirty(None);
    true
}
```

**Issue:** Selection mask is NOT cleared after cut
- `delete_selected_pixels()` only erases pixels (turns alpha to 0)
- Selection mask remains active
- **Expected behavior:** Selection should be cleared after cut to allow new operations

**Keyboard Shortcut Handler:** [Lines 2285-2296](src/app.rs#L2285-L2296)
- Ctrl+X handled in main event loop
- Calls `do_snapshot_op("Cut Selection", |s| { crate::ops::clipboard::cut_selection(s, transparent_cutout); })`
- No selection clearing follows the cut

**Menu Cut Item Handler:** [Lines 4010-4020](src/app.rs#L4010-L4020)
- Edit > Cut menu item
- Same implementation: calls `cut_selection()` without clearing selection

**Where Selection Should Be Cleared:**
1. **Option A (recommended):** Modify `cut_selection()` in clipboard.rs line 568:
   ```rust
   state.delete_selected_pixels();
   state.clear_selection();  // Add this line
   state.mark_dirty(None);
   ```

2. **Option B:** Add clearing after cut in app.rs lines 2291 and 4013:
   ```rust
   cut_applied = crate::ops::clipboard::cut_selection(s, transparent_cutout);
   if cut_applied {
       s.clear_selection();  // Clear after cut
   }
   ```

**Related Context:** Selection clearing is done elsewhere:
- Line 751-754 (when switching projects)
- Line 877 (after pasting)
- Line 1754 (in move-selection tool)

---

## 5. Handle Shift+Stretch Behavior (Shapes Tool)

### Location: `src/components/tools.rs` — Shapes tool handle dragging

**Shapes Drag Handler:** [Lines 8350-8410](src/components/tools.rs#L8350-L8410)

**Shift Constraint Already Implemented:** ✅
- **Line 8367:** Comment "Shift: constrain to 1:1 aspect ratio"
- Implementation at lines 8370-8373:
  ```rust
  if shift_held {
      let side = lx.max(ly);
      lx = side;
      ly = side;
  }
  ```

**Handle Detection:** [Lines 8350-8365](src/components/tools.rs#L8350-L8365)
- Corner handles (`ShapeHandle::TopLeft`, etc.) trigger resize
- Local coordinates computed via rotation un-rotation: lines 8356-8361
  ```rust
  let cos_r = shape_rotation.cos();
  let sin_r = shape_rotation.sin();
  let local_dx = dx * cos_r + dy * sin_r;
  let local_dy = -dx * sin_r + dy * cos_r;
  ```

**Rotation Handling:** Lines 8344-8348
- Shapes maintain rotation during resize
- Center recomputed after constrained size calculated

**Edge Cases:**
- Rectangle handles (line 8331-8348) have special case for width-only handles
- Circle/ellipse handles (line 8344+) constrain both axes
- All compute `anchor_lx, anchor_ly` based on which corner is being dragged

---

## 6. Shift+Constraint for Selection Shapes

### Location: `src/components/tools.rs` — Rectangle/Ellipse selection

**Already Implemented:** ✅
- **Line 5942:** Comment "Shift => constrain to 1:1 aspect ratio (square / circle)"
- Implementation at lines 5944-5948:
  ```rust
  if shift_held && let Some(start) = self.selection_state.drag_start {
      let dx = end.x - start.x;
      let dy = end.y - start.y;
      let side = dx.abs().max(dy.abs());
      end = Pos2::new(start.x + side * dx.signum(), start.y + side * dy.signum());
  }
  ```

---

## Summary Table

| Feature | File | Line(s) | Status | Note |
|---------|------|---------|--------|------|
| **Paste Overlay Handles** | clipboard.rs | 1590-1653 | ⚠️ Incomplete | Shift constraint for 1:1 ratio needs implementation |
| **Line Preview Rendering** | tools.rs | 11201-11680 | ✅ Complete | Uses preview_layer, anti-alias toggle exists |
| **Line Selection Masking** | tools.rs | 11240-11350 | ❌ Missing | Preview doesn't check selection_mask like brush does |
| **Outline Outside Mode** | effects.rs | 2224-2226 | ✅ Complete | Implemented via dilation detection |
| **Outline Anti-aliasing** | effects.rs | 2147-2230 | ❌ Missing | No smoothing toggle available |
| **Selection Cut Behavior** | clipboard.rs | 563-570 | ⚠️ Incomplete | Doesn't clear selection after cut |
| **Shape Handle Shift** | tools.rs | 8367-8373 | ✅ Complete | Already constrains to 1:1 |
| **Selection Shift Constraint** | tools.rs | 5942-5948 | ✅ Complete | Already constrains to square/circle |

---

## Implementation Priorities

### High Priority (Clear Issues)
1. **Selection Cut Clearing:** One-line fix in `cut_selection()` — add `state.clear_selection()`
2. **Line Selection Masking:** Add mask check in bezier point loop (lines 11260-11270)
3. **Paste Overlay Shift Constraint:** Complete handle_resize() logic for 1:1 ratio on all edge/corner handles

### Medium Priority (Feature Gaps)
4. **Outline Anti-aliasing:** Add bool field to OutlineDialog, thread through to outline_core(), apply smoothstep blending

### Low Priority (Polish)
5. **Outline Gap Fix:** Experiment with different distance metrics in dilation/erosion

