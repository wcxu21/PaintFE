// --- Bokeh Blur (disc-shaped kernel) ---

pub fn bokeh_blur(state: &mut CanvasState, layer_idx: usize, radius: f32) {
    if layer_idx >= state.layers.len() {
        return;
    }
    let flat = state.layers[layer_idx].pixels.to_rgba_image();
    let result = bokeh_blur_core(&flat, radius, state.selection_mask.as_ref());
    commit_to_layer(state, layer_idx, &result);
}

pub fn bokeh_blur_from_flat(
    state: &mut CanvasState,
    layer_idx: usize,
    radius: f32,
    original_flat: &RgbaImage,
) {
    let result = bokeh_blur_core(original_flat, radius, state.selection_mask.as_ref());
    commit_to_layer(state, layer_idx, &result);
}

pub fn bokeh_blur_core(flat: &RgbaImage, radius: f32, mask: Option<&GrayImage>) -> RgbaImage {
    if radius < 0.5 {
        return flat.clone();
    }
    let w = flat.width() as usize;
    let h = flat.height() as usize;
    if w == 0 || h == 0 {
        return flat.clone();
    }

    // Build disc kernel (coordinates + weights).
    let r = radius.ceil() as i32;
    let r2 = radius * radius;
    let mut offsets: Vec<(i32, i32, f32)> = Vec::new();
    for dy in -r..=r {
        for dx in -r..=r {
            let d2 = (dx * dx + dy * dy) as f32;
            if d2 <= r2 {
                // Bokeh uses equal weight (disc average).
                offsets.push((dx, dy, 1.0));
            }
        }
    }
    let inv_count = 1.0 / offsets.len() as f32;

    let src_raw = flat.as_raw();
    let mut dst_raw = vec![0u8; w * h * 4];
    let stride = w * 4;
    let mask_raw = mask.map(|m| m.as_raw().as_slice());
    let mask_w = mask.map_or(0, |m| m.width() as usize);
    let mask_h = mask.map_or(0, |m| m.height() as usize);

    dst_raw
        .par_chunks_mut(stride)
        .enumerate()
        .for_each(|(y, row_out)| {
            for x in 0..w {
                let pi = x * 4;
                if let Some(mr) = mask_raw
                    && x < mask_w
                    && y < mask_h
                    && mr[y * mask_w + x] == 0
                {
                    let src_off = y * stride + pi;
                    row_out[pi..pi + 4].copy_from_slice(&src_raw[src_off..src_off + 4]);
                    continue;
                }
                let mut r_sum = 0.0f32;
                let mut g_sum = 0.0f32;
                let mut b_sum = 0.0f32;
                let mut a_sum = 0.0f32;
                for &(dx, dy, _wt) in &offsets {
                    let sx = (x as i32 + dx).clamp(0, w as i32 - 1) as usize;
                    let sy = (y as i32 + dy).clamp(0, h as i32 - 1) as usize;
                    let si = sy * stride + sx * 4;
                    r_sum += src_raw[si] as f32;
                    g_sum += src_raw[si + 1] as f32;
                    b_sum += src_raw[si + 2] as f32;
                    a_sum += src_raw[si + 3] as f32;
                }
                row_out[pi] = (r_sum * inv_count).round().clamp(0.0, 255.0) as u8;
                row_out[pi + 1] = (g_sum * inv_count).round().clamp(0.0, 255.0) as u8;
                row_out[pi + 2] = (b_sum * inv_count).round().clamp(0.0, 255.0) as u8;
                row_out[pi + 3] = (a_sum * inv_count).round().clamp(0.0, 255.0) as u8;
            }
        });

    RgbaImage::from_raw(w as u32, h as u32, dst_raw).unwrap()
}

// --- Motion Blur (directional) ---

