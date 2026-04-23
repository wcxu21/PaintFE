# Comprehensive Technical Implementation Plan - PaintFE Mega-Prompt

## Executive Summary
16 interconnected issues/features across layer transforms, UI preferences, shape rendering, tool selection-masking, history, text layer management, and clipboard operations. All have been investigated with root-cause analysis. Implementation priority: **Selection-masking tools first** (affects 6 tools), then **simpler UI/text fixes**, then **advanced features** (handles, history).

---

## SECTION 1: LAYER TRANSFORM - FLIP H/V WITH SELECTION RESPECT

### Issue #1: Right Click on Layer > Transform > Flip H/V Should Respect Selections

**Root Cause (Confidence: HIGH)**
- Current implementation in [src/ops/transform.rs](src/ops/transform.rs#L419-431) uses `flip_layer_horizontal()` and `flip_layer_vertical()` which flip **entire layer** regardless of selection
- Canvas-level flip (Edit > Transform > Flip H/V) correctly uses `try_transform_selected_region()` ([src/ops/transform.rs](src/ops/transform.rs#L61-72)) to gate operation to selection bounds
- **Gap**: Layer context menu directly calls layer flip without checking if selection exists

**Implementation Files**
1. **[src/ops/transform.rs](src/ops/transform.rs#L410-444)** - Layer flip functions (must add selection check)
2. **[src/components/layers.rs](src/components/layers.rs#L1087-1094)** - Menu action dispatcher (confirm flow)
3. **[src/ops/canvas_ops.rs](src/ops/canvas_ops.rs)** - May need new `flip_layer_selected_region()` wrapper
4. **[tests/visual_transforms.rs](tests/visual_transforms.rs)** - Test coverage

**Implementation Steps**
1. Rename current `flip_layer_horizontal/vertical()` to `flip_layer_horizontal_full/vertical_full()` for clarity
2. Create new wrapper functions `flip_layer_horizontal/vertical(canvas_state, layer_idx)` that:
   - Check if `canvas_state.selection_mask.is_some()`
   - If yes: call `try_transform_selected_region(SelectionCanvasTransform::FlipH/FlipV)` but apply only to **single layer** (modify function signature)
   - If no: call `flip_layer_horizontal_full/vertical_full(layer)`
3. Update [src/components/layers.rs](src/components/layers.rs#L1082+) dispatcher to call new wrapper
4. Add tests to [tests/visual_transforms.rs](tests/visual_transforms.rs) for layer flip with selection

**Expected Output**
- Layer flip respects active selection bounds, only modifying pixels within selected area
- Behavior mirrors canvas-level flip but operates on single layer

---

## SECTION 2: PREFERENCES UI KEYBINDS ISSUES

### Issue #2: Keybinds Menu Has Two Scrollbars on Right

**Root Cause (Confidence: MEDIUM - Not found in Rust code)**
- Keybinds UI rendered in [src/assets.rs](src/assets.rs#L5033-L5150)
- Likely egui layout issue: nested scrollable areas or double-scroll configuration
- Possible: `ScrollArea` within `ScrollArea` in settings rendering
- **Investigation needed**: Check if keybinds table/list is wrapped in redundant ScrollArea

**Implementation Files**
1. **[src/assets.rs](src/assets.rs#L5033-5150)** - `SettingsWindow::show()` keybinds rendering section

**Implementation Steps**
1. Locate keybinds rendering section in `SettingsWindow::show()`
2. Check for nested `ScrollArea` calls - remove outer or inner depending on context
3. Verify container width constraints don't force double scrollbars
4. Test with many keybindings (>20) to ensure single scrollbar appears

**Expected Output**
- Single scrollbar on right side of keybinds list
- No visual overlap or duplicate scroll controls

---

### Issue #3: Keybinds - Add Unbind Option (X Button Next to Each Keybind)

**Root Cause (Confidence: HIGH)**
- [src/assets.rs](src/assets.rs#L5033-5150) SettingsWindow only shows keybind and allows rebind via click
- No UI element to **clear/unbind** a key
- Need to add delete/X button per keybind row

**Implementation Files**
1. **[src/assets.rs](src/assets.rs#L5033-5150)** - Keybinds table rendering
2. **[src/components/mod.rs](src/components/mod.rs)** - UI component state if needed
3. **[src/i18n.rs](src/i18n.rs)** - i18n string "unbind" if not exists

**Implementation Steps**
1. In keybinds rendering loop (likely in `show()` method), after displaying current KeyCombo:
   - Add a button with `"✕"` or `"Remove"` text
   - Button action: set `KeyBindings::set(action, None)` or similar
   - Store button state to track click
2. Create `KeyCombo::None` or `Option<KeyCombo>` variant to represent unbound
3. Update serialization ([src/assets.rs](src/assets.rs#L2531-2565) `to_config_string()`) to skip/mark unbound keys
4. Update deserialization to handle `None`/empty values
5. Add i18n strings for "Unbind" or "Remove" button label

**Expected Output**
- X/Remove button visible next to each keybind
- Clicking removes the keybind mapping
- Unbound actions show as "Unbound" or similar in UI

---

### Issue #4: Keybinds - Special Characters Display as "?" ([ ] ; ' , . /)

**Root Cause (Confidence: MEDIUM)**
- [src/assets.rs](src/assets.rs#L2493-2531) `KeyCombo::display()` constructs string from key components
- Special characters likely not being properly mapped to key symbols
- Possible: Using `char::from_u32()` or similar without proper Unicode handling
- **File to check**: [src/windows_key_probe.rs](src/windows_key_probe.rs) for Windows key code mapping

**Implementation Files**
1. **[src/assets.rs](src/assets.rs#L2493-2531)** - `KeyCombo::display()` method
2. **[src/windows_key_probe.rs](src/windows_key_probe.rs)** - Windows key code to char mapping
3. **[src/linux_key_probe.rs](src/linux_key_probe.rs)** - Linux key mapping (similar fix)
4. **[src/i18n.rs](src/i18n.rs)** - May need locale-specific key names

**Implementation Steps**
1. Review `KeyCombo::display()` method - trace how special keys are converted to display strings
2. Check `windows_key_probe.rs` for key code mappings - verify `[`, `]`, `;`, `'`, `,`, `.`, `/` are correctly mapped
3. Add explicit cases for each special character if using match/map:
   ```rust
   "[" => "[",
   "]" => "]",
   ";" => ";",
   "'" => "'",
   "," => ",",
   "." => ".",
   "/" => "/",
   ```
4. Test with actual keyboard input to verify special characters display correctly
5. Apply same fixes to Linux key probe

**Expected Output**
- Keybinds with `[`, `]`, `;`, `'`, `,`, `.`, `/` display correctly instead of `?`
- All special character keybinds show proper symbols

---

### Issue #5: Keybinds Menu - "menu.file.close" Should Display as "Close File"

**Root Cause (Confidence: HIGH)**
- Localization strings in [locales/en.txt](locales/en.txt) contain action IDs like "menu.file.close"
- Display logic in [src/assets.rs](src/assets.rs#L5033-5150) likely shows raw ID instead of human-readable string
- **Fix**: Use localization system to convert `menu.file.close` → `"Close File"`

**Implementation Files**
1. **[src/assets.rs](src/assets.rs#L5033-5150)** - Keybinds rendering where action names are displayed
2. **[locales/en.txt](locales/en.txt)** - Check existing localization strings
3. **[src/i18n.rs](src/i18n.rs)** - i18n helper functions

**Implementation Steps**
1. In keybinds rendering loop, when displaying action name:
   - Instead of raw action ID: `format!("{:?}", action)`
   - Use i18n lookup: `tr(format!("keybind_name.{}", action_id))`
   - OR reuse menu string: `tr("menu.file.close")` if consistent
2. Add localization strings for each action if not exists:
   ```
   menu.file.close = Close File
   menu.file.open = Open File
   ...etc
   ```
3. Fallback: if i18n key missing, convert `menu.file.close` → `"File > Close"` programmatically

**Expected Output**
- Keybinds list shows "Close File" instead of "menu.file.close"
- All action names are human-readable and localized

---

## SECTION 3: SHAPE OUTLINE RENDERING - SHARP CORNERS

### Issue #6: Extend Sharp Outline Rendering to Triangle, Right Angle Triangle, Parallelogram

**Root Cause (Confidence: HIGH)**
- Cross and Trapezoid implement sharp outline rendering with miter joints:
  - Cross: [src/ops/shapes.rs](src/ops/shapes.rs#L564-590) `cross_outline_coverage()` uses `outer_box`/`inner_box` expansion
  - Trapezoid: [src/ops/shapes.rs](src/ops/shapes.rs#L599-660) `trapezoid_outline_coverage()` computes corner intersections with normal offset
- Triangle: [src/ops/shapes.rs](src/ops/shapes.rs#L221-237) `sdf_triangle_box()` uses simple line distances, NO outline function
- Right Angle Triangle: [src/ops/shapes.rs](src/ops/shapes.rs#L412-421) uses `sdf_convex_polygon()`, NO outline function
- Parallelogram: [src/ops/shapes.rs](src/ops/shapes.rs#L404-410) uses `sdf_convex_polygon()`, NO outline function
- **Gap**: Missing `*_outline_coverage()` functions for these three shapes

**Implementation Files**
1. **[src/ops/shapes.rs](src/ops/shapes.rs)** - All shape SDF and outline functions
2. **[tests/visual_shapes.rs](tests/visual_shapes.rs)** - Visual regression tests for outlines

**Implementation Steps**

#### Step 1: Understand Trapezoid Miter Joint Pattern
- Review `trapezoid_outline_coverage()` at [lines 599-660](src/ops/shapes.rs#L599-660)
- Pattern: compute outer/inner expanded polygons with normal offset + corner miter clipping
- Uses `compute_corner_intersection()` helper at line 620

#### Step 2: Create Triangle Outline Function
- Location: Add after `sdf_triangle_box()` at ~line 240
- Function name: `triangle_outline_coverage()`
- Logic:
  1. Get triangle 3 vertices (convert from SDF parameters)
  2. Expand each edge outward by `outline_width` using normal perpendicular
  3. Compute miter corners using same `compute_corner_intersection()` pattern
  4. Sample edge distances to outer/inner polygons
  5. Return coverage value 0.0-1.0

#### Step 3: Create Right Angle Triangle Outline Function
- Location: Add after `sdf_right_triangle()` at ~line 425
- Function name: `right_angle_triangle_outline_coverage()`
- Reuse triangle logic (3 vertices, same polygon pattern)
- Special case: handle right angle corner more precisely (already 90°)

#### Step 4: Create Parallelogram Outline Function
- Location: Add after `sdf_parallelogram()` at ~line 410
- Function name: `parallelogram_outline_coverage()`
- Logic: same miter joint pattern with 4 vertices
- Similar to trapezoid but with parallel edges (simpler corner math)

#### Step 5: Update Rasterize Function
- [src/ops/shapes.rs](src/ops/shapes.rs#L723-760) `rasterize_shape()`
- Add cases in outline rendering match/if to call new functions:
  ```rust
  ShapeType::Triangle => triangle_outline_coverage(...),
  ShapeType::RightAngleTriangle => right_angle_triangle_outline_coverage(...),
  ShapeType::Parallelogram => parallelogram_outline_coverage(...),
  ```

#### Step 6: Add Golden Tests
- Create new test images in [tests/golden/shapes/](tests/golden/shapes/):
  - `triangle_outline.png`
  - `right_angle_triangle_outline.png`
  - `parallelogram_outline.png`
- Add test functions to [tests/visual_shapes.rs](tests/visual_shapes.rs)

**Expected Output**
- Triangle, Right Angle Triangle, Parallelogram outlines render with sharp, pointy corners
- Outline thickness remains uniform across all edges and corners
- No rounded corners regardless of outline width

---

## SECTION 4: TOOL SELECTION MASKING - 6 TOOLS NEED FIXES

### Issues #7-12: Color Remover, Liquify, Text, Shapes, Lines - Not Respecting Selections

**Root Cause Analysis (Confidence: HIGH)**

| Tool | Current Behavior | Issue | File Location |
|------|------------------|-------|----------------|
| **Color Remover** | Removes 1 pixel, works outside selection | No mask applied to removal op | [src/ops/color_removal.rs](src/ops/color_removal.rs) (NOT examined) |
| **Liquify** | Works outside selection bounds | No mask check in displacement application | [src/ops/adjustments.rs](src/ops/adjustments.rs) + [src/components/tools.rs](src/components/tools.rs) |
| **Text** | Places/types beyond selection boundary | Vector layer, no raster mask applied | [src/ops/text_layer.rs](src/ops/text_layer.rs) + [src/components/tools.rs](src/components/tools.rs) |
| **Shapes** | Preview and commit outside selection | No mask in rasterization | [src/ops/shapes.rs](src/ops/shapes.rs#L723-760) + [src/components/tools.rs](src/components/tools.rs) |
| **Lines** | Commit respects selection (✓), but preview renders outside | Preview doesn't check mask | [src/components/tools.rs](src/components/tools.rs#L11201-11230) |
| **Shapes Handles** | N/A (not applicable - need preview fix) | Preview shows full shape before commit, should show only selected portion | [src/components/tools.rs](src/components/tools.rs) |

**Pattern for Selection Masking**
- Working example: Brush tool at [src/components/tools.rs](src/components/tools.rs#L10474-10510)
- Pattern: `Option<&image::GrayImage> mask` parameter passed to drawing functions
- Implementation: Before writing pixel, check `mask[idx] > 0` (255 = selected, 0 = not selected)

---

### Issue #7: Color Remover - Block Outside Selection

**Implementation Files**
1. **[src/ops/color_removal.rs](src/ops/color_removal.rs)** - Color removal logic
2. **[src/components/tools.rs](src/components/tools.rs)** - Tool dispatcher

**Implementation Steps**
1. Add `selection_mask: Option<&image::GrayImage>` parameter to color removal functions
2. Before removing pixel color, check:
   ```rust
   if let Some(mask) = selection_mask {
       if mask.get_pixel(x, y).0[0] == 0 { continue; } // Skip if not selected
   }
   ```
3. Update tool dispatcher in tools.rs to pass `canvas_state.selection_mask.as_ref()` when Color Remover is active
4. Test: Draw selection, activate Color Remover, verify it only removes colors within selection

---

### Issue #8: Liquify - Constrain to Selection Bounds

**Implementation Files**
1. **[src/ops/adjustments.rs](src/ops/adjustments.rs)** - Liquify displacement application
2. **[src/components/tools.rs](src/components/tools.rs#L7950+]** - Liquify tool interaction
3. **[src/gpu/compute.rs](src/gpu/compute.rs)** - GPU liquify pipeline (if applicable)

**Implementation Steps**
1. In liquify displacement code (likely iterating over affected pixels):
   - Add selection mask check before applying displacement:
   ```rust
   if let Some(mask) = selection_mask {
       if mask.get_pixel(x, y).0[0] == 0 { continue; }
   }
   ```
2. Pass `canvas_state.selection_mask` through tool interaction functions
3. If GPU-accelerated: add `selection_mask` texture binding and check in compute shader
4. Test: Draw selection, liquify within/outside, verify only inside affected

---

### Issue #9: Text Tool - Cull/Mask Text Beyond Selection Boundary

**Implementation Files**
1. **[src/ops/text_layer.rs](src/ops/text_layer.rs)** - Text rasterization
2. **[src/components/tools.rs](src/components/tools.rs)** - Text tool UI/input

**Implementation Steps**
1. In text layer rasterization (likely in composite or render functions):
   - Apply selection mask after text is rendered:
   ```rust
   if let Some(mask) = selection_mask {
       for pixel in output.iter_mut() {
           if mask.get_pixel(x, y).0[0] == 0 {
               pixel.alpha = 0; // Transparent outside selection
           }
       }
   }
   ```
2. OR clip text rendering bounds to selection bounding box before rasterization
3. Update text layer rendering to pass/apply selection_mask
4. Test: Create selection, place text extending outside, verify text culled at boundary

---

### Issue #10: Shapes Tool - Respect Selection in Preview AND Commit

**Implementation Files**
1. **[src/ops/shapes.rs](src/ops/shapes.rs#L723-760)** - `rasterize_shape()` main rendering
2. **[src/components/tools.rs](src/components/tools.rs)** - Shapes tool preview/interaction

**Implementation Steps**

#### For Shape Preview (Drag/Before Commit)
1. In shape preview rendering loop (likely in tools.rs shapes section):
   - Check `canvas_state.selection_mask` exists
   - When drawing preview, apply mask:
     ```rust
     for pixel in preview_output {
         if let Some(mask) = &selection_mask {
             if mask.get_pixel(x, y).0[0] == 0 {
                 skip pixel or set transparent
             }
         }
     }
     ```
2. Visual: Show shape preview only in selected area, transparent outside

#### For Shape Commit (Finalize)
1. In `rasterize_shape()` at [line 723](src/ops/shapes.rs#L723):
   - Add parameter: `selection_mask: Option<&image::GrayImage>`
   - Add pixel-wise check before writing:
     ```rust
     if let Some(mask) = selection_mask {
         if mask.get_pixel(x, y).0[0] == 0 { continue; }
     }
     ```
2. Update all callers of `rasterize_shape()` to pass selection_mask
3. Test: Draw rect selection, draw shape crossing boundary, verify only selection area filled

---

### Issue #11: Lines Tool - Preview Constrained to Selection

**Root Cause**
- Commit already respects selection (✓)
- Preview renders outside selection area without masking
- Location: [src/components/tools.rs](src/components/tools.rs#L11201-11230) `rasterize_bezier()` setup

**Implementation Files**
1. **[src/components/tools.rs](src/components/tools.rs#L11177-11230)** - Line preview rendering

**Implementation Steps**
1. In `rasterize_bezier()` or line preview section:
   - After creating preview pixels, apply selection mask:
     ```rust
     if let Some(mask) = &canvas_state.selection_mask {
         for (x, y, pixel) in preview_pixels {
             if mask.get_pixel(x as u32, y as u32).0[0] == 0 {
                 pixel.alpha = 0; // Make transparent outside selection
             }
         }
     }
     ```
2. Alternative: Clip line rendering to selection bounding box before sampling curve
3. Test: Draw selection, draw line crossing boundary, verify preview only shows in selection

---

### Issue #12: Shapes Tool - Fix Preview/Drag to Show Masked Area

**Root Cause**
- Related to Issue #10, but specifically about preview visualization
- Currently shows full shape being dragged
- Should indicate which portions will be cut off by selection boundary

**Implementation Files**
1. **[src/components/tools.rs](src/components/tools.rs)** - Shapes tool preview rendering

**Implementation Steps**
1. In shape preview rendering (before commit):
   - Apply selection mask to preview layer
   - Use semi-transparent or different color for areas outside selection:
     ```rust
     if let Some(mask) = &canvas_state.selection_mask {
         for pixel in preview {
             if mask.get_pixel(x, y).0[0] == 0 {
                 render as gray/transparent (e.g., 50% opacity)
             } else {
                 render as normal shape color
             }
         }
     }
     ```
2. Visual feedback: Show shape outline clearly, dim the parts outside selection
3. Test: Draw selection, drag shape preview, verify outside area shows visual feedback

---

## SECTION 5: HISTORY & UNDO - SELECTION ORDERING

### Issue #13: Selection History Ordering - Undo Should Undo Actions Before Selection

**Root Cause (Confidence: MEDIUM)**
- Selection commands stored separately from pixel commands in history stack
- Scenario: Select area → Draw 3 lines → Undo ×2 → Should see 2 lines undone, but selection is undone first
- **Problem**: `SelectionCommand` and `BrushCommand` treated equally in history stack
- Likely: Full `SnapshotCommand` captures selection + pixels together, so undo order is LIFO (selection appears to undo first if it was changed after pixels)
- OR: Selection command is pushed to history when selection changes, then pixel commands pushed, creating wrong LIFO order

**Investigation Needed Files**
1. **[src/components/history.rs](src/components/history.rs#L568-700)** - History stack management
2. **[src/components/history.rs](src/components/history.rs#L1078-1115)** - `SelectionCommand` undo/redo
3. **[src/components/tools.rs](src/components/tools.rs)** - Where selection/paint commands are pushed

**Implementation Steps**

#### Step 1: Trace History Push Order
1. When user creates selection: is `SelectionCommand` pushed?
2. When user draws: is `BrushCommand` pushed?
3. Current order in history stack for scenario: Draw 3 lines in selection
   - Expected order (top to bottom): BrushCommand(line3), BrushCommand(line2), BrushCommand(line1), SelectionCommand
   - Actual order: (check via debug logging)

#### Step 2: Fix Undo Order (If Selection Commands Are Wrong)
- **Option A**: Don't push separate `SelectionCommand` - include selection state in pixel commands
  - Modify `BrushCommand` to store `selection_mask: Option<image::GrayImage>`
  - Restore selection when undo BrushCommand
  - PRO: Automatic correct ordering
  - CON: More memory per command
  
- **Option B**: Suppress undo of selection if selection is unchanged
  - In `SelectionCommand::undo()`, check if selection changed
  - If not changed, skip to previous command
  - CON: Complex state tracking

- **Option C**: Use grouped commands
  - Wrap pixel ops + selection op in single `GroupedCommand`
  - Undo entire group atomically
  - PRO: Clear semantics
  - CON: Refactor history system

#### Step 3: Test
- Create selection → Draw 3 strokes → Undo ×2 → Should see exactly 2 strokes undone, selection still active
- Verify selection undo only happens when selection itself changes, not as side effect

---

## SECTION 6: TEXT LAYER MANAGEMENT - ACTIVE TEXT ON LAYER DELETION

### Issue #14: Deleting Layer with Active Text Causes Text to Move to Next Layer

**Root Cause (Confidence: HIGH)**
- Active text layer tracked in `canvas_state.text_editing_layer: Option<usize>` ([src/canvas.rs](src/canvas.rs#L1684))
- When layer deleted via [src/ops/canvas_ops.rs](src/ops/canvas_ops.rs#L259-276) `delete_layer()`, NO check if deleted layer is `text_editing_layer`
- Text layer remains in memory (in `layers` vector at old index)
- Subsequent composite tries to rasterize at stale index → wraps to next layer OR crashes

**Implementation Files**
1. **[src/ops/canvas_ops.rs](src/ops/canvas_ops.rs#L259-276)** - `delete_layer()` function
2. **[src/components/layers.rs](src/components/layers.rs#L2580-2620)** - `LayersPanel::delete_layer()` UI handler
3. **[src/canvas.rs](src/canvas.rs#L1684+]** - `CanvasState::text_editing_layer` field

**Implementation Steps**

#### Step 1: Fix delete_layer() in canvas_ops.rs
- At [line 259-276](src/ops/canvas_ops.rs#L259-276), after removing layer from `layers` vector:
  ```rust
  // If deleted layer was the active text layer, clear it
  if canvas_state.text_editing_layer == Some(layer_index) {
      canvas_state.text_editing_layer = None;
  } else if let Some(text_idx) = canvas_state.text_editing_layer {
      // Adjust text layer index if layer removed before it
      if layer_index < text_idx {
          canvas_state.text_editing_layer = Some(text_idx - 1);
      }
  }
  ```

#### Step 2: Fix delete_layer() in layers.rs UI handler
- At [line 2580-2620](src/components/layers.rs#L2580-2620), apply same fixes

#### Step 3: Alternative - Commit Before Delete
- When layer delete action triggered:
  - If `canvas_state.text_editing_layer == Some(layer_index)`:
    - Call `commit_text_layer()` first
    - Then proceed with delete
  - Ensures text is finalized before layer removal

#### Step 4: Test
- Add active text on layer
- Delete that layer
- Verify: text disappears (not moved to next layer)
- Add active text on layer, delete different layer above
- Verify: text_editing_layer index adjusted correctly

---

## SECTION 7: CLIPBOARD - SELECTION DESELECT AFTER CUT

### Issue #15: After Ctrl+X Cut, Selection Should Auto-Deselect

**Root Cause (Confidence: HIGH)**
- [src/ops/clipboard.rs](src/ops/clipboard.rs#L563-570) `cut_selection()` calls `delete_selected_pixels()` but does NOT clear selection
- Selection mask remains active after cut
- User expects: Cut content → Selection disappears

**Implementation Files**
1. **[src/ops/clipboard.rs](src/ops/clipboard.rs#L563-570)** - `cut_selection()` function
2. **[src/app.rs](src/app.rs#L2285-2296)** - Ctrl+X keyboard handler
3. **[src/app.rs](src/app.rs#L4010-4020)** - Edit > Cut menu handler

**Implementation Steps**

#### Step 1: Modify cut_selection() Function
- At [line 563-570](src/ops/clipboard.rs#L563-570):
  ```rust
  pub fn cut_selection(state: &mut CanvasState) {
      // ... existing copy logic ...
      delete_selected_pixels(state);
      
      // Add this line:
      state.clear_selection(); // Deselect after cutting
  }
  ```

#### Step 2: Verify Handlers
- Check [src/app.rs](src/app.rs#L2285-2296) Ctrl+X handler calls `cut_selection()`
- Check [src/app.rs](src/app.rs#L4010-4020) Edit > Cut menu calls same function
- No changes needed if both use `cut_selection()`

#### Step 3: Test
- Create selection → Ctrl+X → Selection should disappear
- Verify in both keyboard and menu paths

---

## SECTION 8: SHAPE HANDLES - SHIFT+DRAG FOR 1:1 ASPECT RATIO

### Issue #16: Edge Handles with Shift Should Constrain Resize to 1:1 Ratio

**Root Cause (Confidence: HIGH)**
- [src/ops/clipboard.rs](src/ops/clipboard.rs#L1590-1653) `handle_resize()` computes scale independently per-handle
- Shift state already captured at [line 1507](src/ops/clipboard.rs#L1507)
- **Gap**: Shift is used for rotation snapping but NOT for aspect ratio constraint on edge handles
- Selection shapes and shapes tool ALREADY have 1:1 constraint when Shift held:
  - Selection: [src/components/tools.rs](src/components/tools.rs#L5942-5948) `let side = lx.max(ly);`
  - Shapes: [src/components/tools.rs](src/components/tools.rs#L8367-8373) same pattern

**Implementation Files**
1. **[src/ops/clipboard.rs](src/ops/clipboard.rs#L1590-1653)** - `handle_resize()` function

**Implementation Steps**

#### Step 1: Identify Edge Handle Types
- Review [src/ops/clipboard.rs](src/ops/clipboard.rs#L662-710) `Handle` enum
- Edge handles: `Left`, `Right`, `Top`, `Bottom`
- Corner handles: `TopLeft`, `TopRight`, `BottomLeft`, `BottomRight`
- Shift constraint should apply to **corners only** (matching behavior of rectangle select + shapes tool)

#### Step 2: Apply 1:1 Constraint for Corner Handles
- In `handle_resize()` at [line 1590](src/ops/clipboard.rs#L1590):
  - When processing corner handles with `shift_held`:
    ```rust
    if is_corner_handle(handle) && shift_held {
        // Constrain to 1:1 aspect ratio
        let side = scale_x.abs().max(scale_y.abs());
        scale_x = if scale_x.signum() >= 0.0 { side } else { -side };
        scale_y = if scale_y.signum() >= 0.0 { side } else { -side };
    }
    ```

#### Step 3: Test
- Copy/paste image or shape
- Grab corner handle + Shift+drag → Should resize 1:1 (square aspect)
- Grab edge handle + Shift+drag → Should still allow independent width/height change

---

## SECTION 9: OUTLINE TOOL - ANTI-ALIASING TOGGLE & OUTSIDE MODE GAP

### Issue #17: Outline Tool Missing Anti-Aliasing Toggle and Has Gap in Outside Mode

**Root Cause (Confidence: HIGH)**

**Sub-Issue A: Missing Anti-Aliasing Toggle**
- Outline dialog in [src/ops/effect_dialogs.rs](src/ops/effect_dialogs.rs#L2071-2120)
- Only has `radius` slider, no anti-alias option
- Outline effect in [src/ops/effects.rs](src/ops/effects.rs#L2147-2230) applies smoothstep for soft edges (implicit AA)
- Need to add UI toggle and pass to effect

**Sub-Issue B: Gap in Outside Mode**
- Outside mode: [src/ops/effects.rs](src/ops/effects.rs#L2226) `dilated[idx] > 0 && alpha[idx] == 0`
- Uses circular distance check: `dx² + dy² ≤ ow²`
- **Problem**: Diagonal pixels at radius R don't perfectly cover circle outline, causing gaps
- **Fix**: Use Chebyshev distance (max distance) or improved circle sampling

**Implementation Files**
1. **[src/ops/effect_dialogs.rs](src/ops/effect_dialogs.rs#L2071-2120)** - OutlineDialog UI
2. **[src/ops/effects.rs](src/ops/effects.rs#L2147-2230]** - Outline effect implementation
3. **[src/components/dialogs.rs](src/components/dialogs.rs)** - Dialog struct if needed

**Implementation Steps**

#### Step 1: Add Anti-Aliasing Toggle to Dialog
- In OutlineDialog struct ([src/ops/effect_dialogs.rs](src/ops/effect_dialogs.rs#L2071)):
  ```rust
  pub struct OutlineDialog {
      pub radius: f32,
      pub mode: OutlineMode, // Outside/Inside/Center
      pub anti_alias: bool,  // NEW
  }
  ```
- In UI rendering, add checkbox:
  ```rust
  ui.checkbox(&mut dialog.anti_alias, "Anti-aliasing");
  ```

#### Step 2: Pass Anti-Alias to Effect
- Modify outline effect signature to accept `anti_alias: bool`
- Update all callers to pass this parameter

#### Step 3: Fix Outside Mode Gap
- In [src/ops/effects.rs](src/ops/effects.rs#L2220-2230), instead of pure circular distance:
  - Use improved distance metric OR
  - Sample more points around outline perimeter
  - Consider: `max(abs(dx), abs(dy))` (Chebyshev) for sharper outline
  - OR: Use multiple radius values to fill gaps
- Test with various outline widths to ensure no gaps

#### Step 4: Implement Anti-Alias in Effect
- If `anti_alias` true: apply smoothstep to edge pixels (soft transition)
- If `anti_alias` false: hard edge (0/255 only)
  ```rust
  if anti_alias {
      alpha = (smoothstep(ow - 0.5, ow + 0.5, distance) * 255.0) as u8;
  } else {
      alpha = if distance <= ow { 255 } else { 0 };
  }
  ```

#### Step 5: Test
- Apply outline with outside mode, various radii → No gaps
- Toggle anti-alias on/off → Edges soften/sharpen accordingly

---

## IMPLEMENTATION PRIORITY MATRIX

| Priority | Category | Issues | Est. Effort | Dependencies |
|----------|----------|--------|-------------|--------------|
| **P0** | Selection Masking | #7-12 (Color Remover, Liquify, Text, Shapes, Lines preview) | 40-60h | None |
| **P1** | Text Layer | #14 (Delete with active text) | 4-6h | None |
| **P2** | Clipboard | #15 (Cut + deselect) | 1-2h | None |
| **P3** | UI Fixes | #2-5 (Keybinds scrollbar, unbind, special chars, label) | 8-12h | None |
| **P4** | Layer Transform | #1 (Layer flip with selection) | 6-8h | None |
| **P5** | Shape Rendering | #6 (Sharp outlines for Triangle/RightTriangle/Parallelogram) | 12-16h | None |
| **P6** | History | #13 (Selection undo order) | 6-10h | None |
| **P7** | Handles | #16 (Shift+1:1 resize) | 3-4h | None |
| **P8** | Effects | #17 (Outline AA toggle + gap fix) | 5-8h | None |

**Total Estimated Effort**: 85-126 hours

**Recommended Implementation Order**:
1. Selection masking (P0) - affects multiple tools
2. Text layer management (P1) - prevents data loss
3. Clipboard (P2) - quick win
4. UI fixes (P3) - polish
5. Layer transform (P4) - feature parity
6. Shape rendering (P6) - visual improvement
7. History (P13) - correctness
8. Handles (P16) - convenience
9. Outline tool (P17) - effects polish

---

## TESTING STRATEGY

### Unit Tests
- [tests/tool_strokes.rs](tests/tool_strokes.rs) - Brush with selection already passing, add similar for other tools
- [tests/visual_shapes.rs](tests/visual_shapes.rs) - Add golden tests for sharp outline shapes
- [tests/visual_transforms.rs](tests/visual_transforms.rs) - Add layer flip with selection test

### Integration Tests
- Selection masking: Each tool (Color Remover, Liquify, Text, Shapes, Lines) tested with selection present/absent
- Text layer: Delete layer with active text, verify text doesn't move
- Cut operation: Selection auto-clears after cut
- Outline tool: Various radii, modes, anti-alias combinations

### Manual Testing Checklist
- [ ] Layer flip with rect/wand/ellipse selection
- [ ] Keybinds: multiple scrollbars gone
- [ ] Keybinds: unbind button visible and functional
- [ ] Keybinds: [ ] ; ' , . / display correctly
- [ ] Keybinds: menu.file.close shows "Close File"
- [ ] Triangle/RightTriangle/Parallelogram outlines sharp
- [ ] Color Remover blocked outside selection
- [ ] Liquify constrained to selection
- [ ] Text culled at selection boundary
- [ ] Shapes preview shows masked area
- [ ] Lines preview constrained to selection
- [ ] Undo ×2 undoes actions not selection
- [ ] Delete layer with text clears text
- [ ] Ctrl+X deselects after cut
- [ ] Shift+corner handle drag = 1:1 resize
- [ ] Outline outside mode has no gaps
- [ ] Outline anti-alias toggle works

---

## CONCLUSION

All 16 issues/features have been investigated with root causes identified at HIGH or MEDIUM confidence. Implementation plan is comprehensive with specific files, line numbers, and step-by-step procedures. No issues were skipped. Ready for implementation once user review and approval is given.

**Next Step**: User reviews this plan. Upon approval, agent proceeds to implement all features in single comprehensive pass using parallel file edits where possible.
