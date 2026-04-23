impl ToolsPanel {
    pub fn update_gradient_if_dirty(
        &mut self,
        canvas_state: &mut CanvasState,
        gpu_renderer: Option<&mut crate::gpu::GpuRenderer>,
    ) {
        if self.gradient_state.preview_dirty
            && self.gradient_state.drag_start.is_some()
            && !self.gradient_state.dragging
        {
            self.render_gradient_to_preview(canvas_state, gpu_renderer);
        }
        self.gradient_state.preview_dirty = false;
    }

    /// Re-render text preview if context bar changed properties (color, alignment, etc.)
    /// Called outside the allow_input gate so it works when interacting with UI.
    pub fn update_text_if_dirty(
        &mut self,
        canvas_state: &mut CanvasState,
        primary_color_f32: [f32; 4],
    ) {
        if !self.text_state.is_editing || self.text_state.text.is_empty() {
            return;
        }
        // Detect color change
        let current_color = [
            (primary_color_f32[0] * 255.0) as u8,
            (primary_color_f32[1] * 255.0) as u8,
            (primary_color_f32[2] * 255.0) as u8,
            (primary_color_f32[3] * 255.0) as u8,
        ];
        if self.text_state.last_color != current_color {
            self.text_state.last_color = current_color;
            self.text_state.preview_dirty = true;
        }
        if self.text_state.preview_dirty {
            self.render_text_preview(canvas_state, primary_color_f32);
        }
    }

    /// Map a char offset within a logical line to (visual_line_index, char_in_visual_line)
    /// when the text has been word-wrapped.
    /// `logical_line` is the original text (one logical line, no `\n`).
    /// `char_pos` is the char position within that logical line.
    /// `visual_lines` are the output of `word_wrap_line`.
    fn map_char_to_visual_line(
        logical_line: &str,
        char_pos: usize,
        visual_lines: &[String],
    ) -> (usize, usize) {
        if visual_lines.is_empty() {
            return (0, 0);
        }
        if visual_lines.len() == 1 {
            return (0, char_pos.min(visual_lines[0].chars().count()));
        }
        let logical_chars: Vec<char> = logical_line.chars().collect();
        let mut consumed = 0usize;
        for (vi, vline) in visual_lines.iter().enumerate() {
            let vlen = vline.chars().count();
            let line_end = consumed + vlen;
            if char_pos >= consumed && char_pos < line_end {
                return (vi, char_pos - consumed);
            }
            // Count whitespace gap consumed between this visual line and next
            let mut next_start = line_end;
            if vi < visual_lines.len() - 1 {
                while next_start < logical_chars.len() && logical_chars[next_start].is_whitespace()
                {
                    next_start += 1;
                }
            }
            // Cursor in trimmed whitespace ÔåÆ end of this visual line
            if char_pos >= line_end && char_pos < next_start {
                return (vi, vlen);
            }
            consumed = next_start.max(line_end);
        }
        let last = visual_lines.len() - 1;
        (last, visual_lines[last].chars().count())
    }

    /// Inverse of `map_char_to_visual_line`: given a visual line index and char offset,
    /// return the char position within the original logical line.
    fn map_visual_line_to_char(
        logical_line: &str,
        visual_lines: &[String],
        visual_line: usize,
        char_in_line: usize,
    ) -> usize {
        if visual_lines.is_empty() {
            return 0;
        }
        let logical_chars: Vec<char> = logical_line.chars().collect();
        let mut consumed = 0usize;
        for (vi, vline) in visual_lines.iter().enumerate() {
            let vlen = vline.chars().count();
            if vi == visual_line {
                return consumed + char_in_line.min(vlen);
            }
            let line_end = consumed + vlen;
            let mut next_start = line_end;
            while next_start < logical_chars.len() && logical_chars[next_start].is_whitespace() {
                next_start += 1;
            }
            consumed = next_start.max(line_end);
        }
        logical_chars.len()
    }

    /// Given the full text (with `\n`), a byte position, and wrap parameters,
    /// return `(visual_line_index, char_in_visual_line)` accounting for word wrapping.
    fn byte_pos_to_visual(
        text: &str,
        byte_pos: usize,
        font: &ab_glyph::FontArc,
        font_size: f32,
        max_width: f32,
        letter_spacing: f32,
        width_scale: f32,
        height_scale: f32,
    ) -> (usize, usize) {
        let logical_lines: Vec<&str> = text.split('\n').collect();
        let mut logical_byte_start = 0usize;
        let mut visual_line_offset = 0usize;
        for logical_line in &logical_lines {
            let logical_byte_end = logical_byte_start + logical_line.len();
            if byte_pos <= logical_byte_end {
                let char_in_logical = text[logical_byte_start..byte_pos].chars().count();
                let visual = crate::ops::text::word_wrap_line(
                    logical_line,
                    font,
                    font_size,
                    max_width,
                    letter_spacing,
                    width_scale,
                    height_scale,
                );
                let (vl, vc) =
                    Self::map_char_to_visual_line(logical_line, char_in_logical, &visual);
                return (visual_line_offset + vl, vc);
            }
            let visual = crate::ops::text::word_wrap_line(
                logical_line,
                font,
                font_size,
                max_width,
                letter_spacing,
                width_scale,
                height_scale,
            );
            visual_line_offset += visual.len();
            logical_byte_start = logical_byte_end + 1;
        }
        (visual_line_offset.saturating_sub(1), 0)
    }

    /// Given a visual line index and char offset, return the byte position in the original text.
    /// The inverse of `byte_pos_to_visual`.
    fn visual_to_byte_pos(
        text: &str,
        visual_line: usize,
        char_in_line: usize,
        font: &ab_glyph::FontArc,
        font_size: f32,
        max_width: f32,
        letter_spacing: f32,
        width_scale: f32,
        height_scale: f32,
    ) -> usize {
        let logical_lines: Vec<&str> = text.split('\n').collect();
        let mut logical_byte_start = 0usize;
        let mut visual_line_offset = 0usize;
        for logical_line in &logical_lines {
            let visual = crate::ops::text::word_wrap_line(
                logical_line,
                font,
                font_size,
                max_width,
                letter_spacing,
                width_scale,
                height_scale,
            );
            let visual_count = visual.len();
            if visual_line < visual_line_offset + visual_count {
                let local_visual = visual_line - visual_line_offset;
                let char_in_logical = Self::map_visual_line_to_char(
                    logical_line,
                    &visual,
                    local_visual,
                    char_in_line,
                );
                // Convert char offset to byte offset within logical line
                let byte_in_logical: usize = logical_line
                    .chars()
                    .take(char_in_logical)
                    .map(|c| c.len_utf8())
                    .sum();
                return logical_byte_start + byte_in_logical;
            }
            visual_line_offset += visual_count;
            logical_byte_start += logical_line.len() + 1;
        }
        text.len()
    }

    /// Compute visual lines for the full text (with `\n`) using word wrapping.
    /// Returns `Vec<(String, usize)>` where each entry is `(visual_line_text, byte_start_in_original)`.
    fn compute_visual_lines_with_byte_offsets(
        text: &str,
        font: &ab_glyph::FontArc,
        font_size: f32,
        max_width: f32,
        letter_spacing: f32,
        width_scale: f32,
        height_scale: f32,
    ) -> Vec<(String, usize)> {
        let mut result = Vec::new();
        let logical_lines: Vec<&str> = text.split('\n').collect();
        let mut byte_start = 0usize;
        for logical_line in &logical_lines {
            let visual = crate::ops::text::word_wrap_line(
                logical_line,
                font,
                font_size,
                max_width,
                letter_spacing,
                width_scale,
                height_scale,
            );
            let logical_chars: Vec<char> = logical_line.chars().collect();
            let mut char_consumed = 0usize;
            for (vi, vline) in visual.iter().enumerate() {
                // byte_start + char_consumed chars converted to bytes
                let byte_off: usize = logical_chars
                    .iter()
                    .take(char_consumed)
                    .map(|c| c.len_utf8())
                    .sum();
                result.push((vline.clone(), byte_start + byte_off));
                let vlen = vline.chars().count();
                let line_end = char_consumed + vlen;
                // Skip whitespace gap
                let mut next = line_end;
                if vi < visual.len() - 1 {
                    while next < logical_chars.len() && logical_chars[next].is_whitespace() {
                        next += 1;
                    }
                }
                char_consumed = next.max(line_end);
            }
            byte_start += logical_line.len() + 1;
        }
        result
    }

    /// Draw the text tool overlay (border, move handle, blinking cursor, selection
    /// highlight, glyph overlay) when text editing is active.
    /// Also draws faint dotted outlines for non-active text blocks on text layers.
    /// Called outside the `allow_input` gate so it persists when the pointer leaves
    /// the canvas (e.g. while interacting with UI panels).
    pub fn draw_text_overlay(
        &mut self,
        ui: &egui::Ui,
        canvas_state: &CanvasState,
        painter: &egui::Painter,
        canvas_rect: Rect,
        zoom: f32,
    ) {
        let accent = ui.visuals().selection.bg_fill;

        // --- Phase 1: Draw dotted outlines for non-active blocks on text layers ---
        // Show whenever the Text tool is active on a text layer (not just when editing)
        if let Some(layer) = canvas_state.layers.get(canvas_state.active_layer_index)
            && let crate::canvas::LayerContent::Text(ref td) = layer.content
        {
            let dotted_color =
                Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), 70);
            for block in &td.blocks {
                if Some(block.id) == self.text_state.active_block_id {
                    continue; // Active block gets full overlay below
                }
                // Skip truly empty blocks that have no explicit box size
                // (freshly created via click, not yet typed into or resized)
                let has_text = block.runs.iter().any(|r| !r.text.is_empty());
                if !has_text && block.max_width.is_none() {
                    continue;
                }
                let layout = crate::ops::text_layer::compute_block_layout(block);
                let bx = block.position[0];
                let by = block.position[1];
                let bw = if let Some(mw) = block.max_width {
                    mw
                } else {
                    layout.total_width
                };
                let bh = if let Some(mh) = block.max_height {
                    mh.max(layout.total_height)
                } else {
                    layout.total_height
                };
                if bw < 1.0 && bh < 1.0 {
                    continue;
                }
                let pad = 4.0;
                let sx = canvas_rect.min.x + (bx - pad) * zoom;
                let sy = canvas_rect.min.y + (by - pad) * zoom;
                let sw = (bw + pad * 2.0) * zoom;
                let sh = (bh + pad * 2.0) * zoom;
                let r = egui::Rect::from_min_size(Pos2::new(sx, sy), egui::vec2(sw, sh));
                if block.rotation.abs() > 0.001 {
                    let center = r.center();
                    let corners = [
                        rotate_screen_point(r.left_top(), center, block.rotation),
                        rotate_screen_point(r.right_top(), center, block.rotation),
                        rotate_screen_point(r.right_bottom(), center, block.rotation),
                        rotate_screen_point(r.left_bottom(), center, block.rotation),
                    ];
                    draw_dotted_quad(painter, corners, dotted_color, 1.0, 4.0, 3.0);
                } else {
                    // Draw dotted outline
                    draw_dotted_rect(painter, r, dotted_color, 1.0, 4.0, 3.0);
                }
            }
        }

        // --- Phase 2: Active block overlay (full handles, cursor, etc.) ---
        if !self.text_state.is_editing {
            return;
        }
        let origin = match self.text_state.origin {
            Some(o) => o,
            None => return,
        };

        let time = ui.input(|i| i.time);
        self.text_state.cursor_blink_timer = time;

        let font_size = self.text_state.font_size;
        self.ensure_text_font_loaded();
        let ls = self.text_state.letter_spacing;
        let line_height = if let Some(ref font) = self.text_state.loaded_font {
            use ab_glyph::{Font as _, ScaleFont as _};
            font.as_scaled(ab_glyph::PxScale {
                x: font_size * self.text_state.width_scale,
                y: font_size * self.text_state.height_scale,
            })
            .height()
                * self.text_state.line_spacing
        } else {
            font_size * 1.2 * self.text_state.line_spacing
        };

        let lines: Vec<&str> = self.text_state.text.split('\n').collect();
        let num_lines = lines.len().max(1);

        // Compute natural text width (from font metrics)
        let natural_width = if let Some(ref font) = self.text_state.loaded_font {
            use ab_glyph::{Font as _, ScaleFont as _};
            let scaled = font.as_scaled(ab_glyph::PxScale {
                x: font_size * self.text_state.width_scale,
                y: font_size * self.text_state.height_scale,
            });
            // If max_width is set, word-wrap to compute visual lines
            if let Some(mw) = self.text_state.active_block_max_width {
                let visual_lines: Vec<String> = lines
                    .iter()
                    .flat_map(|line| {
                        crate::ops::text::word_wrap_line(
                            line,
                            font,
                            font_size,
                            mw,
                            ls,
                            self.text_state.width_scale,
                            self.text_state.height_scale,
                        )
                    })
                    .collect();
                let mut max_w = font_size * 2.0;
                for line in &visual_lines {
                    let mut w = 0.0f32;
                    let mut prev = None;
                    for ch in line.chars() {
                        let gid = font.glyph_id(ch);
                        if let Some(prev_id) = prev {
                            w += scaled.kern(prev_id, gid);
                            w += ls;
                        }
                        w += scaled.h_advance(gid);
                        prev = Some(gid);
                    }
                    max_w = max_w.max(w);
                }
                max_w
            } else {
                let mut max_w = font_size * 2.0;
                for line in &lines {
                    let mut w = 0.0f32;
                    let mut prev = None;
                    for ch in line.chars() {
                        let gid = font.glyph_id(ch);
                        if let Some(prev_id) = prev {
                            w += scaled.kern(prev_id, gid);
                            w += ls;
                        }
                        w += scaled.h_advance(gid);
                        prev = Some(gid);
                    }
                    max_w = max_w.max(w);
                }
                max_w
            }
        } else {
            font_size * 2.0
        };

        // Display width: use max_width if set, otherwise natural width
        let display_width = if let Some(mw) = self.text_state.active_block_max_width {
            mw.max(font_size * 2.0)
        } else {
            natural_width
        };

        // Compute visual line count (considering word wrap)
        let visual_num_lines = if let (Some(mw), Some(font)) = (
            self.text_state.active_block_max_width,
            &self.text_state.loaded_font,
        ) {
            let visual_lines: Vec<String> = lines
                .iter()
                .flat_map(|line| {
                    crate::ops::text::word_wrap_line(
                        line,
                        font,
                        font_size,
                        mw,
                        ls,
                        self.text_state.width_scale,
                        self.text_state.height_scale,
                    )
                })
                .collect();
            visual_lines.len().max(1)
        } else {
            num_lines
        };

        let text_h = visual_num_lines as f32 * line_height;
        // Cache the active block height for input handling
        self.text_state.active_block_height = text_h;
        // Visual box height: use max_height if set (but never smaller than content)
        let visual_h = if let Some(mh) = self.text_state.active_block_max_height {
            mh.max(text_h)
        } else {
            text_h
        };
        let pad = 4.0;

        let border_x = {
            use crate::ops::text::TextAlignment;
            match self.text_state.alignment {
                TextAlignment::Left => origin[0] - pad,
                TextAlignment::Center => origin[0] - display_width * 0.5 - pad,
                TextAlignment::Right => origin[0] - display_width - pad,
            }
        };
        let border_y = origin[1] - pad;
        let border_w = display_width + pad * 2.0;
        let border_h = visual_h + pad * 2.0;

        let sx = canvas_rect.min.x + border_x * zoom;
        let sy = canvas_rect.min.y + border_y * zoom;
        let sw = border_w * zoom;
        let sh = border_h * zoom;

        let border_rect = egui::Rect::from_min_size(Pos2::new(sx, sy), egui::vec2(sw, sh));

        // Get block rotation for transforming the overlay
        let block_rotation = if let Some(layer) =
            canvas_state.layers.get(canvas_state.active_layer_index)
            && let crate::canvas::LayerContent::Text(ref td) = layer.content
            && let Some(bid) = self.text_state.active_block_id
            && let Some(block) = td.blocks.iter().find(|b| b.id == bid)
        {
            block.rotation
        } else {
            0.0
        };
        let has_rot = block_rotation.abs() > 0.001;
        let rot_pivot = border_rect.center(); // screen-space rotation center

        let accent_semi = Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), 180);
        let accent_fill = Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), 200);
        let accent_faint = Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), 120);

        // Border outline (rotated if needed)
        if has_rot {
            let c = [
                rotate_screen_point(border_rect.left_top(), rot_pivot, block_rotation),
                rotate_screen_point(border_rect.right_top(), rot_pivot, block_rotation),
                rotate_screen_point(border_rect.right_bottom(), rot_pivot, block_rotation),
                rotate_screen_point(border_rect.left_bottom(), rot_pivot, block_rotation),
            ];
            let stroke = egui::Stroke::new(1.0, accent_semi);
            painter.line_segment([c[0], c[1]], stroke);
            painter.line_segment([c[1], c[2]], stroke);
            painter.line_segment([c[2], c[3]], stroke);
            painter.line_segment([c[3], c[0]], stroke);
        } else {
            painter.rect_stroke(
                border_rect,
                0.0,
                egui::Stroke::new(1.0, accent_semi),
                egui::StrokeKind::Middle,
            );
        }

        // --- Resize handles, rotation handle, delete button (text layer only) ---
        if self.text_state.editing_text_layer {
            // --- Resize handles (4 corners) ---
            let handle_size = 5.0; // screen pixels
            let handle_stroke = egui::Stroke::new(1.0, Color32::WHITE);
            let corners_raw = [
                border_rect.left_top(),
                border_rect.right_top(),
                border_rect.left_bottom(),
                border_rect.right_bottom(),
            ];
            for &corner in &corners_raw {
                let c = if has_rot {
                    rotate_screen_point(corner, rot_pivot, block_rotation)
                } else {
                    corner
                };
                let hr = egui::Rect::from_center_size(
                    c,
                    egui::vec2(handle_size * 2.0, handle_size * 2.0),
                );
                painter.rect_filled(hr, 1.0, accent_fill);
                painter.rect_stroke(hr, 1.0, handle_stroke, egui::StrokeKind::Middle);
            }

            // --- Rotation handle (circle above top-center with connector line) ---
            let rot_handle_offset = 20.0; // screen pixels above the box
            let rot_handle_base = border_rect.center_top();
            let rot_center_raw =
                Pos2::new(rot_handle_base.x, rot_handle_base.y - rot_handle_offset);
            let rot_center = if has_rot {
                rotate_screen_point(rot_center_raw, rot_pivot, block_rotation)
            } else {
                rot_center_raw
            };
            let rot_base = if has_rot {
                rotate_screen_point(rot_handle_base, rot_pivot, block_rotation)
            } else {
                rot_handle_base
            };
            painter.line_segment([rot_base, rot_center], egui::Stroke::new(1.0, accent_faint));
            painter.circle_filled(rot_center, 5.0, accent_fill);
            painter.circle_stroke(rot_center, 5.0, egui::Stroke::new(1.0, Color32::WHITE));
            // Small rotation icon (curved arrow hint)
            let arc_r = 3.0;
            painter.line_segment(
                [
                    Pos2::new(rot_center.x - arc_r, rot_center.y - 1.0),
                    Pos2::new(rot_center.x + arc_r, rot_center.y - 1.0),
                ],
                egui::Stroke::new(1.0, Color32::WHITE),
            );

            // --- Delete button (├ù at top-right corner, outside the box) ---
            let del_offset = 14.0; // screen pixels outside top-right
            let del_center_raw = Pos2::new(
                border_rect.max.x + del_offset,
                border_rect.min.y - del_offset,
            );
            let del_center = if has_rot {
                rotate_screen_point(del_center_raw, rot_pivot, block_rotation)
            } else {
                del_center_raw
            };
            let del_radius = 8.0;
            let del_bg = Color32::from_rgba_unmultiplied(200, 60, 60, 220);
            painter.circle_filled(del_center, del_radius, del_bg);
            painter.circle_stroke(
                del_center,
                del_radius,
                egui::Stroke::new(1.0, Color32::WHITE),
            );
            let xr = 3.5;
            let x_stroke = egui::Stroke::new(1.5, Color32::WHITE);
            painter.line_segment(
                [
                    Pos2::new(del_center.x - xr, del_center.y - xr),
                    Pos2::new(del_center.x + xr, del_center.y + xr),
                ],
                x_stroke,
            );
            painter.line_segment(
                [
                    Pos2::new(del_center.x + xr, del_center.y - xr),
                    Pos2::new(del_center.x - xr, del_center.y + xr),
                ],
                x_stroke,
            );
        } // end editing_text_layer handles

        // Move handle (cross circle ÔÇö existing)
        let handle_radius_screen = 6.0;
        let handle_offset_canvas = 10.0 / zoom;
        let (handle_screen_x, handle_screen_y) = {
            use crate::ops::text::TextAlignment;
            match self.text_state.alignment {
                TextAlignment::Left => (
                    canvas_rect.min.x + (origin[0] - handle_offset_canvas) * zoom,
                    canvas_rect.min.y + (origin[1] + font_size * 0.5) * zoom,
                ),
                TextAlignment::Center => (
                    canvas_rect.min.x + origin[0] * zoom,
                    canvas_rect.min.y + (origin[1] - handle_offset_canvas) * zoom,
                ),
                TextAlignment::Right => (
                    canvas_rect.min.x + (origin[0] + handle_offset_canvas) * zoom,
                    canvas_rect.min.y + (origin[1] + font_size * 0.5) * zoom,
                ),
            }
        };
        let handle_pos_raw = Pos2::new(handle_screen_x, handle_screen_y);
        let handle_pos = if has_rot {
            rotate_screen_point(handle_pos_raw, rot_pivot, block_rotation)
        } else {
            handle_pos_raw
        };

        painter.circle_filled(handle_pos, handle_radius_screen, accent_fill);
        painter.circle_stroke(
            handle_pos,
            handle_radius_screen,
            egui::Stroke::new(1.5, Color32::WHITE),
        );
        let hs = 3.0;
        let cross_stroke = egui::Stroke::new(1.0, Color32::WHITE);
        painter.line_segment(
            [
                Pos2::new(handle_pos.x - hs, handle_pos.y),
                Pos2::new(handle_pos.x + hs, handle_pos.y),
            ],
            cross_stroke,
        );
        painter.line_segment(
            [
                Pos2::new(handle_pos.x, handle_pos.y - hs),
                Pos2::new(handle_pos.x, handle_pos.y + hs),
            ],
            cross_stroke,
        );

        // Connector line from handle to border edge
        let connector_target_raw = {
            use crate::ops::text::TextAlignment;
            match self.text_state.alignment {
                TextAlignment::Left => Pos2::new(
                    border_rect.min.x,
                    canvas_rect.min.y + (origin[1] + font_size * 0.5) * zoom,
                ),
                TextAlignment::Center => {
                    Pos2::new(canvas_rect.min.x + origin[0] * zoom, border_rect.min.y)
                }
                TextAlignment::Right => Pos2::new(
                    border_rect.max.x,
                    canvas_rect.min.y + (origin[1] + font_size * 0.5) * zoom,
                ),
            }
        };
        let connector_target = if has_rot {
            rotate_screen_point(connector_target_raw, rot_pivot, block_rotation)
        } else {
            connector_target_raw
        };
        painter.line_segment(
            [handle_pos, connector_target],
            egui::Stroke::new(1.0, accent_faint),
        );

        // Selection highlight (text layer only)
        if self.text_state.editing_text_layer && self.text_state.selection.has_selection() {
            let sel_anchor_flat = {
                let a = self.text_state.selection.anchor;
                let mut off = 0usize;
                if let Some(layer) = canvas_state.layers.get(canvas_state.active_layer_index)
                    && let crate::canvas::LayerContent::Text(ref td) = layer.content
                    && let Some(bid) = self.text_state.active_block_id
                    && let Some(block) = td.blocks.iter().find(|b| b.id == bid)
                {
                    off = block.run_pos_to_flat_offset(a);
                }
                off
            };
            let sel_cursor_flat = self.text_state.cursor_pos;
            let (sel_start, sel_end) = if sel_anchor_flat <= sel_cursor_flat {
                (sel_anchor_flat, sel_cursor_flat)
            } else {
                (sel_cursor_flat, sel_anchor_flat)
            };

            let full_text = &self.text_state.text;
            let cached_lh_sel = if self.text_state.cached_line_height > 0.0 {
                self.text_state.cached_line_height
            } else {
                line_height
            };
            let sel_color = Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), 80);

            // Compute visual lines with byte offsets for wrap-aware selection
            let visual_with_offsets: Vec<(String, usize)> = if let (Some(mw), Some(font)) = (
                self.text_state.active_block_max_width,
                &self.text_state.loaded_font,
            ) {
                Self::compute_visual_lines_with_byte_offsets(
                    full_text,
                    font,
                    font_size,
                    mw,
                    ls,
                    self.text_state.width_scale,
                    self.text_state.height_scale,
                )
            } else {
                // No wrapping ÔÇö one visual line per logical line
                let mut result = Vec::new();
                let mut byte_start = 0usize;
                for line_text in full_text.split('\n') {
                    result.push((line_text.to_string(), byte_start));
                    byte_start += line_text.len() + 1;
                }
                result
            };

            for (line_idx, (line_text, line_byte_start)) in visual_with_offsets.iter().enumerate() {
                let line_byte_end = line_byte_start + line_text.len();

                let hl_start = sel_start.max(*line_byte_start);
                let hl_end = sel_end.min(line_byte_end);

                if hl_start < hl_end {
                    let compute_x = |byte_in_line: usize| -> f32 {
                        let chars_before = line_text[..byte_in_line].chars().count();
                        if !self.text_state.cached_line_advances.is_empty() {
                            self.text_state
                                .cached_line_advances
                                .get(line_idx)
                                .and_then(|a| a.get(chars_before).copied())
                                .unwrap_or(0.0)
                        } else if let Some(ref font) = self.text_state.loaded_font {
                            use ab_glyph::{Font as _, ScaleFont as _};
                            let scaled = font.as_scaled(ab_glyph::PxScale {
                                x: font_size * self.text_state.width_scale,
                                y: font_size * self.text_state.height_scale,
                            });
                            let mut x = 0.0f32;
                            let mut prev = None;
                            for ch in line_text[..byte_in_line].chars() {
                                let gid = font.glyph_id(ch);
                                if let Some(p) = prev {
                                    x += scaled.kern(p, gid);
                                }
                                x += scaled.h_advance(gid);
                                prev = Some(gid);
                            }
                            x
                        } else {
                            0.0
                        }
                    };

                    let x1 = compute_x(hl_start - line_byte_start);
                    let x2 = compute_x(hl_end - line_byte_start);

                    let line_align = {
                        use crate::ops::text::TextAlignment;
                        if self.text_state.alignment == TextAlignment::Left {
                            0.0
                        } else {
                            let line_w = if let Some(ref font) = self.text_state.loaded_font {
                                use ab_glyph::{Font as _, ScaleFont as _};
                                let scaled = font.as_scaled(ab_glyph::PxScale {
                                    x: font_size * self.text_state.width_scale,
                                    y: font_size * self.text_state.height_scale,
                                });
                                let mut w = 0.0f32;
                                let mut prev = None;
                                for ch in line_text.chars() {
                                    let gid = font.glyph_id(ch);
                                    if let Some(p) = prev {
                                        w += scaled.kern(p, gid);
                                    }
                                    w += scaled.h_advance(gid);
                                    prev = Some(gid);
                                }
                                w
                            } else {
                                0.0
                            };
                            match self.text_state.alignment {
                                TextAlignment::Center => -line_w * 0.5,
                                TextAlignment::Right => -line_w,
                                _ => 0.0,
                            }
                        }
                    };

                    let y_off = line_idx as f32 * cached_lh_sel;
                    let r = egui::Rect::from_min_max(
                        Pos2::new(
                            canvas_rect.min.x + (origin[0] + x1 + line_align) * zoom,
                            canvas_rect.min.y + (origin[1] + y_off) * zoom,
                        ),
                        Pos2::new(
                            canvas_rect.min.x + (origin[0] + x2 + line_align) * zoom,
                            canvas_rect.min.y + (origin[1] + y_off + cached_lh_sel) * zoom,
                        ),
                    );
                    if has_rot {
                        // Draw rotated selection quad
                        let corners = [
                            rotate_screen_point(r.left_top(), rot_pivot, block_rotation),
                            rotate_screen_point(r.right_top(), rot_pivot, block_rotation),
                            rotate_screen_point(r.right_bottom(), rot_pivot, block_rotation),
                            rotate_screen_point(r.left_bottom(), rot_pivot, block_rotation),
                        ];
                        let mesh = egui::Mesh::with_texture(egui::TextureId::Managed(0));
                        let mut mesh = mesh;
                        let uv = Pos2::ZERO;
                        for &c in &corners {
                            mesh.vertices.push(egui::epaint::Vertex {
                                pos: c,
                                uv,
                                color: sel_color,
                            });
                        }
                        mesh.indices.extend_from_slice(&[0, 1, 2, 0, 2, 3]);
                        painter.add(egui::Shape::mesh(mesh));
                    } else {
                        painter.rect_filled(r, 0.0, sel_color);
                    }
                }
            }
        }

        // Blinking cursor
        let blink_on = ((time * 2.0) as i32) % 2 == 0;
        if blink_on {
            let (cursor_x_offset, cursor_line) =
                if !self.text_state.text.is_empty() && self.text_state.cursor_pos > 0 {
                    // Use visual (word-wrapped) line mapping when max_width is set
                    let (vis_line, vis_char) = if let (Some(mw), Some(font)) = (
                        self.text_state.active_block_max_width,
                        &self.text_state.loaded_font,
                    ) {
                        Self::byte_pos_to_visual(
                            &self.text_state.text,
                            self.text_state.cursor_pos,
                            font,
                            font_size,
                            mw,
                            ls,
                            self.text_state.width_scale,
                            self.text_state.height_scale,
                        )
                    } else {
                        // No wrapping ÔÇö use logical line mapping
                        let text_before = &self.text_state.text[..self.text_state.cursor_pos];
                        let newlines_before = text_before.matches('\n').count();
                        let last_line = text_before.rsplit('\n').next().unwrap_or(text_before);
                        (newlines_before, last_line.chars().count())
                    };

                    let x_off = if !self.text_state.cached_line_advances.is_empty() {
                        self.text_state
                            .cached_line_advances
                            .get(vis_line)
                            .and_then(|advances| advances.get(vis_char).copied())
                            .unwrap_or(0.0)
                    } else if let Some(ref font) = self.text_state.loaded_font {
                        // Fallback: compute from font metrics for the visual line text
                        use ab_glyph::{Font as _, ScaleFont as _};
                        let scaled = font.as_scaled(ab_glyph::PxScale {
                            x: font_size * self.text_state.width_scale,
                            y: font_size * self.text_state.height_scale,
                        });
                        // Get the visual line text to measure
                        let text_before = &self.text_state.text[..self.text_state.cursor_pos];
                        let last_line = text_before.rsplit('\n').next().unwrap_or(text_before);
                        let mut x = 0.0f32;
                        let mut prev_glyph_id = None;
                        for ch in last_line.chars() {
                            let glyph_id = font.glyph_id(ch);
                            if let Some(prev) = prev_glyph_id {
                                x += scaled.kern(prev, glyph_id);
                            }
                            x += scaled.h_advance(glyph_id);
                            prev_glyph_id = Some(glyph_id);
                        }
                        x
                    } else {
                        0.0
                    };
                    (x_off, vis_line)
                } else {
                    (0.0, 0)
                };

            let cached_lh = if self.text_state.cached_line_height > 0.0 {
                self.text_state.cached_line_height
            } else {
                line_height
            };
            let cursor_y_offset = cursor_line as f32 * cached_lh;

            let align_offset = {
                use crate::ops::text::TextAlignment;
                if self.text_state.alignment == TextAlignment::Left {
                    0.0
                } else {
                    // Get current visual line text for alignment measurement
                    let current_line_text: String = if let (Some(mw), Some(font)) = (
                        self.text_state.active_block_max_width,
                        &self.text_state.loaded_font,
                    ) {
                        let all_visual: Vec<String> = self
                            .text_state
                            .text
                            .split('\n')
                            .flat_map(|line| {
                                crate::ops::text::word_wrap_line(
                                    line,
                                    font,
                                    font_size,
                                    mw,
                                    ls,
                                    self.text_state.width_scale,
                                    self.text_state.height_scale,
                                )
                            })
                            .collect();
                        all_visual.get(cursor_line).cloned().unwrap_or_default()
                    } else {
                        lines.get(cursor_line).unwrap_or(&"").to_string()
                    };
                    let line_w = if let Some(ref font) = self.text_state.loaded_font {
                        use ab_glyph::{Font as _, ScaleFont as _};
                        let scaled = font.as_scaled(ab_glyph::PxScale {
                            x: font_size * self.text_state.width_scale,
                            y: font_size * self.text_state.height_scale,
                        });
                        let mut w = 0.0f32;
                        let mut prev = None;
                        for ch in current_line_text.chars() {
                            let gid = font.glyph_id(ch);
                            if let Some(prev_id) = prev {
                                w += scaled.kern(prev_id, gid);
                            }
                            w += scaled.h_advance(gid);
                            prev = Some(gid);
                        }
                        w
                    } else {
                        0.0
                    };
                    match self.text_state.alignment {
                        TextAlignment::Center => -line_w * 0.5,
                        TextAlignment::Right => -line_w,
                        _ => 0.0,
                    }
                }
            };

            let cx = origin[0] + cursor_x_offset + align_offset;
            let cy = origin[1] + cursor_y_offset;

            let csx = canvas_rect.min.x + cx * zoom;
            let csy = canvas_rect.min.y + cy * zoom;
            let cursor_h = font_size * self.text_state.height_scale * zoom;

            let cursor_top = Pos2::new(csx, csy);
            let cursor_bot = Pos2::new(csx, csy + cursor_h);
            let cursor_top2 = Pos2::new(csx + 1.0, csy);
            let cursor_bot2 = Pos2::new(csx + 1.0, csy + cursor_h);

            let (ct, cb, ct2, cb2) = if has_rot {
                (
                    rotate_screen_point(cursor_top, rot_pivot, block_rotation),
                    rotate_screen_point(cursor_bot, rot_pivot, block_rotation),
                    rotate_screen_point(cursor_top2, rot_pivot, block_rotation),
                    rotate_screen_point(cursor_bot2, rot_pivot, block_rotation),
                )
            } else {
                (cursor_top, cursor_bot, cursor_top2, cursor_bot2)
            };

            painter.line_segment([ct, cb], egui::Stroke::new(1.5, Color32::BLACK));
            painter.line_segment([ct2, cb2], egui::Stroke::new(0.5, Color32::WHITE));
        }
        // Throttle repaint to cursor blink rate (2Hz)
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(500));
    }

    /// Re-render shape preview if context bar / color widget changed properties.
    /// Called outside the allow_input gate so it works when interacting with UI.
    pub fn update_shape_if_dirty(
        &mut self,
        canvas_state: &mut CanvasState,
        primary_color_f32: [f32; 4],
        secondary_color_f32: [f32; 4],
    ) {
        if self.shapes_state.placed.is_none() {
            return;
        }
        let p = self.shapes_state.placed.as_mut().unwrap();
        let current_primary = [
            (primary_color_f32[0] * 255.0) as u8,
            (primary_color_f32[1] * 255.0) as u8,
            (primary_color_f32[2] * 255.0) as u8,
            (primary_color_f32[3] * 255.0) as u8,
        ];
        let current_secondary = [
            (secondary_color_f32[0] * 255.0) as u8,
            (secondary_color_f32[1] * 255.0) as u8,
            (secondary_color_f32[2] * 255.0) as u8,
            (secondary_color_f32[3] * 255.0) as u8,
        ];
        let changed = p.kind != self.shapes_state.selected_shape
            || p.fill_mode != self.shapes_state.fill_mode
            || p.outline_width != self.properties.size
            || p.anti_alias != self.shapes_state.anti_alias
            || p.corner_radius != self.shapes_state.corner_radius
            || p.primary_color != current_primary
            || p.secondary_color != current_secondary;
        if changed {
            p.kind = self.shapes_state.selected_shape;
            p.fill_mode = self.shapes_state.fill_mode;
            p.outline_width = self.properties.size;
            p.anti_alias = self.shapes_state.anti_alias;
            p.corner_radius = self.shapes_state.corner_radius;
            p.primary_color = current_primary;
            p.secondary_color = current_secondary;
            self.render_shape_preview(canvas_state, primary_color_f32, secondary_color_f32);
        }
    }

    /// Commit the gradient preview to the active layer.
    fn commit_gradient(&mut self, canvas_state: &mut CanvasState) {
        if self.gradient_state.drag_start.is_none() {
            return;
        }

        let Some(target_layer_idx) = self.gradient_state.source_layer_index else {
            return;
        };
        if target_layer_idx >= canvas_state.layers.len() {
            self.cancel_gradient(canvas_state);
            return;
        }

        // Capture "before" snapshot BEFORE modifying the layer
        let stroke_event = self.stroke_tracker.finish(canvas_state);

        let is_eraser = self.gradient_state.mode == GradientMode::Transparency;

        // Commit preview to actual layer with proper alpha blending
        if let Some(ref preview) = canvas_state.preview_layer {
            let chunk_data: Vec<(u32, u32, image::RgbaImage)> = preview
                .chunk_keys()
                .filter_map(|(cx, cy)| {
                    preview
                        .get_chunk(cx, cy)
                        .map(|chunk| (cx, cy, chunk.clone()))
                })
                .collect();

            let chunk_size = CHUNK_SIZE;

            if let Some(active_layer) = canvas_state.layers.get_mut(target_layer_idx) {
                for (cx, cy, chunk) in &chunk_data {
                    let base_x = cx * chunk_size;
                    let base_y = cy * chunk_size;
                    let cw = chunk_size.min(canvas_state.width.saturating_sub(base_x));
                    let ch = chunk_size.min(canvas_state.height.saturating_sub(base_y));
                    for ly in 0..ch {
                        for lx in 0..cw {
                            let gx = base_x + lx;
                            let gy = base_y + ly;
                            let src = chunk.get_pixel(lx, ly);
                            if src.0[3] == 0 {
                                continue;
                            }
                            let dst = active_layer.pixels.get_pixel_mut(gx, gy);

                            if is_eraser {
                                // Eraser: reduce layer alpha by mask strength
                                let mask_strength = src.0[3] as f32 / 255.0;
                                let current_a = dst.0[3] as f32 / 255.0;
                                let new_a = (current_a * (1.0 - mask_strength)).max(0.0);
                                dst.0[3] = (new_a * 255.0).round() as u8;
                            } else {
                                // Normal alpha-over blend
                                let sa = src.0[3] as f32 / 255.0;
                                let da = dst.0[3] as f32 / 255.0;
                                let out_a = sa + da * (1.0 - sa);
                                if out_a > 0.0 {
                                    let r = (src.0[0] as f32 * sa
                                        + dst.0[0] as f32 * da * (1.0 - sa))
                                        / out_a;
                                    let g = (src.0[1] as f32 * sa
                                        + dst.0[1] as f32 * da * (1.0 - sa))
                                        / out_a;
                                    let b = (src.0[2] as f32 * sa
                                        + dst.0[2] as f32 * da * (1.0 - sa))
                                        / out_a;
                                    *dst = Rgba([
                                        r.round() as u8,
                                        g.round() as u8,
                                        b.round() as u8,
                                        (out_a * 255.0).round() as u8,
                                    ]);
                                }
                            }
                        }
                    }
                }
                active_layer.invalidate_lod();
                active_layer.gpu_generation += 1;
            }
        }

        // Clear gradient state
        self.gradient_state.drag_start = None;
        self.gradient_state.drag_end = None;
        self.gradient_state.dragging = false;
        self.gradient_state.dragging_handle = None;
        self.gradient_state.source_layer_index = None;

        // Mark dirty and clear preview
        canvas_state.clear_preview_state();
        canvas_state.mark_dirty(None);

        // Store event for history
        if stroke_event.is_some() {
            self.pending_stroke_event = stroke_event;
        }
    }

    /// Cancel gradient operation without committing.
    fn cancel_gradient(&mut self, canvas_state: &mut CanvasState) {
        self.gradient_state.drag_start = None;
        self.gradient_state.drag_end = None;
        self.gradient_state.dragging = false;
        self.gradient_state.dragging_handle = None;
        self.gradient_state.source_layer_index = None;
        self.stroke_tracker.cancel();
        canvas_state.clear_preview_state();
        canvas_state.mark_dirty(None);
    }

    /// Draw gradient handle overlay (start/end points and connecting line).
    fn draw_gradient_overlay(
        &self,
        painter: &egui::Painter,
        canvas_rect: Rect,
        zoom: f32,
        canvas_state: &CanvasState,
    ) {
        let (start, end) = match (self.gradient_state.drag_start, self.gradient_state.drag_end) {
            (Some(s), Some(e)) => (s, e),
            _ => return,
        };

        // Convert canvas coords to screen coords
        let to_screen = |cx: f32, cy: f32| -> Pos2 {
            let image_rect = egui::Rect::from_min_size(
                canvas_rect.min,
                egui::vec2(
                    canvas_state.width as f32 * zoom,
                    canvas_state.height as f32 * zoom,
                ),
            );
            Pos2::new(image_rect.min.x + cx * zoom, image_rect.min.y + cy * zoom)
        };

        let screen_start = to_screen(start.x, start.y);
        let screen_end = to_screen(end.x, end.y);

        // Dashed line connecting start Ôåö end
        let dash_len = 6.0;
        let gap_len = 4.0;
        let line_vec = screen_end - screen_start;
        let line_len = line_vec.length();
        if line_len > 1.0 {
            let dir = line_vec / line_len;
            let mut d = 0.0;
            while d < line_len {
                let seg_start = screen_start + dir * d;
                let seg_end = screen_start + dir * (d + dash_len).min(line_len);
                painter.line_segment([seg_start, seg_end], egui::Stroke::new(1.5, Color32::WHITE));
                painter.line_segment([seg_start, seg_end], egui::Stroke::new(0.5, Color32::BLACK));
                d += dash_len + gap_len;
            }
        }

        // Start handle ÔÇö filled circle with outline
        let handle_r = 5.0;
        painter.circle_filled(screen_start, handle_r + 1.0, Color32::BLACK);
        painter.circle_filled(screen_start, handle_r, Color32::WHITE);
        painter.circle_filled(
            screen_start,
            handle_r - 2.0,
            Color32::from_rgb(100, 180, 255),
        );

        // End handle ÔÇö filled circle with outline
        painter.circle_filled(screen_end, handle_r + 1.0, Color32::BLACK);
        painter.circle_filled(screen_end, handle_r, Color32::WHITE);
        painter.circle_filled(screen_end, handle_r - 2.0, Color32::from_rgb(255, 100, 100));
    }

    /// Context bar: gradient shape, mode, preset, repeat, and gradient strip.
    fn show_gradient_options(
        &mut self,
        ui: &mut egui::Ui,
        assets: &Assets,
        primary_color: Color32,
        secondary_color: Color32,
    ) {
        // Cache primary color for the "Use Primary" button in the gradient bar
        self.gradient_state.cached_primary = Some([
            primary_color.r(),
            primary_color.g(),
            primary_color.b(),
            primary_color.a(),
        ]);

        // Shape dropdown
        ui.label(t!("ctx.shapes.shape"));
        let current_shape = self.gradient_state.shape;
        egui::ComboBox::from_id_salt("gradient_shape")
            .selected_text(current_shape.label())
            .width(120.0)
            .show_ui(ui, |ui| {
                for &shape in GradientShape::all() {
                    if ui
                        .selectable_label(shape == current_shape, shape.label())
                        .clicked()
                    {
                        self.gradient_state.shape = shape;
                        self.gradient_state.preview_dirty = true;
                    }
                }
            });

        ui.separator();

        // Mode toggle
        ui.label(t!("ctx.mode"));
        let mode = self.gradient_state.mode;
        if ui
            .selectable_label(mode == GradientMode::Color, GradientMode::Color.label())
            .clicked()
        {
            self.gradient_state.mode = GradientMode::Color;
            self.gradient_state.lut_dirty = true;
            self.gradient_state.preview_dirty = true;
        }
        if ui
            .selectable_label(
                mode == GradientMode::Transparency,
                GradientMode::Transparency.label(),
            )
            .clicked()
        {
            self.gradient_state.mode = GradientMode::Transparency;
            self.gradient_state.lut_dirty = true;
            self.gradient_state.preview_dirty = true;
        }

        ui.separator();

        // Preset dropdown
        ui.label(t!("ctx.gradient.preset"));
        let current_preset = self.gradient_state.preset;
        let primary_u8 = [
            primary_color.r(),
            primary_color.g(),
            primary_color.b(),
            primary_color.a(),
        ];
        let secondary_u8 = [
            secondary_color.r(),
            secondary_color.g(),
            secondary_color.b(),
            secondary_color.a(),
        ];
        egui::ComboBox::from_id_salt("gradient_preset")
            .selected_text(current_preset.label())
            .width(150.0)
            .show_ui(ui, |ui| {
                for &preset in GradientPreset::all() {
                    if ui
                        .selectable_label(preset == current_preset, preset.label())
                        .clicked()
                    {
                        self.gradient_state
                            .apply_preset(preset, primary_u8, secondary_u8);
                        self.gradient_state.preview_dirty = true;
                    }
                }
            });

        ui.separator();

        // Repeat checkbox
        let mut repeat = self.gradient_state.repeat;
        if ui
            .checkbox(&mut repeat, t!("ctx.gradient.repeat"))
            .changed()
        {
            self.gradient_state.repeat = repeat;
            self.gradient_state.preview_dirty = true;
        }

        ui.separator();

        // Gradient strip preview + stop editor
        self.show_gradient_bar(ui, assets);
    }

    /// Draw the interactive gradient bar with draggable color stops.
    fn show_gradient_bar(&mut self, ui: &mut egui::Ui, assets: &Assets) {
        let bar_width = 200.0f32;
        let bar_height = 12.0f32;
        let stop_radius = 4.0f32;
        let h_pad = stop_radius + 2.0; // horizontal padding so edge handles aren't clipped

        let (response, painter) = ui.allocate_painter(
            egui::vec2(bar_width + h_pad * 2.0, bar_height + stop_radius + 4.0),
            egui::Sense::click_and_drag(),
        );
        let bar_rect = egui::Rect::from_min_size(
            response.rect.min + egui::vec2(h_pad, 1.0),
            egui::vec2(bar_width, bar_height),
        );

        // Draw checkerboard behind bar (for transparency visualization)
        let check_size = 4.0;
        let cols = (bar_width / check_size).ceil() as usize;
        let rows = (bar_height / check_size).ceil() as usize;
        for row in 0..rows {
            for col in 0..cols {
                let color = if (row + col) % 2 == 0 {
                    Color32::from_gray(200)
                } else {
                    Color32::from_gray(255)
                };
                let r = egui::Rect::from_min_size(
                    bar_rect.min + egui::vec2(col as f32 * check_size, row as f32 * check_size),
                    egui::vec2(check_size, check_size),
                )
                .intersect(bar_rect);
                painter.rect_filled(r, 0.0, color);
            }
        }

        // Rebuild LUT if needed for display
        if self.gradient_state.lut_dirty {
            self.gradient_state.rebuild_lut();
        }

        // Draw gradient bar using LUT
        let lut = &self.gradient_state.lut;
        for x in 0..bar_width as usize {
            let t = x as f32 / (bar_width - 1.0);
            let idx = (t * 255.0).round() as usize;
            let off = idx * 4;
            let color =
                Color32::from_rgba_unmultiplied(lut[off], lut[off + 1], lut[off + 2], lut[off + 3]);
            let line_rect = egui::Rect::from_min_size(
                bar_rect.min + egui::vec2(x as f32, 0.0),
                egui::vec2(1.0, bar_height),
            );
            painter.rect_filled(line_rect, 0.0, color);
        }

        // Outline
        painter.rect_stroke(
            bar_rect,
            1.0,
            egui::Stroke::new(1.0, Color32::DARK_GRAY),
            egui::StrokeKind::Middle,
        );

        // Draw stop handles
        let stop_y = bar_rect.max.y + stop_radius + 1.0;
        for (i, stop) in self.gradient_state.stops.iter().enumerate() {
            let stop_x = bar_rect.min.x + stop.position * bar_width;
            let _pos = Pos2::new(stop_x, stop_y);
            let is_selected = self.gradient_state.selected_stop == Some(i);

            // Triangle pointing up at the bar
            let tri_top = Pos2::new(stop_x, bar_rect.max.y + 1.0);
            let tri_left = Pos2::new(stop_x - stop_radius, stop_y);
            let tri_right = Pos2::new(stop_x + stop_radius, stop_y);

            // Outline
            let outline_color = if is_selected {
                Color32::WHITE
            } else {
                Color32::DARK_GRAY
            };
            painter.add(egui::Shape::convex_polygon(
                vec![tri_top, tri_right, tri_left],
                Color32::from_rgba_unmultiplied(
                    stop.color[0],
                    stop.color[1],
                    stop.color[2],
                    stop.color[3],
                ),
                egui::Stroke::new(if is_selected { 2.0 } else { 1.0 }, outline_color),
            ));
        }

        // Handle interactions
        let pointer_pos = response.interact_pointer_pos();
        if let Some(pp) = pointer_pos {
            let t_at_pointer = ((pp.x - bar_rect.min.x) / bar_width).clamp(0.0, 1.0);

            if response.drag_started() {
                // Check if clicking near an existing stop
                let mut hit_stop: Option<usize> = None;
                for (i, stop) in self.gradient_state.stops.iter().enumerate() {
                    let stop_screen_x = bar_rect.min.x + stop.position * bar_width;
                    if (pp.x - stop_screen_x).abs() < stop_radius * 2.0 {
                        hit_stop = Some(i);
                        break;
                    }
                }
                if let Some(idx) = hit_stop {
                    self.gradient_state.selected_stop = Some(idx);
                } else {
                    // Add new stop at click position
                    let new_color = self.gradient_state.sample_lut(t_at_pointer);
                    self.gradient_state
                        .stops
                        .push(GradientStop::new(t_at_pointer, new_color));
                    self.gradient_state
                        .stops
                        .sort_by(|a, b| a.position.partial_cmp(&b.position).unwrap());
                    // Find the newly added stop index
                    for (i, s) in self.gradient_state.stops.iter().enumerate() {
                        if (s.position - t_at_pointer).abs() < 0.001 {
                            self.gradient_state.selected_stop = Some(i);
                            break;
                        }
                    }
                    self.gradient_state.preset = GradientPreset::Custom;
                    self.gradient_state.lut_dirty = true;
                    self.gradient_state.preview_dirty = true;
                }
            }

            // Drag selected stop
            if response.dragged()
                && let Some(sel) = self.gradient_state.selected_stop
                && sel < self.gradient_state.stops.len()
            {
                // Don't allow dragging first/last stops past each other
                let new_pos = t_at_pointer.clamp(0.0, 1.0);
                self.gradient_state.stops[sel].position = new_pos;
                // Re-sort and update selected index
                let sel_stop_pos = self.gradient_state.stops[sel].position;
                let sel_stop_color = self.gradient_state.stops[sel].color;
                self.gradient_state
                    .stops
                    .sort_by(|a, b| a.position.partial_cmp(&b.position).unwrap());
                for (i, s) in self.gradient_state.stops.iter().enumerate() {
                    if (s.position - sel_stop_pos).abs() < 1e-6 && s.color == sel_stop_color {
                        self.gradient_state.selected_stop = Some(i);
                        break;
                    }
                }
                self.gradient_state.preset = GradientPreset::Custom;
                self.gradient_state.lut_dirty = true;
                self.gradient_state.preview_dirty = true;
            }
        }

        // Right-click to delete a stop (if more than 2)
        if response.secondary_clicked()
            && let Some(pp) = ui.input(|i| i.pointer.latest_pos())
            && self.gradient_state.stops.len() > 2
        {
            let mut closest_idx: Option<usize> = None;
            let mut closest_dist = f32::MAX;
            for (i, stop) in self.gradient_state.stops.iter().enumerate() {
                let stop_screen_x = bar_rect.min.x + stop.position * bar_width;
                let d = (pp.x - stop_screen_x).abs();
                if d < stop_radius * 3.0 && d < closest_dist {
                    closest_dist = d;
                    closest_idx = Some(i);
                }
            }
            if let Some(idx) = closest_idx {
                self.gradient_state.stops.remove(idx);
                self.gradient_state.selected_stop = None;
                self.gradient_state.preset = GradientPreset::Custom;
                self.gradient_state.lut_dirty = true;
                self.gradient_state.preview_dirty = true;
            }
        }

        // Color edit for selected stop ÔÇö compact swatch + "Use Primary" button
        if let Some(sel) = self.gradient_state.selected_stop
            && sel < self.gradient_state.stops.len()
        {
            ui.separator();

            let stop_color = self.gradient_state.stops[sel].color;
            let preview_color = Color32::from_rgba_unmultiplied(
                stop_color[0],
                stop_color[1],
                stop_color[2],
                stop_color[3],
            );

            ui.horizontal(|ui| {
                // Color swatch (small)
                let (swatch_rect, _) =
                    ui.allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::hover());
                let p = ui.painter_at(swatch_rect);
                let cs = 4.0;
                for row in 0..4 {
                    for col in 0..4 {
                        let c = if (row + col) % 2 == 0 {
                            Color32::from_gray(200)
                        } else {
                            Color32::WHITE
                        };
                        let r = egui::Rect::from_min_size(
                            swatch_rect.min + egui::vec2(col as f32 * cs, row as f32 * cs),
                            egui::vec2(cs, cs),
                        )
                        .intersect(swatch_rect);
                        p.rect_filled(r, 0.0, c);
                    }
                }
                p.rect_filled(swatch_rect, 2.0, preview_color);
                p.rect_stroke(
                    swatch_rect,
                    2.0,
                    egui::Stroke::new(1.0, Color32::DARK_GRAY),
                    egui::StrokeKind::Middle,
                );

                // Hex label
                ui.label(format!(
                    "#{:02X}{:02X}{:02X}{:02X}",
                    stop_color[0], stop_color[1], stop_color[2], stop_color[3],
                ));

                // "Use Primary" button ÔÇö applies the current primary color to this stop
                if assets
                    .menu_item(ui, Icon::ApplyPrimary, &t!("ctx.gradient.apply_primary"))
                    .clicked()
                    && let Some(pc) = self.gradient_state.cached_primary
                {
                    self.gradient_state.stops[sel].color = pc;
                    let c32 = Color32::from_rgba_unmultiplied(pc[0], pc[1], pc[2], pc[3]);
                    self.gradient_state.stops[sel].hsv = crate::components::colors::color_to_hsv(c32);
                    self.gradient_state.preset = GradientPreset::Custom;
                    self.gradient_state.lut_dirty = true;
                    self.gradient_state.preview_dirty = true;
                }
            });

            // Hint for the user
            ui.label(
                egui::RichText::new(t!("ctx.gradient.color_hint"))
                    .weak()
                    .small(),
            );
        }
    }

    // ====================================================================
    // TEXT TOOL ÔÇö context bar, preview, commit
    // ====================================================================

    fn show_text_options(&mut self, ui: &mut egui::Ui, assets: &Assets) {
        // Async font loading: kick off background thread on first access
        if self.text_state.available_fonts.is_empty() && self.text_state.fonts_loading_rx.is_none()
        {
            let (tx, rx) = std::sync::mpsc::channel();
            self.text_state.fonts_loading_rx = Some(rx);
            std::thread::spawn(move || {
                let fonts = crate::ops::text::enumerate_system_fonts();
                let _ = tx.send(fonts);
            });
        }

        // Check if async font list is ready
        if let Some(ref rx) = self.text_state.fonts_loading_rx
            && let Ok(fonts) = rx.try_recv()
        {
            self.text_state.available_fonts = fonts;
            self.text_state.fonts_loading_rx = None;
            // Refresh weights for current family
            self.text_state.available_weights =
                crate::ops::text::enumerate_font_weights(&self.text_state.font_family);
        }

        // Poll async font preview results. The loader sends multiple batches
        // until all family previews are cached.
        if let Some(rx) = self.text_state.font_preview_rx.take() {
            let mut keep_rx = true;
            loop {
                match rx.try_recv() {
                    Ok(batch) => {
                        for (name, font) in batch {
                            self.text_state.font_preview_pending.remove(&name);
                            self.text_state.font_preview_cache.insert(name, font);
                        }
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => break,
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        keep_rx = false;
                        break;
                    }
                }
            }
            if keep_rx {
                self.text_state.font_preview_rx = Some(rx);
            }
        }

        // Ensure all font previews are loaded in the background with one
        // persistent batch stream (egui virtualized rows can then render from cache).
        if !self.text_state.available_fonts.is_empty()
            && self.text_state.font_preview_rx.is_none()
            && self.text_state.font_preview_pending.is_empty()
            && self.text_state.font_preview_cache.len() < self.text_state.available_fonts.len()
        {
            let missing: Vec<String> = self
                .text_state
                .available_fonts
                .iter()
                .filter(|name| {
                    !self
                        .text_state
                        .font_preview_cache
                        .contains_key(name.as_str())
                })
                .cloned()
                .collect();
            if !missing.is_empty() {
                for name in &missing {
                    self.text_state.font_preview_pending.insert(name.clone());
                }
                let (tx, rx) = std::sync::mpsc::channel();
                self.text_state.font_preview_rx = Some(rx);
                std::thread::spawn(move || {
                    const BATCH: usize = 24;
                    let mut out: Vec<(String, Option<ab_glyph::FontArc>)> =
                        Vec::with_capacity(BATCH);
                    for name in missing {
                        let font = crate::ops::text::load_system_font(&name, 400, false);
                        out.push((name, font));
                        if out.len() >= BATCH {
                            if tx.send(out).is_err() {
                                return;
                            }
                            out = Vec::with_capacity(BATCH);
                        }
                    }
                    if !out.is_empty() {
                        let _ = tx.send(out);
                    }
                });
            }
        }

        ui.label(t!("ctx.text.font"));
        let family_label = self.text_state.font_family.clone();
        let popup_id = ui.make_persistent_id("ctx_text_font_popup");
        self.text_state.font_popup_open = egui::Popup::is_id_open(ui.ctx(), popup_id);

        if self.text_state.available_fonts.is_empty() {
            // Still loading
            ui.add(
                egui::Button::new(t!("ctx.text.loading_fonts")).min_size(egui::vec2(140.0, 0.0)),
            );
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(100));
        } else {
            let button_response = ui.add(
                egui::Button::new(egui::RichText::new(if family_label.len() > 20 {
                    &family_label[..20]
                } else {
                    &family_label
                }))
                .min_size(egui::vec2(140.0, 0.0)),
            );
            if button_response.clicked() {
                egui::Popup::toggle_id(ui.ctx(), popup_id);
            }
            egui::Popup::new(
                popup_id,
                ui.ctx().clone(),
                egui::PopupAnchor::from(&button_response),
                ui.layer_id(),
            )
            .open_memory(None::<egui::SetOpenCommand>)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .show(|ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    const FONT_LIST_HEIGHT: f32 = 300.0;
                    const FONT_POPUP_WIDTH: f32 = 360.0;
                    const FONT_LIST_WIDTH: f32 = FONT_POPUP_WIDTH - 18.0;
                    let preview_ink = {
                        let color = contrast_text_color(ui.visuals().window_fill());
                        [color.r(), color.g(), color.b(), color.a()]
                    };
                    if self.text_state.font_preview_ink.0 != preview_ink {
                        self.text_state.font_preview_ink = FontPreviewInk(preview_ink);
                        self.text_state.font_preview_textures.0.clear();
                    }
                    ui.set_min_width(FONT_POPUP_WIDTH);
                    ui.set_max_width(FONT_POPUP_WIDTH);
                    // Keep popup height stable so clearing search restores a tall list.
                    ui.set_min_height(FONT_LIST_HEIGHT + 38.0);
                    let te_resp = ui.add(
                        egui::TextEdit::singleline(&mut self.text_state.font_search)
                            .hint_text(t!("ctx.text.search"))
                            .desired_width(FONT_LIST_WIDTH - 6.0),
                    );
                    if !te_resp.has_focus() && self.text_state.font_search.is_empty() {
                        te_resp.request_focus();
                    }
                    let search = self.text_state.font_search.to_lowercase();
                    let fonts: Vec<&String> = self
                        .text_state
                        .available_fonts
                        .iter()
                        .filter(|f| search.is_empty() || f.to_lowercase().contains(&search))
                        .collect();

                    let row_height = 22.0;
                    let total_rows = fonts.len().min(200);
                    let list_width = FONT_LIST_WIDTH;
                    egui::ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .min_scrolled_width(list_width)
                        .max_height(FONT_LIST_HEIGHT)
                        .show_rows(ui, row_height, total_rows, |ui, row_range| {
                            for idx in row_range {
                                if idx >= fonts.len() {
                                    break;
                                }
                                let font_name = &fonts[idx];
                                let is_selected = self.text_state.font_family == **font_name;
                                let preview_text = "Abc";
                                let row_width = list_width;
                                let preview_width = 118.0;
                                let gap = 8.0;
                                let name_width = (row_width - preview_width - gap).max(120.0);

                                let (row_rect, row_resp) = ui.allocate_exact_size(
                                    egui::vec2(row_width, row_height),
                                    egui::Sense::click(),
                                );
                                let name_rect = egui::Rect::from_min_max(
                                    row_rect.min,
                                    egui::pos2(row_rect.min.x + name_width, row_rect.max.y),
                                );
                                let preview_rect = egui::Rect::from_min_max(
                                    egui::pos2(name_rect.max.x + gap, row_rect.min.y),
                                    row_rect.max,
                                );
                                let row_clip = row_rect.intersect(ui.clip_rect());
                                let painter = ui.painter().with_clip_rect(row_clip);
                                let row_fully_visible = row_clip.height() >= (row_height - 0.5);

                                if row_resp.hovered() && row_fully_visible {
                                    painter.rect_filled(
                                        row_rect.shrink2(egui::vec2(1.0, 1.0)),
                                        0.0,
                                        ui.visuals().widgets.hovered.weak_bg_fill,
                                    );
                                }

                                if is_selected && row_fully_visible {
                                    painter.rect_filled(
                                        name_rect.shrink2(egui::vec2(2.0, 2.0)),
                                        0.0,
                                        ui.visuals().selection.bg_fill,
                                    );
                                }

                                let display_name = if font_name.len() > 22 {
                                    format!("{}ÔÇª", &font_name[..21])
                                } else {
                                    (*font_name).clone()
                                };
                                painter.text(
                                    egui::pos2(name_rect.min.x + 6.0, name_rect.center().y),
                                    egui::Align2::LEFT_CENTER,
                                    display_name,
                                    egui::FontId::default(),
                                    ui.visuals().text_color(),
                                );

                                match self.text_state.font_preview_cache.get(font_name.as_str()) {
                                    Some(Some(preview_font)) => {
                                        if !self
                                            .text_state
                                            .font_preview_textures
                                            .0
                                            .contains_key(font_name.as_str())
                                        {
                                            let mut preview_coverage = Vec::new();
                                            let mut preview_glyph_cache =
                                                crate::ops::text::GlyphPixelCache::default();
                                            let preview = crate::ops::text::rasterize_text(
                                                preview_font,
                                                preview_text,
                                                18.0,
                                                crate::ops::text::TextAlignment::Left,
                                                0.0,
                                                0.0,
                                                self.text_state.font_preview_ink.0,
                                                true,
                                                false,
                                                false,
                                                false,
                                                false,
                                                256,
                                                64,
                                                &mut preview_coverage,
                                                &mut preview_glyph_cache,
                                                None,
                                                0.0,
                                                1.0,
                                                1.0,
                                                1.0,
                                            );
                                            if preview.buf_w > 0 && preview.buf_h > 0 {
                                                let color_image =
                                                    egui::ColorImage::from_rgba_unmultiplied(
                                                        [
                                                            preview.buf_w as usize,
                                                            preview.buf_h as usize,
                                                        ],
                                                        &preview.buf,
                                                    );
                                                let texture = ui.ctx().load_texture(
                                                    format!("font_preview_tex::{}", font_name),
                                                    color_image,
                                                    egui::TextureOptions::LINEAR,
                                                );
                                                self.text_state
                                                    .font_preview_textures
                                                    .0
                                                    .insert((*font_name).clone(), texture);
                                            }
                                        }

                                        if let Some(texture) = self
                                            .text_state
                                            .font_preview_textures
                                            .0
                                            .get(font_name.as_str())
                                        {
                                            let tex_size = texture.size_vec2();
                                            let max_w = (preview_rect.width() - 24.0).max(8.0);
                                            let max_h = (preview_rect.height() - 4.0).max(8.0);
                                            let scale = (max_w / tex_size.x)
                                                .min(max_h / tex_size.y)
                                                .max(0.01);
                                            let draw_size = tex_size * scale;
                                            let image_rect = egui::Rect::from_min_size(
                                                egui::pos2(
                                                    preview_rect.min.x + 4.0,
                                                    preview_rect.center().y - draw_size.y * 0.5,
                                                ),
                                                draw_size,
                                            );
                                            painter.image(
                                                texture.id(),
                                                image_rect,
                                                egui::Rect::from_min_max(
                                                    egui::pos2(0.0, 0.0),
                                                    egui::pos2(1.0, 1.0),
                                                ),
                                                Color32::WHITE,
                                            );
                                        } else {
                                            painter.text(
                                                egui::pos2(
                                                    preview_rect.min.x + 4.0,
                                                    preview_rect.center().y,
                                                ),
                                                egui::Align2::LEFT_CENTER,
                                                "No preview",
                                                egui::FontId::default(),
                                                ui.visuals().weak_text_color(),
                                            );
                                        }
                                    }
                                    Some(None) => {
                                        painter.text(
                                            egui::pos2(
                                                preview_rect.min.x + 4.0,
                                                preview_rect.center().y,
                                            ),
                                            egui::Align2::LEFT_CENTER,
                                            "No preview",
                                            egui::FontId::default(),
                                            ui.visuals().weak_text_color(),
                                        );
                                    }
                                    None => {
                                        let pending = self
                                            .text_state
                                            .font_preview_pending
                                            .contains(font_name.as_str());
                                        painter.text(
                                            egui::pos2(
                                                preview_rect.min.x + 4.0,
                                                preview_rect.center().y,
                                            ),
                                            egui::Align2::LEFT_CENTER,
                                            if pending { "Loading" } else { "Queued" },
                                            egui::FontId::default(),
                                            ui.visuals().weak_text_color(),
                                        );
                                    }
                                }

                                if row_resp.clicked() {
                                    self.text_state.font_family = (*font_name).clone();
                                    self.text_state.loaded_font = None;
                                    self.text_state.preview_dirty = true;
                                    self.text_state.ctx_bar_style_dirty = true;
                                    self.text_state.cached_raster_key.clear();
                                    // Refresh available weights for new family
                                    self.text_state.available_weights =
                                        crate::ops::text::enumerate_font_weights(
                                            &self.text_state.font_family,
                                        );
                                    if !self
                                        .text_state
                                        .available_weights
                                        .iter()
                                        .any(|w| w.1 == self.text_state.font_weight)
                                    {
                                        self.text_state.font_weight = 400;
                                    }
                                    egui::Popup::close_id(ui.ctx(), popup_id);
                                }
                            }
                        });

                    if !self.text_state.font_preview_pending.is_empty() && total_rows > 0 {
                        ui.ctx()
                            .request_repaint_after(std::time::Duration::from_millis(16));
                    }
                });
            });
            self.text_state.font_popup_open = egui::Popup::is_id_open(ui.ctx(), popup_id);
        }

        // Weight dropdown (only show if more than one weight available)
        if self.text_state.available_weights.len() > 1 {
            ui.separator();
            ui.label(t!("ctx.text.weight"));
            let current_weight_label = self
                .text_state
                .available_weights
                .iter()
                .find(|w| w.1 == self.text_state.font_weight)
                .map(|w| w.0.as_str())
                .unwrap_or("Regular");
            egui::ComboBox::from_id_salt("ctx_text_weight")
                .selected_text(current_weight_label)
                .width(90.0)
                .show_ui(ui, |ui| {
                    for (name, val) in &self.text_state.available_weights {
                        if ui
                            .selectable_label(self.text_state.font_weight == *val, name)
                            .clicked()
                        {
                            self.text_state.font_weight = *val;
                            self.text_state.loaded_font = None;
                            self.text_state.preview_dirty = true;
                            self.text_state.ctx_bar_style_dirty = true;
                            self.text_state.cached_raster_key.clear();
                        }
                    }
                });
        }

        ui.separator();
        // Font size ÔÇö merged DragValue + dropdown (same pattern as brush size widget)
        self.show_text_size_widget(ui, "ctx_text_size", assets);

        ui.separator();
        if ui
            .selectable_label(self.text_state.bold, t!("ctx.text.bold"))
            .clicked()
        {
            self.text_state.bold = !self.text_state.bold;
            self.text_state.loaded_font = None;
            self.text_state.preview_dirty = true;
            self.text_state.ctx_bar_style_dirty = true;
        }
        if ui
            .selectable_label(self.text_state.italic, t!("ctx.text.italic"))
            .clicked()
        {
            self.text_state.italic = !self.text_state.italic;
            self.text_state.loaded_font = None;
            self.text_state.preview_dirty = true;
            self.text_state.ctx_bar_style_dirty = true;
        }
        if ui
            .selectable_label(self.text_state.underline, t!("ctx.text.underline"))
            .clicked()
        {
            self.text_state.underline = !self.text_state.underline;
            self.text_state.preview_dirty = true;
            self.text_state.ctx_bar_style_dirty = true;
        }
        if ui
            .selectable_label(self.text_state.strikethrough, t!("ctx.text.strikethrough"))
            .clicked()
        {
            self.text_state.strikethrough = !self.text_state.strikethrough;
            self.text_state.preview_dirty = true;
            self.text_state.ctx_bar_style_dirty = true;
        }

        ui.separator();
        // Alignment: single cycle-toggle button
        let align_label = self.text_state.alignment.label();
        if ui
            .add(egui::Button::new(align_label).min_size(egui::vec2(50.0, 0.0)))
            .clicked()
        {
            self.text_state.alignment = match self.text_state.alignment {
                crate::ops::text::TextAlignment::Left => crate::ops::text::TextAlignment::Center,
                crate::ops::text::TextAlignment::Center => crate::ops::text::TextAlignment::Right,
                crate::ops::text::TextAlignment::Right => crate::ops::text::TextAlignment::Left,
            };
            self.text_state.preview_dirty = true;
        }

        ui.separator();
        ui.label(t!("ctx.text.letter_spacing"));
        if ui
            .add(
                egui::DragValue::new(&mut self.text_state.letter_spacing)
                    .speed(0.1)
                    .suffix("px"),
            )
            .changed()
        {
            self.text_state.preview_dirty = true;
        }

        ui.separator();
        ui.label("Letter Width");
        if ui
            .add(
                egui::DragValue::new(&mut self.text_state.width_scale)
                    .speed(0.01)
                    .range(0.01..=f32::MAX)
                    .suffix("├ù"),
            )
            .changed()
        {
            self.text_state.preview_dirty = true;
        }

        ui.label("Letter Height");
        if ui
            .add(
                egui::DragValue::new(&mut self.text_state.height_scale)
                    .speed(0.01)
                    .range(0.01..=f32::MAX)
                    .suffix("├ù"),
            )
            .changed()
        {
            self.text_state.preview_dirty = true;
        }

        ui.separator();
        ui.label(t!("ctx.text.line_spacing"));
        if ui
            .add(
                egui::DragValue::new(&mut self.text_state.line_spacing)
                    .speed(0.01)
                    .range(0.5..=5.0)
                    .suffix("├ù"),
            )
            .changed()
        {
            self.text_state.preview_dirty = true;
        }

        ui.separator();
        // Anti-alias: compact "AA" toggle with tooltip
        let aa_resp = ui.selectable_label(self.text_state.anti_alias, t!("ctx.anti_alias"));
        if aa_resp.clicked() {
            self.text_state.anti_alias = !self.text_state.anti_alias;
            self.text_state.preview_dirty = true;
        }
        aa_resp.on_hover_text(t!("ctx.anti_alias_tooltip"));

        ui.separator();
        ui.label(t!("ctx.blend"));
        egui::ComboBox::from_id_salt("ctx_text_blend")
            .selected_text(self.properties.blending_mode.name())
            .width(80.0)
            .show_ui(ui, |ui| {
                for mode in BlendMode::all() {
                    if ui
                        .selectable_label(self.properties.blending_mode == *mode, mode.name())
                        .clicked()
                    {
                        self.properties.blending_mode = *mode;
                    }
                }
            });
    }

    fn ensure_text_font_loaded(&mut self) {
        // Compute effective weight: if bold toggled and weight is normal, use Bold weight
        let effective_weight = if self.text_state.bold {
            if self.text_state.font_weight < 600 {
                700u16
            } else {
                (self.text_state.font_weight + 200).min(900)
            }
        } else {
            self.text_state.font_weight
        };
        let key = format!(
            "{}:{}:{}",
            self.text_state.font_family, effective_weight, self.text_state.italic
        );
        if self.text_state.loaded_font_key != key || self.text_state.loaded_font.is_none() {
            self.text_state.loaded_font = crate::ops::text::load_system_font(
                &self.text_state.font_family,
                effective_weight,
                self.text_state.italic,
            );
            self.text_state.loaded_font_key = key;
            // Clear glyph pixel cache when font changes (different outlines)
            self.text_state.glyph_cache.clear();
        }
    }

    /// Sync tool-state styling (color, font, etc.) to the active text block's
    /// single run so that `force_rasterize_text_layer` renders with the
    /// user-facing properties, not the stored defaults. Only applies to
    /// single-run blocks; multi-run blocks maintain per-run formatting.
    fn sync_text_layer_run_style(&self, canvas_state: &mut CanvasState) {
        let bid = match self.text_state.active_block_id {
            Some(id) => id,
            None => return,
        };
        let aidx = canvas_state.active_layer_index;
        if let Some(layer) = canvas_state.layers.get_mut(aidx)
            && let crate::canvas::LayerContent::Text(ref mut td) = layer.content
            && let Some(block) = td.blocks.iter_mut().find(|b| b.id == bid)
        {
            let mut changed = false;
            // Sync paragraph-level properties (line_spacing)
            if block.paragraph.line_spacing.to_bits() != self.text_state.line_spacing.to_bits() {
                block.paragraph.line_spacing = self.text_state.line_spacing;
                changed = true;
            }
            // Sync run-level properties (single-run blocks only)
            if block.runs.len() <= 1
                && let Some(run) = block.runs.first_mut()
            {
                if run.style.color != self.text_state.last_color {
                    run.style.color = self.text_state.last_color;
                    changed = true;
                }
                if run.style.font_family != self.text_state.font_family {
                    run.style.font_family = self.text_state.font_family.clone();
                    changed = true;
                }
                if run.style.font_weight != self.text_state.font_weight {
                    run.style.font_weight = self.text_state.font_weight;
                    changed = true;
                }
                if run.style.font_size != self.text_state.font_size {
                    run.style.font_size = self.text_state.font_size;
                    changed = true;
                }
                if run.style.italic != self.text_state.italic {
                    run.style.italic = self.text_state.italic;
                    changed = true;
                }
                if run.style.underline != self.text_state.underline {
                    run.style.underline = self.text_state.underline;
                    changed = true;
                }
                if run.style.strikethrough != self.text_state.strikethrough {
                    run.style.strikethrough = self.text_state.strikethrough;
                    changed = true;
                }
                if run.style.letter_spacing.to_bits() != self.text_state.letter_spacing.to_bits() {
                    run.style.letter_spacing = self.text_state.letter_spacing;
                    changed = true;
                }
                if run.style.width_scale.to_bits() != self.text_state.width_scale.to_bits() {
                    run.style.width_scale = self.text_state.width_scale;
                    changed = true;
                }
                if run.style.height_scale.to_bits() != self.text_state.height_scale.to_bits() {
                    run.style.height_scale = self.text_state.height_scale;
                    changed = true;
                }
            }
            if changed {
                td.mark_dirty();
            }
        }
    }

    fn render_text_preview(&mut self, canvas_state: &mut CanvasState, primary_color_f32: [f32; 4]) {
        let origin = match self.text_state.origin {
            Some(o) => o,
            None => return,
        };
        if self.text_state.text.is_empty() {
            canvas_state.clear_preview_state();
            // For text layers, force-rasterize so the layer reflects the empty state
            // (e.g. after deleting all text the old pixels are cleared).
            if self.text_state.editing_text_layer {
                self.sync_text_layer_run_style(canvas_state);
                let idx = canvas_state.active_layer_index;
                canvas_state.force_rasterize_text_layer(idx);
                canvas_state.mark_dirty(None);
            }
            self.text_state.preview_dirty = false;
            return;
        }

        self.ensure_text_font_loaded();
        let font = match &self.text_state.loaded_font {
            Some(f) => f.clone(),
            None => return,
        };

        // For text layer editing: skip full pixel rasterization ÔÇö only compute
        // lightweight layout metrics for cursor/overlay, then force-rasterize
        // the layer via TextLayerData pipeline. This avoids a double full
        // rasterization per keystroke (was: rasterize_text + force_rasterize).
        if self.text_state.editing_text_layer {
            let active_max_width = self.text_state.active_block_max_width;
            let metrics = crate::ops::text::compute_text_layout_metrics(
                &font,
                &self.text_state.text,
                self.text_state.font_size,
                active_max_width,
                self.text_state.letter_spacing,
                self.text_state.line_spacing,
                self.text_state.width_scale,
                self.text_state.height_scale,
            );
            self.text_state.cached_line_advances = metrics.line_advances;
            self.text_state.cached_line_height = metrics.line_height;
            canvas_state.clear_preview_state();
            self.sync_text_layer_run_style(canvas_state);
            let idx = canvas_state.active_layer_index;
            canvas_state.force_rasterize_text_layer(idx);
            canvas_state.mark_dirty(None);
            self.text_state.preview_dirty = false;
            return;
        }

        let color = [
            (primary_color_f32[0] * 255.0) as u8,
            (primary_color_f32[1] * 255.0) as u8,
            (primary_color_f32[2] * 255.0) as u8,
            (primary_color_f32[3] * 255.0) as u8,
        ];

        // Pass max_width for word wrapping when editing a text layer block
        let active_max_width = if self.text_state.editing_text_layer {
            self.text_state.active_block_max_width
        } else {
            None
        };
        let result = crate::ops::text::rasterize_text(
            &font,
            &self.text_state.text,
            self.text_state.font_size,
            self.text_state.alignment,
            origin[0],
            origin[1],
            color,
            self.text_state.anti_alias,
            self.text_state.bold,
            self.text_state.italic,
            self.text_state.underline,
            self.text_state.strikethrough,
            canvas_state.width,
            canvas_state.height,
            &mut self.text_state.coverage_buf,
            &mut self.text_state.glyph_cache,
            active_max_width,
            self.text_state.letter_spacing,
            self.text_state.line_spacing,
            self.text_state.width_scale,
            self.text_state.height_scale,
        );

        // Cache cursor metrics from rasterization (Opt 7)
        self.text_state.cached_line_advances = result.line_advances;
        self.text_state.cached_line_height = result.line_height;

        if result.buf_w == 0 || result.buf_h == 0 {
            canvas_state.clear_preview_state();
            self.text_state.preview_dirty = false;
            return;
        }

        let off_x = result.off_x;
        let off_y = result.off_y;
        let buf_w = result.buf_w;
        let buf_h = result.buf_h;

        // Cache the rasterized buffer for fast drag moves
        self.text_state.cached_raster_buf = result.buf.clone();
        self.text_state.cached_raster_w = buf_w;
        self.text_state.cached_raster_h = buf_h;
        self.text_state.cached_raster_off_x = off_x;
        self.text_state.cached_raster_off_y = off_y;
        self.text_state.cached_raster_origin = self.text_state.origin;

        // Raster-stamp text editing: build preview overlay for non-text-layer editing
        // (text layer editing has already returned above via the lightweight metrics path)
        let mut preview = TiledImage::new(canvas_state.width, canvas_state.height);
        preview.blit_rgba_at(off_x, off_y, buf_w, buf_h, &result.buf);
        Self::adjust_preview_region_for_selection(
            &mut preview,
            off_x,
            off_y,
            buf_w,
            buf_h,
            canvas_state.selection_mask.as_ref(),
            0.0,
        );

        canvas_state.preview_layer = Some(preview);
        canvas_state.preview_blend_mode = self.properties.blending_mode;
        // Opt 2: Only force full composite when blend mode isn't Normal
        canvas_state.preview_force_composite = self.properties.blending_mode != BlendMode::Normal;
        canvas_state.preview_is_eraser = false;
        canvas_state.preview_downscale = 1;
        canvas_state.preview_flat_ready = false;
        // Opt 2: Limit composite/extraction to the visible portion of the text region.
        let visible_bounds = Self::clip_preview_bounds(canvas_state, off_x, off_y, buf_w, buf_h);
        canvas_state.preview_stroke_bounds = visible_bounds;
        // Opt 3: Use dirty_rect to signal texture update needed instead of
        // full cache invalidation. This lets the display path do a set()
        // instead of creating a brand new texture handle.
        if canvas_state.preview_texture_cache.is_some() {
            canvas_state.preview_dirty_rect = visible_bounds;
        } else {
            // First time: force texture creation
            canvas_state.preview_texture_cache = None;
        }
        canvas_state.mark_dirty(None);
        self.text_state.preview_dirty = false;
    }

    /// Load a text layer's data into the text tool state for editing.
    fn load_text_layer_for_editing(&mut self, canvas_state: &mut CanvasState) {
        self.load_text_layer_block(canvas_state, None, None);
    }

    /// Restore the raster text tool style that was saved before entering text-layer editing.
    /// Call this when starting a new raster text session to prevent text-layer properties
    /// from bleeding into the raster text tool.
    fn restore_raster_style(&mut self) {
        let s = self.text_state.saved_raster_style.clone();
        self.text_state.font_family = s.font_family;
        self.text_state.font_size = s.font_size;
        self.text_state.font_weight = s.font_weight;
        self.text_state.bold = s.bold;
        self.text_state.italic = s.italic;
        self.text_state.underline = s.underline;
        self.text_state.strikethrough = s.strikethrough;
        self.text_state.letter_spacing = s.letter_spacing;
        self.text_state.width_scale = s.width_scale;
        self.text_state.height_scale = s.height_scale;
        self.text_state.line_spacing = s.line_spacing;
        self.text_state.alignment = s.alignment;
        self.text_state.last_color = s.last_color;
        // Force font reload for the restored family
        self.text_state.loaded_font_key.clear();
        self.text_state.loaded_font = None;
    }

    /// Load a specific block from a text layer for editing.
    /// If `block_id` is None, loads the first block.
    /// If `click_pos` is provided, hit-tests blocks or creates a new one.
    fn load_text_layer_block(
        &mut self,
        canvas_state: &mut CanvasState,
        block_id: Option<u64>,
        click_pos: Option<[f32; 2]>,
    ) {
        let layer_idx = canvas_state.active_layer_index;
        let layer = match canvas_state.layers.get(layer_idx) {
            Some(l) => l,
            None => return,
        };
        let text_data = match &layer.content {
            crate::canvas::LayerContent::Text(td) => td,
            _ => return,
        };

        // Capture "before" snapshot for TextLayerEditCommand undo (only on first load per session).
        // Subsequent block switches within the same text layer session keep the original snapshot.
        if self.text_state.text_layer_before.is_none() {
            self.text_state.text_layer_before = Some((layer_idx, text_data.clone()));
        }

        // Determine which block to edit
        let target_block_id = if let Some(bid) = block_id {
            Some(bid)
        } else if let Some(pos) = click_pos {
            // Hit-test: find block at click position
            crate::ops::text_layer::hit_test_blocks(text_data, pos[0], pos[1])
                .map(|idx| text_data.blocks[idx].id)
        } else {
            // Default: first block
            text_data.blocks.first().map(|b| b.id)
        };

        // Get the block data (or create new block)
        let (
            block_text,
            style,
            position,
            bid,
            alignment,
            block_max_width,
            block_max_height,
            block_line_spacing,
        );
        if let Some(bid_val) = target_block_id {
            if let Some(block) = text_data.blocks.iter().find(|b| b.id == bid_val) {
                block_text = block.flat_text();
                style = block
                    .runs
                    .first()
                    .map(|r| r.style.clone())
                    .unwrap_or_default();
                position = block.position;
                bid = bid_val;
                alignment = block.paragraph.alignment;
                block_max_width = block.max_width;
                block_max_height = block.max_height;
                block_line_spacing = block.paragraph.line_spacing;
            } else {
                return;
            }
        } else if let Some(pos) = click_pos {
            // Create a new block at click position ÔÇö need mutable access
            let layer_mut = match canvas_state.layers.get_mut(canvas_state.active_layer_index) {
                Some(l) => l,
                None => return,
            };
            let td = match &mut layer_mut.content {
                crate::canvas::LayerContent::Text(td) => td,
                _ => return,
            };
            let new_id = td.add_block(pos[0], pos[1]);
            block_text = String::new();
            style = td.primary_style().clone();
            position = pos;
            bid = new_id;
            alignment = crate::ops::text_layer::TextAlignment::Left;
            block_max_width = None;
            block_max_height = None;
            block_line_spacing = 1.0;
        } else {
            return;
        };

        // Save raster text style before overwriting with text-layer properties, so we can
        // restore it when the user switches back to a raster layer.
        if !self.text_state.editing_text_layer {
            self.text_state.saved_raster_style = SavedRasterStyle {
                font_family: self.text_state.font_family.clone(),
                font_size: self.text_state.font_size,
                font_weight: self.text_state.font_weight,
                bold: self.text_state.bold,
                italic: self.text_state.italic,
                underline: self.text_state.underline,
                strikethrough: self.text_state.strikethrough,
                letter_spacing: self.text_state.letter_spacing,
                width_scale: self.text_state.width_scale,
                height_scale: self.text_state.height_scale,
                line_spacing: self.text_state.line_spacing,
                alignment: self.text_state.alignment,
                last_color: self.text_state.last_color,
            };
        }

        self.text_state.origin = Some(position);
        self.text_state.text = block_text.clone();
        self.text_state.cursor_pos = block_text.len();
        self.text_state.is_editing = true;
        self.text_state.editing_text_layer = true;
        self.text_state.editing_layer_index = Some(canvas_state.active_layer_index);
        self.text_state.active_block_id = Some(bid);
        self.text_state.selection = crate::ops::text_layer::TextSelection::default();
        self.text_state.font_family = style.font_family.clone();
        self.text_state.font_size = style.font_size;
        self.text_state.font_weight = style.font_weight;
        self.text_state.bold = style.font_weight >= 700;
        self.text_state.italic = style.italic;
        self.text_state.underline = style.underline;
        self.text_state.strikethrough = style.strikethrough;
        self.text_state.last_color = style.color;
        self.text_state.letter_spacing = style.letter_spacing;
        self.text_state.width_scale = style.width_scale;
        self.text_state.height_scale = style.height_scale;
        self.text_state.line_spacing = block_line_spacing;
        self.text_state.preview_dirty = true;
        self.text_state.active_block_max_width = block_max_width;
        self.text_state.active_block_max_height = block_max_height;
        self.text_state.text_box_drag = None;
        self.text_state.active_block_height = 0.0;

        // Convert alignment
        self.text_state.alignment = match alignment {
            crate::ops::text_layer::TextAlignment::Left => crate::ops::text::TextAlignment::Left,
            crate::ops::text_layer::TextAlignment::Center => {
                crate::ops::text::TextAlignment::Center
            }
            crate::ops::text_layer::TextAlignment::Right => crate::ops::text::TextAlignment::Right,
        };

        // Force font re-load for the new family
        self.text_state.loaded_font_key.clear();
        self.text_state.loaded_font = None;

        // Load layer effects into tool state for the effects panel
        if let Some(layer) = canvas_state.layers.get(canvas_state.active_layer_index)
            && let crate::canvas::LayerContent::Text(ref td) = layer.content
        {
            self.text_state.text_effects = td.effects.clone();
            // Load warp from the active block
            if let Some(bid) = self.text_state.active_block_id
                && let Some(block) = td.blocks.iter().find(|b| b.id == bid)
            {
                self.text_state.text_warp = block.warp.clone();
                self.text_state.glyph_overrides = block.glyph_overrides.clone();
            }
        }
        self.text_state.text_effects_dirty = false;
        self.text_state.text_warp_dirty = false;
        self.text_state.glyph_edit_mode = false;
        self.text_state.selected_glyphs.clear();
        self.text_state.cached_glyph_bounds.clear();
        self.text_state.glyph_bounds_dirty = true;
        self.text_state.glyph_drag = None;
        self.text_state.glyph_overrides_dirty = false;

        // Set the editing layer marker so ensure_text_layers_rasterized skips
        // this layer (we handle rasterization explicitly via force_rasterize).
        canvas_state.text_editing_layer = Some(canvas_state.active_layer_index);
        // Force-rasterize so the layer pixels are up-to-date (e.g. after commit
        // of a previous block, the committed text is visible).
        let idx = canvas_state.active_layer_index;
        canvas_state.force_rasterize_text_layer(idx);
        canvas_state.mark_dirty(None);

        self.stroke_tracker
            .start_preview_tool(canvas_state.active_layer_index, "Text");
    }

    fn commit_text(&mut self, canvas_state: &mut CanvasState) {
        if self.text_state.text.is_empty() || self.text_state.origin.is_none() {
            self.text_state.is_editing = false;
            self.text_state.editing_text_layer = false;
            self.text_state.editing_layer_index = None;
            self.text_state.active_block_id = None;
            self.text_state.selection = crate::ops::text_layer::TextSelection::default();
            self.text_state.origin = None;
            canvas_state.text_editing_layer = None;
            canvas_state.clear_preview_state();
            return;
        }

        // Text layer mode: write back to TextLayerData instead of blending into pixels
        if self.text_state.editing_text_layer {
            self.commit_text_layer(canvas_state);
            return;
        }

        // Apply mirror to text preview before committing
        canvas_state.mirror_preview_layer();

        // Set stroke bounds from preview so undo/redo captures the affected region
        if let Some(bounds) = canvas_state.preview_stroke_bounds {
            self.stroke_tracker.expand_bounds(bounds);
        }

        let stroke_event = self.stroke_tracker.finish(canvas_state);

        let blend_mode = self.properties.blending_mode;
        let selection_mask = canvas_state.selection_mask.clone();
        let target_layer_idx = self
            .text_state
            .editing_layer_index
            .unwrap_or(canvas_state.active_layer_index);
        if let Some(ref preview) = canvas_state.preview_layer
            && let Some(active_layer) = canvas_state.layers.get_mut(target_layer_idx)
        {
            let chunk_data: Vec<(u32, u32, image::RgbaImage)> = preview
                .chunk_keys()
                .filter_map(|(cx, cy)| preview.get_chunk(cx, cy).map(|c| (cx, cy, c.clone())))
                .collect();
            let chunk_size = crate::canvas::CHUNK_SIZE;
            for (cx, cy, chunk) in &chunk_data {
                let base_x = cx * chunk_size;
                let base_y = cy * chunk_size;
                let cw = chunk_size.min(canvas_state.width.saturating_sub(base_x));
                let ch = chunk_size.min(canvas_state.height.saturating_sub(base_y));
                for ly in 0..ch {
                    for lx in 0..cw {
                        let gx = base_x + lx;
                        let gy = base_y + ly;
                        if !Self::selection_allows(selection_mask.as_ref(), gx, gy) {
                            continue;
                        }
                        let src = *chunk.get_pixel(lx, ly);
                        if src.0[3] == 0 {
                            continue;
                        }
                        let dst = active_layer.pixels.get_pixel_mut(gx, gy);
                        *dst = CanvasState::blend_pixel_static(*dst, src, blend_mode, 1.0);
                    }
                }
            }
            active_layer.invalidate_lod();
            active_layer.gpu_generation += 1;
        }

        self.text_state.text.clear();
        self.text_state.cursor_pos = 0;
        self.text_state.is_editing = false;
        self.text_state.editing_text_layer = false;
        self.text_state.editing_layer_index = None;
        self.text_state.active_block_id = None;
        self.text_state.selection = crate::ops::text_layer::TextSelection::default();
        self.text_state.origin = None;
        canvas_state.clear_preview_state();
        canvas_state.mark_dirty(None);

        if stroke_event.is_some() {
            self.pending_stroke_event = stroke_event;
        }
    }

    // ====================================================================
    // TEXT LAYER EDITING HELPERS (Batch 3+4: multi-run, multi-block)
    // ====================================================================

    /// Insert text at cursor position in the active text layer block.
    /// Handles selection deletion before insertion.
    fn text_layer_insert_text(&mut self, canvas_state: &mut CanvasState, text: &str) {
        let bid = match self.text_state.active_block_id {
            Some(id) => id,
            None => return,
        };

        // Delete selection first if any
        if self.text_state.selection.has_selection() {
            self.text_layer_delete_selection(canvas_state);
        }

        let cursor = self.text_state.cursor_pos;

        if let Some(layer) = canvas_state.layers.get_mut(canvas_state.active_layer_index)
            && let crate::canvas::LayerContent::Text(ref mut td) = layer.content
            && let Some(idx) = td.blocks.iter().position(|b| b.id == bid)
        {
            td.blocks[idx].insert_text_at(cursor, text);
            td.mark_dirty();
            self.text_state.text = td.blocks[idx].flat_text();
            self.text_state.cursor_pos = cursor + text.len();
        }

        self.text_state.selection = crate::ops::text_layer::TextSelection::default();
        self.text_state.preview_dirty = true;
    }

    /// Backspace in text layer mode: delete selection or char before cursor.
    fn text_layer_backspace(&mut self, canvas_state: &mut CanvasState) {
        if self.text_state.selection.has_selection() {
            self.text_layer_delete_selection(canvas_state);
            return;
        }

        let bid = match self.text_state.active_block_id {
            Some(id) => id,
            None => return,
        };
        let cursor = self.text_state.cursor_pos;
        if cursor == 0 {
            return;
        }

        // Find the byte length of the char before cursor
        let prev_char_len = self.text_state.text[..cursor]
            .chars()
            .last()
            .map(|c| c.len_utf8())
            .unwrap_or(0);
        if prev_char_len == 0 {
            return;
        }

        let del_start = cursor - prev_char_len;
        let del_end = cursor;

        if let Some(layer) = canvas_state.layers.get_mut(canvas_state.active_layer_index)
            && let crate::canvas::LayerContent::Text(ref mut td) = layer.content
            && let Some(idx) = td.blocks.iter().position(|b| b.id == bid)
        {
            td.blocks[idx].delete_range(del_start, del_end);
            td.mark_dirty();
            self.text_state.text = td.blocks[idx].flat_text();
            self.text_state.cursor_pos = del_start;
        }

        self.text_state.selection = crate::ops::text_layer::TextSelection::default();
        self.text_state.preview_dirty = true;
    }

    /// Delete key in text layer mode: delete selection or char after cursor.
    fn text_layer_delete(&mut self, canvas_state: &mut CanvasState) {
        if self.text_state.selection.has_selection() {
            self.text_layer_delete_selection(canvas_state);
            return;
        }

        let bid = match self.text_state.active_block_id {
            Some(id) => id,
            None => return,
        };
        let cursor = self.text_state.cursor_pos;
        if cursor >= self.text_state.text.len() {
            return;
        }

        let next_char_len = self.text_state.text[cursor..]
            .chars()
            .next()
            .map(|c| c.len_utf8())
            .unwrap_or(0);
        if next_char_len == 0 {
            return;
        }

        let del_start = cursor;
        let del_end = cursor + next_char_len;

        if let Some(layer) = canvas_state.layers.get_mut(canvas_state.active_layer_index)
            && let crate::canvas::LayerContent::Text(ref mut td) = layer.content
            && let Some(idx) = td.blocks.iter().position(|b| b.id == bid)
        {
            td.blocks[idx].delete_range(del_start, del_end);
            td.mark_dirty();
            self.text_state.text = td.blocks[idx].flat_text();
        }

        self.text_state.selection = crate::ops::text_layer::TextSelection::default();
        self.text_state.preview_dirty = true;
    }

    /// Move the text cursor up or down by `direction` visual lines (-1 = up, +1 = down).
    /// Tries to preserve the x-position (character column) across lines.
    fn text_move_cursor_vertical(
        &mut self,
        direction: i32,
        shift_held: bool,
        canvas_state: &mut CanvasState,
    ) {
        let text = &self.text_state.text;
        if text.is_empty() {
            return;
        }
        let font_size = self.text_state.font_size;
        let ls = self.text_state.letter_spacing;

        // Determine current visual line and char offset
        let (cur_vis_line, cur_vis_char) = if let (Some(mw), Some(font)) = (
            self.text_state.active_block_max_width,
            &self.text_state.loaded_font,
        ) {
            Self::byte_pos_to_visual(
                text,
                self.text_state.cursor_pos,
                font,
                font_size,
                mw,
                ls,
                self.text_state.width_scale,
                self.text_state.height_scale,
            )
        } else {
            // No wrapping ÔÇö use logical lines
            let before = &text[..self.text_state.cursor_pos];
            let line_idx = before.matches('\n').count();
            let last_line = before.rsplit('\n').next().unwrap_or(before);
            (line_idx, last_line.chars().count())
        };

        // Compute total visual line count
        let total_visual_lines = if let (Some(mw), Some(font)) = (
            self.text_state.active_block_max_width,
            &self.text_state.loaded_font,
        ) {
            text.split('\n')
                .flat_map(|line| {
                    crate::ops::text::word_wrap_line(
                        line,
                        font,
                        font_size,
                        mw,
                        ls,
                        self.text_state.width_scale,
                        self.text_state.height_scale,
                    )
                })
                .count()
        } else {
            text.split('\n').count()
        };

        // Compute target visual line
        let target_line = if direction < 0 {
            if cur_vis_line == 0 {
                // Already on first line ÔÇö move cursor to start of text
                self.text_state.cursor_pos = 0;
                if self.text_state.editing_text_layer {
                    self.text_layer_update_selection(canvas_state, shift_held);
                }
                return;
            }
            cur_vis_line - 1
        } else {
            if cur_vis_line >= total_visual_lines.saturating_sub(1) {
                // Already on last line ÔÇö move cursor to end of text
                self.text_state.cursor_pos = text.len();
                if self.text_state.editing_text_layer {
                    self.text_layer_update_selection(canvas_state, shift_held);
                }
                return;
            }
            cur_vis_line + 1
        };

        // Get the x-position of the cursor on the current line (in pixels)
        // to find the closest char on the target line
        let cursor_x = self
            .text_state
            .cached_line_advances
            .get(cur_vis_line)
            .and_then(|adv| adv.get(cur_vis_char).copied())
            .unwrap_or(0.0);

        // Find the closest char position on the target line
        let target_char =
            if let Some(advances) = self.text_state.cached_line_advances.get(target_line) {
                let mut best = 0;
                let mut best_dist = f32::MAX;
                for (ci, &adv) in advances.iter().enumerate() {
                    let dist = (adv - cursor_x).abs();
                    if dist < best_dist {
                        best_dist = dist;
                        best = ci;
                    }
                }
                // Clamp to line char count (advances has count+1 entries)
                best.min(advances.len().saturating_sub(1))
            } else {
                cur_vis_char
            };

        // Convert (target_line, target_char) back to byte position
        let new_byte_pos = if let (Some(mw), Some(font)) = (
            self.text_state.active_block_max_width,
            &self.text_state.loaded_font,
        ) {
            Self::visual_to_byte_pos(
                text,
                target_line,
                target_char,
                font,
                font_size,
                mw,
                ls,
                self.text_state.width_scale,
                self.text_state.height_scale,
            )
        } else {
            // No wrapping ÔÇö use logical lines
            let lines: Vec<&str> = text.split('\n').collect();
            let clamped = target_line.min(lines.len().saturating_sub(1));
            let line_start: usize = lines.iter().take(clamped).map(|l| l.len() + 1).sum();
            let line_text = lines[clamped];
            let clamped_char = target_char.min(line_text.chars().count());
            let byte_offset: usize = line_text
                .chars()
                .take(clamped_char)
                .map(|c| c.len_utf8())
                .sum();
            line_start + byte_offset
        };

        self.text_state.cursor_pos = new_byte_pos;
        if self.text_state.editing_text_layer {
            self.text_layer_update_selection(canvas_state, shift_held);
        }
    }

    /// Delete the currently selected range in the active text layer block.
    fn text_layer_delete_selection(&mut self, canvas_state: &mut CanvasState) {
        let bid = match self.text_state.active_block_id {
            Some(id) => id,
            None => return,
        };

        if !self.text_state.selection.has_selection() {
            return;
        }

        // Get ordered byte offsets
        let (start, end) = if let Some(layer) =
            canvas_state.layers.get(canvas_state.active_layer_index)
            && let crate::canvas::LayerContent::Text(ref td) = layer.content
            && let Some(block) = td.blocks.iter().find(|b| b.id == bid)
        {
            self.text_state.selection.ordered_flat_offsets(block)
        } else {
            return;
        };

        if let Some(layer) = canvas_state.layers.get_mut(canvas_state.active_layer_index)
            && let crate::canvas::LayerContent::Text(ref mut td) = layer.content
            && let Some(idx) = td.blocks.iter().position(|b| b.id == bid)
        {
            td.blocks[idx].delete_range(start, end);
            td.mark_dirty();
            self.text_state.text = td.blocks[idx].flat_text();
            self.text_state.cursor_pos = start;
        }

        self.text_state.selection = crate::ops::text_layer::TextSelection::default();
        self.text_state.preview_dirty = true;
    }

    /// Update selection after cursor movement.
    /// If shift is held, extend selection; otherwise collapse.
    fn text_layer_update_selection(&mut self, canvas_state: &mut CanvasState, shift_held: bool) {
        let bid = match self.text_state.active_block_id {
            Some(id) => id,
            None => return,
        };

        if let Some(layer) = canvas_state.layers.get(canvas_state.active_layer_index)
            && let crate::canvas::LayerContent::Text(ref td) = layer.content
            && let Some(block) = td.blocks.iter().find(|b| b.id == bid)
        {
            let new_pos = block.flat_offset_to_run_pos(self.text_state.cursor_pos);
            self.text_state.selection.cursor = new_pos;
            if !shift_held {
                self.text_state.selection.anchor = new_pos;
            }
        }
    }

    /// Toggle a style property on the selection in a text layer block.
    fn text_layer_toggle_style(
        &mut self,
        canvas_state: &mut CanvasState,
        apply: impl Fn(&mut crate::ops::text_layer::TextStyle),
    ) {
        let bid = match self.text_state.active_block_id {
            Some(id) => id,
            None => return,
        };

        if !self.text_state.selection.has_selection() {
            return;
        }

        // Get ordered byte offsets
        let (start, end) = if let Some(layer) =
            canvas_state.layers.get(canvas_state.active_layer_index)
            && let crate::canvas::LayerContent::Text(ref td) = layer.content
            && let Some(block) = td.blocks.iter().find(|b| b.id == bid)
        {
            self.text_state.selection.ordered_flat_offsets(block)
        } else {
            return;
        };

        if let Some(layer) = canvas_state.layers.get_mut(canvas_state.active_layer_index)
            && let crate::canvas::LayerContent::Text(ref mut td) = layer.content
            && let Some(idx) = td.blocks.iter().position(|b| b.id == bid)
        {
            td.blocks[idx].apply_style_to_range(start, end, apply);
            td.mark_dirty();
            // Refresh flat text (run splitting might have changed byte offsets)
            self.text_state.text = td.blocks[idx].flat_text();
        }

        self.text_state.preview_dirty = true;
    }

    /// Cycle to the next/previous text block (Tab / Shift+Tab).
    fn text_layer_cycle_block(&mut self, canvas_state: &mut CanvasState, reverse: bool) {
        let bid = match self.text_state.active_block_id {
            Some(id) => id,
            None => return,
        };

        // First, sync the current block (commit position/text)
        if let Some(layer) = canvas_state.layers.get(canvas_state.active_layer_index)
            && let crate::canvas::LayerContent::Text(ref td) = layer.content
        {
            let block_count = td.blocks.len();
            if block_count <= 1 {
                return;
            }

            let current_idx = td.blocks.iter().position(|b| b.id == bid).unwrap_or(0);
            let next_idx = if reverse {
                if current_idx == 0 {
                    block_count - 1
                } else {
                    current_idx - 1
                }
            } else {
                (current_idx + 1) % block_count
            };
            let next_id = td.blocks[next_idx].id;

            // Load the next block
            self.load_text_layer_block(canvas_state, Some(next_id), None);
        }
    }

    /// Commit text edits back to the active text layer's `TextLayerData`.
    fn commit_text_layer(&mut self, canvas_state: &mut CanvasState) {
        use crate::ops::text_layer::{TextAlignment as TLA, TextStyle as TLS};

        let origin = self.text_state.origin.unwrap_or([100.0, 100.0]);
        let text = self.text_state.text.clone();
        let color = self.text_state.last_color;

        let alignment = match self.text_state.alignment {
            crate::ops::text::TextAlignment::Left => TLA::Left,
            crate::ops::text::TextAlignment::Center => TLA::Center,
            crate::ops::text::TextAlignment::Right => TLA::Right,
        };

        let new_style = TLS {
            font_family: self.text_state.font_family.clone(),
            font_weight: self.text_state.font_weight,
            font_size: self.text_state.font_size,
            italic: self.text_state.italic,
            underline: self.text_state.underline,
            strikethrough: self.text_state.strikethrough,
            color,
            letter_spacing: self.text_state.letter_spacing,
            baseline_offset: 0.0,
            width_scale: self.text_state.width_scale,
            height_scale: self.text_state.height_scale,
        };

        // Update the TextLayerData on the active layer
        if let Some(layer) = canvas_state.layers.get_mut(canvas_state.active_layer_index)
            && let crate::canvas::LayerContent::Text(ref mut td) = layer.content
        {
            if let Some(block_id) = self.text_state.active_block_id {
                // Multi-block mode: update the specific active block
                if let Some(block) = td.blocks.iter_mut().find(|b| b.id == block_id) {
                    block.position = origin;
                    block.paragraph.alignment = alignment;
                    block.paragraph.line_spacing = self.text_state.line_spacing;
                    block.max_width = self.text_state.active_block_max_width;
                    block.max_height = self.text_state.active_block_max_height;
                    // If the block has only one run (no per-run formatting),
                    // update its text and style directly
                    if block.runs.len() <= 1
                        && let Some(run) = block.runs.first_mut()
                    {
                        run.text = text;
                        run.style = new_style;
                    }
                    // If multi-run: text is already synced during editing,
                    // just update position and alignment
                }
            } else {
                // Legacy path: update the first block
                if let Some(block) = td.blocks.first_mut() {
                    block.position = origin;
                    block.paragraph.alignment = alignment;
                    if let Some(run) = block.runs.first_mut() {
                        run.text = text;
                        run.style = new_style;
                    }
                }
            }
            // Empty blocks are kept as placeholders ÔÇö use delete (├ù) button to remove
            // td.remove_empty_blocks();
            // Note: effects and warp are NOT overwritten here.
            // The layer settings dialog writes directly to TextLayerData,
            // so we must not clobber those values with stale tool-state copies.
            // Write glyph overrides back to the active block
            if let Some(block_id) = self.text_state.active_block_id
                && let Some(block) = td.blocks.iter_mut().find(|b| b.id == block_id)
            {
                block.glyph_overrides = self.text_state.glyph_overrides.clone();
                block.cleanup_glyph_overrides();
            }
            td.mark_dirty();
        }

        // --- Text Layer Undo via TextLayerEditCommand ---
        // Use the captured "before" snapshot and current state for efficient vector-data undo.
        // Cancel the stroke tracker (it was only used for preview positioning).
        self.stroke_tracker.cancel();
        if let Some((layer_idx, before_td)) = self.text_state.text_layer_before.take() {
            // Capture "after" state
            if let Some(layer) = canvas_state.layers.get(layer_idx)
                && let crate::canvas::LayerContent::Text(ref after_td) = layer.content
            {
                let mut cmd = crate::components::history::TextLayerEditCommand::new_from(
                    "Text Edit".to_string(),
                    layer_idx,
                    before_td,
                );
                cmd.set_after_from(after_td.clone());
                self.pending_history_commands.push(Box::new(cmd));
            }
        }

        // Clean up editing state
        self.text_state.text.clear();
        self.text_state.cursor_pos = 0;
        self.text_state.is_editing = false;
        self.text_state.editing_text_layer = false;
        self.text_state.editing_layer_index = None;
        self.text_state.active_block_id = None;
        self.text_state.selection = crate::ops::text_layer::TextSelection::default();
        self.text_state.text_effects = crate::ops::text_layer::TextEffects::default();
        self.text_state.text_effects_dirty = false;
        self.text_state.text_warp = crate::ops::text_layer::TextWarp::None;
        self.text_state.text_warp_dirty = false;
        self.text_state.text_box_drag = None;
        self.text_state.active_block_max_width = None;
        self.text_state.active_block_max_height = None;
        self.text_state.active_block_height = 0.0;
        self.text_state.glyph_edit_mode = false;
        self.text_state.selected_glyphs.clear();
        self.text_state.cached_glyph_bounds.clear();
        self.text_state.glyph_bounds_dirty = true;
        self.text_state.glyph_drag = None;
        self.text_state.glyph_overrides.clear();
        self.text_state.glyph_overrides_dirty = false;
        self.text_state.text_layer_drag_cached = false;
        self.text_state.text_layer_before = None;
        self.text_state.origin = None;
        canvas_state.text_editing_layer = None;
        canvas_state.clear_preview_state();
        canvas_state.mark_dirty(None);
    }

    // ====================================================================
    // LIQUIFY TOOL ÔÇö context bar, preview, commit
    // ====================================================================

}