pub fn motion_blur(state: &mut CanvasState, layer_idx: usize, angle_deg: f32, distance: f32) {
    if layer_idx >= state.layers.len() {
        return;
    }
    let flat = state.layers[layer_idx].pixels.to_rgba_image();
    let result = motion_blur_core(&flat, angle_deg, distance, state.selection_mask.as_ref());
    commit_to_layer(state, layer_idx, &result);
}

pub fn motion_blur_from_flat(
    state: &mut CanvasState,
    layer_idx: usize,
    angle_deg: f32,
    distance: f32,
    original_flat: &RgbaImage,
) {
    let result = motion_blur_core(
        original_flat,
        angle_deg,
        distance,
        state.selection_mask.as_ref(),
    );
    commit_to_layer(state, layer_idx, &result);
}

pub fn motion_blur_core(
    flat: &RgbaImage,
    angle_deg: f32,
    distance: f32,
    mask: Option<&GrayImage>,
) -> RgbaImage {
    if distance < 1.0 {
        return flat.clone();
    }
    let w = flat.width() as usize;
    let h = flat.height() as usize;
    if w == 0 || h == 0 {
        return flat.clone();
    }

    let angle = angle_deg.to_radians();
    let steps = distance.ceil() as i32;
    let dx = angle.cos();
    let dy = angle.sin();
    let inv_steps = 1.0 / (steps * 2 + 1) as f32;

    let src_raw = flat.as_raw();
    let mut dst_raw = vec![0u8; w * h * 4];
    let stride = w * 4;
    let mask_raw = mask.map(|m| m.as_raw().as_slice());
    let mask_w = mask.map_or(0, |m| m.width() as usize);
    let mask_h = mask.map_or(0, |m| m.height() as usize);

    dst_raw
        .par_chunks_mut(stride)
        .enumerate()
        .for_each(|(y, row_out)| {
            for x in 0..w {
                let pi = x * 4;
                if let Some(mr) = mask_raw
                    && x < mask_w
                    && y < mask_h
                    && mr[y * mask_w + x] == 0
                {
                    let src_off = y * stride + pi;
                    row_out[pi..pi + 4].copy_from_slice(&src_raw[src_off..src_off + 4]);
                    continue;
                }
                let mut r_sum = 0.0f32;
                let mut g_sum = 0.0f32;
                let mut b_sum = 0.0f32;
                let mut a_sum = 0.0f32;
                for i in -steps..=steps {
                    let sx = (x as f32 + i as f32 * dx).round() as i32;
                    let sy = (y as f32 + i as f32 * dy).round() as i32;
                    let sx = sx.clamp(0, w as i32 - 1) as usize;
                    let sy = sy.clamp(0, h as i32 - 1) as usize;
                    let si = sy * stride + sx * 4;
                    r_sum += src_raw[si] as f32;
                    g_sum += src_raw[si + 1] as f32;
                    b_sum += src_raw[si + 2] as f32;
                    a_sum += src_raw[si + 3] as f32;
                }
                row_out[pi] = (r_sum * inv_steps).round().clamp(0.0, 255.0) as u8;
                row_out[pi + 1] = (g_sum * inv_steps).round().clamp(0.0, 255.0) as u8;
                row_out[pi + 2] = (b_sum * inv_steps).round().clamp(0.0, 255.0) as u8;
                row_out[pi + 3] = (a_sum * inv_steps).round().clamp(0.0, 255.0) as u8;
            }
        });

    RgbaImage::from_raw(w as u32, h as u32, dst_raw).unwrap()
}

// --- Box Blur (square kernel, separable for speed) ---

pub fn box_blur(state: &mut CanvasState, layer_idx: usize, radius: f32) {
    if layer_idx >= state.layers.len() {
        return;
    }
    let flat = state.layers[layer_idx].pixels.to_rgba_image();
    let result = box_blur_core(&flat, radius, state.selection_mask.as_ref());
    commit_to_layer(state, layer_idx, &result);
}

pub fn box_blur_from_flat(
    state: &mut CanvasState,
    layer_idx: usize,
    radius: f32,
    original_flat: &RgbaImage,
) {
    let result = box_blur_core(original_flat, radius, state.selection_mask.as_ref());
    commit_to_layer(state, layer_idx, &result);
}

