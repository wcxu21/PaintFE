# PaintFE Modularization Handoff

## Current Status

- Phase 1 is complete enough for Phase 2 work.
- `assets.rs`, `ops/dialogs.rs`, `ops/effect_dialogs.rs`, `ops/effects.rs`, and `components/tools.rs` are façade-style roots with extracted internals.
- `canvas.rs` and `app.rs` are now segmented roots that preserve the existing public API and compile cleanly.
- `ops/text_layer.rs` is now segmented into façade + top-level chunks and compiles cleanly.
- `app/runtime.rs` is now wired through extracted update helpers instead of keeping all logic inline in `fn update()`.

## Phase 2 Structure In Progress

### `src/canvas.rs`

- Root is now a tiny façade.
- Stable split in this batch:
  - `src/canvas/defs.rs`
  - `src/canvas/selection.rs`
  - `src/canvas/tiled_image.rs`
  - `src/canvas/layers.rs`
  - `src/canvas/mirror.rs`
  - `src/canvas/canvas_state.rs`
  - `src/canvas/view_full.rs`
  - `src/canvas/view/core.rs`
  - `src/canvas/view/overlay.rs`
  - `src/canvas/view/helpers.rs`

### `src/app.rs`

- Root is now a tiny façade.
- Stable split in this batch:
  - `src/app/types.rs`
  - `src/app/bootstrap.rs`
  - `src/app/runtime.rs`
  - `src/app/project_io.rs`
  - `src/app/ops.rs`
  - `src/app/ops/helpers.rs`
  - `src/app/ops/dialogs.rs`
  - `src/app/panels.rs`
- `src/app/runtime.rs` now delegates to extracted update chunks under `src/app/runtime/update/`:
  - `lifecycle_async.rs`
  - `input_shortcuts.rs`
  - `dialogs_menu.rs`
  - `canvas_tail.rs`

### `src/ops/text_layer.rs`

- Root is now a tiny façade.
- Stable split in this batch:
  - `src/ops/text_layer/core.rs`
  - `src/ops/text_layer/data_impl.rs`
  - `src/ops/text_layer/block_impl.rs`
  - `src/ops/text_layer/selection.rs`
  - `src/ops/text_layer/raster.rs`
  - `src/ops/text_layer/layout.rs`
  - `src/ops/text_layer/warp.rs`
  - `src/ops/text_layer/effects.rs`

### New Facade Root

- `src/document/*` now provides submodule façades for:
  - selection
  - tiled_image
  - layer
  - mirror
  - canvas_state
  - text
- `src/render/*` now exists as façade modules over current CPU/GPU rendering code.
- `src/services/*` now exists as façade modules over current IO/project/clipboard/scripting/IPC code.
- Removed stale duplicate legacy files `src/canvas/view.rs` and `src/canvas/view_impl.rs`; live canvas view code is the `view_full.rs` façade plus `src/canvas/view/*` chunks.

## Next Cheap Wins

1. Keep `crate::canvas::*` and `crate::app::*` stable while moving call sites toward `crate::document::*`.
2. Start pointing any new code at `crate::render::*` and `crate::services::*` instead of top-level `gpu`, `io`, `ipc`, or `project`.
3. Peel `CanvasState`-specific helpers out of `src/canvas/view_full.rs` when a behavior change is already touching them.
4. `src/app/ops/dialogs.rs` is still the next large dispatcher if more decomposition is needed.
5. Preferred next split for `src/app/runtime/update/input_shortcuts.rs` and `src/app/runtime/update/dialogs_menu.rs`: continue breaking them down by behavior/helper family now that `src/app/runtime.rs` itself is just orchestration.
6. Phase 3 should introduce service traits only after the façade module boundaries have been exercised by real patches.
7. Current hard hotspots by size are `src/app/runtime.rs` and `src/app/ops/dialogs.rs`.
9. `src/canvas/view_full.rs` is now only a façade; the real remaining canvas-view chunks are `src/canvas/view/core.rs` and `src/canvas/view/overlay.rs`.
8. `src/ops/text_layer.rs` has already been reduced to sub-1k chunks and is no longer a primary hotspot.

## Validation

- Use `cargo check` after each extraction batch.
- Avoid full test runs unless a behavior patch follows.
- Latest extraction batch status: `cargo check` passes.
- Note: there is exploratory prep under `src/app/runtime/update/`, but it is not wired into the build because raw `include!` body chunking broke shared local scope.
