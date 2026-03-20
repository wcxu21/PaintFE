# PaintFE — Automated Test System Plan

> **Purpose**: Define a comprehensive automated test infrastructure that validates PaintFE's tools, filters, effects, adjustments, transforms, selections, layers, blend modes, I/O, scripting, and clipboard operations — catching pixel-level regressions between releases with zero manual effort.

---

## Table of Contents

1. [Design Philosophy](#1-design-philosophy)
2. [Architecture Overview](#2-architecture-overview)
3. [Infrastructure & Dependencies](#3-infrastructure--dependencies)
4. [Test Tiers](#4-test-tiers)
   - Tier 1: Unit Tests (pure functions)
   - Tier 2: Integration Tests (CanvasState / composite pipeline)
   - Tier 3: Visual Regression Tests (pixel-exact snapshot comparison)
   - Tier 4: Scripting-Driven End-to-End Tests (CLI + Rhai)
5. [Test Categories — Detailed Coverage](#5-test-categories--detailed-coverage)
6. [Reference Image (Golden File) System](#6-reference-image-golden-file-system)
7. [Diff & Reporting](#7-diff--reporting)
8. [CI Integration](#8-ci-integration)
9. [Implementation Phases](#9-implementation-phases)
10. [File Layout](#10-file-layout)
11. [Exclusions](#11-exclusions)
12. [Appendix: Full Feature Coverage Matrix](#appendix-full-feature-coverage-matrix)

---

## 1. Design Philosophy

| Principle | Rationale |
|-----------|-----------|
| **Deterministic** | Every test must produce byte-identical output on any machine. No random seeds unless fixed, no system-time-dependent behaviour, no floating-point non-determinism beyond controlled tolerance. |
| **Headless-first** | Tests run via `cargo test` with no GPU, no window, no display server. Visual regression tests use the existing CLI + Rhai scripting pipeline (`execute_script_sync`, `load_image_sync`, `CanvasState::composite`) rather than launching a GUI. |
| **Golden-file comparison** | Reference images (`.png`) are committed to the repo. Tests composite output and compare pixel-by-pixel. Mismatches produce a diff image highlighting changed pixels. |
| **Layered confidence** | Unit tests catch function-level bugs fast. Integration tests catch composition bugs. Visual regression tests catch "the output looks wrong" problems across the full pipeline. |
| **Incremental adoption** | Each phase delivers value independently. Phase 1 can ship alone and still catch regressions in filters/adjustments. |
| **No GPU required for CI** | All tests use CPU paths. GPU paths can optionally be tested locally but are not gated in CI. |

---

## 2. Architecture Overview

```
┌─────────────────────────────────────────────────────────────────┐
│                        cargo test                                │
│                                                                  │
│  ┌──────────┐  ┌──────────────────┐  ┌──────────────────────┐   │
│  │ Tier 1   │  │ Tier 2           │  │ Tier 3               │   │
│  │ Unit     │  │ Integration      │  │ Visual Regression    │   │
│  │          │  │                  │  │                      │   │
│  │ Pure fn  │  │ CanvasState      │  │ Build scene →        │   │
│  │ tests in │  │ manipulation,    │  │ composite →          │   │
│  │ src/ops/ │  │ layer ops,       │  │ compare vs golden    │   │
│  │ modules  │  │ selection math,  │  │ PNG files            │   │
│  │          │  │ history, I/O     │  │                      │   │
│  └──────────┘  └──────────────────┘  └──────────────────────┘   │
│                                                                  │
│  ┌──────────────────────────────────────────────────────────┐   │
│  │ Tier 4: Scripting E2E (optional, uses CLI binary)        │   │
│  │  paintfe --input test.png --script test.rhai -o out.png  │   │
│  │  Compare out.png vs golden                                │   │
│  └──────────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────────┘
```

**Key insight**: PaintFE already has a synchronous, headless execution path:
- `load_image_sync(path)` → `CanvasState`
- `execute_script_sync(source, pixels, w, h, mask)` → processed pixels
- `CanvasState::composite()` → `RgbaImage`
- `encode_and_write(img, path, format, quality, compression)` → saved file

This means we can drive the entire render pipeline without a window, GPU, or event loop.

---

## 3. Infrastructure & Dependencies

### New dev-dependencies (add to Cargo.toml)

```toml
[dev-dependencies]
image = { version = "0.24.7", features = ["png"] }  # already a dep, but ensure available in tests
```

No external test frameworks needed — Rust's built-in `#[test]` + `assert!` is sufficient. The custom diff/report utilities will be written as helper functions in a `tests/` support module.

### Test helper crate (internal)

A shared `tests/common/mod.rs` module providing:

```rust
/// Load a test fixture image from tests/fixtures/
fn load_fixture(name: &str) -> RgbaImage;

/// Load (or create on first run) reference golden image
fn load_golden(name: &str) -> Option<RgbaImage>;

/// Save golden image (for initial generation / --update-golden mode)
fn save_golden(name: &str, img: &RgbaImage);

/// Compare two images pixel-by-pixel with tolerance
fn compare_images(actual: &RgbaImage, expected: &RgbaImage, tolerance: u8) -> CompareResult;

/// Generate a visual diff image (green = match, red = mismatch, blue = size mismatch)
fn generate_diff_image(actual: &RgbaImage, expected: &RgbaImage) -> RgbaImage;

/// Save actual + diff images for debugging when a test fails  
fn save_failure_artifacts(test_name: &str, actual: &RgbaImage, diff: &RgbaImage);

/// Build a standard test canvas (e.g. 512×512 with gradient + shapes + text regions)
fn create_test_canvas(width: u32, height: u32) -> CanvasState;

/// Build a CanvasState with a specific flat RGBA buffer
fn canvas_from_flat(pixels: Vec<u8>, w: u32, h: u32) -> CanvasState;

/// Extract active layer as flat RGBA
fn extract_layer_flat(state: &CanvasState, layer: usize) -> Vec<u8>;
```

### The `CompareResult` struct

```rust
struct CompareResult {
    matches: bool,
    total_pixels: u64,
    mismatched_pixels: u64,
    max_channel_diff: u8,           // worst single-channel deviation
    mean_channel_diff: f64,         // average across all mismatched pixels
    mismatch_percentage: f64,       // 0.0–100.0
    dimensions_match: bool,
    actual_size: (u32, u32),
    expected_size: (u32, u32),
}
```

---

## 4. Test Tiers

### Tier 1 — Unit Tests (in-module `#[cfg(test)]`)

Pure-function tests embedded directly in the source modules. These are fast, isolated, and test mathematical correctness.

**Where**: `#[cfg(test)] mod tests { ... }` blocks inside:
- `src/ops/filters.rs`
- `src/ops/effects.rs`
- `src/ops/adjustments.rs`
- `src/ops/transform.rs`
- `src/ops/shapes.rs`
- `src/ops/text.rs`
- `src/ops/text_layer.rs`
- `src/ops/clipboard.rs`
- `src/canvas.rs` (blend functions, TiledImage, selection math)
- `src/components/history.rs`
- `src/i18n.rs`
- `src/io.rs`

**Pattern**:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gaussian_blur_identity() {
        // sigma=0 should produce identical output
        let img = RgbaImage::from_pixel(8, 8, Rgba([128, 64, 32, 255]));
        let result = parallel_gaussian_blur_pub(&img, 0.0);
        assert_eq!(img, result);
    }
    
    #[test]
    fn test_invert_roundtrip() {
        // invert(invert(x)) == x
        let mut state = test_canvas_state(4, 4);
        let original = state.layers[0].pixels.extract_region_rgba(0, 0, 4, 4);
        invert_colors(&mut state, 0);
        invert_colors(&mut state, 0);
        let result = state.layers[0].pixels.extract_region_rgba(0, 0, 4, 4);
        assert_eq!(original, result);
    }
}
```

**What to test**:
- Identity / no-op cases (blur sigma=0, brightness=0/contrast=0, resize to same dims)
- Roundtrip invariants (invert×2, flip×2, rotate 90×4)
- Boundary conditions (1×1 image, 0-width selection, empty layer)
- Known-value spot checks (specific pixel after a known operation)

---

### Tier 2 — Integration Tests (`tests/` directory)

These test multi-step workflows: create canvas → add layers → apply ops → composite → verify.

**Where**: `tests/*.rs` files (Rust integration test convention — each file is a separate test binary with access to `pub` items from the crate).

**What to test**:
- Layer compositing with blend modes
- Selection masking (apply effect only within selection)
- Undo/redo correctness (apply → undo → pixels match original)
- Multi-layer compositing order
- PFE save/load roundtrip (save → load → composite → identical to pre-save composite)
- Image format I/O roundtrip (save PNG → load → identical pixels)
- Text layer rasterization (create text layer → rasterize → non-empty pixels)
- Script execution on CanvasState (via `execute_script_sync`)

---

### Tier 3 — Visual Regression Tests (`tests/visual/`)

The core of the system. Each test:
1. Builds a `CanvasState` programmatically (or loads from a fixture)
2. Applies one or more operations
3. Composites to an `RgbaImage`
4. Compares pixel-by-pixel against a committed golden `.png`
5. On mismatch: saves `{test_name}_actual.png` and `{test_name}_diff.png` to `tests/output/` and fails

**Where**: `tests/visual/*.rs` — one file per category (tools, filters, effects, adjustments, transforms, blending, selections, layers, shapes, text).

**Golden file workflow**:
```
# First time (generate golden files):
GENERATE_GOLDEN=1 cargo test --test visual

# Normal test run (compare against golden):
cargo test --test visual

# Update a specific golden after intentional change:
GENERATE_GOLDEN=1 cargo test --test visual::filters::test_gaussian_blur
```

---

### Tier 4 — Scripting E2E Tests (`tests/e2e/`)

Tests that exercise the full CLI binary with Rhai scripts, validating the scripting API produces correct output. These use `std::process::Command` to invoke the built binary.

**Where**: `tests/e2e/*.rs` + `tests/e2e/scripts/*.rhai`

**Pattern**:
```rust
#[test]
fn test_script_invert_via_cli() {
    let output = temp_dir().join("invert_out.png");
    let status = Command::new(cargo_bin("PaintFE"))
        .args(&["--input", "tests/fixtures/gradient_8x8.png",
                "--script", "tests/e2e/scripts/invert.rhai",
                "--output", output.to_str().unwrap()])
        .status()
        .expect("failed to run PaintFE CLI");
    assert!(status.success());
    
    let actual = image::open(&output).unwrap().to_rgba8();
    let expected = load_golden("e2e_invert_gradient");
    assert_images_match(&actual, &expected, 0);
}
```

---

## 5. Test Categories — Detailed Coverage

### 5A. Filters (21 effects)

Each filter tested with a standardised source image and known parameters.

| Filter | Test Strategy | Golden |
|--------|--------------|--------|
| Gaussian Blur | sigma=2.0 on gradient image | ✓ |
| Box Blur | radius=3 | ✓ |
| Motion Blur | angle=45°, distance=10 | ✓ |
| Bokeh Blur | radius=5 | ✓ |
| Zoom Blur | center, strength=0.5 | ✓ |
| Sharpen | amount=1.0, radius=1.0 | ✓ |
| Reduce Noise | strength=0.5, radius=2 | ✓ |
| Median | radius=2 | ✓ |
| Pixelate | block_size=8 | ✓ |
| Crystallize | cell_size=16, seed=42 | ✓ |
| Dents | scale=20, amplitude=10, turbulence=2, seed=42 | ✓ |
| Bulge | amount=0.5 | ✓ |
| Twist | angle=45° | ✓ |
| Add Noise | amount=0.3, mono=false, seed=42 | ✓ |
| Glow | radius=3, intensity=0.5 | ✓ |
| Vignette | amount=0.8, softness=0.5 | ✓ |
| Halftone | dot_size=4, angle=45 | ✓ |
| Ink | strength=1.0, threshold=0.5 | ✓ |
| Oil Painting | radius=3, levels=20 | ✓ |
| Pixel Drag | direction=0, amount=20, density=0.5, seed=42 | ✓ |
| RGB Displace | x=5, y=3 | ✓ |
| Color Filter | color=[255,0,0,128], intensity=0.5 | ✓ |

**Test image**: 64×64 synthetic gradient with colour bands (red-green horizontal, blue vertical) — small enough for fast tests, complex enough to reveal filter artefacts.

Plus unit tests for identity/roundtrip:
- `blur(sigma=0)` → identity
- `sharpen(amount=0)` → identity
- `pixelate(block_size=1)` → identity
- `brightness_contrast(0, 0)` → identity

### 5B. Adjustments (20 operations)

| Adjustment | Test Parameters | Unit/Roundtrip Tests |
|-----------|----------------|---------------------|
| Invert | — | invert×2 = identity |
| Invert Alpha | — | invert_alpha×2 = identity |
| Desaturate | — | desaturate(already_grey) = identity |
| Sepia | — | ✓ golden |
| Auto Levels | — | ✓ golden (on known histogram) |
| Brightness/Contrast | B=+30, C=+20 | B=0,C=0 = identity |
| HSL | H=+30°, S=-20, L=+10 | H=0,S=0,L=0 = identity |
| Exposure | +1.0 EV | 0.0 = identity |
| Highlights/Shadows | H=+30, S=-20 | 0,0 = identity |
| Levels | black=0, white=255, gamma=1.0 | defaults = identity |
| Curves | straight line | straight = identity |
| Temperature/Tint | temp=+20, tint=0 | 0,0 = identity |
| Threshold | 128 | ✓ golden |
| Posterize | 4 levels | 256 levels ≈ identity |
| Color Balance | shadows=+10 red | all zeros = identity |
| Gradient Map | custom LUT | ✓ golden |
| Black & White | equal weights | ✓ golden |
| Vibrance | +50 | 0 = identity |

### 5C. Transforms

| Transform | Test | Verification |
|-----------|------|-------------|
| Flip Horizontal | 8×8 numbered pixels | pixel[0,0] ↔ pixel[7,0] |
| Flip Vertical | 8×8 numbered pixels | pixel[0,0] ↔ pixel[0,7] |
| Rotate 90° CW | 4×2 → 2×4 | known pixel positions |
| Rotate 90° CCW | 4×2 → 2×4 | known pixel positions |
| Rotate 180° | — | rotate180 = flip_h + flip_v |
| Flip×2 roundtrip | — | flip(flip(x)) == x |
| Rotate×4 roundtrip | — | rotate90×4 == identity |
| Resize (Nearest) | 8×8 → 16×16 | ✓ golden, known pixel doubling |
| Resize (Bilinear) | 8×8 → 16×16 | ✓ golden |
| Resize (Bicubic) | 8×8 → 16×16 | ✓ golden |
| Resize (Lanczos3) | 8×8 → 16×16 | ✓ golden |
| Resize Canvas | 8×8 → 12×12, anchor=top-left | ✓ transparent border |
| Resize down | 64×64 → 8×8 | ✓ golden |
| Aspect preserve | 100×50 resize to 50×50 | verify 50×25 |

**Multi-layer transform test**: Create 3 layers → flip canvas horizontal → verify all 3 layers flipped.

### 5D. Blend Modes (25 modes)

Test methodology: two 8×8 layers with known pixel patterns, composite with each blend mode, verify against golden.

**Layer A (bottom)**: Horizontal gradient 0→255 in all channels  
**Layer B (top)**: Vertical gradient 0→255 in all channels  

| Blend Mode | Golden | Additional |
|-----------|--------|-----------|
| Normal | ✓ | Also test with 50% opacity |
| Multiply | ✓ | — |
| Screen | ✓ | — |
| Overlay | ✓ | — |
| HardLight | ✓ | — |
| SoftLight | ✓ | — |
| Additive | ✓ | — |
| Lighten | ✓ | — |
| Darken | ✓ | — |
| ColorBurn | ✓ | — |
| ColorDodge | ✓ | — |
| Difference | ✓ | diff(A, A) = black |
| Exclusion | ✓ | — |
| Negation | ✓ | — |
| Reflect | ✓ | — |
| Glow | ✓ | — |
| Subtract | ✓ | — |
| Divide | ✓ | — |
| LinearBurn | ✓ | — |
| VividLight | ✓ | — |
| LinearLight | ✓ | — |
| PinLight | ✓ | — |
| HardMix | ✓ | — |
| Xor | ✓ | — |
| Overwrite | ✓ | — |

**Opacity tests**: Normal blend at opacity 0%, 25%, 50%, 75%, 100%.

### 5E. Selections

| Test | Description |
|------|------------|
| Rect selection → fill | Select 10×10 rect, fill white → only rect affected |
| Ellipse selection → invert | Select ellipse, apply invert → only ellipse inverted |
| Select All → Deselect | select_all then deselect → mask is None |
| Invert selection | select rect → invert → complement selected |
| Feather selection | radius=3 on rect → soft edges ✓ golden |
| Expand selection | expand rect by 5px → larger rect |
| Contract selection | contract rect by 5px → smaller rect |
| Selection + blend mode | Effect applied through selection mask with blend |
| Selection → crop | Select region → crop_to_selection → dims match |
| Selection modes | Replace, Add, Subtract, Intersect on two rects |
| Magic wand (CPU) | Known-colour region, threshold=30 → ✓ golden mask |
| Color range selection | hue=0 (red), tolerance=30 → correct mask |

### 5F. Layer Operations

| Test | Description |
|------|------------|
| Add layer | Add → layer count +1, new layer transparent |
| Delete layer | Delete → layer count -1, active index valid |
| Duplicate layer | Duplicate → pixel-identical copy, independent |
| Move layer order | Move up/down → composite order changes |
| Layer visibility | Hide layer → composite excludes it |
| Layer opacity | 50% opacity → composite result matches golden |
| Merge down | 2 layers → merge → 1 layer with correct composite |
| Merge down as mask | Top layer luminance → bottom layer alpha |
| Flatten | 3 layers → flatten → 1 layer = composite() |
| Layer rename | Rename → name persists |
| Import layer | Import from file → correct dimensions |
| Flip single layer | Flip one of two layers → only that layer flipped |

### 5G. Shapes (17 shapes)

Each shape drawn on a 64×64 canvas with fill colour [255,0,0,255], stroke [0,0,255,255], stroke width 2.

| Shape | Golden |
|-------|--------|
| Rectangle | ✓ |
| Ellipse | ✓ |
| RoundedRect | ✓ |
| Triangle | ✓ |
| RightTriangle | ✓ |
| Pentagon | ✓ |
| Hexagon | ✓ |
| Octagon | ✓ |
| Trapezoid | ✓ |
| Parallelogram | ✓ |
| Diamond | ✓ |
| Cross | ✓ |
| Star5 | ✓ |
| Star6 | ✓ |
| Heart | ✓ |
| Check | ✓ |
| Arrow | ✓ |

### 5H. Drawing Tools

These are tested via the Rhai scripting API where possible, or by directly constructing pixel buffers.

| Tool | Test Approach |
|------|--------------|
| Brush (circle tip) | Construct brush LUT, simulate stroke via `draw_circle_no_dirty` on test TiledImage, compare golden |
| Brush (image tip) | Load tip mask, simulate stamp, compare golden |
| Pencil | 1px hard stroke, verify pixel positions |
| Eraser | Stroke on filled layer → alpha=0 in stroke path |
| Line | Draw line from (0,0) to (63,63), verify Bresenham path |
| Fill | Flood fill on known shape, compare mask/golden |
| Gradient (linear) | Scripting API or direct call to gradient rasterizer, compare golden |
| Clone Stamp | Source from layer A, stamp to B, verify copied pixels |
| Smudge | Smudge stroke on gradient → verify colour mixing |
| Color Picker | Pick pixel → verify returned colour |

**Note**: Interactive tools (MovePixels, MoveSelection, Zoom, Pan, PerspectiveCrop, Liquify, MeshWarp, ContentAware) are difficult to test headlessly because they rely on mouse event sequences. These are covered at a lower priority via:
- Liquify/MeshWarp: test the underlying `warp_displacement_region` and `warp_mesh_catmull_rom` functions directly (Tier 1 unit tests)
- MovePixels: test `extract_to_overlay` + `commit` via integration tests (Tier 2)
- PerspectiveCrop: test `affine_transform_layer` directly

### 5I. Text Layers

| Test | Description |
|------|------------|
| Create text layer | Add text layer → LayerContent::Text confirmed |
| Rasterize text | Set text → rasterize → non-empty pixels |
| Text with styles | Bold + italic + colour → ✓ golden |
| Multi-block text | Two blocks at different positions → ✓ golden |
| Word wrap | Long text with max_width → correct line breaks |
| Text warp: Arc | bend=0.5 → ✓ golden |
| Text warp: Circular | radius=100 → ✓ golden |
| Text outline effect | width=2, outside → ✓ golden |
| Text shadow effect | offset=(3,3), blur=2 → ✓ golden |
| Text gradient fill | linear gradient → ✓ golden |
| Glyph overrides | Move glyph[0] offset=(5,5) → ✓ golden |
| Rasterize to raster | Convert → LayerContent::Raster, pixels preserved |
| Text layer PFE roundtrip | Save V2 → load → text data identical |

**Font pinning**: Text tests MUST use a bundled test font (e.g. ship a small open-source `.ttf` in `tests/fixtures/fonts/`) to ensure consistent rendering across machines. The test helper will set the font path before rasterization.

### 5J. History (Undo/Redo)

| Test | Description |
|------|------------|
| Brush undo | Stroke → undo → pixels match original |
| Filter undo | Blur → undo → pixels match original |
| Snapshot undo | Resize → undo → dimensions + pixels match |
| SingleLayerSnapshot undo | Dialog effect → undo → single layer restored |
| TextLayerEdit undo | Edit text → undo → text data restored |
| Multi-step undo/redo | 3 ops → undo×3 → redo×3 → final state matches |
| Undo after layer delete | Delete → undo → layer restored with pixels |
| History memory pruning | Push commands exceeding limit → oldest pruned |

### 5K. I/O Format Roundtrip

| Format | Test |
|--------|------|
| PNG | save → load → pixel-identical |
| JPEG | save(q=100) → load → within tolerance (lossy) |
| BMP | save → load → pixel-identical |
| TGA | save → load → pixel-identical |
| TIFF (none) | save → load → pixel-identical |
| TIFF (LZW) | save → load → pixel-identical |
| WebP | save → load → within tolerance (lossy) |
| GIF (static) | save → load → quantized palette (verify dims + no crash) |
| PFE V1 | save → load → composite matches |
| PFE V2 (text layers) | save → load → text data + pixels preserved |
| PFE version compat | V0 → V1 → V2 all loadable |

### 5L. Scripting API

Test each Rhai host function via `execute_script_sync`:

| Category | Functions Tested |
|----------|-----------------|
| Canvas read | `width()`, `height()`, `is_selected()` |
| Pixel read/write | `get_pixel`, `set_pixel`, `get_r/g/b/a`, `set_r/g/b/a` |
| Bulk iteration | `for_each_pixel`, `for_region`, `map_channels` |
| Effects (~22) | Each `apply_*` function with known params |
| Transforms | `flip_horizontal`, `flip_vertical`, `rotate_180`, `resize_image`, etc. |
| Utility | `rgb_to_hsl`/`hsl_to_rgb` roundtrip, `clamp`, math functions |
| Canvas ops | `resize_canvas`, `rotate_canvas_90cw`, etc. — verify dimensions change |
| Sandbox limits | Script exceeding max_operations → error (not hang) |

### 5M. Render / Generate Effects

| Effect | Test |
|--------|------|
| Grid | cell_size=16, line_width=1 → ✓ golden |
| Drop Shadow | offset=(5,5), blur=3 → ✓ golden |
| Outline | width=2, red → ✓ golden |
| Contours | threshold=128 → ✓ golden |

---

## 6. Reference Image (Golden File) System

### Directory structure
```
tests/
  golden/                    # Committed reference images (git-tracked)
    filters/
      gaussian_blur_s2.png
      box_blur_r3.png
      ...
    adjustments/
      brightness_30_contrast_20.png
      hsl_h30_s-20_l10.png
      ...
    blending/
      normal_100.png
      normal_50.png
      multiply.png
      ...
    transforms/
      resize_nearest_16x16.png
      ...
    selections/
      rect_fill.png
      feather_r3.png
      ...
    shapes/
      rectangle.png
      ellipse.png
      ...
    layers/
      merge_down.png
      flatten_3_layers.png
      ...
    text/
      basic_text.png
      text_arc_warp.png
      ...
    tools/
      brush_circle_stroke.png
      ...
    io/
      pfe_v2_roundtrip.png
      ...
    scripting/
      script_invert.png
      ...
    generate/
      grid_16.png
      drop_shadow.png
      ...
  output/                    # Generated on test failure (git-ignored)
    {test_name}_actual.png
    {test_name}_diff.png
  fixtures/                  # Input test images and data (git-tracked)
    gradient_8x8.png         # Synthetic 8×8 gradient (known pixel values)
    gradient_64x64.png       # Larger test image
    photo_small.png          # Small realistic photo (64×64 crop) for filter tests
    checkerboard_8x8.png     # Alternating B&W for blend mode tests
    shapes_canvas.png        # Pre-built canvas with various shapes
    fonts/
      test_font.ttf          # Bundled open-source font for deterministic text rendering
    scripts/
      invert.rhai
      blur_and_flip.rhai
      resize_test.rhai
      ...
```

### Golden file generation & update

**Environment variable control**:
- `GENERATE_GOLDEN=1` — write actual output as the new golden file instead of comparing
- `GOLDEN_TOLERANCE=2` — per-channel tolerance (default: 0 for pixel-exact)

**Update workflow after intentional changes**:
```bash
# Regenerate all golden files
GENERATE_GOLDEN=1 cargo test --test visual 2>&1 | tee golden_update.log

# Regenerate only one category
GENERATE_GOLDEN=1 cargo test --test visual::filters

# Review changes
git diff --stat tests/golden/
# Visually inspect changed images before committing
```

### Size budget

Golden files should be small (8×8 to 64×64 pixels). At 64×64 PNG, each file is ~1–4 KB. With ~200 golden files, total storage is ~200–800 KB — negligible in the repo.

---

## 7. Diff & Reporting

### Diff image format

When a test fails, two files are saved:
- `tests/output/{test_name}_actual.png` — what the test produced
- `tests/output/{test_name}_diff.png` — visual diff

The diff image encoding:
- **Green pixel**: actual matches expected (within tolerance)
- **Red pixel**: mismatch — brightness proportional to magnitude of difference
- **Blue border/pixel**: dimension mismatch (actual size ≠ expected size)

### Console output on failure

```
FAILED: test_gaussian_blur_s2
  Dimensions: 64×64 (match)
  Mismatched pixels: 147 / 4096 (3.59%)
  Max channel diff: 12
  Mean channel diff: 3.4
  Artifacts saved:
    tests/output/gaussian_blur_s2_actual.png
    tests/output/gaussian_blur_s2_diff.png
  Golden file:
    tests/golden/filters/gaussian_blur_s2.png
```

### Summary report

An optional post-test summary (as a `#[test]` that runs last or a build script) can generate a Markdown report:

```markdown
## Visual Regression Report — 2026-03-19
| Category | Pass | Fail | Skip |
|----------|------|------|------|
| Filters | 21 | 1 | 0 |
| Adjustments | 20 | 0 | 0 |
| ...
| **Total** | **185** | **1** | **3** |
```

---

## 8. CI Integration

### Addition to `.github/workflows/ci.yml`

```yaml
  test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - name: Run tests
        run: cargo test --all-targets -- --test-threads=1
        env:
          GOLDEN_TOLERANCE: "0"
      - name: Upload failure artifacts
        if: failure()
        uses: actions/upload-artifact@v4
        with:
          name: test-failures
          path: tests/output/
          retention-days: 14
```

**Key decisions**:
- `--test-threads=1` for visual tests to avoid non-determinism from rayon's thread pool (unit tests can parallelise)
- Upload `tests/output/` as GitHub Actions artefacts on failure so developers can inspect diffs without reproducing locally
- Golden tolerance = 0 in CI (pixel-exact); developers can use higher tolerance locally during development

### Pre-push hook (optional)

```bash
#!/bin/sh
# .git/hooks/pre-push
cargo test --lib --test unit -- --quiet || exit 1
```

---

## 9. Implementation Phases

### Phase 1 — Foundation + Filters/Adjustments (~first batch)

**Deliverables**:
1. `tests/common/mod.rs` — test helpers (load fixture, compare images, generate diff, save artefacts)
2. `tests/fixtures/` — synthetic test images (gradient 8×8, gradient 64×64, checkerboard)
3. Unit tests in `src/ops/filters.rs` — identity + roundtrip tests for all filters
4. Unit tests in `src/ops/adjustments.rs` — identity + roundtrip tests for all adjustments
5. Visual regression tests: `tests/visual/filters.rs` — 22 golden-file tests
6. Visual regression tests: `tests/visual/adjustments.rs` — 20 golden-file tests
7. Golden files generated for all filter + adjustment tests
8. CI workflow updated to run tests

**Why first**: Filters and adjustments are pure functions with no UI dependency — easiest to test and most likely to regress when optimising core image processing code.

### Phase 2 — Transforms + Blend Modes + Canvas Ops

**Deliverables**:
1. Unit tests in `src/ops/transform.rs` — roundtrip tests (flip×2, rotate×4)
2. Unit tests in `src/canvas.rs` — blend mode spot checks (known input → known output)
3. Visual regression: `tests/visual/transforms.rs` — resize, flip, rotate goldens
4. Visual regression: `tests/visual/blending.rs` — all 25 blend modes + opacity variants
5. Visual regression: `tests/visual/canvas_ops.rs` — resize canvas, flatten
6. Integration tests: `tests/integration/layers.rs` — add/delete/duplicate/merge/flatten/reorder

### Phase 3 — Selections + Shapes + Tools

**Deliverables**:
1. Unit tests for selection mask operations (feather, expand, contract, invert)
2. Visual regression: `tests/visual/selections.rs` — selection + effect application goldens
3. Visual regression: `tests/visual/shapes.rs` — all 17 shapes as golden files
4. Integration tests for drawing tools (brush stroke via direct function call, not UI events)
5. Integration tests for Fill/Gradient tool core functions
6. Integration tests for Liquify/MeshWarp underlying warp functions

### Phase 4 — Text Layers + History + I/O

**Deliverables**:
1. Integration tests: text layer creation, rasterization, effects, warps, glyph overrides
2. Visual regression: `tests/visual/text.rs` — text rendering goldens (requires bundled font)
3. Integration tests: `tests/integration/history.rs` — undo/redo correctness for all command types
4. Integration tests: `tests/integration/io.rs` — format roundtrip tests (PNG, BMP, TGA, TIFF, PFE V1/V2)
5. Visual regression: `tests/visual/generate.rs` — Grid, DropShadow, Outline, Contours goldens

### Phase 5 — Scripting E2E + Polish

**Deliverables**:
1. `tests/e2e/scripting.rs` — each Rhai host function tested via `execute_script_sync`
2. `tests/e2e/cli.rs` — CLI binary invoked with `Command`, output compared to golden
3. Rhai scripts in `tests/fixtures/scripts/` covering all API tiers
4. Sandbox limit tests (max operations, call depth)
5. Optional: CI artefact upload, Markdown summary report

---

## 10. File Layout

```
tests/
  common/
    mod.rs              # Shared test utilities (compare, diff, fixture loading)
  
  # Tier 1 — Unit tests are in-module (#[cfg(test)] in src/ops/*.rs, src/canvas.rs)
  
  # Tier 2 — Integration tests
  integration/
    layers.rs           # Layer add/delete/merge/flatten/reorder
    history.rs          # Undo/redo for all command types
    io.rs               # Format roundtrip tests
    clipboard.rs        # Copy/paste/cut
    
  # Tier 3 — Visual regression tests
  visual/
    mod.rs              # Shared visual test setup
    filters.rs          # All filter golden tests
    adjustments.rs      # All adjustment golden tests
    transforms.rs       # Flip/rotate/resize golden tests
    blending.rs         # All 25 blend modes golden tests
    selections.rs       # Selection + effect application
    shapes.rs           # All 17 shapes
    tools.rs            # Brush/pencil/line/fill/gradient
    text.rs             # Text layer rendering
    generate.rs         # Grid/shadow/outline/contours
    canvas_ops.rs       # Resize canvas, flatten
  
  # Tier 4 — End-to-end
  e2e/
    scripting.rs        # Rhai API tests via execute_script_sync
    cli.rs              # CLI binary tests
    scripts/            # Rhai test scripts
      invert.rhai
      blur_s2.rhai
      flip_and_resize.rhai
      set_all_red.rhai
      sandbox_limit.rhai
      ...
  
  # Data
  fixtures/
    gradient_8x8.png
    gradient_64x64.png
    photo_small.png
    checkerboard_8x8.png
    two_layer_canvas.pfe
    text_layer_canvas.pfe
    fonts/
      test_font.ttf     # Open-source font for deterministic text tests
  
  golden/               # Reference images (git-tracked, ~200 files, ~500 KB total)
    filters/
    adjustments/
    blending/
    transforms/
    selections/
    shapes/
    layers/
    text/
    tools/
    generate/
    io/
    scripting/
  
  output/               # Failure artefacts (git-ignored)
```

Add to `.gitignore`:
```
tests/output/
```

---

## 11. Exclusions

These features are **excluded** from automated testing (manual testing only):

| Feature | Reason |
|---------|--------|
| AI Background Removal | Requires external ONNX Runtime DLL + model file; non-deterministic across platforms |
| GPU compute pipelines | Requires GPU hardware. CPU fallbacks are tested instead. GPU tests are optional/local-only. |
| GUI interactions (mouse events, dialogs) | No headless GUI framework. Tools are tested via their underlying functions, not via simulated clicks. |
| System clipboard | OS-dependent, needs display server. Internal clipboard buffer is tested. |
| File dialogs (rfd) | OS-dependent native dialogs, not testable headlessly |
| Single-instance IPC | Windows named pipes, platform-specific |
| Print | OS-dependent print dialog |
| Animated GIF/APNG export | Frame-timing dependent; tested only for "does not crash" + correct frame count |
| RAW file decoding | Requires proprietary camera RAW samples; tested only via format detection, not pixel accuracy |
| Theme / UI layout | Visual-only, no pixel output to compare |

---

## Appendix: Full Feature Coverage Matrix

Estimated **~220 test cases** across all tiers:

| Category | Tier 1 (Unit) | Tier 2 (Integration) | Tier 3 (Visual) | Tier 4 (E2E) | Total |
|----------|:---:|:---:|:---:|:---:|:---:|
| Filters | 10 | — | 22 | 5 | 37 |
| Adjustments | 12 | — | 20 | 5 | 37 |
| Transforms | 8 | 4 | 10 | 3 | 25 |
| Blend Modes | 5 | — | 27 | — | 32 |
| Selections | 4 | 6 | 6 | 2 | 18 |
| Layers | — | 12 | 3 | — | 15 |
| Shapes | — | — | 17 | — | 17 |
| Tools | 2 | 6 | 6 | — | 14 |
| Text Layers | 2 | 8 | 6 | — | 16 |
| History | — | 8 | — | — | 8 |
| I/O Roundtrip | — | 11 | — | 2 | 13 |
| Scripting API | — | — | — | 12 | 12 |
| Generate Effects | — | — | 4 | — | 4 |
| **Total** | **43** | **55** | **121** | **29** | **~248** |

### Test execution time budget

- Tier 1 (unit): <2 seconds — pure function tests on tiny images
- Tier 2 (integration): <10 seconds — CanvasState manipulation
- Tier 3 (visual): <30 seconds — 121 tests × ~250ms each (64×64 images, CPU-only compositing)
- Tier 4 (E2E): <20 seconds — CLI binary invocation + script execution

**Total**: <60 seconds for the full suite — fast enough to run on every push.

---

## 12. GPU-Dependent & Interactive Tool Testing Plan

The following tools are tightly coupled to GPU pipelines or UI input event sequences, making headless unit/integration testing impractical. This section describes strategies for testing them.

### 12A. Problem Statement

These tools cannot be tested via the current `cargo test` framework:

| Tool | Blocking Dependency |
|------|-------------------|
| Fill Bucket | `perform_flood_fill` requires `Option<&mut GpuRenderer>` for the distance map; CPU fallback uses async rayon worker + channel |
| Magic Wand | Same GPU flood fill pipeline as Fill |
| Gradient | `render_gradient_to_preview` is private on `ToolsPanel`, CPU fallback inline |
| Liquify | `GpuLiquifyPipeline` for real-time warp; CPU fallback exists (`warp_displacement_region`) but not wired for stroke simulation |
| Mesh Warp | `GpuMeshWarpDisplacementPipeline` for displacement; CPU fallback exists (`generate_displacement_from_mesh_fast`) |
| Move Pixels | Multi-step UI state machine (click→drag→commit), pixel extraction/paste via `CanvasState` overlay |
| Move Selection | Selection mask translation tied to mouse drag events |
| Perspective Crop | 4-corner interactive UI + `affine_transform_layer` commit |
| Content-Aware Brush | Inpainting requires patch-match + multi-step UI |
| Zoom / Pan | Pure viewport transforms — nothing to pixel-test |

### 12B. Strategy 1 — Extract & Test Underlying Pure Functions

Many GPU tools have CPU fallback functions that *can* be tested directly:

| Function | Location | Testable? |
|----------|----------|-----------|
| `warp_displacement_region(src, field, w, h)` | `src/ops/transform.rs` | **Yes** — pure CPU warp |
| `warp_displacement_full(src, field, w, h)` | `src/ops/transform.rs` | **Yes** — full-image warp |
| `warp_mesh_catmull_rom(src, original, deformed, grid_size)` | `src/ops/transform.rs` | **Yes** — mesh warp end-to-end |
| `generate_displacement_from_mesh_fast(original, deformed, grid, w, h)` | `src/ops/transform.rs` | **Yes** — displacement field only |
| `catmull_rom_surface(grid, u, v)` | `src/ops/text_layer.rs` | **Yes** — math function |
| `affine_transform_layer(pixels, w, h, matrix)` | `src/ops/transform.rs` | **Yes** — used by perspective crop |
| `compute_flood_distance_map(source, start, w, h)` | `src/components/tools.rs` | Check visibility — may need pub |
| `inpaint_region(source, mask, w, h)` | `src/ops/inpaint.rs` | **Yes** if pub |

**Action items**:
1. Make `warp_mesh_catmull_rom`, `generate_displacement_from_mesh_fast`, `affine_transform_layer` pub if not already
2. Write golden tests for each using small synthetic inputs (32×32 gradient warped by known displacement)
3. Test `catmull_rom_surface` with known control points against hand-computed expected values

### 12C. Strategy 2 — Rhai Scripting Bridge

Some tools can be tested indirectly through the scripting API if we add thin script-callable wrappers:

```rhai
// Hypothetical future API additions:
fill_at(x, y, tolerance, color_r, color_g, color_b, color_a);
select_magic_wand(x, y, tolerance, mode);  // "replace"|"add"|"subtract"|"intersect"
gradient_linear(x1, y1, x2, y2, color1, color2);
```

These would call the CPU fallback path internally (no GPU needed). Adding these to the Rhai host API would enable automated testing of tool logic without GPU or UI scaffolding.

**Action items**:
1. Add `fill_at()` to Rhai API — calls `perform_flood_fill` with `gpu_renderer: None`
2. Add `select_color_range()` and `select_rect()`/`select_ellipse()` to Rhai API
3. Test via `execute_script_sync` in `tests/scripting.rs`

### 12D. Strategy 3 — Automated UI Testing (E2E)

For full end-to-end validation of GPU-dependent tools with real rendering, use automated UI interaction:

#### Option A: Built-in Test Mode (Recommended)

Add a `--test-mode <script.json>` CLI flag that:
1. Launches the full GUI with GPU
2. Reads a JSON test script describing mouse/keyboard actions
3. Executes actions with timing delays
4. Captures the final canvas composite as a PNG
5. Compares against a golden file
6. Exits with code 0 (pass) or 1 (fail)

**Test script format**:
```json
{
  "name": "fill_bucket_basic",
  "setup": {
    "canvas_size": [64, 64],
    "layers": [{ "fill": [255, 255, 255, 255] }]
  },
  "actions": [
    { "select_tool": "Fill" },
    { "set_color": [255, 0, 0, 255] },
    { "set_tolerance": 50 },
    { "click": [32, 32] },
    { "wait_ms": 500 }
  ],
  "verify": {
    "golden": "tests/golden/e2e/fill_bucket_basic.png",
    "tolerance": 2,
    "pixel_checks": [
      { "pos": [32, 32], "expected": [255, 0, 0, 255], "tolerance": 0 }
    ]
  }
}
```

**Execution**:
```powershell
cargo run -- --test-mode tests/e2e_scripts/fill_bucket_basic.json
```

**Advantages**: Real GPU rendering, real compositor, real tool state machines.
**Disadvantages**: Requires display (no headless CI without virtual framebuffer), slow (~2s per test), flaky timing.

#### Option B: Windows UI Automation (AutoHotkey / PowerShell)

Use external automation to drive the already-running PaintFE binary:

```powershell
# Launch app, wait for window
Start-Process "cargo" "run" -WorkingDirectory $PSScriptRoot
Start-Sleep -Seconds 5

# Use SendKeys or Win32 API to simulate:
# 1. Select fill tool (keyboard shortcut)
# 2. Click canvas at (x, y)
# 3. Wait for render
# 4. Ctrl+S to save
# 5. Compare output file against golden

# AutoHotkey alternative:
# ahk_script.ahk sends keystrokes + mouse clicks
```

**Advantages**: No code changes needed.
**Disadvantages**: Extremely fragile, OS-specific, relies on window position/timing.

#### Option C: Integration Test with GPU Context (Advanced)

Create a headless wgpu device in the test:

```rust
#[test]
fn fill_tool_gpu() {
    // Create headless GPU context
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::default());
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::LowPower,
        compatible_surface: None,
        force_fallback_adapter: true,  // software renderer
    })).unwrap();
    let (device, queue) = pollster::block_on(adapter.request_device(...)).unwrap();
    
    // Build GpuRenderer with this device
    let gpu_renderer = GpuRenderer::new(device, queue);
    
    // Now call tool functions with real GPU context
    let mut tools = ToolsPanel::default();
    let mut state = CanvasState::new(64, 64);
    tools.perform_flood_fill(&mut state, (32, 32), false, RED_F32, WHITE_F32, Some(&mut gpu_renderer));
    
    // Compare result
    let result = state.composite();
    assert_golden("e2e", "fill_gpu", &result);
}
```

**Advantages**: Real GPU code paths tested, deterministic, fast, runs in CI.
**Disadvantages**: Requires `force_fallback_adapter` (software GPU) which may behave differently from real hardware. `GpuRenderer::new` may need refactoring to accept external device/queue.

**Action items** (if pursuing Option C):
1. Refactor `GpuRenderer::new` to accept pre-built `wgpu::Device` + `wgpu::Queue`
2. Add `pollster` and `wgpu` to `[dev-dependencies]`
3. Create `tests/gpu_tools.rs` with `#[cfg(feature = "gpu-tests")]` gate

### 12E. Recommended Implementation Order

1. **Immediate (no code changes)**: Test underlying pure functions (Strategy 1)
2. **Short-term**: Add Rhai wrappers for fill/selection (Strategy 2)
3. **Medium-term**: Built-in `--test-mode` JSON runner (Strategy 3, Option A)
4. **Long-term/optional**: Headless GPU context tests (Strategy 3, Option C)

### 12F. Quick Fail Summary Command

To check whether any tests failed across all executables in a single glance:

```powershell
# PowerShell — one-liner: exits 0 if all pass, 1 if any fail
cargo test 2>&1 | Tee-Object -Variable testOutput; if ($testOutput -match 'FAILED') { Write-Host "`n❌ SOME TESTS FAILED" -ForegroundColor Red; exit 1 } else { Write-Host "`n✅ ALL TESTS PASSED" -ForegroundColor Green }
```

```powershell
# Compact summary only:
cargo test 2>&1 | Select-String "test result:" | ForEach-Object { $_.Line }
```

```powershell
# Total pass/fail count:
$r = cargo test 2>&1 | Select-String "(\d+) passed.*?(\d+) failed"; $p = ($r | ForEach-Object { [int]$_.Matches[0].Groups[1].Value } | Measure-Object -Sum).Sum; $f = ($r | ForEach-Object { [int]$_.Matches[0].Groups[2].Value } | Measure-Object -Sum).Sum; Write-Host "Total: $p passed, $f failed"
```

---

## Notes for Implementation

1. **Test image generation**: The first helper to write is `create_test_gradient(w, h) -> RgbaImage` which produces a deterministic gradient image. All visual tests derive their input from this or other deterministic generators — never from external/random sources.

2. **Parallelism control**: Visual regression tests should set `rayon::ThreadPoolBuilder::new().num_threads(1).build_global()` in a test init (or use `--test-threads=1`) to avoid non-determinism from parallel compositing.

3. **Float tolerance**: Some filters use `f32` math internally which may produce ±1 LSB differences across platforms. The golden comparison should support a `tolerance: u8` parameter (default 0, but settable per-test for known-imprecise operations like Gaussian blur).

4. **Font determinism**: Text tests are the trickiest because font rasterization depends on the font engine. Bundling a specific `.ttf` and loading it directly (via `ab_glyph`) instead of using system font enumeration ensures cross-platform consistency.

5. **Test ordering**: Tests should be independent — no shared mutable state, no reliance on execution order. Each test creates its own `CanvasState` from scratch.

6. **Leveraging existing CLI**: Many tests can be structured as Rhai scripts executed via `execute_script_sync`, which provides the most integration coverage with the least test code. The scripting API already wraps most filter/effect/transform operations.