pub fn box_blur_core(flat: &RgbaImage, radius: f32, mask: Option<&GrayImage>) -> RgbaImage {
    if radius < 0.5 {
        return flat.clone();
    }
    let w = flat.width() as usize;
    let h = flat.height() as usize;
    if w == 0 || h == 0 {
        return flat.clone();
    }

    let r = radius.ceil() as usize;
    let kernel_size = r * 2 + 1;
    let inv_k = 1.0 / (kernel_size as f32);
    let src_raw = flat.as_raw();

    // Separable: horizontal pass
    let mut h_buf = vec![0.0f32; w * h * 4];
    h_buf
        .par_chunks_mut(w * 4)
        .enumerate()
        .for_each(|(y, row_out)| {
            for x in 0..w {
                let mut sums = [0.0f32; 4];
                for k in 0..kernel_size {
                    let sx = (x as i32 + k as i32 - r as i32).clamp(0, w as i32 - 1) as usize;
                    let si = y * w * 4 + sx * 4;
                    for c in 0..4 {
                        sums[c] += src_raw[si + c] as f32;
                    }
                }
                let oi = x * 4;
                for c in 0..4 {
                    row_out[oi + c] = sums[c] * inv_k;
                }
            }
        });

    // Vertical pass
    let mut v_buf = vec![0.0f32; w * h * 4];
    v_buf
        .par_chunks_mut(w * 4)
        .enumerate()
        .for_each(|(y, row_out)| {
            for x in 0..w {
                let mut sums = [0.0f32; 4];
                for k in 0..kernel_size {
                    let sy = (y as i32 + k as i32 - r as i32).clamp(0, h as i32 - 1) as usize;
                    let si = sy * w * 4 + x * 4;
                    for c in 0..4 {
                        sums[c] += h_buf[si + c];
                    }
                }
                let oi = x * 4;
                for c in 0..4 {
                    row_out[oi + c] = sums[c] * inv_k;
                }
            }
        });

    // Apply mask
    let mask_raw = mask.map(|m| m.as_raw().as_slice());
    let mask_w = mask.map_or(0, |m| m.width() as usize);
    let mask_h = mask.map_or(0, |m| m.height() as usize);
    let mut dst_raw = vec![0u8; w * h * 4];
    dst_raw
        .par_chunks_mut(w * 4)
        .enumerate()
        .for_each(|(y, row_out)| {
            for x in 0..w {
                let pi = x * 4;
                if let Some(mr) = mask_raw
                    && x < mask_w
                    && y < mask_h
                    && mr[y * mask_w + x] == 0
                {
                    let si = y * w * 4 + pi;
                    row_out[pi..pi + 4].copy_from_slice(&src_raw[si..si + 4]);
                    continue;
                }
                let vi = y * w * 4 + pi;
                for c in 0..4 {
                    row_out[pi + c] = v_buf[vi + c].round().clamp(0.0, 255.0) as u8;
                }
            }
        });

    RgbaImage::from_raw(w as u32, h as u32, dst_raw).unwrap()
}

// --- Zoom Blur (radial speed-zoom effect) ---

pub fn zoom_blur_core(
    flat: &RgbaImage,
    center_x: f32,        // 0.0–1.0 normalized horizontal position of the zoom origin
    center_y: f32,        // 0.0–1.0 normalized vertical position of the zoom origin
    strength: f32,        // 0.0–1.0: fraction of distance to sample back toward center
    samples: u32,         // quality: 8 (fast) / 16 (normal) / 32 (high)
    tint_color: [f32; 4], // RGBA 0–1 tint applied near the zoom origin (if tint_strength > 0)
    tint_strength: f32,   // 0.0 = no tint, 1.0 = full tint at dead-center
    mask: Option<&GrayImage>,
) -> RgbaImage {
    if strength < 0.001 {
        return flat.clone();
    }
    let w = flat.width() as usize;
    let h = flat.height() as usize;
    if w == 0 || h == 0 {
        return flat.clone();
    }

    let cx = center_x * w as f32;
    let cy = center_y * h as f32;
    let s = strength.clamp(0.0, 0.99);
    let n = samples.max(2) as usize;
    let inv_n = 1.0 / n as f32;

    // Max distance from center to any corner — used to normalise tint falloff.
    let max_dist = [
        (cx, cy),
        (w as f32 - cx, cy),
        (cx, h as f32 - cy),
        (w as f32 - cx, h as f32 - cy),
    ]
    .iter()
    .map(|(dx, dy)| (dx * dx + dy * dy).sqrt())
    .fold(0.0f32, f32::max)
    .max(1.0);

    let src_raw = flat.as_raw();
    let stride = w * 4;
    let mask_raw = mask.map(|m| m.as_raw().as_slice());
    let mask_w = mask.map_or(0, |m| m.width() as usize);
    let mask_h = mask.map_or(0, |m| m.height() as usize);

    let mut dst_raw = vec![0u8; w * h * 4];
    dst_raw
        .par_chunks_mut(stride)
        .enumerate()
        .for_each(|(y, row_out)| {
            for x in 0..w {
                let pi = x * 4;
                if let Some(mr) = mask_raw
                    && x < mask_w
                    && y < mask_h
                    && mr[y * mask_w + x] == 0
                {
                    let src_off = y * stride + pi;
                    row_out[pi..pi + 4].copy_from_slice(&src_raw[src_off..src_off + 4]);
                    continue;
                }
                let px = x as f32;
                let py = y as f32;
                let dx = px - cx;
                let dy = py - cy;

                // Sample from the pixel position back toward the zoom center.
                // i=0 → pixel position (t=1.0), i=n-1 → closest to center (t=1-s).
                let mut r_sum = 0.0f32;
                let mut g_sum = 0.0f32;
                let mut b_sum = 0.0f32;
                let mut a_sum = 0.0f32;
                for i in 0..n {
                    let t = 1.0 - s * (i as f32 / (n - 1) as f32);
                    let sx = (cx + dx * t).round() as i32;
                    let sy = (cy + dy * t).round() as i32;
                    let sx = sx.clamp(0, w as i32 - 1) as usize;
                    let sy = sy.clamp(0, h as i32 - 1) as usize;
                    let si = sy * stride + sx * 4;
                    r_sum += src_raw[si] as f32;
                    g_sum += src_raw[si + 1] as f32;
                    b_sum += src_raw[si + 2] as f32;
                    a_sum += src_raw[si + 3] as f32;
                }
                let mut r = r_sum * inv_n;
                let mut g = g_sum * inv_n;
                let mut b = b_sum * inv_n;
                let mut a = a_sum * inv_n;

                // Optional radial tint — strongest at the zoom origin, fading to zero at corners.
                if tint_strength > 0.001 {
                    let dist = (dx * dx + dy * dy).sqrt();
                    let t = (1.0 - dist / max_dist).max(0.0) * tint_strength;
                    r = r + (tint_color[0] * 255.0 - r) * t;
                    g = g + (tint_color[1] * 255.0 - g) * t;
                    b = b + (tint_color[2] * 255.0 - b) * t;
                    a = a + (tint_color[3] * 255.0 - a) * t;
                }

                row_out[pi] = r.round().clamp(0.0, 255.0) as u8;
                row_out[pi + 1] = g.round().clamp(0.0, 255.0) as u8;
                row_out[pi + 2] = b.round().clamp(0.0, 255.0) as u8;
                row_out[pi + 3] = a.round().clamp(0.0, 255.0) as u8;
            }
        });

    RgbaImage::from_raw(w as u32, h as u32, dst_raw).unwrap()
}

// ============================================================================
// DISTORTION EFFECTS
// ============================================================================

// --- Crystallize (Voronoi polygon effect) ---

