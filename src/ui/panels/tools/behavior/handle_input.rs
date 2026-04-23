impl ToolsPanel {
pub fn handle_input(
        &mut self,
        ui: &egui::Ui,
        canvas_state: &mut CanvasState,
        canvas_pos: Option<(u32, u32)>,
        canvas_pos_f32: Option<(f32, f32)>,
        canvas_pos_f32_clamped: Option<(f32, f32)>,
        canvas_pos_unclamped: Option<(f32, f32)>,
        raw_motion_events: &[(f32, f32)],
        painter: &egui::Painter,
        canvas_rect: Rect,
        zoom: f32,
        primary_color_f32: [f32; 4],
        secondary_color_f32: [f32; 4],
        mut gpu_renderer: Option<&mut crate::gpu::GpuRenderer>,
    ) {
        let mut stroke_event: Option<StrokeEvent> = None;

        self.poll_active_layer_rgba_prewarm();
        self.maybe_prewarm_active_layer_rgba(canvas_state);

        // -- Auto-commit on layer change ---------------------------
        // If the active layer index or layer count changed (user clicked a
        // different layer, reordered, deleted, etc.), commit any tool that
        // has uncommitted state ÔÇö mirrors the auto-commit-on-tool-switch
        // logic below.
        let prev_layer_index = self.last_tracked_layer_index;
        let layer_changed = canvas_state.active_layer_index != self.last_tracked_layer_index
            || canvas_state.layers.len() != self.last_tracked_layer_count;
        if layer_changed {
            // Line
            if self.active_tool == Tool::Line
                && self.line_state.line_tool.stage == LineStage::Editing
            {
                let line_editing_mask = canvas_state.edit_layer_mask
                    && canvas_state
                        .layers
                        .get(canvas_state.active_layer_index)
                        .is_some_and(|l| l.has_live_mask());
                if let Some(final_bounds) = self.line_state.line_tool.last_bounds {
                    self.stroke_tracker.expand_bounds(final_bounds);
                }
                let auto_commit_event = self.stroke_tracker.finish(canvas_state);
                let mirror_bounds = canvas_state.mirror_preview_layer();
                if line_editing_mask {
                    self.commit_preview_to_layer_mask(canvas_state, false);
                } else {
                    self.commit_bezier_to_layer(canvas_state, secondary_color_f32);
                }
                let base_dirty = self.line_state.line_tool.last_bounds;
                let combined = match (base_dirty, mirror_bounds) {
                    (Some(b), Some(m)) => Some(b.union(m)),
                    (Some(b), None) => Some(b),
                    (None, Some(m)) => Some(m),
                    (None, None) => None,
                };
                if let Some(dirty) = combined {
                    canvas_state.mark_dirty(Some(dirty));
                } else {
                    self.mark_full_dirty(canvas_state);
                }
                canvas_state.clear_preview_state();
                self.line_state.line_tool.stage = LineStage::Idle;
                self.line_state.line_tool.last_bounds = None;
                stroke_event = auto_commit_event;
            }
            // Fill
            if self.active_tool == Tool::Fill && self.fill_state.active_fill.is_some() {
                self.commit_fill_preview(canvas_state);
                canvas_state.clear_preview_state();
                self.clear_fill_preview_state();
                canvas_state.mark_dirty(None);
            }
            // Gradient
            if self.active_tool == Tool::Gradient && self.gradient_state.drag_start.is_some() {
                self.gradient_state.commit_pending = true;
                self.gradient_state.commit_pending_frame = 0;
            }
            // Text
            if self.active_tool == Tool::Text && self.text_state.is_editing {
                if !self.text_state.text.is_empty() {
                    self.stroke_tracker
                        .start_preview_tool(canvas_state.active_layer_index, "Text");
                    self.text_state.commit_pending = true;
                    self.text_state.commit_pending_frame = 0;
                } else {
                    self.text_state.is_editing = false;
                    self.text_state.editing_text_layer = false;
                    self.text_state.origin = None;
                    canvas_state.text_editing_layer = None;
                    canvas_state.clear_preview_state();
                }
            }
            // Liquify
            if self.active_tool == Tool::Liquify && self.liquify_state.is_active {
                self.liquify_state.commit_pending = true;
                self.liquify_state.commit_pending_frame = 0;
            }
            // Mesh Warp
            if self.active_tool == Tool::MeshWarp && self.mesh_warp_state.is_active {
                self.mesh_warp_state.commit_pending = true;
                self.mesh_warp_state.commit_pending_frame = 0;
            }
            // Shape
            if self.active_tool == Tool::Shapes && self.shapes_state.placed.is_some() {
                if self.shapes_state.source_layer_index.is_none() {
                    self.shapes_state.source_layer_index = Some(prev_layer_index);
                }
                self.commit_shape(canvas_state);
            }

            self.last_tracked_layer_index = canvas_state.active_layer_index;
            self.last_tracked_layer_count = canvas_state.layers.len();

            // Auto-switch tool for text layers (also called from app.rs
            // immediately after layers panel for zero-delay switching).
            self.auto_switch_tool_for_layer(canvas_state);
        }

        // -- Deferred gradient commit --
        // When commit_pending is true, we defer the actual commit by one
        // frame so the loading bar ("Committing: Gradient") has a chance
        // to render before the synchronous work blocks the UI thread.
        if self.gradient_state.commit_pending {
            if self.gradient_state.commit_pending_frame == 0 {
                // Frame 0: loading bar will render; advance counter.
                self.gradient_state.commit_pending_frame = 1;
                ui.ctx().request_repaint();
                return;
            } else {
                // Frame 1+: actually commit now (loading bar is visible).
                self.gradient_state.commit_pending = false;
                self.gradient_state.commit_pending_frame = 0;
                self.commit_gradient(canvas_state);
                ui.ctx().request_repaint();
            }
        }

        // -- Deferred text commit --
        if self.text_state.commit_pending {
            if self.text_state.commit_pending_frame == 0 {
                self.text_state.commit_pending_frame = 1;
                ui.ctx().request_repaint();
                return;
            } else {
                self.text_state.commit_pending = false;
                self.text_state.commit_pending_frame = 0;
                self.commit_text(canvas_state);
                ui.ctx().request_repaint();
            }
        }

        // -- Deferred liquify commit --
        if self.liquify_state.commit_pending {
            if self.liquify_state.commit_pending_frame == 0 {
                self.liquify_state.commit_pending_frame = 1;
                ui.ctx().request_repaint();
                return;
            } else {
                self.liquify_state.commit_pending = false;
                self.liquify_state.commit_pending_frame = 0;
                self.commit_liquify(canvas_state);
                ui.ctx().request_repaint();
            }
        }

        // -- Deferred mesh warp commit --
        if self.mesh_warp_state.commit_pending {
            if self.mesh_warp_state.commit_pending_frame == 0 {
                self.mesh_warp_state.commit_pending_frame = 1;
                ui.ctx().request_repaint();
                return;
            } else {
                self.mesh_warp_state.commit_pending = false;
                self.mesh_warp_state.commit_pending_frame = 0;
                self.commit_mesh_warp(canvas_state);
                ui.ctx().request_repaint();
            }
        }

        // Auto-commit B├®zier line if tool changed away from Line tool
        if self.active_tool != Tool::Line && self.line_state.line_tool.stage == LineStage::Editing {
            let line_editing_mask = canvas_state.edit_layer_mask
                && canvas_state
                    .layers
                    .get(canvas_state.active_layer_index)
                    .is_some_and(|l| l.has_live_mask());
            // Capture "before" for undo (layer still unchanged, line is in preview)
            if let Some(final_bounds) = self.line_state.line_tool.last_bounds {
                self.stroke_tracker.expand_bounds(final_bounds);
            }
            let auto_commit_event = self.stroke_tracker.finish(canvas_state);

            let mirror_bounds = canvas_state.mirror_preview_layer();
            if line_editing_mask {
                self.commit_preview_to_layer_mask(canvas_state, false);
            } else {
                self.commit_bezier_to_layer(canvas_state, secondary_color_f32);
            }

            // Mark dirty: combine original line bounds with mirrored bounds
            let base_dirty = self.line_state.line_tool.last_bounds;
            let combined = match (base_dirty, mirror_bounds) {
                (Some(b), Some(m)) => Some(b.union(m)),
                (Some(b), None) => Some(b),
                (None, Some(m)) => Some(m),
                (None, None) => None,
            };
            if let Some(dirty) = combined {
                canvas_state.mark_dirty(Some(dirty));
            } else {
                self.mark_full_dirty(canvas_state);
            }

            canvas_state.clear_preview_state();
            self.line_state.line_tool.stage = LineStage::Idle;
            self.line_state.line_tool.last_bounds = None; // Reset bounds

            // Store the event for pickup by app.rs
            stroke_event = auto_commit_event;
        }

        // Auto-commit Fill if tool changed away from Fill tool
        if self.active_tool != Tool::Fill && self.fill_state.active_fill.is_some() {
            self.commit_fill_preview(canvas_state);
            canvas_state.clear_preview_state();
            self.clear_fill_preview_state();
            canvas_state.mark_dirty(None);
        }

        // Auto-commit Magic Wand if tool changed away from Magic Wand tool
        if self.active_tool != Tool::MagicWand && self.magic_wand_has_session() {
            self.finalize_magic_wand_session(canvas_state);
            canvas_state.mark_dirty(None);
        }

        // Auto-commit Gradient if tool changed away from Gradient tool
        if self.active_tool != Tool::Gradient && self.gradient_state.drag_start.is_some() {
            self.gradient_state.commit_pending = true;
            self.gradient_state.commit_pending_frame = 0;
        }

        // Auto-commit Text if tool changed away
        if self.active_tool != Tool::Text && self.text_state.is_editing {
            if !self.text_state.text.is_empty() {
                self.stroke_tracker
                    .start_preview_tool(canvas_state.active_layer_index, "Text");
                self.commit_text(canvas_state);
            } else {
                self.text_state.is_editing = false;
                self.text_state.editing_text_layer = false;
                self.text_state.origin = None;
                canvas_state.text_editing_layer = None;
                canvas_state.clear_preview_state();
            }
        }

        // Auto-commit Liquify if tool changed away
        if self.active_tool != Tool::Liquify && self.liquify_state.is_active {
            self.liquify_state.commit_pending = true;
            self.liquify_state.commit_pending_frame = 0;
        }

        // Auto-commit Mesh Warp if tool changed away
        if self.active_tool != Tool::MeshWarp && self.mesh_warp_state.is_active {
            self.mesh_warp_state.commit_pending = true;
            self.mesh_warp_state.commit_pending_frame = 0;
        }

        // Auto-commit Shape if tool changed away
        if self.active_tool != Tool::Shapes && self.shapes_state.placed.is_some() {
            self.commit_shape(canvas_state);
        }

        // On Wayland the stylus may fire Touch events instead of Pointer events,
        // so augment each pointer query with a Touch-event fallback.
        let is_primary_down = ui.input(|i| {
            i.pointer.primary_down()
                || i.events.iter().any(|e| {
                    matches!(
                        e,
                        egui::Event::Touch {
                            phase: egui::TouchPhase::Start | egui::TouchPhase::Move,
                            ..
                        }
                    )
                })
        });
        let is_primary_released = ui.input(|i| {
            i.pointer.primary_released()
                || i.events.iter().any(|e| {
                    matches!(
                        e,
                        egui::Event::Touch {
                            phase: egui::TouchPhase::End,
                            ..
                        }
                    )
                })
        });
        let is_primary_clicked = ui.input(|i| i.pointer.primary_clicked());
        let is_primary_pressed = ui.input(|i| {
            i.pointer.primary_pressed()
                || i.events.iter().any(|e| {
                    matches!(
                        e,
                        egui::Event::Touch {
                            phase: egui::TouchPhase::Start,
                            ..
                        }
                    )
                })
        }); // Just pressed this frame
        let is_secondary_down = ui.input(|i| i.pointer.secondary_down());
        let is_secondary_pressed =
            ui.input(|i| i.pointer.button_pressed(egui::PointerButton::Secondary));
        let is_secondary_released = ui.input(|i| i.pointer.secondary_released());
        let is_secondary_clicked = ui.input(|i| i.pointer.secondary_clicked());
        let shift_held = ui.input(|i| i.modifiers.shift);
        let enter_pressed =
            ui.input(|i| i.key_pressed(egui::Key::Enter)) || self.injected_enter_pressed;
        let escape_pressed_global =
            ui.input(|i| i.key_pressed(egui::Key::Escape)) || self.injected_escape_pressed;
        self.injected_enter_pressed = false;
        self.injected_escape_pressed = false;

        // Read pen/touch pressure from egui touch events (Apple Pencil, Wacom, etc.)
        // Uses the latest Touch event's force value; falls back to 1.0 (no pen).
        let touch_pressure = ui.input(|i| {
            let mut pressure = None;
            for ev in &i.events {
                if let egui::Event::Touch {
                    force: Some(f),
                    phase,
                    ..
                } = ev
                    && matches!(phase, egui::TouchPhase::Start | egui::TouchPhase::Move)
                {
                    pressure = Some(*f);
                }
            }
            pressure
        });
        if let Some(p) = touch_pressure {
            self.tool_state.current_pressure = p.clamp(0.0, 1.0);
        } else if !is_primary_down && !is_secondary_down {
            // Reset to full pressure when not actively drawing
            self.tool_state.current_pressure = 1.0;
        }

        match self.active_tool {
            Tool::Brush | Tool::Eraser | Tool::Pencil => {
                // Guard: auto-rasterize text layers before destructive drawing
                if (is_primary_pressed || is_secondary_down)
                    && let Some(layer) = canvas_state.layers.get(canvas_state.active_layer_index)
                    && layer.is_text_layer()
                {
                    self.pending_auto_rasterize = Some(canvas_state.active_layer_index);
                    // Return early ÔÇö app.rs will rasterize and we'll continue next frame
                    return;
                }

                let is_painting = is_primary_down || is_secondary_down;
                let editing_mask = canvas_state.edit_layer_mask
                    && canvas_state
                        .layers
                        .get(canvas_state.active_layer_index)
                        .is_some_and(|l| l.has_live_mask());
                let resize_modifier_held =
                    self.active_tool == Tool::Brush && self.brush_resize_drag_modifier_held(ui);
                let resize_drag_threshold = 4.0;

                if self.active_tool == Tool::Brush
                    && is_primary_pressed
                    && resize_modifier_held
                    && let Some((x, y)) = canvas_pos_unclamped
                        .or(canvas_pos_f32)
                        .or(canvas_pos.map(|(x, y)| (x as f32, y as f32)))
                {
                    self.tool_state.brush_resize_drag_origin = Some(Pos2::new(x, y));
                    self.tool_state.brush_resize_drag_start_size = self.properties.size;
                    self.tool_state.brush_resize_drag_active = false;
                    self.reset_brush_pointer_state();
                    return;
                }

                if self.active_tool == Tool::Brush
                    && let Some(origin) = self.tool_state.brush_resize_drag_origin
                {
                    let current_drag_pos = canvas_pos_unclamped
                        .or(canvas_pos_f32)
                        .or(canvas_pos.map(|(x, y)| (x as f32, y as f32)));
                    if is_primary_down {
                        if let Some((x, y)) = current_drag_pos {
                            let current = Pos2::new(x, y);
                            let delta = current - origin;
                            if self.tool_state.brush_resize_drag_active
                                || delta.length() >= resize_drag_threshold
                            {
                                self.tool_state.brush_resize_drag_active = true;
                                self.properties.size =
                                    (self.tool_state.brush_resize_drag_start_size + delta.x)
                                        .clamp(1.0, 500.0);
                                self.reset_brush_pointer_state();
                                ui.ctx().request_repaint();
                            }
                        }
                        return;
                    }
                    if is_primary_released {
                        let was_active = self.tool_state.brush_resize_drag_active;
                        self.tool_state.brush_resize_drag_origin = None;
                        self.tool_state.brush_resize_drag_active = false;
                        if was_active {
                            return;
                        }
                        if resize_modifier_held
                            && self.tool_state.last_brush_pos.is_some()
                            && let (Some(last), Some(current)) =
                                (self.tool_state.last_brush_pos, canvas_pos)
                        {
                            stroke_event = self.commit_brush_straight_line(
                                canvas_state,
                                last,
                                current,
                                primary_color_f32,
                                secondary_color_f32,
                            );
                            if stroke_event.is_some() {
                                self.pending_stroke_event = stroke_event;
                            }
                            self.tool_state.last_brush_pos = Some(current);
                            self.tool_state.last_precise_pos =
                                Some(Pos2::new(current.0 as f32, current.1 as f32));
                            ui.ctx().request_repaint();
                            return;
                        }
                    }
                }

                // TASK 2: Shift+Click straight line
                // Trigger when mouse is pressed (not dragged) with Shift held
                if is_primary_pressed
                    && shift_held
                    && !resize_modifier_held
                    && self.tool_state.last_brush_pos.is_some()
                    && let (Some(last), Some(current)) =
                        (self.tool_state.last_brush_pos, canvas_pos)
                {
                    stroke_event = self.commit_brush_straight_line(
                        canvas_state,
                        last,
                        current,
                        primary_color_f32,
                        secondary_color_f32,
                    );

                    // Store stroke event for app.rs to pick up (before returning)
                    if stroke_event.is_some() {
                        self.pending_stroke_event = stroke_event;
                    }

                    self.tool_state.last_brush_pos = Some(current);
                    self.tool_state.last_precise_pos =
                        Some(Pos2::new(current.0 as f32, current.1 as f32));
                    ui.ctx().request_repaint();
                    return; // Don't process as normal painting
                }

                if is_painting {
                    // Use float position for smooth sub-pixel drawing; fall back
                    // to integer pos converted to float if unavailable.
                    let current_f32 =
                        canvas_pos_f32.or_else(|| canvas_pos.map(|(x, y)| (x as f32, y as f32)));
                    if let (Some(cf), Some(current_pos)) = (current_f32, canvas_pos) {
                        // Initialize on first paint
                        if self.tool_state.last_pos.is_none() {
                            self.tool_state.using_secondary_color = is_secondary_down;
                            self.tool_state.last_precise_pos = Some(Pos2::new(cf.0, cf.1));
                            self.tool_state.distance_remainder = 0.0;
                            self.tool_state.smooth_pos = Some(Pos2::new(cf.0, cf.1));

                            // Start stroke tracking for Undo/Redo
                            let is_eraser = self.active_tool == Tool::Eraser;
                            if editing_mask {
                                let layer_idx = canvas_state.active_layer_index;
                                let (before_mask, before_enabled) = canvas_state
                                    .layers
                                    .get(layer_idx)
                                    .map(|l| (l.mask.clone(), l.mask_enabled))
                                    .unwrap_or((None, true));
                                self.stroke_tracker.start_preview_mask_tool(
                                    layer_idx,
                                    if is_eraser {
                                        "Layer Mask Erase"
                                    } else {
                                        "Layer Mask Paint"
                                    },
                                    before_mask,
                                    before_enabled,
                                );
                            } else if is_eraser {
                                // Eraser uses preview layer as an erase mask ÔÇö capture before at commit time
                                self.stroke_tracker.start_preview_tool(
                                    canvas_state.active_layer_index,
                                    "Eraser Stroke",
                                );
                            } else {
                                // Brush/Pencil uses preview layer - we'll capture before right before commit
                                let description = if self.active_tool == Tool::Pencil {
                                    "Pencil Stroke"
                                } else {
                                    "Brush Stroke"
                                };
                                self.stroke_tracker.start_preview_tool(
                                    canvas_state.active_layer_index,
                                    description,
                                );
                            }

                            // Initialize/clear preview layer for stroke compositing (all tools: Brush, Pencil, Eraser)
                            if canvas_state.preview_layer.is_none()
                                || canvas_state.preview_layer.as_ref().unwrap().width()
                                    != canvas_state.width
                                || canvas_state.preview_layer.as_ref().unwrap().height()
                                    != canvas_state.height
                            {
                                canvas_state.preview_layer =
                                    Some(TiledImage::new(canvas_state.width, canvas_state.height));
                            } else {
                                // Clear existing preview layer
                                if let Some(ref mut preview) = canvas_state.preview_layer {
                                    preview.clear();
                                }
                            }
                            if is_eraser {
                                // Mark preview as eraser mask mode
                                canvas_state.preview_is_eraser = true;
                                canvas_state.preview_force_composite = true;
                                canvas_state.preview_targets_mask = false;
                                canvas_state.preview_mask_reveal = false;
                            } else {
                                // Set preview blend mode to match tool's blending mode
                                canvas_state.preview_blend_mode = self.properties.blending_mode;
                                canvas_state.preview_targets_mask = false;
                                canvas_state.preview_mask_reveal = false;
                            }

                            if editing_mask {
                                // In mask edit mode, preview alpha represents conceal/reveal delta.
                                canvas_state.preview_force_composite = true;
                                canvas_state.preview_targets_mask = true;
                                canvas_state.preview_mask_reveal = is_eraser;
                                canvas_state.preview_is_eraser = false;
                            }
                        }

                        let is_eraser = self.active_tool == Tool::Eraser;
                        let use_secondary = self.tool_state.using_secondary_color;

                        // ============================================================
                        // SUB-FRAME EVENT PROCESSING
                        // Process ALL PointerMoved events that occurred since last
                        // frame. At 21 FPS with 1000Hz mouse, this captures ~48
                        // intermediate positions per frame instead of just 1.
                        // All painting is CPU-only; GPU upload happens ONCE at the end.
                        // ============================================================

                        // Build the list of positions to paint through this frame.
                        // Use raw_motion_events if available, otherwise fall back
                        // to the single current position.
                        let positions: Vec<(f32, f32)> = if !raw_motion_events.is_empty() {
                            raw_motion_events.to_vec()
                        } else {
                            vec![cf]
                        };

                        // ============================================================
                        // SPEED-ADAPTIVE EMA SMOOTHING
                        // Applies an exponential moving average to each raw
                        // mouse position before painting.  This directly rounds
                        // off angular corners caused by straight-line segments
                        // between sparse/distant mouse samples.
                        //
                        // Speed-adaptive alpha:
                        //   Close movement (< 1.5 px) ÔåÆ alpha = 1.0  (raw, precise)
                        //   Far movement   (> ~20 px) ÔåÆ alpha Ôëê 0.55 (strong smoothing)
                        //
                        // At 1000 Hz sub-frame input the per-sample distance is
                        // small, so the smoothing is gentle ÔÇö but it accumulates
                        // across several consecutive direction changes, naturally
                        // rounding corners.  At frame-rate input (big jumps) the
                        // smoothing is stronger, eliminating visible polygon edges.
                        // ============================================================
                        let smoothed_positions: Vec<(f32, f32)> = {
                            let mut result = Vec::with_capacity(positions.len());
                            for &pos in &positions {
                                let raw = Pos2::new(pos.0, pos.1);
                                let smoothed = if let Some(prev) = self.tool_state.smooth_pos {
                                    let dx = raw.x - prev.x;
                                    let dy = raw.y - prev.y;
                                    let dist = (dx * dx + dy * dy).sqrt();
                                    // Speed-adaptive alpha:
                                    // dist < 1.5 ÔåÆ 1.0  (no smoothing)
                                    // dist ÔåÆ Ôê×   ÔåÆ 0.55 (max smoothing)
                                    let alpha = if dist < 1.5 {
                                        1.0
                                    } else {
                                        (0.55 + 1.8 / (dist + 1.8)).min(1.0)
                                    };
                                    Pos2::new(prev.x + alpha * dx, prev.y + alpha * dy)
                                } else {
                                    raw
                                };
                                self.tool_state.smooth_pos = Some(smoothed);
                                result.push((smoothed.x, smoothed.y));
                            }
                            result
                        };

                        // Accumulate a single dirty rect for the entire frame
                        let mut frame_dirty_rect = Rect::NOTHING;

                        // Mirror mode: generate mirrored positions for drawing
                        let mirror = canvas_state.mirror_mode;
                        let mw = canvas_state.width;
                        let mh = canvas_state.height;

                        for &pos in &smoothed_positions {
                            // Generate all positions (original + mirrored)
                            let positions_to_draw = mirror.mirror_positions(pos.0, pos.1, mw, mh);

                            // Also mirror the start position for line drawing
                            let start_precise = self.tool_state.last_precise_pos;

                            for &mpos in positions_to_draw.iter() {
                                // CPU-only paint step: lerp from last_precise_pos ÔåÆ pos
                                let modified_rect = if self.active_tool == Tool::Pencil {
                                    if let Some(start_p) = start_precise {
                                        let start_mirrors =
                                            mirror.mirror_positions(start_p.x, start_p.y, mw, mh);
                                        let arm_idx = positions_to_draw
                                            .iter()
                                            .position(|p| *p == mpos)
                                            .unwrap_or(0);
                                        let start_m = if arm_idx < start_mirrors.len {
                                            start_mirrors.data[arm_idx]
                                        } else {
                                            (start_p.x, start_p.y)
                                        };
                                        self.draw_pixel_line_and_get_bounds(
                                            canvas_state,
                                            start_m,
                                            mpos,
                                            use_secondary,
                                            primary_color_f32,
                                            secondary_color_f32,
                                        )
                                    } else {
                                        self.draw_pixel_and_get_bounds(
                                            canvas_state,
                                            mpos,
                                            use_secondary,
                                            primary_color_f32,
                                            secondary_color_f32,
                                        )
                                    }
                                } else if let Some(start_p) = start_precise {
                                    // Mirror the start point to match this arm
                                    let start_mirrors =
                                        mirror.mirror_positions(start_p.x, start_p.y, mw, mh);
                                    let arm_idx = positions_to_draw
                                        .iter()
                                        .position(|p| *p == mpos)
                                        .unwrap_or(0);
                                    let start_m = if arm_idx < start_mirrors.len {
                                        start_mirrors.data[arm_idx]
                                    } else {
                                        (start_p.x, start_p.y)
                                    };
                                    self.draw_line_and_get_bounds(
                                        canvas_state,
                                        start_m,
                                        mpos,
                                        is_eraser,
                                        use_secondary,
                                        primary_color_f32,
                                        secondary_color_f32,
                                    )
                                } else {
                                    self.draw_circle_and_get_bounds(
                                        canvas_state,
                                        mpos,
                                        is_eraser,
                                        use_secondary,
                                        primary_color_f32,
                                        secondary_color_f32,
                                    )
                                };

                                // Grow the frame's union dirty rect (CPU only, no GPU calls)
                                frame_dirty_rect = frame_dirty_rect.union(modified_rect);
                            }

                            // Update last position for next paint step (always track the original, not mirrored)
                            self.tool_state.last_precise_pos = Some(Pos2::new(pos.0, pos.1));
                        }

                        // Track stroke bounds for undo/redo
                        self.stroke_tracker.expand_bounds(frame_dirty_rect);

                        // ============================================================
                        // SINGLE GPU UPLOAD: Mark dirty ONCE for all sub-frame events
                        // ============================================================
                        if frame_dirty_rect.is_positive() {
                            if editing_mask {
                                // Keep edits in preview during drag; commit once on release.
                                // This avoids per-frame mask writes + full masked-layer uploads.
                                canvas_state.mark_preview_changed_rect(frame_dirty_rect);
                            } else {
                                // All tools (Brush, Pencil, Eraser) use the preview path now
                                canvas_state.mark_preview_changed_rect(frame_dirty_rect);
                            }
                        }

                        self.tool_state.last_pos = Some(current_pos);
                        self.tool_state.last_brush_pos = Some(current_pos);
                        ui.ctx().request_repaint();
                    }
                } else {
                    // Mouse released - commit and reset state
                    if self.tool_state.last_pos.is_some() {
                        let is_eraser = self.active_tool == Tool::Eraser;

                        // Capture "before" NOW (layer still unchanged), then commit
                        // Finish stroke tracking BEFORE commit - this captures "before" from unchanged layer
                        stroke_event = self.stroke_tracker.finish(canvas_state);

                        if editing_mask {
                            // Commit mask once at stroke end for smooth interactive dragging.
                            self.commit_preview_to_layer_mask(canvas_state, is_eraser);
                            canvas_state.clear_preview_state();
                        } else if is_eraser {
                            // Commit the eraser mask to the active layer
                            self.commit_eraser_to_layer(canvas_state);
                        } else {
                            // Commit the preview layer to the active layer (Brush/Pencil)
                            self.commit_bezier_to_layer(canvas_state, primary_color_f32);
                        }
                        // Clear preview layer
                        canvas_state.clear_preview_state();
                        // Mark only stroke bounds dirty (not full canvas)
                        if let Some(ref ev) = stroke_event {
                            canvas_state.mark_dirty(Some(ev.bounds.expand(12.0)));
                        } else {
                            self.mark_full_dirty(canvas_state);
                        }
                    }

                    self.reset_brush_pointer_state();
                }
            }
            Tool::Line => {
                let line_editing_mask = canvas_state.edit_layer_mask
                    && canvas_state
                        .layers
                        .get(canvas_state.active_layer_index)
                        .is_some_and(|l| l.has_live_mask());

                // Guard: auto-rasterize text layers before destructive line tool
                if is_primary_pressed
                    && let Some(layer) = canvas_state.layers.get(canvas_state.active_layer_index)
                    && layer.is_text_layer()
                {
                    self.pending_auto_rasterize = Some(canvas_state.active_layer_index);
                    return;
                }

                // TASK 1: Advanced B├®zier Line Tool
                match self.line_state.line_tool.stage {
                    LineStage::Idle => {
                        // Clear the require_mouse_release flag when mouse is released
                        if is_primary_released {
                            self.line_state.line_tool.require_mouse_release = false;
                        }

                        if !self.line_state.line_tool.require_mouse_release {
                            // Normal behavior: start line on click (or click and drag)
                            if is_primary_pressed && let Some(pos) = canvas_pos {
                                let pos2 = Pos2::new(pos.0 as f32, pos.1 as f32);
                                self.line_state.line_tool.control_points = [pos2; 4];
                                self.line_state.line_tool.stage = LineStage::Dragging;
                                self.line_state.line_tool.last_bounds = None;
                            }
                        } else {
                            // After committing a line: require drag or release+click
                            if is_primary_pressed && let Some(pos) = canvas_pos {
                                // Store initial position to detect drag
                                let pos2 = Pos2::new(pos.0 as f32, pos.1 as f32);
                                self.line_state.line_tool.initial_mouse_pos = Some(pos2);
                                self.line_state.line_tool.control_points = [pos2; 4];
                            }

                            // Transition to Dragging only if the mouse moved while pressed
                            if is_primary_down
                                && self.line_state.line_tool.initial_mouse_pos.is_some()
                                && let Some(pos) = canvas_pos
                            {
                                let current_pos = Pos2::new(pos.0 as f32, pos.1 as f32);
                                let initial_pos =
                                    self.line_state.line_tool.initial_mouse_pos.unwrap();

                                // Check if mouse moved enough to be considered a drag (more than 2 pixels)
                                if (current_pos - initial_pos).length() > 2.0 {
                                    self.line_state.line_tool.stage = LineStage::Dragging;
                                    self.line_state.line_tool.last_bounds = None;
                                    self.line_state.line_tool.require_mouse_release = false;
                                }
                            }

                            // If mouse was released without dragging, reset
                            if is_primary_released
                                && self.line_state.line_tool.initial_mouse_pos.is_some()
                            {
                                self.line_state.line_tool.initial_mouse_pos = None;
                            }
                        }
                    }
                    LineStage::Dragging => {
                        if let Some(pos) = canvas_pos {
                            let raw_pos = Pos2::new(pos.0 as f32, pos.1 as f32);
                            let p0 = self.line_state.line_tool.control_points[0];
                            // Snap to nearest 45┬░ angle when Shift is held
                            let pos2 = if shift_held {
                                let dx = raw_pos.x - p0.x;
                                let dy = raw_pos.y - p0.y;
                                let angle = dy.atan2(dx);
                                let snap = std::f32::consts::PI / 4.0; // 45┬░
                                let snapped_angle = (angle / snap).round() * snap;
                                let dist = (dx * dx + dy * dy).sqrt();
                                Pos2::new(
                                    p0.x + dist * snapped_angle.cos(),
                                    p0.y + dist * snapped_angle.sin(),
                                )
                            } else {
                                raw_pos
                            };
                            // Update end point
                            self.line_state.line_tool.control_points[3] = pos2;
                            let midpoint = Pos2::new((p0.x + pos2.x) / 2.0, (p0.y + pos2.y) / 2.0);
                            self.line_state.line_tool.control_points[1] = midpoint;
                            self.line_state.line_tool.control_points[2] = midpoint;

                            // OPTIMIZATION: Calculate bounds and use smart dirty rects
                            let current_bounds = self.get_bezier_bounds(
                                self.line_state.line_tool.control_points,
                                canvas_state.width,
                                canvas_state.height,
                            );

                            // Rasterize preview with focused clearing
                            self.rasterize_bezier(
                                canvas_state,
                                self.line_state.line_tool.control_points,
                                self.properties.color,
                                self.line_state.line_tool.options.pattern,
                                self.line_state.line_tool.options.cap_style,
                                self.line_state.line_tool.last_bounds,
                            );

                            // Preview layer changed - use combined bounds for partial GPU upload
                            let dirty_bounds = match self.line_state.line_tool.last_bounds {
                                Some(last) => last.union(current_bounds),
                                None => current_bounds,
                            };
                            canvas_state.mark_preview_changed_rect(dirty_bounds);

                            // Track bounds for next frame
                            self.line_state.line_tool.last_bounds = Some(current_bounds);
                        }

                        if is_primary_released {
                            // Enter editing stage - start tracking for undo/redo
                            self.line_state.line_tool.stage = LineStage::Editing;

                            // Initialize tracking values for change detection
                            self.line_state.line_tool.last_size = self.properties.size;
                            self.line_state.line_tool.last_pattern =
                                self.line_state.line_tool.options.pattern;
                            self.line_state.line_tool.last_cap_style =
                                self.line_state.line_tool.options.cap_style;
                            self.line_state.line_tool.last_end_shape =
                                self.line_state.line_tool.options.end_shape;
                            self.line_state.line_tool.last_arrow_side =
                                self.line_state.line_tool.options.arrow_side;
                            self.line_state.line_tool.last_anti_alias =
                                self.line_state.line_tool.options.anti_alias;

                            // Start stroke tracking (line uses preview layer like Brush)
                            if line_editing_mask {
                                let layer_idx = canvas_state.active_layer_index;
                                let (before_mask, before_enabled) = canvas_state
                                    .layers
                                    .get(layer_idx)
                                    .map(|l| (l.mask.clone(), l.mask_enabled))
                                    .unwrap_or((None, true));
                                self.stroke_tracker.start_preview_mask_tool(
                                    layer_idx,
                                    "Line Mask",
                                    before_mask,
                                    before_enabled,
                                );
                            } else {
                                self.stroke_tracker
                                    .start_preview_tool(canvas_state.active_layer_index, "Line");
                            }
                            if let Some(bounds) = self.line_state.line_tool.last_bounds {
                                self.stroke_tracker.expand_bounds(bounds);
                            }
                        }
                    }
                    LineStage::Editing => {
                        let mouse_pos = ui.input(|i| i.pointer.interact_pos());

                        // Check if settings changed and re-render if needed
                        let settings_changed =
                            (self.properties.size - self.line_state.line_tool.last_size).abs()
                                > 0.1
                                || self.line_state.line_tool.options.pattern
                                    != self.line_state.line_tool.last_pattern
                                || self.line_state.line_tool.options.cap_style
                                    != self.line_state.line_tool.last_cap_style
                                || self.line_state.line_tool.options.end_shape
                                    != self.line_state.line_tool.last_end_shape
                                || self.line_state.line_tool.options.arrow_side
                                    != self.line_state.line_tool.last_arrow_side
                                || self.line_state.line_tool.options.anti_alias
                                    != self.line_state.line_tool.last_anti_alias;

                        if settings_changed {
                            // Update tracked values
                            self.line_state.line_tool.last_size = self.properties.size;
                            self.line_state.line_tool.last_pattern =
                                self.line_state.line_tool.options.pattern;
                            self.line_state.line_tool.last_cap_style =
                                self.line_state.line_tool.options.cap_style;
                            self.line_state.line_tool.last_end_shape =
                                self.line_state.line_tool.options.end_shape;
                            self.line_state.line_tool.last_arrow_side =
                                self.line_state.line_tool.options.arrow_side;
                            self.line_state.line_tool.last_anti_alias =
                                self.line_state.line_tool.options.anti_alias;

                            // Re-calculate bounds
                            let current_bounds = self.get_bezier_bounds(
                                self.line_state.line_tool.control_points,
                                canvas_state.width,
                                canvas_state.height,
                            );

                            // Re-rasterize with new settings
                            self.rasterize_bezier(
                                canvas_state,
                                self.line_state.line_tool.control_points,
                                self.properties.color,
                                self.line_state.line_tool.options.pattern,
                                self.line_state.line_tool.options.cap_style,
                                self.line_state.line_tool.last_bounds,
                            );

                            // Mark dirty for preview update
                            let dirty_bounds = match self.line_state.line_tool.last_bounds {
                                Some(last) => last.union(current_bounds),
                                None => current_bounds,
                            };
                            canvas_state.mark_preview_changed_rect(dirty_bounds);
                            self.line_state.line_tool.last_bounds = Some(current_bounds);

                            ui.ctx().request_repaint();
                        }

                        // Handle Enter key or tool change to commit
                        if enter_pressed {
                            // Capture "before" for undo (layer still unchanged, line is in preview)
                            if let Some(final_bounds) = self.line_state.line_tool.last_bounds {
                                self.stroke_tracker.expand_bounds(final_bounds);
                            }
                            stroke_event = self.stroke_tracker.finish(canvas_state);

                            // Commit the line to the actual layer
                            let mirror_bounds = canvas_state.mirror_preview_layer();
                            if line_editing_mask {
                                self.commit_preview_to_layer_mask(canvas_state, false);
                            } else {
                                self.commit_bezier_to_layer(canvas_state, secondary_color_f32);
                            }

                            // Mark dirty: combine original line bounds with mirrored bounds
                            let base_dirty = self.line_state.line_tool.last_bounds;
                            let combined = match (base_dirty, mirror_bounds) {
                                (Some(b), Some(m)) => Some(b.union(m)),
                                (Some(b), None) => Some(b),
                                (None, Some(m)) => Some(m),
                                (None, None) => None,
                            };
                            if let Some(dirty) = combined {
                                canvas_state.mark_dirty(Some(dirty));
                            } else {
                                self.mark_full_dirty(canvas_state);
                            }

                            canvas_state.clear_preview_state();
                            self.line_state.line_tool.stage = LineStage::Idle;
                            self.line_state.line_tool.last_bounds = None; // Reset bounds
                            self.line_state.line_tool.require_mouse_release = false; // Allow new line after Enter
                            self.line_state.line_tool.initial_mouse_pos = None;
                            ui.ctx().request_repaint();
                        } else if let Some(screen_pos) = mouse_pos {
                            let handle_radius = 8.0; // Screen pixels

                            // Check if starting to drag a handle
                            // -- Compute pan handle screen position --
                            // Pan handle sits 22px above-left of P0 in screen space so it
                            // doesn't overlap the start (green) endpoint handle.
                            let p0_screen = self.canvas_pos2_to_screen(
                                self.line_state.line_tool.control_points[0],
                                canvas_rect,
                                zoom,
                            );
                            let pan_offset_screen = egui::Vec2::new(-22.0, -22.0);
                            let pan_handle_screen = p0_screen + pan_offset_screen;
                            let pan_hit_radius = 10.0f32;

                            // -- Hover tracking for pan handle (cursor icon) --
                            let hovering_pan =
                                (pan_handle_screen - screen_pos).length() < pan_hit_radius;
                            self.line_state.line_tool.pan_handle_hovering = hovering_pan;

                            if is_primary_down
                                && self.line_state.line_tool.dragging_handle.is_none()
                                && !self.line_state.line_tool.pan_handle_dragging
                            {
                                // -- Check pan handle first --
                                if is_primary_pressed && hovering_pan {
                                    let canvas_pos_now =
                                        self.screen_to_canvas_pos2(screen_pos, canvas_rect, zoom);
                                    self.line_state.line_tool.pan_handle_dragging = true;
                                    self.line_state.line_tool.pan_drag_canvas_start =
                                        Some(canvas_pos_now);
                                } else {
                                    // -- Check individual endpoint / control handles --
                                    let mut clicked_handle = false;
                                    for i in 0..4 {
                                        let handle_screen = self.canvas_pos2_to_screen(
                                            self.line_state.line_tool.control_points[i],
                                            canvas_rect,
                                            zoom,
                                        );
                                        if (handle_screen - screen_pos).length() < handle_radius {
                                            self.line_state.line_tool.dragging_handle = Some(i);
                                            clicked_handle = true;
                                            break;
                                        }
                                    }

                                    // If clicked but didn't hit any handle, commit current line
                                    if !clicked_handle && is_primary_pressed {
                                        // Capture "before" for undo (layer still unchanged, line is in preview)
                                        if let Some(final_bounds) =
                                            self.line_state.line_tool.last_bounds
                                        {
                                            self.stroke_tracker.expand_bounds(final_bounds);
                                        }
                                        stroke_event = self.stroke_tracker.finish(canvas_state);

                                        // Commit the current line
                                        let mirror_bounds = canvas_state.mirror_preview_layer();
                                        if line_editing_mask {
                                            self.commit_preview_to_layer_mask(canvas_state, false);
                                        } else {
                                            self.commit_bezier_to_layer(
                                                canvas_state,
                                                secondary_color_f32,
                                            );
                                        }

                                        // Mark dirty: combine original line bounds with mirrored bounds
                                        let base_dirty = self.line_state.line_tool.last_bounds;
                                        let combined = match (base_dirty, mirror_bounds) {
                                            (Some(b), Some(m)) => Some(b.union(m)),
                                            (Some(b), None) => Some(b),
                                            (None, Some(m)) => Some(m),
                                            (None, None) => None,
                                        };
                                        if let Some(dirty) = combined {
                                            canvas_state.mark_dirty(Some(dirty));
                                        } else {
                                            self.mark_full_dirty(canvas_state);
                                        }

                                        canvas_state.clear_preview_state();
                                        self.line_state.line_tool.stage = LineStage::Idle;
                                        self.line_state.line_tool.last_bounds = None;
                                        self.line_state.line_tool.require_mouse_release = true;
                                        self.line_state.line_tool.initial_mouse_pos = None;
                                        self.line_state.line_tool.pan_handle_hovering = false;

                                        ui.ctx().request_repaint();
                                    }
                                }
                            }

                            // -- Pan drag: translate all 4 control points together --
                            if is_primary_down && self.line_state.line_tool.pan_handle_dragging {
                                let canvas_pos_now =
                                    self.screen_to_canvas_pos2(screen_pos, canvas_rect, zoom);
                                if let Some(drag_start) =
                                    self.line_state.line_tool.pan_drag_canvas_start
                                {
                                    let delta = canvas_pos_now - drag_start;
                                    if delta.length() > 0.0 {
                                        for pt in
                                            self.line_state.line_tool.control_points.iter_mut()
                                        {
                                            pt.x = (pt.x + delta.x)
                                                .clamp(0.0, canvas_state.width as f32 - 1.0);
                                            pt.y = (pt.y + delta.y)
                                                .clamp(0.0, canvas_state.height as f32 - 1.0);
                                        }
                                        self.line_state.line_tool.pan_drag_canvas_start =
                                            Some(canvas_pos_now);

                                        let current_bounds = self.get_bezier_bounds(
                                            self.line_state.line_tool.control_points,
                                            canvas_state.width,
                                            canvas_state.height,
                                        );
                                        self.stroke_tracker.expand_bounds(current_bounds);
                                        self.rasterize_bezier(
                                            canvas_state,
                                            self.line_state.line_tool.control_points,
                                            self.properties.color,
                                            self.line_state.line_tool.options.pattern,
                                            self.line_state.line_tool.options.cap_style,
                                            self.line_state.line_tool.last_bounds,
                                        );
                                        let dirty_bounds =
                                            match self.line_state.line_tool.last_bounds {
                                                Some(last) => last.union(current_bounds),
                                                None => current_bounds,
                                            };
                                        canvas_state.mark_preview_changed_rect(dirty_bounds);
                                        self.line_state.line_tool.last_bounds =
                                            Some(current_bounds);
                                        ui.ctx().request_repaint();
                                    }
                                }
                            }

                            // -- Individual handle drag --
                            if is_primary_down
                                && !self.line_state.line_tool.pan_handle_dragging
                                && let Some(handle_idx) = self.line_state.line_tool.dragging_handle
                            {
                                let canvas_pos_float =
                                    self.screen_to_canvas_pos2(screen_pos, canvas_rect, zoom);
                                // Clamp to canvas bounds
                                let clamped = Pos2::new(
                                    canvas_pos_float
                                        .x
                                        .clamp(0.0, canvas_state.width as f32 - 1.0),
                                    canvas_pos_float
                                        .y
                                        .clamp(0.0, canvas_state.height as f32 - 1.0),
                                );
                                self.line_state.line_tool.control_points[handle_idx] = clamped;

                                // OPTIMIZATION: Calculate bounds and use smart dirty rects
                                let current_bounds = self.get_bezier_bounds(
                                    self.line_state.line_tool.control_points,
                                    canvas_state.width,
                                    canvas_state.height,
                                );

                                // Track bounds for undo/redo
                                self.stroke_tracker.expand_bounds(current_bounds);

                                // Re-rasterize the curve with focused clearing
                                self.rasterize_bezier(
                                    canvas_state,
                                    self.line_state.line_tool.control_points,
                                    self.properties.color,
                                    self.line_state.line_tool.options.pattern,
                                    self.line_state.line_tool.options.cap_style,
                                    self.line_state.line_tool.last_bounds,
                                );

                                // Preview layer changed - use combined bounds for partial GPU upload
                                let dirty_bounds = match self.line_state.line_tool.last_bounds {
                                    Some(last) => last.union(current_bounds),
                                    None => current_bounds,
                                };
                                canvas_state.mark_preview_changed_rect(dirty_bounds);

                                // Track bounds for next frame
                                self.line_state.line_tool.last_bounds = Some(current_bounds);

                                ui.ctx().request_repaint();
                            }

                            // -- Release all handles --------------------------------------------------
                            if is_primary_released {
                                self.line_state.line_tool.dragging_handle = None;
                                self.line_state.line_tool.pan_handle_dragging = false;
                                self.line_state.line_tool.pan_drag_canvas_start = None;
                            }

                            // Draw handles and control lines
                            self.draw_bezier_handles(painter, canvas_rect, zoom);
                        }
                    }
                }
            }

            Tool::RectangleSelect | Tool::EllipseSelect => {
                let esc_pressed = escape_pressed_global;
                let alt_held = ui.input(|i| i.modifiers.alt);
                let ctrl_held = ui.input(|i| i.modifiers.command);
                let is_secondary_pressed =
                    ui.input(|i| i.pointer.button_pressed(egui::PointerButton::Secondary));

                // Esc or Enter: deselect
                if esc_pressed || enter_pressed {
                    canvas_state.clear_selection();
                    self.selection_state.dragging = false;
                    self.selection_state.drag_start = None;
                    self.selection_state.drag_end = None;
                    self.selection_state.right_click_drag = false;
                    canvas_state.mark_dirty(None);
                    ui.ctx().request_repaint();
                }

                // Start drag on primary OR secondary press.
                // At drag start, lock the effective mode from modifier keys:
                //   Shift+Alt ÔåÆ Intersect
                //   Ctrl      ÔåÆ Add
                //   Alt       ÔåÆ Subtract
                //   Right-click ÔåÆ Subtract (when context bar mode is Replace)
                //   else      ÔåÆ context bar mode
                if (is_primary_pressed || is_secondary_pressed)
                    && !self.selection_state.dragging
                    && let Some(pos_f) = canvas_pos_unclamped
                {
                    let pos2 = Pos2::new(pos_f.0, pos_f.1);
                    self.selection_state.dragging = true;
                    self.selection_state.drag_start = Some(pos2);
                    self.selection_state.drag_end = Some(pos2);
                    self.selection_state.right_click_drag = is_secondary_pressed;
                    // Determine and lock effective mode
                    self.selection_state.drag_effective_mode = if is_secondary_pressed {
                        SelectionMode::Subtract
                    } else if shift_held && alt_held {
                        SelectionMode::Intersect
                    } else if ctrl_held {
                        SelectionMode::Add
                    } else if alt_held {
                        SelectionMode::Subtract
                    } else {
                        self.selection_state.mode
                    };
                }

                let any_button_down = is_primary_down || is_secondary_down;
                let any_button_released = is_primary_released || is_secondary_released;

                if any_button_down
                    && self.selection_state.dragging
                    && let Some(pos_f) = canvas_pos_unclamped
                {
                    let mut end = Pos2::new(pos_f.0, pos_f.1);

                    // Shift => constrain to 1:1 aspect ratio (square / circle)
                    if shift_held && let Some(start) = self.selection_state.drag_start {
                        let dx = end.x - start.x;
                        let dy = end.y - start.y;
                        let side = dx.abs().max(dy.abs());
                        end = Pos2::new(start.x + side * dx.signum(), start.y + side * dy.signum());
                    }
                    self.selection_state.drag_end = Some(end);
                    ui.ctx().request_repaint();
                }

                if any_button_released && self.selection_state.dragging {
                    let effective_mode = self.selection_state.drag_effective_mode;
                    self.selection_state.dragging = false;
                    self.selection_state.right_click_drag = false;

                    if let (Some(start), Some(end)) = (
                        self.selection_state.drag_start,
                        self.selection_state.drag_end,
                    ) {
                        let raw_min_x = start.x.min(end.x).max(0.0);
                        let raw_min_y = start.y.min(end.y).max(0.0);
                        let raw_max_x = start.x.max(end.x).max(0.0);
                        let raw_max_y = start.y.max(end.y).max(0.0);
                        let min_x = (raw_min_x as u32).min(canvas_state.width.saturating_sub(1));
                        let min_y = (raw_min_y as u32).min(canvas_state.height.saturating_sub(1));
                        let max_x = (raw_max_x as u32).min(canvas_state.width.saturating_sub(1));
                        let max_y = (raw_max_y as u32).min(canvas_state.height.saturating_sub(1));

                        let sel_before = canvas_state.selection_mask.clone();

                        // Allow single-pixel selections for pixel art accuracy.
                        // Deselect only happens on a zero-size drag (start == end,
                        // which resolves to a single point).
                        if max_x > min_x || max_y > min_y {
                            let shape = match self.active_tool {
                                Tool::RectangleSelect => SelectionShape::Rectangle {
                                    min_x,
                                    min_y,
                                    max_x,
                                    max_y,
                                },
                                Tool::EllipseSelect => {
                                    let cx = (min_x as f32 + max_x as f32) / 2.0;
                                    let cy = (min_y as f32 + max_y as f32) / 2.0;
                                    let rx = (max_x as f32 - min_x as f32) / 2.0;
                                    let ry = (max_y as f32 - min_y as f32) / 2.0;
                                    SelectionShape::Ellipse { cx, cy, rx, ry }
                                }
                                _ => unreachable!(),
                            };

                            canvas_state.apply_selection_shape(&shape, effective_mode);
                            canvas_state.mark_dirty(None);
                        } else {
                            // Zero-size click => deselect
                            canvas_state.clear_selection();
                            canvas_state.mark_dirty(None);
                        }

                        let sel_after = canvas_state.selection_mask.clone();
                        let tool_name = match self.active_tool {
                            Tool::RectangleSelect => "Rectangle Select",
                            _ => "Ellipse Select",
                        };
                        self.pending_history_commands
                            .push(Box::new(SelectionCommand::new(
                                tool_name, sel_before, sel_after,
                            )));
                    }

                    self.selection_state.drag_start = None;
                    self.selection_state.drag_end = None;
                    ui.ctx().request_repaint();
                }
            }

            // Move tools are handled by app.rs via PasteOverlay / mask shifting.
            Tool::MovePixels | Tool::MoveSelection => {}

            // Magic Wand tool - color-based selection with live preview
            Tool::MagicWand => {
                let esc_pressed = escape_pressed_global;
                let shift_held_mw = ui.input(|i| i.modifiers.shift);
                let alt_held_mw = ui.input(|i| i.modifiers.alt);
                let ctrl_held_mw = ui.input(|i| i.modifiers.command);
                let click_scope =
                    if (ctrl_held_mw && shift_held_mw) || self.magic_wand_state.global_select {
                        MagicWandScope::Global
                    } else {
                        MagicWandScope::Contiguous
                    };
                let click_mode = if is_primary_clicked || is_secondary_clicked {
                    if is_secondary_clicked {
                        SelectionMode::Subtract
                    } else if ctrl_held_mw && shift_held_mw {
                        SelectionMode::Add
                    } else if shift_held_mw && alt_held_mw {
                        SelectionMode::Intersect
                    } else if ctrl_held_mw {
                        SelectionMode::Add
                    } else if alt_held_mw {
                        SelectionMode::Subtract
                    } else {
                        self.selection_state.mode
                    }
                } else {
                    self.selection_state.mode
                };

                // Poll async distance map computation
                if let Some(rx) = &self.magic_wand_state.async_rx {
                    if let Ok(result) = rx.try_recv() {
                        match result {
                            MagicWandAsyncResult::Ready { request_id, index } => {
                                if let Some(pending) =
                                    self.magic_wand_state.pending_operation.take()
                                    && pending.request_id == request_id
                                {
                                    self.magic_wand_state.operations.push(MagicWandOperation {
                                        start_x: pending.start_x,
                                        start_y: pending.start_y,
                                        target_color: pending.target_color,
                                        combine_mode: pending.combine_mode,
                                        scope: pending.scope,
                                        distance_mode: pending.distance_mode,
                                        connectivity: pending.connectivity,
                                        region_index: index,
                                    });
                                }
                            }
                        }
                        self.magic_wand_state.async_rx = None;
                        self.magic_wand_state.computing = false;
                        self.magic_wand_state.last_applied_tolerance = -1.0;
                        self.magic_wand_state.last_applied_aa = !self.magic_wand_state.anti_aliased;
                        self.magic_wand_state.preview_pending = true;
                        self.magic_wand_state.tolerance_changed_at = None;
                        ui.ctx().request_repaint();
                    } else {
                        // Still computing ÔÇö keep repainting to poll
                        ui.ctx().request_repaint();
                    }
                }

                // Commit on Enter or clear selection on Escape.
                // Escape should behave like deselect for selection tools and
                // must not restore a prior mask from the wand session cache.
                if enter_pressed || esc_pressed {
                    if esc_pressed {
                        canvas_state.clear_selection();
                        canvas_state.mark_dirty(None);
                        self.clear_magic_wand_async_state();
                        ui.ctx().request_repaint();
                    } else if self.magic_wand_has_session() {
                        self.finalize_magic_wand_session(canvas_state);
                        ui.ctx().request_repaint();
                    }
                }

                // Click to start new selection (or commit current + start new)
                if (is_primary_clicked || is_secondary_clicked)
                    && let Some(pos) = canvas_pos
                {
                    if click_mode == SelectionMode::Replace && self.magic_wand_has_session() {
                        self.finalize_magic_wand_session(canvas_state);
                    }
                    if click_mode == SelectionMode::Replace {
                        self.clear_magic_wand_async_state();
                        self.begin_magic_wand_session_if_needed(canvas_state);
                        canvas_state.selection_mask = None;
                        canvas_state.invalidate_selection_overlay();
                        canvas_state.mark_dirty(None);
                    } else {
                        self.begin_magic_wand_session_if_needed(canvas_state);
                    }

                    self.perform_magic_wand_selection(
                        canvas_state,
                        pos,
                        click_mode,
                        click_scope,
                        gpu_renderer.as_deref_mut(),
                    );
                    ui.ctx().request_repaint();
                }

                // Re-threshold the distance map only when tolerance or anti-alias changed
                let has_map = !self.magic_wand_state.operations.is_empty();
                if has_map
                    && self.active_tool == Tool::MagicWand
                    && (self.magic_wand_state.preview_pending
                        || (self.magic_wand_state.tolerance
                            - self.magic_wand_state.last_applied_tolerance)
                            .abs()
                            > 0.001
                        || self.magic_wand_state.anti_aliased
                            != self.magic_wand_state.last_applied_aa)
                {
                    self.magic_wand_state.preview_pending = true;
                    self.maybe_spawn_magic_wand_preview(canvas_state, gpu_renderer.as_deref_mut());
                    ui.ctx().request_repaint();
                }
            }

            // Fill tool - flood fill with live preview
            Tool::Fill => {
                let esc_pressed = escape_pressed_global;
                let fill_pressed = is_primary_pressed || is_secondary_pressed;

                // Guard: auto-rasterize text layers before destructive fill
                if fill_pressed
                    && let Some(layer) = canvas_state.layers.get(canvas_state.active_layer_index)
                    && layer.is_text_layer()
                {
                    self.pending_auto_rasterize = Some(canvas_state.active_layer_index);
                    return;
                }

                if let Some(rx) = &self.fill_state.async_rx {
                    if let Ok(result) = rx.try_recv() {
                        self.fill_state.async_rx = None;
                        self.fill_state.preview_in_flight = false;
                        if result.request_id == self.fill_state.preview_request_id {
                            if gpu_renderer.is_some() {
                                // GPU path: don't set last_threshold before rendering.
                                // render_fill_preview_gpu compares last_threshold to detect
                                // the first render and computes the full dirty region.
                                self.fill_state.active_fill = Some(ActiveFillRegion {
                                    start_x: result.start_x,
                                    start_y: result.start_y,
                                    layer_idx: canvas_state.active_layer_index,
                                    target_color: result.target_color,
                                    region_index: result.region_index,
                                    fill_mask: result.fill_mask,
                                    fill_bbox: None,
                                    last_threshold: None,
                                });
                                self.fill_state.tolerance_changed_at = None;
                                self.render_fill_preview_gpu(
                                    canvas_state,
                                    gpu_renderer.as_deref_mut().unwrap(),
                                );
                            } else {
                                self.fill_state.active_fill = Some(ActiveFillRegion {
                                    start_x: result.start_x,
                                    start_y: result.start_y,
                                    layer_idx: canvas_state.active_layer_index,
                                    target_color: result.target_color,
                                    region_index: result.region_index,
                                    fill_mask: result.fill_mask,
                                    fill_bbox: result.fill_bbox,
                                    last_threshold: Some(result.threshold),
                                });
                                self.fill_state.last_preview_tolerance = self.fill_state.tolerance;
                                self.fill_state.last_preview_aa = self.fill_state.anti_aliased;
                                self.fill_state.tolerance_changed_at = None;
                                self.apply_fill_preview_patch(
                                    canvas_state,
                                    result.dirty_bbox,
                                    result.fill_bbox,
                                    &result.preview_region,
                                    result.preview_region_w,
                                    result.preview_region_h,
                                );
                            }
                        }
                        if self.fill_state.recalc_pending {
                            self.maybe_spawn_fill_preview(
                                canvas_state,
                                gpu_renderer.as_deref_mut(),
                            );
                        }
                        ui.ctx().request_repaint();
                    } else if self.fill_state.preview_in_flight {
                        ui.ctx().request_repaint();
                    }
                }

                // Commit on Enter or cancel on Escape
                if enter_pressed || esc_pressed {
                    if self.fill_state.active_fill.is_some() && !esc_pressed {
                        self.commit_fill_preview(canvas_state);
                    } else if self.fill_state.active_fill.is_some() {
                        // Cancel preview on Escape
                        self.clear_fill_preview_state();
                        canvas_state.clear_preview_state();
                        self.stroke_tracker.cancel();
                    }
                }

                // Fill on button press so the pending preview commits and reseeds
                // immediately within the same click instead of waiting for release.
                if fill_pressed && let Some(pos) = canvas_pos {
                    self.handle_fill_click(
                        canvas_state,
                        pos,
                        is_secondary_pressed,
                        shift_held,
                        primary_color_f32,
                        secondary_color_f32,
                        gpu_renderer.as_deref_mut(),
                    );
                }

                // Update preview if tolerance/AA/color changed
                if self.fill_state.active_fill.is_some() && self.active_tool == Tool::Fill {
                    // Detect color/opacity change from the colors panel
                    let current_color_f32 = if self.fill_state.use_secondary_color {
                        secondary_color_f32
                    } else {
                        primary_color_f32
                    };
                    let current_color_u8 = Rgba([
                        (current_color_f32[0] * 255.0) as u8,
                        (current_color_f32[1] * 255.0) as u8,
                        (current_color_f32[2] * 255.0) as u8,
                        (current_color_f32[3] * 255.0) as u8,
                    ]);
                    if self.fill_state.fill_color_u8 != Some(current_color_u8) {
                        self.fill_state.fill_color_u8 = Some(current_color_u8);
                        self.fill_state.tolerance_changed_at = Some(Instant::now());
                        self.fill_state.recalc_pending = true;
                    }

                    self.maybe_spawn_fill_preview(canvas_state, gpu_renderer.as_deref_mut());

                    if self.fill_state.recalc_pending || self.fill_state.preview_in_flight {
                        ui.ctx().request_repaint();
                    }
                }
            }

            // Color picker tool - sample colors
            Tool::ColorPicker => {
                if (is_primary_clicked || is_secondary_clicked)
                    && let Some(pos) = canvas_pos
                {
                    // TODO: Implement color picker logic
                    self.pick_color_at_position(canvas_state, pos, is_secondary_clicked);
                }
            }

            // ================================================================
            // TEXT TOOL ÔÇö click to place cursor, type to add text
            // ================================================================
            Tool::Text => {
                let escape_pressed = escape_pressed_global;
                let ctrl_enter =
                    ui.input(|i| i.key_pressed(egui::Key::Enter) && i.modifiers.command);

                // Apply context bar style changes to selection (text layer mode)
                if self.text_state.ctx_bar_style_dirty
                    && self.text_state.editing_text_layer
                    && self.text_state.selection.has_selection()
                {
                    self.text_state.ctx_bar_style_dirty = false;
                    let bold = self.text_state.bold;
                    let italic = self.text_state.italic;
                    let underline = self.text_state.underline;
                    let strikethrough = self.text_state.strikethrough;
                    let weight = if bold {
                        if self.text_state.font_weight < 600 {
                            700u16
                        } else {
                            self.text_state.font_weight
                        }
                    } else {
                        self.text_state.font_weight
                    };
                    let font_family = self.text_state.font_family.clone();
                    let font_size = self.text_state.font_size;
                    self.text_layer_toggle_style(canvas_state, move |s| {
                        s.font_weight = weight;
                        s.italic = italic;
                        s.underline = underline;
                        s.strikethrough = strikethrough;
                        s.font_family = font_family.clone();
                        s.font_size = font_size;
                    });
                } else {
                    self.text_state.ctx_bar_style_dirty = false;
                }

                // Sync effects changes from UI panel to the text layer
                if self.text_state.text_effects_dirty && self.text_state.editing_text_layer {
                    self.text_state.text_effects_dirty = false;
                    if let Some(layer) =
                        canvas_state.layers.get_mut(canvas_state.active_layer_index)
                        && let crate::canvas::LayerContent::Text(ref mut td) = layer.content
                    {
                        td.effects = self.text_state.text_effects.clone();
                        td.mark_effects_dirty();
                    }
                    // Force-rasterize the editing layer so the change is visible immediately
                    let idx = canvas_state.active_layer_index;
                    canvas_state.force_rasterize_text_layer(idx);
                    canvas_state.mark_dirty(None);
                }

                // Sync warp changes from UI panel to the text layer block
                if self.text_state.text_warp_dirty && self.text_state.editing_text_layer {
                    self.text_state.text_warp_dirty = false;
                    if let Some(layer) =
                        canvas_state.layers.get_mut(canvas_state.active_layer_index)
                        && let crate::canvas::LayerContent::Text(ref mut td) = layer.content
                    {
                        if let Some(bid) = self.text_state.active_block_id
                            && let Some(block) = td.blocks.iter_mut().find(|b| b.id == bid)
                        {
                            block.warp = self.text_state.text_warp.clone();
                        }
                        td.mark_dirty();
                    }
                    // Force-rasterize the editing layer so the change is visible immediately
                    let idx = canvas_state.active_layer_index;
                    canvas_state.force_rasterize_text_layer(idx);
                    canvas_state.mark_dirty(None);
                }

                // Sync glyph overrides from glyph edit mode to the text layer block
                if self.text_state.glyph_overrides_dirty && self.text_state.editing_text_layer {
                    self.text_state.glyph_overrides_dirty = false;
                    if let Some(layer) =
                        canvas_state.layers.get_mut(canvas_state.active_layer_index)
                        && let crate::canvas::LayerContent::Text(ref mut td) = layer.content
                    {
                        if let Some(bid) = self.text_state.active_block_id
                            && let Some(block) = td.blocks.iter_mut().find(|b| b.id == bid)
                        {
                            block.glyph_overrides = self.text_state.glyph_overrides.clone();
                            block.cleanup_glyph_overrides();
                        }
                        td.mark_dirty();
                    }
                    // Force-rasterize the editing layer so the change is visible immediately
                    let idx = canvas_state.active_layer_index;
                    canvas_state.force_rasterize_text_layer(idx);
                    canvas_state.mark_dirty(None);
                    self.text_state.preview_dirty = true;
                }

                // Escape: cancel editing
                if escape_pressed && self.text_state.is_editing {
                    self.text_state.text.clear();
                    self.text_state.cursor_pos = 0;
                    self.text_state.is_editing = false;
                    self.text_state.editing_text_layer = false;
                    self.text_state.active_block_id = None;
                    self.text_state.selection = crate::ops::text_layer::TextSelection::default();
                    self.text_state.origin = None;
                    self.text_state.dragging_handle = false;
                    canvas_state.text_editing_layer = None;
                    canvas_state.clear_preview_state();
                    canvas_state.mark_dirty(None);
                }

                // Ctrl+Enter: commit text
                if ctrl_enter && self.text_state.is_editing && !self.text_state.text.is_empty() {
                    // Use deferred commit for loading bar
                    self.stroke_tracker
                        .start_preview_tool(canvas_state.active_layer_index, "Text");
                    self.text_state.commit_pending = true;
                    self.text_state.commit_pending_frame = 0;
                }

                // Enter (without Ctrl): insert newline
                if enter_pressed && !ctrl_enter && self.text_state.is_editing {
                    if self.text_state.editing_text_layer {
                        // Text layer mode: insert into the active block's runs
                        self.text_layer_insert_text(canvas_state, "\n");
                    } else {
                        self.text_state
                            .text
                            .insert(self.text_state.cursor_pos, '\n');
                        self.text_state.cursor_pos += 1;
                        self.text_state.preview_dirty = true;
                    }
                }

                // Compute move handle position ÔÇö alignment-aware
                let handle_radius_screen = 6.0;
                let handle_offset_canvas = 10.0 / zoom; // close distance for fine control
                let handle_canvas_pos = self.text_state.origin.map(|o| {
                    use crate::ops::text::TextAlignment;
                    match self.text_state.alignment {
                        TextAlignment::Left => {
                            // Handle to the left
                            (
                                o[0] - handle_offset_canvas,
                                o[1] + self.text_state.font_size * 0.5,
                            )
                        }
                        TextAlignment::Center => {
                            // Handle above, centered horizontally
                            (o[0], o[1] - handle_offset_canvas)
                        }
                        TextAlignment::Right => {
                            // Handle to the right
                            (
                                o[0] + handle_offset_canvas,
                                o[1] + self.text_state.font_size * 0.5,
                            )
                        }
                    }
                });

                // Detect hover over the move handle (for cursor icon)
                // Use canvas_pos_unclamped so the handle is reachable even when text is off-canvas
                self.text_state.hovering_handle = if self.text_state.is_editing {
                    if let (Some(pos_f), Some(hp)) = (canvas_pos_unclamped, handle_canvas_pos) {
                        let dx = (pos_f.0 - hp.0) * zoom;
                        let dy = (pos_f.1 - hp.1) * zoom;
                        (dx * dx + dy * dy).sqrt() < handle_radius_screen + 4.0
                    } else {
                        false
                    }
                } else {
                    false
                };

                // Detect hover over rotation / resize handles (for cursor icon)
                self.text_state.hovering_rotation_handle = false;
                self.text_state.hovering_resize_handle = None;
                if self.text_state.is_editing
                    && self.text_state.editing_text_layer
                    && !self.text_state.dragging_handle
                    && self.text_state.text_box_drag.is_none()
                    && let Some(pos_f) = canvas_pos_unclamped
                    && let Some(origin) = self.text_state.origin
                {
                    let font_size = self.text_state.font_size;
                    let display_width = if let Some(mw) = self.text_state.active_block_max_width {
                        mw.max(font_size * 2.0)
                    } else if let Some(ref font) = self.text_state.loaded_font {
                        use ab_glyph::{Font as _, ScaleFont as _};
                        let scaled = font.as_scaled(ab_glyph::PxScale {
                            x: font_size * self.text_state.width_scale,
                            y: font_size * self.text_state.height_scale,
                        });
                        let lines: Vec<&str> = self.text_state.text.split('\n').collect();
                        let mut max_w = font_size * 2.0;
                        for line in &lines {
                            let mut w = 0.0f32;
                            let mut prev = None;
                            for ch in line.chars() {
                                let gid = font.glyph_id(ch);
                                if let Some(prev_id) = prev {
                                    w += scaled.kern(prev_id, gid);
                                }
                                w += scaled.h_advance(gid);
                                prev = Some(gid);
                            }
                            max_w = max_w.max(w);
                        }
                        max_w
                    } else {
                        font_size * 2.0
                    };
                    let text_h = self.text_state.active_block_height;
                    let visual_h = if let Some(mh) = self.text_state.active_block_max_height {
                        mh.max(text_h)
                    } else {
                        text_h
                    };
                    let pad = 4.0;
                    let bx = {
                        use crate::ops::text::TextAlignment;
                        match self.text_state.alignment {
                            TextAlignment::Left => origin[0] - pad,
                            TextAlignment::Center => origin[0] - display_width * 0.5 - pad,
                            TextAlignment::Right => origin[0] - display_width - pad,
                        }
                    };
                    let by = origin[1] - pad;
                    let bw = display_width + pad * 2.0;
                    let _bh = visual_h + pad * 2.0;
                    let s_left = canvas_rect.min.x + bx * zoom;
                    let s_top = canvas_rect.min.y + by * zoom;
                    let s_right = s_left + bw * zoom;

                    let mx = pos_f.0 * zoom + canvas_rect.min.x;
                    let my = pos_f.1 * zoom + canvas_rect.min.y;

                    // Get block rotation and inverse-rotate the mouse to axis-aligned space
                    let blk_rot = if let Some(layer) =
                        canvas_state.layers.get(canvas_state.active_layer_index)
                        && let crate::canvas::LayerContent::Text(ref td) = layer.content
                        && let Some(bid) = self.text_state.active_block_id
                        && let Some(block) = td.blocks.iter().find(|b| b.id == bid)
                    {
                        block.rotation
                    } else {
                        0.0
                    };

                    let s_bottom = s_top + (visual_h + pad * 2.0) * zoom;
                    let pivot = Pos2::new((s_left + s_right) * 0.5, (s_top + s_bottom) * 0.5);
                    let mp = rotate_screen_point(Pos2::new(mx, my), pivot, -blk_rot);
                    let sx = mp.x;
                    let sy = mp.y;

                    let rot_handle_offset = 20.0;
                    let rot_cx = (s_left + s_right) * 0.5;
                    let rot_cy = s_top - rot_handle_offset;
                    let rot_dist = ((sx - rot_cx).powi(2) + (sy - rot_cy).powi(2)).sqrt();
                    if rot_dist < 10.0 {
                        self.text_state.hovering_rotation_handle = true;
                    } else {
                        // Check corner resize handles hover
                        let hit_r = 8.0;
                        let bh_total = (visual_h + pad * 2.0) * zoom;
                        let corners = [
                            (s_left, s_top, TextBoxDragType::ResizeTopLeft),
                            (s_right, s_top, TextBoxDragType::ResizeTopRight),
                            (s_left, s_top + bh_total, TextBoxDragType::ResizeBottomLeft),
                            (
                                s_right,
                                s_top + bh_total,
                                TextBoxDragType::ResizeBottomRight,
                            ),
                        ];
                        for &(cx, cy, drag_type) in &corners {
                            if (sx - cx).abs() < hit_r && (sy - cy).abs() < hit_r {
                                self.text_state.hovering_resize_handle = Some(drag_type);
                                break;
                            }
                        }
                        // Check edge resize handles hover (midpoints of left/right edges)
                        if self.text_state.hovering_resize_handle.is_none() {
                            let mid_y = s_top + bh_total * 0.5;
                            if (sx - s_left).abs() < hit_r && (sy - mid_y).abs() < bh_total * 0.5 {
                                self.text_state.hovering_resize_handle =
                                    Some(TextBoxDragType::ResizeLeft);
                            } else if (sx - s_right).abs() < hit_r
                                && (sy - mid_y).abs() < bh_total * 0.5
                            {
                                self.text_state.hovering_resize_handle =
                                    Some(TextBoxDragType::ResizeRight);
                            }
                        }
                    }
                }

                // Handle dragging the move handle
                // Use canvas_pos_unclamped so the handle is grabbable even when text is off-canvas
                if is_primary_pressed
                    && self.text_state.is_editing
                    && self.text_state.text_box_drag.is_none()
                    && let (Some(pos_f), Some(hp)) = (canvas_pos_unclamped, handle_canvas_pos)
                {
                    let dx = (pos_f.0 - hp.0) * zoom;
                    let dy = (pos_f.1 - hp.1) * zoom;
                    let dist = (dx * dx + dy * dy).sqrt();
                    if dist < handle_radius_screen + 4.0 {
                        self.text_state.dragging_handle = true;
                        if let Some(origin) = self.text_state.origin {
                            self.text_state.drag_offset =
                                [pos_f.0 - origin[0], pos_f.1 - origin[1]];
                        }
                        // --- Text layer drag optimization: cache block pixels ---
                        // At drag start, extract the active block's rasterized pixels
                        // into cached_raster_buf and re-rasterize the layer without
                        // the active block. During drag, we blit the cached block at
                        // the new offset into preview_layer (avoiding per-frame
                        // re-rasterization of the entire text layer).
                        self.text_state.text_layer_drag_cached = false;
                        if self.text_state.editing_text_layer
                            && let Some(bid) = self.text_state.active_block_id
                            && let Some(layer) =
                                canvas_state.layers.get_mut(canvas_state.active_layer_index)
                            && let crate::canvas::LayerContent::Text(ref mut td) = layer.content
                            && let Some(block_idx) = td.blocks.iter().position(|b| b.id == bid)
                        {
                            // 1. Ensure the layer is fully rasterized first
                            let w = canvas_state.width;
                            let h = canvas_state.height;

                            // 2. Extract the active block's tight pixel buffer.
                            //    Use the per-block cached_raster if available,
                            //    otherwise rasterize just this block into a temp buffer.
                            let block = &td.blocks[block_idx];
                            let (buf, buf_w, buf_h, off_x, off_y, origin) = if let Some(ref cached) =
                                block.cached_raster
                                && cached.buf_w > 0
                                && cached.buf_h > 0
                            {
                                let dx = (block.position[0] - cached.origin[0]).round() as i32;
                                let dy = (block.position[1] - cached.origin[1]).round() as i32;
                                (
                                    cached.buf.clone(),
                                    cached.buf_w,
                                    cached.buf_h,
                                    cached.off_x + dx,
                                    cached.off_y + dy,
                                    block.position,
                                )
                            } else {
                                // No per-block cache ÔÇö rasterize just this block
                                let mut temp = crate::canvas::TiledImage::new(w, h);
                                let mut cov = std::mem::take(&mut canvas_state.text_coverage_buf);
                                let mut gc = std::mem::take(&mut canvas_state.text_glyph_cache);
                                crate::ops::text_layer::rasterize_single_block(
                                    &mut td.blocks[block_idx],
                                    td.text_content_generation,
                                    w,
                                    h,
                                    &mut temp,
                                    &mut cov,
                                    &mut gc,
                                );
                                canvas_state.text_coverage_buf = cov;
                                canvas_state.text_glyph_cache = gc;
                                // Extract tight bounds from the temp tiled image
                                let block_pos = td.blocks[block_idx].position;
                                let raw = temp.extract_region_rgba(0, 0, w, h);
                                // Find tight AABB of non-transparent pixels
                                let (bx, by, bw, bh, tight) =
                                    crate::ops::text_layer::find_tight_bounds_rgba(&raw, w, h);
                                if bw > 0 && bh > 0 {
                                    (tight, bw, bh, bx as i32, by as i32, block_pos)
                                } else {
                                    (Vec::new(), 0, 0, 0, 0, block_pos)
                                }
                            };

                            if buf_w > 0 && buf_h > 0 {
                                self.text_state.cached_raster_buf = buf;
                                self.text_state.cached_raster_w = buf_w;
                                self.text_state.cached_raster_h = buf_h;
                                self.text_state.cached_raster_off_x = off_x;
                                self.text_state.cached_raster_off_y = off_y;
                                self.text_state.cached_raster_origin = Some(origin);

                                // 3. Re-rasterize the layer WITHOUT the active block.
                                //    Temporarily empty the block's runs, rasterize, restore.
                                let saved_runs = std::mem::take(&mut td.blocks[block_idx].runs);
                                td.mark_dirty();
                                {
                                    let mut cov =
                                        std::mem::take(&mut canvas_state.text_coverage_buf);
                                    let mut gc = std::mem::take(&mut canvas_state.text_glyph_cache);
                                    let new_pixels = td.rasterize(w, h, &mut cov, &mut gc);
                                    td.raster_generation = td.cache_generation;
                                    layer.pixels = new_pixels;
                                    layer.invalidate_lod();
                                    layer.gpu_generation += 1;
                                    canvas_state.text_coverage_buf = cov;
                                    canvas_state.text_glyph_cache = gc;
                                }
                                // Restore the block's runs
                                if let crate::canvas::LayerContent::Text(ref mut td2) =
                                    canvas_state.layers[canvas_state.active_layer_index].content
                                {
                                    td2.blocks[block_idx].runs = saved_runs;
                                    // Mark dirty again so the full layer will re-rasterize on drag end
                                    td2.mark_position_dirty();
                                }

                                self.text_state.text_layer_drag_cached = true;
                            }
                        }
                    }
                }

                // --- Resize / Rotate / Delete handle interactions ---
                // Use canvas_pos_unclamped because handles (rotation, delete) can be outside image bounds
                if is_primary_pressed
                    && self.text_state.is_editing
                    && !self.text_state.dragging_handle
                    && self.text_state.text_box_drag.is_none()
                    && self.text_state.editing_text_layer
                    && let Some(pos_f) = canvas_pos_unclamped
                    && let Some(origin) = self.text_state.origin
                {
                    let font_size = self.text_state.font_size;
                    let display_width = if let Some(mw) = self.text_state.active_block_max_width {
                        mw.max(font_size * 2.0)
                    } else {
                        // Compute natural width for handle positioning
                        if let Some(ref font) = self.text_state.loaded_font {
                            use ab_glyph::{Font as _, ScaleFont as _};
                            let scaled = font.as_scaled(ab_glyph::PxScale {
                                x: font_size * self.text_state.width_scale,
                                y: font_size * self.text_state.height_scale,
                            });
                            let lines: Vec<&str> = self.text_state.text.split('\n').collect();
                            let mut max_w = font_size * 2.0;
                            for line in &lines {
                                let mut w = 0.0f32;
                                let mut prev = None;
                                for ch in line.chars() {
                                    let gid = font.glyph_id(ch);
                                    if let Some(prev_id) = prev {
                                        w += scaled.kern(prev_id, gid);
                                    }
                                    w += scaled.h_advance(gid);
                                    prev = Some(gid);
                                }
                                max_w = max_w.max(w);
                            }
                            max_w
                        } else {
                            font_size * 2.0
                        }
                    };
                    let text_h = self.text_state.active_block_height;
                    let visual_h = if let Some(mh) = self.text_state.active_block_max_height {
                        mh.max(text_h)
                    } else {
                        text_h
                    };
                    let pad = 4.0;
                    let bx = {
                        use crate::ops::text::TextAlignment;
                        match self.text_state.alignment {
                            TextAlignment::Left => origin[0] - pad,
                            TextAlignment::Center => origin[0] - display_width * 0.5 - pad,
                            TextAlignment::Right => origin[0] - display_width - pad,
                        }
                    };
                    let by = origin[1] - pad;
                    let bw = display_width + pad * 2.0;
                    let bh = visual_h + pad * 2.0;

                    // Screen-space corners
                    let s_left = canvas_rect.min.x + bx * zoom;
                    let s_top = canvas_rect.min.y + by * zoom;
                    let s_right = s_left + bw * zoom;
                    let s_bottom = s_top + bh * zoom;

                    // Inverse-rotate mouse position into axis-aligned space
                    let blk_rot = if let Some(layer) =
                        canvas_state.layers.get(canvas_state.active_layer_index)
                        && let crate::canvas::LayerContent::Text(ref td) = layer.content
                        && let Some(bid) = self.text_state.active_block_id
                        && let Some(block) = td.blocks.iter().find(|b| b.id == bid)
                    {
                        block.rotation
                    } else {
                        0.0
                    };
                    let pivot = Pos2::new((s_left + s_right) * 0.5, (s_top + s_bottom) * 0.5);
                    let raw_sx = pos_f.0 * zoom + canvas_rect.min.x;
                    let raw_sy = pos_f.1 * zoom + canvas_rect.min.y;
                    let mp = rotate_screen_point(Pos2::new(raw_sx, raw_sy), pivot, -blk_rot);
                    let sx = mp.x;
                    let sy = mp.y;
                    let hit_r = 8.0; // screen pixel hit radius

                    // Check delete button first (top-right outside)
                    let del_offset = 14.0;
                    let del_cx = s_right + del_offset;
                    let del_cy = s_top - del_offset;
                    let del_dist = ((sx - del_cx).powi(2) + (sy - del_cy).powi(2)).sqrt();
                    if del_dist < 12.0 {
                        // Delete this block
                        self.text_state.text_box_click_guard = true;
                        if let Some(bid) = self.text_state.active_block_id {
                            if let Some(layer) =
                                canvas_state.layers.get_mut(canvas_state.active_layer_index)
                                && let crate::canvas::LayerContent::Text(ref mut td) = layer.content
                            {
                                td.blocks.retain(|b| b.id != bid);
                                td.mark_dirty();
                            }
                            self.text_state.text.clear();
                            self.text_state.cursor_pos = 0;
                            self.text_state.is_editing = false;
                            self.text_state.editing_text_layer = false;
                            self.text_state.active_block_id = None;
                            self.text_state.origin = None;
                            self.text_state.active_block_max_width = None;
                            self.text_state.active_block_max_height = None;
                            canvas_state.text_editing_layer = None;
                            canvas_state.clear_preview_state();
                            let idx = canvas_state.active_layer_index;
                            canvas_state.force_rasterize_text_layer(idx);
                            canvas_state.mark_dirty(None);
                        }
                    }
                    // Check rotation handle (above top-center)
                    else {
                        let rot_handle_offset = 20.0;
                        let rot_cx = (s_left + s_right) * 0.5;
                        let rot_cy = s_top - rot_handle_offset;
                        let rot_dist = ((sx - rot_cx).powi(2) + (sy - rot_cy).powi(2)).sqrt();
                        if rot_dist < 10.0 {
                            self.text_state.text_box_click_guard = true;
                            self.text_state.text_box_drag = Some(TextBoxDragType::Rotate);
                            self.text_state.text_box_drag_start_mouse = [pos_f.0, pos_f.1];
                            // Get current block rotation
                            let cur_rot = if let Some(layer) =
                                canvas_state.layers.get(canvas_state.active_layer_index)
                                && let crate::canvas::LayerContent::Text(ref td) = layer.content
                                && let Some(bid) = self.text_state.active_block_id
                                && let Some(block) = td.blocks.iter().find(|b| b.id == bid)
                            {
                                block.rotation
                            } else {
                                0.0
                            };
                            self.text_state.text_box_drag_start_rotation = cur_rot;
                            self.text_state.text_box_drag_start_origin = origin;
                        } else {
                            // Check corner resize handles
                            let corners_canvas = [
                                (bx, by, TextBoxDragType::ResizeTopLeft),
                                (bx + bw, by, TextBoxDragType::ResizeTopRight),
                                (bx, by + bh, TextBoxDragType::ResizeBottomLeft),
                                (bx + bw, by + bh, TextBoxDragType::ResizeBottomRight),
                            ];
                            for &(cx, cy, drag_type) in &corners_canvas {
                                let scx = canvas_rect.min.x + cx * zoom;
                                let scy = canvas_rect.min.y + cy * zoom;
                                if (sx - scx).abs() < hit_r && (sy - scy).abs() < hit_r {
                                    self.text_state.text_box_click_guard = true;
                                    self.text_state.text_box_drag = Some(drag_type);
                                    self.text_state.text_box_drag_start_mouse = [pos_f.0, pos_f.1];
                                    self.text_state.text_box_drag_start_width =
                                        self.text_state.active_block_max_width;
                                    self.text_state.text_box_drag_start_height = Some(
                                        self.text_state
                                            .active_block_max_height
                                            .map(|mh| mh.max(self.text_state.active_block_height))
                                            .unwrap_or(self.text_state.active_block_height),
                                    );
                                    self.text_state.text_box_drag_start_origin = origin;
                                    break;
                                }
                            }
                        }
                    }
                }

                // Process text box drag (resize / rotate)
                if let Some(drag_type) = self.text_state.text_box_drag {
                    if is_primary_down && let Some(pos_f) = canvas_pos_unclamped {
                        let raw_dx = pos_f.0 - self.text_state.text_box_drag_start_mouse[0];
                        let raw_dy = pos_f.1 - self.text_state.text_box_drag_start_mouse[1];
                        let start_origin = self.text_state.text_box_drag_start_origin;
                        let font_size = self.text_state.font_size;

                        // Project drag delta onto the box's local axes (rotation-aware)
                        let blk_rot = if let Some(layer) =
                            canvas_state.layers.get(canvas_state.active_layer_index)
                            && let crate::canvas::LayerContent::Text(ref td) = layer.content
                            && let Some(bid) = self.text_state.active_block_id
                            && let Some(block) = td.blocks.iter().find(|b| b.id == bid)
                        {
                            block.rotation
                        } else {
                            0.0
                        };
                        let cos_r = blk_rot.cos();
                        let sin_r = blk_rot.sin();
                        // Local-X and local-Y components of the drag delta
                        let dx = raw_dx * cos_r + raw_dy * sin_r;
                        let dy = -raw_dx * sin_r + raw_dy * cos_r;

                        let content_height = self.text_state.active_block_height;

                        // Helper closure: compute natural text width from font metrics
                        let compute_natural_width = || -> f32 {
                            if let Some(ref font) = self.text_state.loaded_font {
                                use ab_glyph::{Font as _, ScaleFont as _};
                                let scaled = font.as_scaled(ab_glyph::PxScale {
                                    x: font_size * self.text_state.width_scale,
                                    y: font_size * self.text_state.height_scale,
                                });
                                let ls = self.text_state.letter_spacing;
                                let lines: Vec<&str> = self.text_state.text.split('\n').collect();
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
                            } else {
                                font_size * 2.0
                            }
                        };

                        // Determine width change
                        let is_right = matches!(
                            drag_type,
                            TextBoxDragType::ResizeRight
                                | TextBoxDragType::ResizeTopRight
                                | TextBoxDragType::ResizeBottomRight
                        );
                        let is_left = matches!(
                            drag_type,
                            TextBoxDragType::ResizeLeft
                                | TextBoxDragType::ResizeTopLeft
                                | TextBoxDragType::ResizeBottomLeft
                        );
                        let is_top = matches!(
                            drag_type,
                            TextBoxDragType::ResizeTopLeft | TextBoxDragType::ResizeTopRight
                        );
                        let is_bottom = matches!(
                            drag_type,
                            TextBoxDragType::ResizeBottomLeft | TextBoxDragType::ResizeBottomRight
                        );

                        if is_right || is_left {
                            let start_w = self
                                .text_state
                                .text_box_drag_start_width
                                .unwrap_or_else(&compute_natural_width);
                            let new_w = if is_right {
                                (start_w + dx).max(font_size * 2.0)
                            } else {
                                (start_w - dx).max(font_size * 2.0)
                            };
                            self.text_state.active_block_max_width = Some(new_w);

                            // For left-side handles, shift origin along local-X to keep right edge fixed
                            let mut new_origin_x = start_origin[0];
                            let mut new_origin_y = start_origin[1];
                            if is_left {
                                let width_delta = start_w - new_w;
                                new_origin_x += width_delta * cos_r;
                                new_origin_y += width_delta * sin_r;
                            }

                            // For top handles, shift origin along local-Y to keep bottom edge fixed
                            if is_top {
                                let start_h = self
                                    .text_state
                                    .text_box_drag_start_height
                                    .unwrap_or(content_height);
                                let new_h = (start_h - dy).max(font_size);
                                let h_delta = start_h - new_h;
                                // Shift origin along local-Y direction
                                new_origin_x += h_delta * (-sin_r);
                                new_origin_y += h_delta * cos_r;
                                self.text_state.active_block_max_height = Some(new_h);
                            }

                            // For bottom handles, set max_height from drag
                            if is_bottom {
                                let start_h = self
                                    .text_state
                                    .text_box_drag_start_height
                                    .unwrap_or(content_height);
                                let new_h = (start_h + dy).max(font_size);
                                self.text_state.active_block_max_height = Some(new_h);
                            }

                            if is_left || is_top {
                                self.text_state.origin = Some([new_origin_x, new_origin_y]);
                            }

                            // Write to TextBlock immediately
                            if let Some(layer) =
                                canvas_state.layers.get_mut(canvas_state.active_layer_index)
                                && let crate::canvas::LayerContent::Text(ref mut td) = layer.content
                                && let Some(bid) = self.text_state.active_block_id
                                && let Some(block) = td.blocks.iter_mut().find(|b| b.id == bid)
                            {
                                block.max_width = Some(new_w);
                                if is_top || is_bottom {
                                    block.max_height = self.text_state.active_block_max_height;
                                }
                                if is_left || is_top {
                                    block.position = [new_origin_x, new_origin_y];
                                }
                                td.mark_dirty();
                            }
                            let idx = canvas_state.active_layer_index;
                            canvas_state.force_rasterize_text_layer(idx);
                            canvas_state.mark_dirty(None);
                        }

                        if matches!(drag_type, TextBoxDragType::Rotate) {
                            // Compute rotation angle from mouse position relative to box center
                            // Box center must match the overlay rotation pivot
                            let display_w = self
                                .text_state
                                .active_block_max_width
                                .unwrap_or(font_size * 2.0)
                                .max(font_size * 2.0);
                            let text_h = self.text_state.active_block_height;
                            let visual_h = self
                                .text_state
                                .active_block_max_height
                                .map(|mh| mh.max(text_h))
                                .unwrap_or(text_h);
                            let box_center_x = {
                                use crate::ops::text::TextAlignment;
                                match self.text_state.alignment {
                                    TextAlignment::Left => start_origin[0] + display_w * 0.5,
                                    TextAlignment::Center => start_origin[0],
                                    TextAlignment::Right => start_origin[0] - display_w * 0.5,
                                }
                            };
                            let box_center_y = start_origin[1] + visual_h * 0.5;
                            let start_angle = (self.text_state.text_box_drag_start_mouse[1]
                                - box_center_y)
                                .atan2(self.text_state.text_box_drag_start_mouse[0] - box_center_x);
                            let current_angle =
                                (pos_f.1 - box_center_y).atan2(pos_f.0 - box_center_x);
                            let delta = current_angle - start_angle;
                            let new_rotation = self.text_state.text_box_drag_start_rotation + delta;
                            // Write rotation to TextBlock
                            if let Some(layer) =
                                canvas_state.layers.get_mut(canvas_state.active_layer_index)
                                && let crate::canvas::LayerContent::Text(ref mut td) = layer.content
                                && let Some(bid) = self.text_state.active_block_id
                                && let Some(block) = td.blocks.iter_mut().find(|b| b.id == bid)
                            {
                                block.rotation = new_rotation;
                                td.mark_dirty();
                            }
                            let idx = canvas_state.active_layer_index;
                            canvas_state.force_rasterize_text_layer(idx);
                            canvas_state.mark_dirty(None);
                        }
                    }
                    if is_primary_released {
                        self.text_state.text_box_drag = None;
                    }
                }

                if self.text_state.dragging_handle
                    && is_primary_down
                    && let Some(pos_f) = canvas_pos_unclamped
                {
                    let new_x = pos_f.0 - self.text_state.drag_offset[0];
                    let new_y = pos_f.1 - self.text_state.drag_offset[1];
                    self.text_state.origin = Some([new_x, new_y]);

                    if self.text_state.editing_text_layer && self.text_state.text_layer_drag_cached
                    {
                        // Fast path: use cached block pixels + preview layer.
                        // layer.pixels already shows all blocks EXCEPT the dragging one
                        // (set up at drag start). We just blit the cached block at the
                        // new offset ÔÇö zero re-rasterization per frame.
                        if let Some(cached_origin) = self.text_state.cached_raster_origin {
                            let dx = new_x - cached_origin[0];
                            let dy = new_y - cached_origin[1];
                            let off_x = self.text_state.cached_raster_off_x + dx as i32;
                            let off_y = self.text_state.cached_raster_off_y + dy as i32;
                            let buf_w = self.text_state.cached_raster_w;
                            let buf_h = self.text_state.cached_raster_h;

                            let mut preview =
                                TiledImage::new(canvas_state.width, canvas_state.height);
                            preview.blit_rgba_at(
                                off_x,
                                off_y,
                                buf_w,
                                buf_h,
                                &self.text_state.cached_raster_buf,
                            );

                            canvas_state.preview_layer = Some(preview);
                            canvas_state.preview_blend_mode = BlendMode::Normal;
                            canvas_state.preview_force_composite = false;
                            canvas_state.preview_is_eraser = false;
                            canvas_state.preview_downscale = 1;
                            canvas_state.preview_flat_ready = false;
                            let visible_bounds =
                                Self::clip_preview_bounds(canvas_state, off_x, off_y, buf_w, buf_h);
                            // Merge old + new preview bounds so both regions get recomposited
                            let combined_bounds =
                                if let Some(old) = canvas_state.preview_stroke_bounds {
                                    Some(old.union(visible_bounds.unwrap_or(old)))
                                } else {
                                    visible_bounds
                                };
                            canvas_state.preview_stroke_bounds = combined_bounds;
                            if canvas_state.preview_texture_cache.is_some() {
                                canvas_state.preview_dirty_rect = combined_bounds;
                            } else {
                                canvas_state.preview_texture_cache = None;
                            }
                            // Use targeted dirty rect instead of full-canvas dirty
                            if let Some(bounds) = combined_bounds {
                                canvas_state.mark_dirty(Some(bounds));
                            } else {
                                canvas_state.mark_dirty(None);
                            }
                        }
                        self.text_state.preview_dirty = false;
                    } else if self.text_state.editing_text_layer {
                        // Fallback: force-rasterize (only if cached drag setup failed)
                        if let Some(layer) =
                            canvas_state.layers.get_mut(canvas_state.active_layer_index)
                            && let crate::canvas::LayerContent::Text(ref mut td) = layer.content
                            && let Some(bid) = self.text_state.active_block_id
                            && let Some(block) = td.blocks.iter_mut().find(|b| b.id == bid)
                        {
                            block.position = [new_x, new_y];
                            td.mark_position_dirty();
                        }
                        let idx = canvas_state.active_layer_index;
                        canvas_state.force_rasterize_text_layer(idx);
                        canvas_state.mark_dirty(None);
                        self.text_state.preview_dirty = false;
                    } else {
                        // Raster text: reuse cached raster buffer and re-blit at new origin offset.
                        if self.text_state.cached_raster_w > 0
                            && self.text_state.cached_raster_h > 0
                        {
                            if let Some(cached_origin) = self.text_state.cached_raster_origin {
                                let dx = new_x - cached_origin[0];
                                let dy = new_y - cached_origin[1];
                                let off_x = self.text_state.cached_raster_off_x + dx as i32;
                                let off_y = self.text_state.cached_raster_off_y + dy as i32;
                                let buf_w = self.text_state.cached_raster_w;
                                let buf_h = self.text_state.cached_raster_h;

                                let mut preview =
                                    TiledImage::new(canvas_state.width, canvas_state.height);
                                preview.blit_rgba_at(
                                    off_x,
                                    off_y,
                                    buf_w,
                                    buf_h,
                                    &self.text_state.cached_raster_buf,
                                );
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
                                canvas_state.preview_force_composite =
                                    self.properties.blending_mode != BlendMode::Normal;
                                canvas_state.preview_is_eraser = false;
                                canvas_state.preview_downscale = 1;
                                canvas_state.preview_flat_ready = false;
                                let visible_bounds = Self::clip_preview_bounds(
                                    canvas_state,
                                    off_x,
                                    off_y,
                                    buf_w,
                                    buf_h,
                                );
                                canvas_state.preview_stroke_bounds = visible_bounds;
                                if canvas_state.preview_texture_cache.is_some() {
                                    canvas_state.preview_dirty_rect = visible_bounds;
                                } else {
                                    canvas_state.preview_texture_cache = None;
                                }
                                canvas_state.mark_dirty(None);
                                self.text_state.preview_dirty = false;
                            } else {
                                self.text_state.preview_dirty = true;
                            }
                        } else {
                            self.text_state.preview_dirty = true;
                        }
                    }
                }

                if is_primary_released {
                    // Finalize text layer drag: apply final position, re-rasterize once
                    if self.text_state.dragging_handle
                        && self.text_state.text_layer_drag_cached
                        && self.text_state.editing_text_layer
                    {
                        if let Some(origin) = self.text_state.origin
                            && let Some(layer) =
                                canvas_state.layers.get_mut(canvas_state.active_layer_index)
                            && let crate::canvas::LayerContent::Text(ref mut td) = layer.content
                            && let Some(bid) = self.text_state.active_block_id
                            && let Some(block) = td.blocks.iter_mut().find(|b| b.id == bid)
                        {
                            block.position = [origin[0], origin[1]];
                            td.mark_position_dirty();
                        }
                        let idx = canvas_state.active_layer_index;
                        canvas_state.force_rasterize_text_layer(idx);
                        canvas_state.preview_layer = None;
                        canvas_state.mark_dirty(None);
                        self.text_state.text_layer_drag_cached = false;
                    }
                    self.text_state.dragging_handle = false;
                    // Finish glyph drag
                    if self.text_state.glyph_drag.is_some() {
                        self.text_state.glyph_drag = None;
                    }
                }

                // Click to place origin (or commit existing + start new)
                // Only if not dragging the handle
                let any_popup_open = egui::Popup::is_any_open(ui.ctx());
                if is_primary_clicked
                    && !self.text_state.dragging_handle
                    && !self.text_state.text_box_click_guard
                    && !any_popup_open
                {
                    // Check if click is on the handle ÔÇö if so, skip placement
                    let on_handle = if let (Some(pos_f), Some(hp)) =
                        (canvas_pos_unclamped, handle_canvas_pos)
                    {
                        let dx = (pos_f.0 - hp.0) * zoom;
                        let dy = (pos_f.1 - hp.1) * zoom;
                        (dx * dx + dy * dy).sqrt() < handle_radius_screen + 4.0
                    } else {
                        false
                    };

                    if !on_handle && let Some(pos_f) = canvas_pos_f32 {
                        if self.text_state.is_editing && !self.text_state.text.is_empty() {
                            if self.text_state.editing_text_layer {
                                // Text layer mode: check if clicking on the same vs different block
                                let is_text_layer = canvas_state
                                    .layers
                                    .get(canvas_state.active_layer_index)
                                    .is_some_and(|l| l.is_text_layer());
                                if is_text_layer {
                                    // Hit-test to find which block the click is on
                                    let clicked_block_id = if let Some(layer) =
                                        canvas_state.layers.get(canvas_state.active_layer_index)
                                        && let crate::canvas::LayerContent::Text(ref td) =
                                            layer.content
                                    {
                                        crate::ops::text_layer::hit_test_blocks(
                                            td, pos_f.0, pos_f.1,
                                        )
                                        .map(|idx| td.blocks[idx].id)
                                    } else {
                                        None
                                    };

                                    let same_block = self.text_state.active_block_id.is_some()
                                        && clicked_block_id == self.text_state.active_block_id;

                                    if same_block {
                                        // Click in the same block ÔÇö move cursor, don't commit
                                        // Use cached line advances and preview origin for positionÔåÆcursor mapping
                                        if let Some(origin) = self.text_state.origin {
                                            let rel_x = pos_f.0 - origin[0];
                                            let rel_y = pos_f.1 - origin[1];
                                            let lh = self.text_state.cached_line_height.max(1.0);

                                            // Compute visual lines (word-wrapped) for correct mapping
                                            let visual_line_count = if let (Some(mw), Some(font)) = (
                                                self.text_state.active_block_max_width,
                                                &self.text_state.loaded_font,
                                            ) {
                                                let ls = self.text_state.letter_spacing;
                                                let vlines: Vec<String> = self
                                                    .text_state
                                                    .text
                                                    .split('\n')
                                                    .flat_map(|line| {
                                                        crate::ops::text::word_wrap_line(
                                                            line,
                                                            font,
                                                            self.text_state.font_size,
                                                            mw,
                                                            ls,
                                                            self.text_state.width_scale,
                                                            self.text_state.height_scale,
                                                        )
                                                    })
                                                    .collect();
                                                vlines.len()
                                            } else {
                                                self.text_state.text.split('\n').count()
                                            };
                                            let line_idx = ((rel_y / lh).floor() as usize)
                                                .min(visual_line_count.saturating_sub(1));

                                            // Find approximate char position within visual line using cached_line_advances
                                            let mut best_pos = 0;
                                            if !self.text_state.cached_line_advances.is_empty()
                                                && line_idx
                                                    < self.text_state.cached_line_advances.len()
                                            {
                                                let advances =
                                                    &self.text_state.cached_line_advances[line_idx];
                                                let mut best_dist = f32::MAX;
                                                for (ci, &adv) in advances.iter().enumerate() {
                                                    let dist = (adv - rel_x).abs();
                                                    if dist < best_dist {
                                                        best_dist = dist;
                                                        best_pos = ci;
                                                    }
                                                }
                                                // Clamp to visual line char count
                                                let max_chars = advances.len().saturating_sub(1);
                                                best_pos = best_pos.min(max_chars);
                                            }

                                            // Convert (visual_line, char_in_line) back to byte offset
                                            let byte_pos = if let (Some(mw), Some(font)) = (
                                                self.text_state.active_block_max_width,
                                                &self.text_state.loaded_font,
                                            ) {
                                                let ls = self.text_state.letter_spacing;
                                                Self::visual_to_byte_pos(
                                                    &self.text_state.text,
                                                    line_idx,
                                                    best_pos,
                                                    font,
                                                    self.text_state.font_size,
                                                    mw,
                                                    ls,
                                                    self.text_state.width_scale,
                                                    self.text_state.height_scale,
                                                )
                                            } else {
                                                // No wrapping ÔÇö use logical line mapping
                                                let lines: Vec<&str> =
                                                    self.text_state.text.split('\n').collect();
                                                let clamped =
                                                    line_idx.min(lines.len().saturating_sub(1));
                                                let line_start: usize = lines
                                                    .iter()
                                                    .take(clamped)
                                                    .map(|l| l.len() + 1)
                                                    .sum();
                                                let line_text = lines[clamped];
                                                let clamped_pos =
                                                    best_pos.min(line_text.chars().count());
                                                let byte_offset: usize = line_text
                                                    .chars()
                                                    .take(clamped_pos)
                                                    .map(|c| c.len_utf8())
                                                    .sum();
                                                line_start + byte_offset
                                            };
                                            self.text_state.cursor_pos = byte_pos;
                                            // Update selection
                                            let shift_held = ui.input(|i| i.modifiers.shift);
                                            self.text_layer_update_selection(
                                                canvas_state,
                                                shift_held,
                                            );
                                            self.text_state.preview_dirty = true;
                                        }
                                    } else {
                                        // Different block or empty area ÔÇö commit and load new
                                        self.stroke_tracker.start_preview_tool(
                                            canvas_state.active_layer_index,
                                            "Text",
                                        );
                                        self.commit_text(canvas_state);
                                        if self.pending_stroke_event.is_none()
                                            && let Some(evt) = stroke_event.take()
                                        {
                                            self.pending_stroke_event = Some(evt);
                                        }
                                        // Load block at click position (or create new)
                                        self.load_text_layer_block(
                                            canvas_state,
                                            None,
                                            Some([pos_f.0, pos_f.1]),
                                        );
                                    }
                                    // Skip the default click-to-place behavior
                                } else {
                                    // Commit raster text and start new
                                    self.stroke_tracker.start_preview_tool(
                                        canvas_state.active_layer_index,
                                        "Text",
                                    );
                                    self.commit_text(canvas_state);
                                    if self.pending_stroke_event.is_none()
                                        && let Some(evt) = stroke_event.take()
                                    {
                                        self.pending_stroke_event = Some(evt);
                                    }
                                    self.text_state.origin = Some([pos_f.0, pos_f.1]);
                                    self.text_state.is_editing = true;
                                    self.text_state.editing_text_layer = false;
                                    self.text_state.editing_layer_index =
                                        Some(canvas_state.active_layer_index);
                                    self.text_state.active_block_id = None;
                                    self.text_state.text.clear();
                                    self.text_state.cursor_pos = 0;
                                    self.text_state.preview_dirty = true;
                                    self.restore_raster_style();
                                    self.stroke_tracker.start_preview_tool(
                                        canvas_state.active_layer_index,
                                        "Text",
                                    );
                                }
                            } else {
                                // Raster text mode: commit current text first
                                self.stroke_tracker
                                    .start_preview_tool(canvas_state.active_layer_index, "Text");
                                self.commit_text(canvas_state);
                                if self.pending_stroke_event.is_none()
                                    && let Some(evt) = stroke_event.take()
                                {
                                    self.pending_stroke_event = Some(evt);
                                }
                                self.text_state.origin = Some([pos_f.0, pos_f.1]);
                                self.text_state.is_editing = true;
                                self.text_state.editing_text_layer = false;
                                self.text_state.editing_layer_index =
                                    Some(canvas_state.active_layer_index);
                                self.text_state.active_block_id = None;
                                self.text_state.text.clear();
                                self.text_state.cursor_pos = 0;
                                self.text_state.preview_dirty = true;
                                self.restore_raster_style();
                                self.stroke_tracker
                                    .start_preview_tool(canvas_state.active_layer_index, "Text");
                            }
                        } else if self.text_state.is_editing && self.text_state.text.is_empty() {
                            // Already editing but empty ÔÇö for text layers, switch block
                            if self.text_state.editing_text_layer {
                                self.commit_text(canvas_state);
                                if self.pending_stroke_event.is_none()
                                    && let Some(evt) = stroke_event.take()
                                {
                                    self.pending_stroke_event = Some(evt);
                                }
                                self.load_text_layer_block(
                                    canvas_state,
                                    None,
                                    Some([pos_f.0, pos_f.1]),
                                );
                            } else {
                                // Move the empty text origin
                                self.text_state.origin = Some([pos_f.0, pos_f.1]);
                                self.text_state.preview_dirty = true;
                            }
                        } else {
                            // Check if active layer is a text layer
                            let is_text_layer = canvas_state
                                .layers
                                .get(canvas_state.active_layer_index)
                                .is_some_and(|l| l.is_text_layer());

                            if is_text_layer {
                                // Load text layer data with hit-test at click position
                                self.load_text_layer_block(
                                    canvas_state,
                                    None,
                                    Some([pos_f.0, pos_f.1]),
                                );
                            } else {
                                // Start new text at this position (raster stamp mode)
                                self.text_state.origin = Some([pos_f.0, pos_f.1]);
                                self.text_state.is_editing = true;
                                self.text_state.editing_text_layer = false;
                                self.text_state.editing_layer_index =
                                    Some(canvas_state.active_layer_index);
                                self.text_state.active_block_id = None;
                                self.text_state.text.clear();
                                self.text_state.cursor_pos = 0;
                                self.text_state.preview_dirty = true;
                                self.restore_raster_style();
                                self.stroke_tracker
                                    .start_preview_tool(canvas_state.active_layer_index, "Text");
                            }
                        }
                    }
                }

                // Clear the click guard on mouse release (after the click handler above)
                if is_primary_released {
                    self.text_state.text_box_click_guard = false;
                }

                // Detect color changes from the color widget
                let current_color = [
                    (primary_color_f32[0] * 255.0) as u8,
                    (primary_color_f32[1] * 255.0) as u8,
                    (primary_color_f32[2] * 255.0) as u8,
                    (primary_color_f32[3] * 255.0) as u8,
                ];
                if self.text_state.is_editing && self.text_state.last_color != current_color {
                    self.text_state.last_color = current_color;
                    self.text_state.preview_dirty = true;
                }

                // Capture text input ÔÇö but only when the canvas widget has
                // focus.  When a DragValue or other UI widget is focused the
                // same keystrokes would otherwise leak into the text layer.
                let canvas_has_focus = ui.ctx().memory(|m| {
                    match m.focused() {
                        Some(id) => canvas_state.canvas_widget_id == Some(id),
                        None => true, // no widget focused ÔÇö canvas owns input
                    }
                });
                let allow_text_capture = !self.text_state.font_popup_open
                    && (canvas_has_focus || !ui.ctx().egui_wants_keyboard_input());
                if self.text_state.is_editing && allow_text_capture {
                    let events: Vec<egui::Event> = ui.input(|i| i.events.clone());
                    let shift_held = ui.input(|i| i.modifiers.shift);
                    let ctrl_held = ui.input(|i| i.modifiers.command);

                    // Ctrl+B / Ctrl+I / Ctrl+U formatting shortcuts (text layer only)
                    if self.text_state.editing_text_layer && ctrl_held {
                        let b_pressed = ui.input(|i| i.key_pressed(egui::Key::B));
                        let i_pressed = ui.input(|i| i.key_pressed(egui::Key::I));
                        let u_pressed = ui.input(|i| i.key_pressed(egui::Key::U));
                        let a_pressed = ui.input(|i| i.key_pressed(egui::Key::A));

                        if b_pressed {
                            self.text_layer_toggle_style(canvas_state, |s| {
                                s.font_weight = if s.font_weight >= 700 { 400 } else { 700 }
                            });
                        }
                        if i_pressed {
                            self.text_layer_toggle_style(canvas_state, |s| s.italic = !s.italic);
                        }
                        if u_pressed {
                            self.text_layer_toggle_style(canvas_state, |s| {
                                s.underline = !s.underline
                            });
                        }
                        if a_pressed {
                            // Select all text in active block
                            self.text_state.selection.anchor =
                                crate::ops::text_layer::RunPosition {
                                    run_index: 0,
                                    byte_offset: 0,
                                };
                            let len = self.text_state.text.len();
                            self.text_state.cursor_pos = len;
                            // Get run position for end of text
                            if let Some(layer) =
                                canvas_state.layers.get(canvas_state.active_layer_index)
                                && let crate::canvas::LayerContent::Text(ref td) = layer.content
                                && let Some(bid) = self.text_state.active_block_id
                                && let Some(block) = td.blocks.iter().find(|b| b.id == bid)
                            {
                                self.text_state.selection.cursor =
                                    block.flat_offset_to_run_pos(len);
                            } else {
                                self.text_state.selection.cursor =
                                    crate::ops::text_layer::RunPosition {
                                        run_index: 0,
                                        byte_offset: len,
                                    };
                            }
                        }
                    }

                    // Tab: cycle between blocks (text layer only)
                    if self.text_state.editing_text_layer {
                        let tab_pressed = ui.input(|i| i.key_pressed(egui::Key::Tab));
                        if tab_pressed {
                            self.text_layer_cycle_block(canvas_state, shift_held);
                        }
                    }

                    for event in &events {
                        match event {
                            egui::Event::Text(t) => {
                                if self.text_state.editing_text_layer {
                                    self.text_layer_insert_text(canvas_state, t);
                                } else {
                                    // Delete selection first if any
                                    self.text_state
                                        .text
                                        .insert_str(self.text_state.cursor_pos, t);
                                    self.text_state.cursor_pos += t.len();
                                    self.text_state.preview_dirty = true;
                                }
                            }
                            egui::Event::Key {
                                key: egui::Key::Backspace,
                                pressed: true,
                                ..
                            } => {
                                if self.text_state.editing_text_layer {
                                    self.text_layer_backspace(canvas_state);
                                } else if self.text_state.cursor_pos > 0 {
                                    // Remove one char before cursor
                                    let mut chars: Vec<char> =
                                        self.text_state.text.chars().collect();
                                    let char_idx = self.text_state.text
                                        [..self.text_state.cursor_pos]
                                        .chars()
                                        .count();
                                    if char_idx > 0 {
                                        let remove_idx = char_idx - 1;
                                        chars.remove(remove_idx);
                                        self.text_state.text = chars.into_iter().collect();
                                        let new_char_pos = remove_idx;
                                        self.text_state.cursor_pos = self
                                            .text_state
                                            .text
                                            .chars()
                                            .take(new_char_pos)
                                            .map(|c| c.len_utf8())
                                            .sum();
                                    }
                                    self.text_state.preview_dirty = true;
                                }
                            }
                            egui::Event::Key {
                                key: egui::Key::ArrowLeft,
                                pressed: true,
                                ..
                            } => {
                                if self.text_state.cursor_pos > 0 {
                                    let chars: Vec<char> = self.text_state.text
                                        [..self.text_state.cursor_pos]
                                        .chars()
                                        .collect();
                                    if let Some(last) = chars.last() {
                                        self.text_state.cursor_pos -= last.len_utf8();
                                    }
                                }
                                // Selection: update or collapse
                                if self.text_state.editing_text_layer {
                                    self.text_layer_update_selection(canvas_state, shift_held);
                                }
                            }
                            egui::Event::Key {
                                key: egui::Key::ArrowRight,
                                pressed: true,
                                ..
                            } => {
                                if self.text_state.cursor_pos < self.text_state.text.len()
                                    && let Some(c) = self.text_state.text
                                        [self.text_state.cursor_pos..]
                                        .chars()
                                        .next()
                                {
                                    self.text_state.cursor_pos += c.len_utf8();
                                }
                                if self.text_state.editing_text_layer {
                                    self.text_layer_update_selection(canvas_state, shift_held);
                                }
                            }
                            egui::Event::Key {
                                key: egui::Key::ArrowUp,
                                pressed: true,
                                ..
                            } => {
                                // Move cursor to same x-position on the previous visual line
                                self.text_move_cursor_vertical(-1, shift_held, canvas_state);
                            }
                            egui::Event::Key {
                                key: egui::Key::ArrowDown,
                                pressed: true,
                                ..
                            } => {
                                // Move cursor to same x-position on the next visual line
                                self.text_move_cursor_vertical(1, shift_held, canvas_state);
                            }
                            egui::Event::Key {
                                key: egui::Key::Delete,
                                pressed: true,
                                ..
                            } => {
                                if self.text_state.editing_text_layer {
                                    self.text_layer_delete(canvas_state);
                                } else if self.text_state.cursor_pos < self.text_state.text.len() {
                                    let mut chars: Vec<char> =
                                        self.text_state.text.chars().collect();
                                    let char_idx = self.text_state.text
                                        [..self.text_state.cursor_pos]
                                        .chars()
                                        .count();
                                    if char_idx < chars.len() {
                                        chars.remove(char_idx);
                                        self.text_state.text = chars.into_iter().collect();
                                    }
                                    self.text_state.preview_dirty = true;
                                }
                            }
                            egui::Event::Key {
                                key: egui::Key::Home,
                                pressed: true,
                                ..
                            } => {
                                let text_before =
                                    &self.text_state.text[..self.text_state.cursor_pos];
                                if let Some(nl) = text_before.rfind('\n') {
                                    self.text_state.cursor_pos = nl + 1;
                                } else {
                                    self.text_state.cursor_pos = 0;
                                }
                                if self.text_state.editing_text_layer {
                                    self.text_layer_update_selection(canvas_state, shift_held);
                                }
                            }
                            egui::Event::Key {
                                key: egui::Key::End,
                                pressed: true,
                                ..
                            } => {
                                let text_after =
                                    &self.text_state.text[self.text_state.cursor_pos..];
                                if let Some(nl) = text_after.find('\n') {
                                    self.text_state.cursor_pos += nl;
                                } else {
                                    self.text_state.cursor_pos = self.text_state.text.len();
                                }
                                if self.text_state.editing_text_layer {
                                    self.text_layer_update_selection(canvas_state, shift_held);
                                }
                            }
                            egui::Event::Paste(t) => {
                                let filtered: String = t
                                    .chars()
                                    .filter(|c| !c.is_control() || *c == '\n')
                                    .collect();
                                if !filtered.is_empty() {
                                    if self.text_state.editing_text_layer {
                                        self.text_layer_insert_text(canvas_state, &filtered);
                                    } else {
                                        self.text_state
                                            .text
                                            .insert_str(self.text_state.cursor_pos, &filtered);
                                        self.text_state.cursor_pos += filtered.len();
                                        self.text_state.preview_dirty = true;
                                    }
                                }
                            }
                            _ => {}
                        }
                    }

                    // Re-render preview if dirty
                    if self.text_state.preview_dirty {
                        self.render_text_preview(canvas_state, primary_color_f32);
                    }

                    // Text overlay drawn by canvas.rs after handle_input returns
                }
            }

            // ================================================================
            // LIQUIFY TOOL ÔÇö click+drag to push/pull pixels
            // ================================================================
            Tool::Liquify => {
                // Guard: auto-rasterize text layers before destructive liquify
                if is_primary_pressed
                    && let Some(layer) = canvas_state.layers.get(canvas_state.active_layer_index)
                    && layer.is_text_layer()
                {
                    self.pending_auto_rasterize = Some(canvas_state.active_layer_index);
                    return;
                }

                let escape_pressed = escape_pressed_global;

                // Escape: reset
                if escape_pressed && self.liquify_state.is_active {
                    self.liquify_state.displacement = None;
                    self.liquify_state.is_active = false;
                    self.liquify_state.source_snapshot = None;
                    self.liquify_state.source_layer_index = None;
                    self.liquify_state.warp_buffer.clear();
                    self.liquify_state.dirty_rect = None;
                    canvas_state.clear_preview_state();
                    canvas_state.mark_dirty(None);
                }

                // Enter: commit
                if enter_pressed && self.liquify_state.is_active {
                    self.liquify_state.commit_pending = true;
                    self.liquify_state.commit_pending_frame = 0;
                }

                // Mouse drag: apply displacement
                if is_primary_down {
                    if let Some(pos_f) = canvas_pos_f32 {
                        // Initialize on first use
                        if !self.liquify_state.is_active {
                            self.liquify_state.is_active = true;
                            if let Some(layer) =
                                canvas_state.layers.get(canvas_state.active_layer_index)
                            {
                                self.liquify_state.source_snapshot =
                                    Some(layer.pixels.to_rgba_image());
                                self.liquify_state.source_layer_index =
                                    Some(canvas_state.active_layer_index);
                            }
                            self.liquify_state.displacement =
                                Some(crate::ops::transform::DisplacementField::new(
                                    canvas_state.width,
                                    canvas_state.height,
                                ));
                            self.stroke_tracker
                                .start_preview_tool(canvas_state.active_layer_index, "Liquify");
                            // Tell GPU pipeline the source snapshot changed
                            if let Some(ref mut gpu) = gpu_renderer {
                                gpu.liquify_pipeline.invalidate_source();
                            }
                        }

                        if let Some(ref mut disp) = self.liquify_state.displacement {
                            let radius = self.properties.size;
                            let cx = pos_f.0;
                            let cy = pos_f.1;

                            match self.liquify_state.mode {
                                LiquifyMode::Push => {
                                    // Get delta from last position
                                    if let Some(last) = self.liquify_state.last_pos {
                                        let dx = cx - last[0];
                                        let dy = cy - last[1];
                                        if dx.abs() > 0.1 || dy.abs() > 0.1 {
                                            disp.apply_push(
                                                cx,
                                                cy,
                                                dx * self.liquify_state.strength,
                                                dy * self.liquify_state.strength,
                                                radius,
                                                self.liquify_state.strength,
                                            );
                                        }
                                    }
                                }
                                LiquifyMode::Expand => {
                                    disp.apply_expand(
                                        cx,
                                        cy,
                                        radius,
                                        self.liquify_state.strength * 2.0,
                                    );
                                }
                                LiquifyMode::Contract => {
                                    disp.apply_contract(
                                        cx,
                                        cy,
                                        radius,
                                        self.liquify_state.strength * 2.0,
                                    );
                                }
                                LiquifyMode::TwirlCW => {
                                    disp.apply_twirl(
                                        cx,
                                        cy,
                                        radius,
                                        self.liquify_state.strength * 0.05,
                                        true,
                                    );
                                }
                                LiquifyMode::TwirlCCW => {
                                    disp.apply_twirl(
                                        cx,
                                        cy,
                                        radius,
                                        self.liquify_state.strength * 0.05,
                                        false,
                                    );
                                }
                            }

                            // Update dirty rect
                            let r = radius as i32 + 2;
                            let new_dirty =
                                [cx as i32 - r, cy as i32 - r, cx as i32 + r, cy as i32 + r];
                            self.liquify_state.dirty_rect =
                                Some(match self.liquify_state.dirty_rect {
                                    Some(d) => [
                                        d[0].min(new_dirty[0]),
                                        d[1].min(new_dirty[1]),
                                        d[2].max(new_dirty[2]),
                                        d[3].max(new_dirty[3]),
                                    ],
                                    None => new_dirty,
                                });

                            self.liquify_state.last_pos = Some([cx, cy]);
                            self.render_liquify_preview(canvas_state, gpu_renderer.as_deref_mut());
                        }
                    }
                } else if self.liquify_state.last_pos.is_some() {
                    self.liquify_state.last_pos = None;
                    self.liquify_state.dirty_rect = None;
                }
            }

            // ================================================================
            // MESH WARP TOOL ÔÇö drag control points to warp image
            // ================================================================
            Tool::MeshWarp => {
                // Guard: auto-rasterize text layers before destructive mesh warp
                if let Some(layer) = canvas_state.layers.get(canvas_state.active_layer_index)
                    && layer.is_text_layer()
                {
                    self.pending_auto_rasterize = Some(canvas_state.active_layer_index);
                    return;
                }

                let escape_pressed = escape_pressed_global;

                // Escape: reset to original
                if escape_pressed && self.mesh_warp_state.is_active {
                    self.mesh_warp_state.points = self.mesh_warp_state.original_points.clone();
                }

                // Enter: commit
                if enter_pressed && self.mesh_warp_state.is_active {
                    self.mesh_warp_state.commit_pending = true;
                    self.mesh_warp_state.commit_pending_frame = 0;
                }

                // Auto-initialize grid as soon as tool is selected
                if !self.mesh_warp_state.is_active {
                    self.init_mesh_warp_grid(canvas_state);
                    self.stroke_tracker
                        .start_preview_tool(canvas_state.active_layer_index, "Mesh Warp");
                } else {
                    // Detect stale snapshot (layer changed since snapshot was taken)
                    let cur_layer_idx = canvas_state.active_layer_index;
                    let cur_gen = canvas_state
                        .layers
                        .get(cur_layer_idx)
                        .map(|l| l.gpu_generation)
                        .unwrap_or(0);
                    if cur_layer_idx != self.mesh_warp_state.snapshot_layer_index
                        || cur_gen != self.mesh_warp_state.snapshot_generation
                    {
                        // Re-snapshot: layer content or active layer changed
                        self.init_mesh_warp_grid(canvas_state);
                        self.stroke_tracker
                            .start_preview_tool(cur_layer_idx, "Mesh Warp");
                    }
                }
                if self.mesh_warp_state.is_active {
                    // Find nearest control point for hover/drag
                    // Use clamped coordinates so edge/corner handles are reachable
                    let mesh_pos = canvas_pos_f32_clamped.or(canvas_pos_f32);
                    if let Some(pos_f) = mesh_pos {
                        let hit_radius = 12.0 / zoom; // screen pixels ÔåÆ canvas pixels

                        if is_primary_pressed {
                            // Start drag
                            let mut best_idx = None;
                            let mut best_dist = f32::MAX;
                            for (i, pt) in self.mesh_warp_state.points.iter().enumerate() {
                                let dx = pos_f.0 - pt[0];
                                let dy = pos_f.1 - pt[1];
                                let dist = (dx * dx + dy * dy).sqrt();
                                if dist < hit_radius && dist < best_dist {
                                    best_dist = dist;
                                    best_idx = Some(i);
                                }
                            }
                            self.mesh_warp_state.dragging_index = best_idx;
                        }

                        if is_primary_down && let Some(idx) = self.mesh_warp_state.dragging_index {
                            self.mesh_warp_state.points[idx] = [pos_f.0, pos_f.1];
                        }

                        if is_primary_released {
                            self.mesh_warp_state.dragging_index = None;
                        }

                        // Hover detection
                        if !is_primary_down {
                            let mut hover = None;
                            let mut best_dist = f32::MAX;
                            for (i, pt) in self.mesh_warp_state.points.iter().enumerate() {
                                let dx = pos_f.0 - pt[0];
                                let dy = pos_f.1 - pt[1];
                                let dist = (dx * dx + dy * dy).sqrt();
                                if dist < hit_radius && dist < best_dist {
                                    best_dist = dist;
                                    hover = Some(i);
                                }
                            }
                            self.mesh_warp_state.hover_index = hover;
                        }
                    }

                    // Draw overlay (grid + handles) ÔÇö no live preview, warp only on commit
                    self.draw_mesh_warp_overlay(ui, painter, canvas_rect, zoom, canvas_state);
                }
            }

            // ================================================================
            // COLOR REMOVER TOOL ÔÇö click to remove color
            // ================================================================
            Tool::ColorRemover => {
                // Guard: auto-rasterize text layers before destructive color removal
                if is_primary_clicked
                    && let Some(layer) = canvas_state.layers.get(canvas_state.active_layer_index)
                    && layer.is_text_layer()
                {
                    self.pending_auto_rasterize = Some(canvas_state.active_layer_index);
                    return;
                }
                if is_primary_clicked && let Some(pos) = canvas_pos {
                    self.commit_color_removal(canvas_state, pos.0, pos.1);
                }
            }

            // ================================================================
            // SMUDGE TOOL ÔÇö drag to smear/blend canvas pixels
            // ================================================================
            Tool::Smudge => {
                if is_primary_pressed {
                    // Guard: auto-rasterize text layers before destructive smudge
                    if let Some(layer) = canvas_state.layers.get(canvas_state.active_layer_index)
                        && layer.is_text_layer()
                    {
                        self.pending_auto_rasterize = Some(canvas_state.active_layer_index);
                        return;
                    }

                    // Stroke start: pick up the color at the start position
                    if let Some(pos) = canvas_pos {
                        if let Some(layer) =
                            canvas_state.layers.get(canvas_state.active_layer_index)
                        {
                            let px = layer.pixels.get_pixel(
                                pos.0.min(canvas_state.width.saturating_sub(1)),
                                pos.1.min(canvas_state.height.saturating_sub(1)),
                            );
                            self.smudge_state.pickup_color =
                                [px[0] as f32, px[1] as f32, px[2] as f32, px[3] as f32];
                        }
                        self.smudge_state.is_stroking = true;
                        let layer_pixels = canvas_state
                            .layers
                            .get(canvas_state.active_layer_index)
                            .map(|l| l.pixels.clone());
                        if let Some(pixels) = layer_pixels {
                            self.stroke_tracker.start_direct_tool(
                                canvas_state.active_layer_index,
                                "Smudge",
                                &pixels,
                            );
                        }
                    }
                }
                if is_primary_down
                    && self.smudge_state.is_stroking
                    && let Some(pos) = canvas_pos
                {
                    let radius = (self.properties.size * 0.5).max(1.0);
                    self.draw_smudge_no_dirty(canvas_state, pos.0, pos.1);
                    let dirty = egui::Rect::from_min_max(
                        egui::pos2(
                            (pos.0 as f32 - radius - 2.0).max(0.0),
                            (pos.1 as f32 - radius - 2.0).max(0.0),
                        ),
                        egui::pos2(pos.0 as f32 + radius + 2.0, pos.1 as f32 + radius + 2.0),
                    );
                    self.stroke_tracker.expand_bounds(dirty);
                    canvas_state.mark_dirty(Some(dirty));
                }
                if is_primary_released && self.smudge_state.is_stroking {
                    self.smudge_state.is_stroking = false;
                    if let Some(se) = self.stroke_tracker.finish(canvas_state) {
                        self.pending_stroke_event = Some(se);
                    }
                }
            }

            // ================================================================
            // SHAPES TOOL ÔÇö click+drag to draw, then adjust
            // ================================================================
            Tool::Shapes => {
                // Guard: auto-rasterize text layers before destructive shape drawing
                if is_primary_pressed
                    && let Some(layer) = canvas_state.layers.get(canvas_state.active_layer_index)
                    && layer.is_text_layer()
                {
                    self.pending_auto_rasterize = Some(canvas_state.active_layer_index);
                    return;
                }

                let escape_pressed = escape_pressed_global;

                // Escape: cancel
                if escape_pressed {
                    if self.shapes_state.placed.is_some() {
                        self.shapes_state.placed = None;
                        self.shapes_state.source_layer_index = None;
                        canvas_state.clear_preview_state();
                        canvas_state.mark_dirty(None);
                    } else if self.shapes_state.is_drawing {
                        self.shapes_state.is_drawing = false;
                        self.shapes_state.draw_start = None;
                        self.shapes_state.draw_end = None;
                        self.shapes_state.source_layer_index = None;
                        canvas_state.clear_preview_state();
                        canvas_state.mark_dirty(None);
                    }
                }

                // Enter: commit placed shape
                if enter_pressed && self.shapes_state.placed.is_some() {
                    self.stroke_tracker
                        .start_preview_tool(canvas_state.active_layer_index, "Shape");
                    self.commit_shape(canvas_state);
                }

                if let Some(pos_f) = canvas_pos_unclamped {
                    if self.shapes_state.placed.is_some() {
                        // Shape is placed ÔÇö handle move/resize/rotate
                        let hit_radius = 10.0 / zoom;

                        // Determine what was hit on press
                        let mut should_commit = false;
                        let mut need_preview = false;

                        if is_primary_pressed {
                            use crate::ops::shapes::ShapeHandle;
                            let placed = self.shapes_state.placed.as_ref().unwrap();
                            let cos_r = placed.rotation.cos();
                            let sin_r = placed.rotation.sin();

                            let corners_local = [
                                (-placed.hw, -placed.hh),
                                (placed.hw, -placed.hh),
                                (placed.hw, placed.hh),
                                (-placed.hw, placed.hh),
                            ];
                            let corners_canvas: Vec<(f32, f32)> = corners_local
                                .iter()
                                .map(|(cx, cy)| {
                                    (
                                        cx * cos_r - cy * sin_r + placed.cx,
                                        cx * sin_r + cy * cos_r + placed.cy,
                                    )
                                })
                                .collect();

                            let top_mid_x = (corners_canvas[0].0 + corners_canvas[1].0) * 0.5;
                            let top_mid_y = (corners_canvas[0].1 + corners_canvas[1].1) * 0.5;
                            let right_mid_x = (corners_canvas[1].0 + corners_canvas[2].0) * 0.5;
                            let right_mid_y = (corners_canvas[1].1 + corners_canvas[2].1) * 0.5;
                            let bottom_mid_x = (corners_canvas[2].0 + corners_canvas[3].0) * 0.5;
                            let bottom_mid_y = (corners_canvas[2].1 + corners_canvas[3].1) * 0.5;
                            let left_mid_x = (corners_canvas[3].0 + corners_canvas[0].0) * 0.5;
                            let left_mid_y = (corners_canvas[3].1 + corners_canvas[0].1) * 0.5;
                            let rot_offset = 20.0 / zoom;
                            let dir_x = top_mid_x - placed.cx;
                            let dir_y = top_mid_y - placed.cy;
                            let dir_len = (dir_x * dir_x + dir_y * dir_y).sqrt().max(0.001);
                            let rot_handle_x = top_mid_x + (dir_x / dir_len) * rot_offset;
                            let rot_handle_y = top_mid_y + (dir_y / dir_len) * rot_offset;

                            let handle_names = [
                                ShapeHandle::TopLeft,
                                ShapeHandle::TopRight,
                                ShapeHandle::BottomRight,
                                ShapeHandle::BottomLeft,
                            ];
                            let mut hit: Option<ShapeHandle> = None;

                            // Check rotation handle
                            let dx = pos_f.0 - rot_handle_x;
                            let dy = pos_f.1 - rot_handle_y;
                            if (dx * dx + dy * dy).sqrt() < hit_radius {
                                hit = Some(ShapeHandle::Rotate);
                            }

                            // Check corners
                            if hit.is_none() {
                                for (i, &(cx, cy)) in corners_canvas.iter().enumerate() {
                                    let dx = pos_f.0 - cx;
                                    let dy = pos_f.1 - cy;
                                    if (dx * dx + dy * dy).sqrt() < hit_radius {
                                        hit = Some(handle_names[i]);
                                        break;
                                    }
                                }
                            }

                            // Check edge-midpoint handles
                            if hit.is_none() {
                                let edge_handles = [
                                    (ShapeHandle::Top, top_mid_x, top_mid_y),
                                    (ShapeHandle::Right, right_mid_x, right_mid_y),
                                    (ShapeHandle::Bottom, bottom_mid_x, bottom_mid_y),
                                    (ShapeHandle::Left, left_mid_x, left_mid_y),
                                ];
                                for (edge_handle, cx, cy) in edge_handles {
                                    let dx = pos_f.0 - cx;
                                    let dy = pos_f.1 - cy;
                                    if (dx * dx + dy * dy).sqrt() < hit_radius {
                                        hit = Some(edge_handle);
                                        break;
                                    }
                                }
                            }

                            // Check inside shape for move
                            if hit.is_none() {
                                let dx = pos_f.0 - placed.cx;
                                let dy = pos_f.1 - placed.cy;
                                let lx = (dx * cos_r + dy * sin_r).abs();
                                let ly = (-dx * sin_r + dy * cos_r).abs();
                                if lx <= placed.hw + hit_radius && ly <= placed.hh + hit_radius {
                                    hit = Some(ShapeHandle::Move);
                                }
                            }

                            let pcx = placed.cx;
                            let pcy = placed.cy;
                            let prot = placed.rotation;
                            // Drop the immutable borrow before mutable access below
                            let _ = placed;

                            if let Some(h) = hit {
                                let p = self.shapes_state.placed.as_mut().unwrap();
                                p.handle_dragging = Some(h);
                                p.drag_offset = [pos_f.0 - pcx, pos_f.1 - pcy];
                                if h == ShapeHandle::Rotate {
                                    p.rotate_start_angle = (pos_f.1 - pcy).atan2(pos_f.0 - pcx);
                                    p.rotate_start_rotation = prot;
                                }
                                // For corner resize: compute anchor = opposite corner in canvas coords
                                let cos_r = prot.cos();
                                let sin_r = prot.sin();
                                let (anchor_lx, anchor_ly) = match h {
                                    ShapeHandle::TopLeft => (p.hw, p.hh),
                                    ShapeHandle::TopRight => (-p.hw, p.hh),
                                    ShapeHandle::BottomRight => (-p.hw, -p.hh),
                                    ShapeHandle::BottomLeft => (p.hw, -p.hh),
                                    ShapeHandle::Top => (0.0, p.hh),
                                    ShapeHandle::Right => (-p.hw, 0.0),
                                    ShapeHandle::Bottom => (0.0, -p.hh),
                                    ShapeHandle::Left => (p.hw, 0.0),
                                    _ => (0.0, 0.0),
                                };
                                p.drag_anchor = [
                                    anchor_lx * cos_r - anchor_ly * sin_r + pcx,
                                    anchor_lx * sin_r + anchor_ly * cos_r + pcy,
                                ];
                            } else {
                                should_commit = true;
                            }
                        }

                        if should_commit {
                            self.stroke_tracker
                                .start_preview_tool(canvas_state.active_layer_index, "Shape");
                            self.commit_shape(canvas_state);
                            if self.pending_stroke_event.is_none()
                                && let Some(evt) = stroke_event.take()
                            {
                                self.pending_stroke_event = Some(evt);
                            }
                        }

                        if is_primary_down && !should_commit {
                            use crate::ops::shapes::ShapeHandle;
                            if let Some(ref mut p) = self.shapes_state.placed {
                                match p.handle_dragging {
                                    Some(ShapeHandle::Move) => {
                                        p.cx = pos_f.0 - p.drag_offset[0];
                                        p.cy = pos_f.1 - p.drag_offset[1];
                                        need_preview = true;
                                    }
                                    Some(ShapeHandle::Rotate) => {
                                        let angle = (pos_f.1 - p.cy).atan2(pos_f.0 - p.cx);
                                        let mut new_rot = p.rotate_start_rotation
                                            + (angle - p.rotate_start_angle);
                                        // Shift: snap to 45┬░ increments
                                        if shift_held {
                                            let snap = std::f32::consts::FRAC_PI_4; // 45┬░
                                            new_rot = (new_rot / snap).round() * snap;
                                        }
                                        p.rotation = new_rot;
                                        need_preview = true;
                                    }
                                    Some(_handle) => {
                                        // Resize: corners are two-axis, edge handles are one-axis.
                                        let ax = p.drag_anchor[0];
                                        let ay = p.drag_anchor[1];
                                        let mx = pos_f.0;
                                        let my = pos_f.1;
                                        // Half-sizes in local (rotated) space
                                        let cos_r = p.rotation.cos();
                                        let sin_r = p.rotation.sin();
                                        let dx = (mx - ax) * 0.5;
                                        let dy = (my - ay) * 0.5;
                                        let local_dx = dx * cos_r + dy * sin_r;
                                        let local_dy = -dx * sin_r + dy * cos_r;

                                        if matches!(
                                            _handle,
                                            ShapeHandle::Top
                                                | ShapeHandle::Right
                                                | ShapeHandle::Bottom
                                                | ShapeHandle::Left
                                        ) {
                                            // Opposite edge midpoint is fixed anchor. For edge handles,
                                            // only one local axis changes to avoid perpendicular drift.
                                            match _handle {
                                                ShapeHandle::Top | ShapeHandle::Bottom => {
                                                    let new_hh = local_dy.abs().max(2.0);
                                                    let center_local = match _handle {
                                                        ShapeHandle::Top => (0.0, -new_hh),
                                                        ShapeHandle::Bottom => (0.0, new_hh),
                                                        _ => (0.0, 0.0),
                                                    };
                                                    p.cx = ax + center_local.0 * cos_r
                                                        - center_local.1 * sin_r;
                                                    p.cy = ay
                                                        + center_local.0 * sin_r
                                                        + center_local.1 * cos_r;
                                                    p.hh = new_hh;
                                                }
                                                ShapeHandle::Left | ShapeHandle::Right => {
                                                    let new_hw = local_dx.abs().max(2.0);
                                                    let center_local = match _handle {
                                                        ShapeHandle::Right => (new_hw, 0.0),
                                                        ShapeHandle::Left => (-new_hw, 0.0),
                                                        _ => (0.0, 0.0),
                                                    };
                                                    p.cx = ax + center_local.0 * cos_r
                                                        - center_local.1 * sin_r;
                                                    p.cy = ay
                                                        + center_local.0 * sin_r
                                                        + center_local.1 * cos_r;
                                                    p.hw = new_hw;
                                                }
                                                _ => {}
                                            }
                                        } else {
                                            let mut lx = local_dx.abs();
                                            let mut ly = local_dy.abs();
                                            // Shift: constrain to 1:1 aspect ratio
                                            if shift_held {
                                                let side = lx.max(ly);
                                                lx = side;
                                                ly = side;
                                            }
                                            // Recompute center from anchor + constrained size
                                            // The dragged corner in local space is the negation of the anchor's local offset
                                            let (anchor_lx, anchor_ly) = match _handle {
                                                ShapeHandle::TopLeft => (lx, ly),
                                                ShapeHandle::TopRight => (-lx, ly),
                                                ShapeHandle::BottomRight => (-lx, -ly),
                                                ShapeHandle::BottomLeft => (lx, -ly),
                                                _ => (0.0, 0.0),
                                            };
                                            let new_cx =
                                                ax + (-anchor_lx) * cos_r - (-anchor_ly) * sin_r;
                                            let new_cy =
                                                ay + (-anchor_lx) * sin_r + (-anchor_ly) * cos_r;
                                            p.cx = new_cx;
                                            p.cy = new_cy;
                                            p.hw = lx.max(2.0);
                                            p.hh = ly.max(2.0);
                                        }
                                        need_preview = true;
                                    }
                                    None => {}
                                }
                            }
                        }

                        if need_preview {
                            self.render_shape_preview(
                                canvas_state,
                                primary_color_f32,
                                secondary_color_f32,
                            );
                        }

                        if is_primary_released && let Some(ref mut p) = self.shapes_state.placed {
                            p.handle_dragging = None;
                        }
                    } else {
                        // Drawing mode
                        if is_primary_pressed && !self.shapes_state.is_drawing {
                            self.shapes_state.is_drawing = true;
                            self.shapes_state.draw_start = Some([pos_f.0, pos_f.1]);
                            self.shapes_state.draw_end = Some([pos_f.0, pos_f.1]);
                            self.shapes_state.source_layer_index =
                                Some(canvas_state.active_layer_index);
                            self.stroke_tracker
                                .start_preview_tool(canvas_state.active_layer_index, "Shape");
                        }

                        if is_primary_down && self.shapes_state.is_drawing {
                            let mut end = [pos_f.0, pos_f.1];
                            // Shift: constrain to square/circle
                            if shift_held && let Some(start) = self.shapes_state.draw_start {
                                let dx = (end[0] - start[0]).abs();
                                let dy = (end[1] - start[1]).abs();
                                let side = dx.max(dy);
                                end[0] = start[0] + side * (end[0] - start[0]).signum();
                                end[1] = start[1] + side * (end[1] - start[1]).signum();
                            }
                            self.shapes_state.draw_end = Some(end);
                            self.render_shape_preview(
                                canvas_state,
                                primary_color_f32,
                                secondary_color_f32,
                            );
                        }

                        if is_primary_released && self.shapes_state.is_drawing {
                            self.shapes_state.is_drawing = false;
                            // Convert to placed shape for manipulation
                            if let (Some(start), Some(end)) =
                                (self.shapes_state.draw_start, self.shapes_state.draw_end)
                            {
                                let cx = (start[0] + end[0]) * 0.5;
                                let cy = (start[1] + end[1]) * 0.5;
                                let hw = ((end[0] - start[0]) * 0.5).abs();
                                let hh = ((end[1] - start[1]) * 0.5).abs();
                                if hw > 2.0 && hh > 2.0 {
                                    let primary = [
                                        (primary_color_f32[0] * 255.0) as u8,
                                        (primary_color_f32[1] * 255.0) as u8,
                                        (primary_color_f32[2] * 255.0) as u8,
                                        (primary_color_f32[3] * 255.0) as u8,
                                    ];
                                    let secondary = [
                                        (secondary_color_f32[0] * 255.0) as u8,
                                        (secondary_color_f32[1] * 255.0) as u8,
                                        (secondary_color_f32[2] * 255.0) as u8,
                                        (secondary_color_f32[3] * 255.0) as u8,
                                    ];
                                    self.shapes_state.placed =
                                        Some(crate::ops::shapes::PlacedShape {
                                            cx,
                                            cy,
                                            hw,
                                            hh,
                                            rotation: 0.0,
                                            kind: self.shapes_state.selected_shape,
                                            fill_mode: self.shapes_state.fill_mode,
                                            outline_width: self.properties.size,
                                            primary_color: primary,
                                            secondary_color: secondary,
                                            anti_alias: self.shapes_state.anti_alias,
                                            corner_radius: self.shapes_state.corner_radius,
                                            handle_dragging: None,
                                            drag_offset: [0.0, 0.0],
                                            drag_anchor: [0.0, 0.0],
                                            rotate_start_angle: 0.0,
                                            rotate_start_rotation: 0.0,
                                        });
                                    self.render_shape_preview(
                                        canvas_state,
                                        primary_color_f32,
                                        secondary_color_f32,
                                    );
                                } else {
                                    self.shapes_state.draw_start = None;
                                    self.shapes_state.draw_end = None;
                                    self.shapes_state.source_layer_index = None;
                                    canvas_state.clear_preview_state();
                                }
                            }
                        }
                    }
                }

                // Property sync is handled by update_shape_if_dirty() called from canvas update loop

                // Draw shape overlay (bounding box, handles)
                if self.shapes_state.placed.is_some() {
                    self.draw_shape_overlay(painter, canvas_rect, zoom);
                }
            }

            // ================================================================
            // GRADIENT TOOL ÔÇö click+drag to define gradient direction
            // ================================================================
            Tool::Gradient => {
                let escape_pressed = escape_pressed_global;

                // Guard: auto-rasterize text layers before destructive gradient
                if is_primary_pressed
                    && let Some(layer) = canvas_state.layers.get(canvas_state.active_layer_index)
                    && layer.is_text_layer()
                {
                    self.pending_auto_rasterize = Some(canvas_state.active_layer_index);
                    return;
                }

                // Escape cancels active gradient
                if escape_pressed && self.gradient_state.drag_start.is_some() {
                    self.cancel_gradient(canvas_state);
                }

                // Enter commits active gradient  (deferred for loading bar)
                if enter_pressed
                    && self.gradient_state.drag_start.is_some()
                    && !self.gradient_state.dragging
                {
                    self.gradient_state.commit_pending = true;
                    self.gradient_state.commit_pending_frame = 0;
                }

                // Update stops from primary/secondary if using PrimarySecondary preset
                // and colors have changed
                if self.gradient_state.preset == GradientPreset::PrimarySecondary {
                    let p = [
                        (primary_color_f32[0] * 255.0) as u8,
                        (primary_color_f32[1] * 255.0) as u8,
                        (primary_color_f32[2] * 255.0) as u8,
                        (primary_color_f32[3] * 255.0) as u8,
                    ];
                    let s = [
                        (secondary_color_f32[0] * 255.0) as u8,
                        (secondary_color_f32[1] * 255.0) as u8,
                        (secondary_color_f32[2] * 255.0) as u8,
                        (secondary_color_f32[3] * 255.0) as u8,
                    ];
                    if self.gradient_state.stops.len() >= 2
                        && (self.gradient_state.stops[0].color != p
                            || self.gradient_state.stops[self.gradient_state.stops.len() - 1].color
                                != s)
                    {
                        self.gradient_state.stops[0] =
                            GradientStop::new(self.gradient_state.stops[0].position, p);
                        let last = self.gradient_state.stops.len() - 1;
                        self.gradient_state.stops[last] =
                            GradientStop::new(self.gradient_state.stops[last].position, s);
                        self.gradient_state.lut_dirty = true;
                    }
                } else if self.gradient_state.preset == GradientPreset::ForegroundTransparent {
                    let p = [
                        (primary_color_f32[0] * 255.0) as u8,
                        (primary_color_f32[1] * 255.0) as u8,
                        (primary_color_f32[2] * 255.0) as u8,
                        (primary_color_f32[3] * 255.0) as u8,
                    ];
                    if self.gradient_state.stops.len() >= 2
                        && self.gradient_state.stops[0].color != p
                    {
                        self.gradient_state.stops[0] =
                            GradientStop::new(self.gradient_state.stops[0].position, p);
                        let last = self.gradient_state.stops.len() - 1;
                        self.gradient_state.stops[last] = GradientStop::new(
                            self.gradient_state.stops[last].position,
                            [p[0], p[1], p[2], 0],
                        );
                        self.gradient_state.lut_dirty = true;
                    }
                }

                // Start/grab gradient ÔÇö allow clicking outside canvas so handles
                // can be dragged off-edge (e.g. gradient extends past border)
                if is_primary_pressed && let Some(pos_f) = canvas_pos_unclamped {
                    let click_pos = Pos2::new(pos_f.0, pos_f.1);

                    // Check if clicking near an existing handle first
                    let handle_grab_radius = (12.0 / zoom).max(6.0); // screen-space ~12px
                    let mut grabbed_handle: Option<usize> = None;

                    if let Some(start) = self.gradient_state.drag_start
                        && (click_pos - start).length() < handle_grab_radius
                    {
                        grabbed_handle = Some(0);
                    }
                    if let Some(end) = self.gradient_state.drag_end
                        && (click_pos - end).length() < handle_grab_radius
                    {
                        grabbed_handle = Some(1);
                    }

                    if let Some(handle_idx) = grabbed_handle {
                        // Grab existing handle for repositioning
                        self.gradient_state.dragging = true;
                        self.gradient_state.dragging_handle = Some(handle_idx);
                    } else {
                        // No handle hit ÔÇö commit previous and start new gradient.
                        // Immediate commit here (no defer) because the new
                        // gradient's drag_start is set on the same frame and
                        // commit_gradient clears it.  The old preview was
                        // already rendered at full res on mouse release.
                        if self.gradient_state.drag_start.is_some() {
                            self.commit_gradient(canvas_state);
                        }

                        self.gradient_state.drag_start = Some(click_pos);
                        self.gradient_state.drag_end = Some(click_pos);
                        self.gradient_state.dragging = true;
                        self.gradient_state.dragging_handle = None;
                        self.gradient_state.source_layer_index =
                            Some(canvas_state.active_layer_index);

                        // Start stroke tracking
                        self.stroke_tracker
                            .start_preview_tool(canvas_state.active_layer_index, "Gradient");
                        self.stroke_tracker.expand_bounds(egui::Rect::from_min_max(
                            egui::pos2(0.0, 0.0),
                            egui::pos2(canvas_state.width as f32, canvas_state.height as f32),
                        ));

                        if self.gradient_state.lut_dirty {
                            self.gradient_state.rebuild_lut();
                        }
                    }
                }

                if is_primary_down
                    && self.gradient_state.dragging
                    && let Some(pos_f) = canvas_pos_unclamped
                {
                    let new_pos = Pos2::new(pos_f.0, pos_f.1);
                    match self.gradient_state.dragging_handle {
                        Some(0) => self.gradient_state.drag_start = Some(new_pos),
                        Some(1) => self.gradient_state.drag_end = Some(new_pos),
                        _ => self.gradient_state.drag_end = Some(new_pos),
                    }
                    self.render_gradient_to_preview(canvas_state, gpu_renderer.as_deref_mut());
                    ui.ctx().request_repaint();
                }

                if is_primary_released && self.gradient_state.dragging {
                    // Save handle index BEFORE clearing it so we update the correct handle
                    let released_handle = self.gradient_state.dragging_handle;
                    self.gradient_state.dragging = false;
                    self.gradient_state.dragging_handle = None;
                    if let Some(pos_f) = canvas_pos_unclamped {
                        let new_pos = Pos2::new(pos_f.0, pos_f.1);
                        match released_handle {
                            Some(0) => self.gradient_state.drag_start = Some(new_pos),
                            Some(1) => self.gradient_state.drag_end = Some(new_pos),
                            _ => self.gradient_state.drag_end = Some(new_pos),
                        }
                        self.render_gradient_to_preview(canvas_state, gpu_renderer.as_deref_mut());
                    }
                }

                // If gradient is active (not dragging) and LUT changed, re-render
                if !self.gradient_state.dragging
                    && self.gradient_state.drag_start.is_some()
                    && self.gradient_state.lut_dirty
                {
                    self.render_gradient_to_preview(canvas_state, gpu_renderer);
                    ui.ctx().request_repaint();
                }
                // preview_dirty is handled separately via update_gradient_if_dirty()
                // so it works even when handle_input is blocked by UI interactions

                // Draw overlay handles
                self.draw_gradient_overlay(painter, canvas_rect, zoom, canvas_state);
            }

            // ================================================================
            // CLONE STAMP ÔÇö Alt+click to set source, then paint from offset
            // ================================================================
            Tool::CloneStamp => {
                // Guard: auto-rasterize text layers before destructive clone stamp
                if (is_primary_pressed || is_secondary_down)
                    && let Some(layer) = canvas_state.layers.get(canvas_state.active_layer_index)
                    && layer.is_text_layer()
                {
                    self.pending_auto_rasterize = Some(canvas_state.active_layer_index);
                    return;
                }

                let is_painting = is_primary_down || is_secondary_down;
                let alt_held = ui.input(|i| i.modifiers.alt);

                // Enter/Escape: clear source point
                if enter_pressed || escape_pressed_global {
                    self.clone_stamp_state.source = None;
                    self.clone_stamp_state.offset = None;
                    self.clone_stamp_state.offset_locked = false;
                }

                // Alt+Click: set source point
                if alt_held && is_primary_clicked {
                    if let Some(pos_f) = canvas_pos_f32 {
                        self.clone_stamp_state.source = Some(Pos2::new(pos_f.0, pos_f.1));
                        self.clone_stamp_state.offset = None;
                        self.clone_stamp_state.offset_locked = false;
                    }
                } else if !alt_held && self.clone_stamp_state.source.is_some() {
                    // Normal painting with clone source
                    if is_painting {
                        let current_f32 = canvas_pos_f32
                            .or_else(|| canvas_pos.map(|(x, y)| (x as f32, y as f32)));
                        if let (Some(cf), Some(_current_pos)) = (current_f32, canvas_pos) {
                            let current = Pos2::new(cf.0, cf.1);

                            // Lock offset on first paint of this stroke
                            if !self.clone_stamp_state.offset_locked {
                                let src = self.clone_stamp_state.source.unwrap();
                                self.clone_stamp_state.offset =
                                    Some(Vec2::new(src.x - current.x, src.y - current.y));
                                self.clone_stamp_state.offset_locked = true;
                            }

                            // Initialize stroke on first frame
                            if self.tool_state.last_pos.is_none() {
                                self.tool_state.last_precise_pos = Some(current);
                                self.tool_state.distance_remainder = 0.0;
                                self.tool_state.smooth_pos = Some(current);

                                self.stroke_tracker.start_preview_tool(
                                    canvas_state.active_layer_index,
                                    "Clone Stamp",
                                );

                                // Init preview layer
                                if canvas_state.preview_layer.is_none()
                                    || canvas_state.preview_layer.as_ref().unwrap().width()
                                        != canvas_state.width
                                    || canvas_state.preview_layer.as_ref().unwrap().height()
                                        != canvas_state.height
                                {
                                    canvas_state.preview_layer = Some(TiledImage::new(
                                        canvas_state.width,
                                        canvas_state.height,
                                    ));
                                } else if let Some(ref mut preview) = canvas_state.preview_layer {
                                    preview.clear();
                                }
                                canvas_state.preview_blend_mode = BlendMode::Normal;
                            }

                            // Sub-frame events
                            let positions: Vec<(f32, f32)> = if !raw_motion_events.is_empty() {
                                raw_motion_events.to_vec()
                            } else {
                                vec![cf]
                            };

                            // EMA smoothing (same as brush)
                            let smoothed_positions: Vec<(f32, f32)> = {
                                let mut result = Vec::with_capacity(positions.len());
                                for &pos in &positions {
                                    let raw = Pos2::new(pos.0, pos.1);
                                    let smoothed = if let Some(prev) = self.tool_state.smooth_pos {
                                        let dx = raw.x - prev.x;
                                        let dy = raw.y - prev.y;
                                        let dist = (dx * dx + dy * dy).sqrt();
                                        let alpha = if dist < 1.5 {
                                            1.0
                                        } else {
                                            (0.55 + 1.8 / (dist + 1.8)).min(1.0)
                                        };
                                        Pos2::new(prev.x + alpha * dx, prev.y + alpha * dy)
                                    } else {
                                        raw
                                    };
                                    self.tool_state.smooth_pos = Some(smoothed);
                                    result.push((smoothed.x, smoothed.y));
                                }
                                result
                            };

                            let offset = self.clone_stamp_state.offset.unwrap();
                            let mut frame_dirty_rect = Rect::NOTHING;

                            for &pos in &smoothed_positions {
                                let start_precise = self.tool_state.last_precise_pos;
                                let modified_rect = if let Some(start_p) = start_precise {
                                    self.clone_stamp_line(
                                        canvas_state,
                                        (start_p.x, start_p.y),
                                        (pos.0, pos.1),
                                        offset,
                                    )
                                } else {
                                    self.clone_stamp_circle(canvas_state, (pos.0, pos.1), offset)
                                };
                                frame_dirty_rect = frame_dirty_rect.union(modified_rect);
                                self.tool_state.last_precise_pos = Some(Pos2::new(pos.0, pos.1));
                            }

                            self.stroke_tracker.expand_bounds(frame_dirty_rect);
                            if frame_dirty_rect.is_positive() {
                                canvas_state.mark_preview_changed_rect(frame_dirty_rect);
                            }

                            self.tool_state.last_pos = Some(canvas_pos.unwrap());
                            self.tool_state.last_brush_pos = Some(canvas_pos.unwrap());
                            ui.ctx().request_repaint();
                        }
                    } else {
                        // Mouse released ÔÇö commit
                        if self.tool_state.last_pos.is_some() {
                            stroke_event = self.stroke_tracker.finish(canvas_state);
                            self.commit_bezier_to_layer(canvas_state, primary_color_f32);
                            canvas_state.clear_preview_state();
                            if let Some(ref ev) = stroke_event {
                                canvas_state.mark_dirty(Some(ev.bounds.expand(12.0)));
                            } else {
                                self.mark_full_dirty(canvas_state);
                            }
                        }
                        self.tool_state.last_pos = None;
                        self.tool_state.last_precise_pos = None;
                        self.tool_state.distance_remainder = 0.0;
                        self.tool_state.smooth_pos = None;
                        self.clone_stamp_state.offset_locked = false;
                    }
                }
            }

            // ================================================================
            // CONTENT AWARE BRUSH ÔÇö healing brush, samples surrounding texture
            // ================================================================
            Tool::ContentAwareBrush => {
                // Auto-rasterize guard: if active layer is a text layer, rasterize first
                if (is_primary_pressed || is_secondary_down)
                    && canvas_state.active_layer_index < canvas_state.layers.len()
                    && canvas_state.layers[canvas_state.active_layer_index].is_text_layer()
                {
                    self.pending_auto_rasterize = Some(canvas_state.active_layer_index);
                    return;
                }

                let is_painting = is_primary_down || is_secondary_down;
                if is_painting {
                    let current_f32 =
                        canvas_pos_f32.or_else(|| canvas_pos.map(|(x, y)| (x as f32, y as f32)));
                    if let (Some(cf), Some(_current_pos)) = (current_f32, canvas_pos) {
                        let current = Pos2::new(cf.0, cf.1);

                        // Initialize stroke
                        if self.tool_state.last_pos.is_none() {
                            self.tool_state.last_precise_pos = Some(current);
                            self.tool_state.distance_remainder = 0.0;
                            self.tool_state.smooth_pos = Some(current);
                            self.content_aware_state.stroke_points.clear();

                            // For async modes snapshot original pixels + init hole mask
                            self.content_aware_state.stroke_original = None;
                            self.content_aware_state.hole_mask = None;
                            if self.content_aware_state.quality.is_async() {
                                let idx = canvas_state.active_layer_index;
                                if idx < canvas_state.layers.len() {
                                    self.content_aware_state.stroke_original =
                                        Some(canvas_state.layers[idx].pixels.to_rgba_image());
                                    self.content_aware_state.hole_mask = Some(GrayImage::new(
                                        canvas_state.width,
                                        canvas_state.height,
                                    ));
                                }
                            }

                            self.stroke_tracker
                                .start_preview_tool(canvas_state.active_layer_index, "Heal Brush");

                            // Init preview layer
                            if canvas_state.preview_layer.is_none()
                                || canvas_state.preview_layer.as_ref().unwrap().width()
                                    != canvas_state.width
                                || canvas_state.preview_layer.as_ref().unwrap().height()
                                    != canvas_state.height
                            {
                                canvas_state.preview_layer =
                                    Some(TiledImage::new(canvas_state.width, canvas_state.height));
                            } else if let Some(ref mut preview) = canvas_state.preview_layer {
                                preview.clear();
                            }
                            canvas_state.preview_blend_mode = BlendMode::Normal;
                        }

                        // Sub-frame events
                        let positions: Vec<(f32, f32)> = if !raw_motion_events.is_empty() {
                            raw_motion_events.to_vec()
                        } else {
                            vec![cf]
                        };

                        // EMA smoothing
                        let smoothed_positions: Vec<(f32, f32)> = {
                            let mut result = Vec::with_capacity(positions.len());
                            for &pos in &positions {
                                let raw = Pos2::new(pos.0, pos.1);
                                let smoothed = if let Some(prev) = self.tool_state.smooth_pos {
                                    let dx = raw.x - prev.x;
                                    let dy = raw.y - prev.y;
                                    let dist = (dx * dx + dy * dy).sqrt();
                                    let alpha = if dist < 1.5 {
                                        1.0
                                    } else {
                                        (0.55 + 1.8 / (dist + 1.8)).min(1.0)
                                    };
                                    Pos2::new(prev.x + alpha * dx, prev.y + alpha * dy)
                                } else {
                                    raw
                                };
                                self.tool_state.smooth_pos = Some(smoothed);
                                result.push((smoothed.x, smoothed.y));
                            }
                            result
                        };

                        let mut frame_dirty_rect = Rect::NOTHING;

                        for &pos in &smoothed_positions {
                            self.content_aware_state
                                .stroke_points
                                .push(Pos2::new(pos.0, pos.1));

                            // Mark brush footprint in hole_mask for async modes
                            if let Some(ref mut mask) = self.content_aware_state.hole_mask {
                                let r = (self.properties.size / 2.0).max(1.0);
                                let ir = r as u32 + 1;
                                let x0 = (pos.0 - r).max(0.0) as u32;
                                let x1 =
                                    ((pos.0 + r) as u32).min(canvas_state.width.saturating_sub(1));
                                let y0 = (pos.1 - r).max(0.0) as u32;
                                let y1 =
                                    ((pos.1 + r) as u32).min(canvas_state.height.saturating_sub(1));
                                let _ = ir;
                                for py in y0..=y1 {
                                    for px in x0..=x1 {
                                        let ddx = px as f32 - pos.0;
                                        let ddy = py as f32 - pos.1;
                                        if ddx * ddx + ddy * ddy <= r * r {
                                            mask.put_pixel(px, py, image::Luma([255u8]));
                                        }
                                    }
                                }
                            }

                            let start_precise = self.tool_state.last_precise_pos;
                            let modified_rect = if let Some(start_p) = start_precise {
                                self.heal_line(canvas_state, (start_p.x, start_p.y), (pos.0, pos.1))
                            } else {
                                self.heal_circle(canvas_state, (pos.0, pos.1))
                            };
                            frame_dirty_rect = frame_dirty_rect.union(modified_rect);
                            self.tool_state.last_precise_pos = Some(Pos2::new(pos.0, pos.1));
                        }

                        self.stroke_tracker.expand_bounds(frame_dirty_rect);
                        if frame_dirty_rect.is_positive() {
                            canvas_state.mark_preview_changed_rect(frame_dirty_rect);
                        }

                        self.tool_state.last_pos = Some(canvas_pos.unwrap());
                        self.tool_state.last_brush_pos = Some(canvas_pos.unwrap());
                        ui.ctx().request_repaint();
                    }
                } else {
                    // Mouse released ÔÇö commit
                    if self.tool_state.last_pos.is_some() {
                        // Schedule async PatchMatch job if quality requires it
                        if self.content_aware_state.quality.is_async()
                            && let (Some(orig), Some(hmask)) = (
                                self.content_aware_state.stroke_original.take(),
                                self.content_aware_state.hole_mask.take(),
                            )
                        {
                            self.content_aware_state.pending_inpaint =
                                Some(crate::ops::inpaint::InpaintRequest {
                                    original_flat: orig,
                                    hole_mask: hmask,
                                    patch_size: self.content_aware_state.patch_size,
                                    iterations: self.content_aware_state.quality.patchmatch_iters(),
                                    layer_idx: canvas_state.active_layer_index,
                                });
                        }

                        stroke_event = self.stroke_tracker.finish(canvas_state);
                        self.commit_bezier_to_layer(canvas_state, primary_color_f32);
                        canvas_state.clear_preview_state();
                        if let Some(ref ev) = stroke_event {
                            canvas_state.mark_dirty(Some(ev.bounds.expand(12.0)));
                        } else {
                            self.mark_full_dirty(canvas_state);
                        }
                    }
                    self.tool_state.last_pos = None;
                    self.tool_state.last_precise_pos = None;
                    self.tool_state.distance_remainder = 0.0;
                    self.tool_state.smooth_pos = None;
                    self.content_aware_state.stroke_points.clear();
                }
            }

            // ================================================================
            // LASSO SELECT ÔÇö freeform polygon selection
            // ================================================================
            Tool::Lasso => {
                let esc_pressed = escape_pressed_global;
                let alt_held_l = ui.input(|i| i.modifiers.alt);
                let is_secondary_pressed =
                    ui.input(|i| i.pointer.button_pressed(egui::PointerButton::Secondary));

                // Esc / Enter: clear selection and cancel any in-progress lasso path.
                if esc_pressed || enter_pressed {
                    if self.lasso_state.dragging {
                        self.lasso_state.dragging = false;
                        self.lasso_state.points.clear();
                    }
                    canvas_state.clear_selection();
                    canvas_state.mark_dirty(None);
                    ui.ctx().request_repaint();
                }

                // Start lasso drag ÔÇö lock effective mode from modifier keys at drag start
                let ctrl_held_l = ui.input(|i| i.modifiers.command);
                if (is_primary_pressed || is_secondary_pressed)
                    && !self.lasso_state.dragging
                    && let Some(pos_f) = canvas_pos_unclamped
                {
                    self.lasso_state.dragging = true;
                    self.lasso_state.right_click_drag = is_secondary_pressed;
                    self.lasso_state.drag_effective_mode = if is_secondary_pressed {
                        SelectionMode::Subtract
                    } else if shift_held && alt_held_l {
                        SelectionMode::Intersect
                    } else if ctrl_held_l {
                        SelectionMode::Add
                    } else if alt_held_l {
                        SelectionMode::Subtract
                    } else {
                        self.lasso_state.mode
                    };
                    self.lasso_state.points.clear();
                    self.lasso_state.points.push(Pos2::new(pos_f.0, pos_f.1));
                }

                // Accumulate points while dragging
                let any_button_down = is_primary_down || is_secondary_down;
                if any_button_down && self.lasso_state.dragging {
                    if let Some(pos_f) = canvas_pos_unclamped {
                        let p = Pos2::new(pos_f.0, pos_f.1);
                        // Only add if moved at least 1px from last point
                        if let Some(last) = self.lasso_state.points.last() {
                            let d = (*last - p).length();
                            if d >= 1.0 {
                                self.lasso_state.points.push(p);
                            }
                        }
                    }
                    ui.ctx().request_repaint();
                }

                // Draw lasso preview path
                if self.lasso_state.dragging && self.lasso_state.points.len() >= 2 {
                    let screen_pts: Vec<Pos2> = self
                        .lasso_state
                        .points
                        .iter()
                        .map(|cp| {
                            Pos2::new(
                                canvas_rect.min.x + cp.x * zoom,
                                canvas_rect.min.y + cp.y * zoom,
                            )
                        })
                        .collect();
                    // Draw the path
                    painter.add(egui::Shape::line(
                        screen_pts.clone(),
                        egui::Stroke::new(1.5, Color32::WHITE),
                    ));
                    painter.add(egui::Shape::line(
                        screen_pts,
                        egui::Stroke::new(0.8, Color32::from_black_alpha(150)),
                    ));
                }

                // Finish on release ÔÇö rasterize polygon into selection mask
                let any_button_released = is_primary_released || is_secondary_released;
                if any_button_released && self.lasso_state.dragging {
                    let effective_mode = self.lasso_state.drag_effective_mode;
                    self.lasso_state.dragging = false;
                    let pts = std::mem::take(&mut self.lasso_state.points);
                    let sel_before = canvas_state.selection_mask.clone();
                    if pts.len() >= 3 {
                        // Scanline-fill the polygon into the selection mask
                        Self::apply_lasso_selection(canvas_state, &pts, effective_mode);
                        canvas_state.mark_dirty(None);
                    } else {
                        // Tiny lasso ÔåÆ deselect
                        canvas_state.clear_selection();
                        canvas_state.mark_dirty(None);
                    }
                    let sel_after = canvas_state.selection_mask.clone();
                    self.pending_history_commands
                        .push(Box::new(SelectionCommand::new(
                            "Lasso Select",
                            sel_before,
                            sel_after,
                        )));
                    ui.ctx().request_repaint();
                }
            }

            // ================================================================
            // ZOOM TOOL ÔÇö click/drag to zoom
            // ================================================================
            Tool::Zoom => {
                self.zoom_pan_action = ZoomPanAction::None;
                let min_drag_screen_px = 30.0; // minimum screen-pixel drag before zoom-to-rect

                // Track potential drag start (but don't commit to dragging yet)
                if is_primary_pressed
                    && !self.zoom_tool_state.dragging
                    && let Some(pos_f) = canvas_pos_f32
                {
                    let p = Pos2::new(pos_f.0, pos_f.1);
                    self.zoom_tool_state.drag_start = Some(p);
                    self.zoom_tool_state.drag_end = Some(p);
                }

                // While held, update end point; only enter drag mode once threshold exceeded
                if is_primary_down
                    && self.zoom_tool_state.drag_start.is_some()
                    && let Some(pos_f) = canvas_pos_f32
                {
                    let end = Pos2::new(pos_f.0, pos_f.1);
                    self.zoom_tool_state.drag_end = Some(end);

                    if !self.zoom_tool_state.dragging
                        && let Some(s) = self.zoom_tool_state.drag_start
                    {
                        let dx = (end.x - s.x) * zoom;
                        let dy = (end.y - s.y) * zoom;
                        if dx.abs().max(dy.abs()) >= min_drag_screen_px {
                            self.zoom_tool_state.dragging = true;
                        }
                    }
                    ui.ctx().request_repaint();
                }

                // Draw zoom rect preview only after threshold exceeded
                if self.zoom_tool_state.dragging
                    && let (Some(s), Some(e)) = (
                        self.zoom_tool_state.drag_start,
                        self.zoom_tool_state.drag_end,
                    )
                {
                    let accent = ui.visuals().selection.bg_fill;
                    let min = Pos2::new(s.x.min(e.x), s.y.min(e.y));
                    let max = Pos2::new(s.x.max(e.x), s.y.max(e.y));
                    let screen_min = Pos2::new(
                        canvas_rect.min.x + min.x * zoom,
                        canvas_rect.min.y + min.y * zoom,
                    );
                    let screen_max = Pos2::new(
                        canvas_rect.min.x + max.x * zoom,
                        canvas_rect.min.y + max.y * zoom,
                    );
                    let r = Rect::from_min_max(screen_min, screen_max);
                    let accent_faint =
                        Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), 30);
                    painter.rect_filled(r, 0.0, accent_faint);
                    painter.rect_stroke(
                        r,
                        0.0,
                        egui::Stroke::new(1.0, accent),
                        egui::StrokeKind::Middle,
                    );
                }

                // On release: zoom-to-rect if dragging, otherwise simple click zoom
                if is_primary_released {
                    if self.zoom_tool_state.dragging {
                        self.zoom_tool_state.dragging = false;
                        if let (Some(s), Some(e)) = (
                            self.zoom_tool_state.drag_start,
                            self.zoom_tool_state.drag_end,
                        ) {
                            self.zoom_pan_action = ZoomPanAction::ZoomToRect {
                                min_x: s.x.min(e.x),
                                min_y: s.y.min(e.y),
                                max_x: s.x.max(e.x),
                                max_y: s.y.max(e.y),
                            };
                        }
                    } else {
                        // Simple click ÔÇö zoom direction based on toggle
                        if let Some(pos_f) = canvas_pos_f32 {
                            if self.zoom_tool_state.zoom_out_mode {
                                self.zoom_pan_action = ZoomPanAction::ZoomOut {
                                    canvas_x: pos_f.0,
                                    canvas_y: pos_f.1,
                                };
                            } else {
                                self.zoom_pan_action = ZoomPanAction::ZoomIn {
                                    canvas_x: pos_f.0,
                                    canvas_y: pos_f.1,
                                };
                            }
                        }
                    }
                    self.zoom_tool_state.drag_start = None;
                    self.zoom_tool_state.drag_end = None;
                }

                // Right-click: always does the opposite of the toggle
                if is_secondary_clicked && let Some(pos_f) = canvas_pos_f32 {
                    if self.zoom_tool_state.zoom_out_mode {
                        self.zoom_pan_action = ZoomPanAction::ZoomIn {
                            canvas_x: pos_f.0,
                            canvas_y: pos_f.1,
                        };
                    } else {
                        self.zoom_pan_action = ZoomPanAction::ZoomOut {
                            canvas_x: pos_f.0,
                            canvas_y: pos_f.1,
                        };
                    }
                }
            }

            // ================================================================
            // PAN TOOL ÔÇö click+drag to pan viewport
            // ================================================================
            Tool::Pan => {
                self.zoom_pan_action = ZoomPanAction::None;
                // Drag delta in screen coordinates ÔåÆ pass directly to Canvas pan_offset
                if is_primary_down {
                    let drag_delta = ui.input(|i| i.pointer.delta());
                    if drag_delta.length_sq() > 0.0 {
                        self.zoom_pan_action = ZoomPanAction::Pan {
                            dx: drag_delta.x,
                            dy: drag_delta.y,
                        };
                    }
                    ui.ctx().request_repaint();
                }
            }

            // ================================================================
            // PERSPECTIVE CROP ÔÇö 4-corner quad, drag handles
            // ================================================================
            Tool::PerspectiveCrop => {
                let esc_pressed = escape_pressed_global;

                if esc_pressed {
                    self.perspective_crop_state.active = false;
                    self.perspective_crop_state.dragging_corner = None;
                    ui.ctx().request_repaint();
                }

                // Auto-init: place quad immediately when tool is selected
                if self.perspective_crop_state.needs_auto_init {
                    self.perspective_crop_state.needs_auto_init = false;
                    let w = canvas_state.width as f32;
                    let h = canvas_state.height as f32;
                    let m = 0.1; // 10% inset
                    self.perspective_crop_state.corners = [
                        Pos2::new(w * m, h * m),                 // top-left
                        Pos2::new(w * (1.0 - m), h * m),         // top-right
                        Pos2::new(w * (1.0 - m), h * (1.0 - m)), // bottom-right
                        Pos2::new(w * m, h * (1.0 - m)),         // bottom-left
                    ];
                    self.perspective_crop_state.active = true;
                    ui.ctx().request_repaint();
                }

                if self.perspective_crop_state.active {
                    // Enter: apply crop
                    if enter_pressed {
                        if let Some(cmd) = Self::apply_perspective_crop(
                            canvas_state,
                            &self.perspective_crop_state.corners,
                        ) {
                            self.pending_history_commands.push(cmd);
                        }
                        self.perspective_crop_state.active = false;
                        self.perspective_crop_state.dragging_corner = None;
                        canvas_state.mark_dirty(None);
                        ui.ctx().request_repaint();
                    }

                    let handle_r = 6.0 / zoom; // handles stay the same screen size

                    // Hit-test corners
                    if is_primary_pressed && let Some(pos_f) = canvas_pos_f32 {
                        let mp = Pos2::new(pos_f.0, pos_f.1);
                        for (i, &corner) in self.perspective_crop_state.corners.iter().enumerate() {
                            if (corner - mp).length() < handle_r + 4.0 / zoom {
                                self.perspective_crop_state.dragging_corner = Some(i);
                                break;
                            }
                        }
                    }

                    // Drag selected corner
                    if is_primary_down
                        && let Some(idx) = self.perspective_crop_state.dragging_corner
                        && let Some(pos_f) = canvas_pos_f32
                    {
                        let clamped = Pos2::new(
                            pos_f.0.clamp(0.0, canvas_state.width as f32 - 1.0),
                            pos_f.1.clamp(0.0, canvas_state.height as f32 - 1.0),
                        );
                        self.perspective_crop_state.corners[idx] = clamped;
                        ui.ctx().request_repaint();
                    }

                    if is_primary_released {
                        self.perspective_crop_state.dragging_corner = None;
                    }

                    // Draw the quad outline and handles
                    let corners = &self.perspective_crop_state.corners;
                    let screen: Vec<Pos2> = corners
                        .iter()
                        .map(|c| {
                            Pos2::new(
                                canvas_rect.min.x + c.x * zoom,
                                canvas_rect.min.y + c.y * zoom,
                            )
                        })
                        .collect();

                    // Dim area outside quad
                    painter.rect_filled(canvas_rect, 0.0, Color32::from_black_alpha(80));

                    // Draw filled quad (clear the dimming inside)
                    let mut quad_mesh = egui::Mesh::default();
                    for &sp in &screen {
                        quad_mesh.colored_vertex(sp, Color32::TRANSPARENT);
                    }
                    quad_mesh.add_triangle(0, 1, 2);
                    quad_mesh.add_triangle(0, 2, 3);
                    // We can't clear the dimming easily, so draw quad outline only

                    // Quad outline
                    let accent = ui.visuals().selection.bg_fill;
                    for i in 0..4 {
                        let a = screen[i];
                        let b = screen[(i + 1) % 4];
                        painter.line_segment([a, b], egui::Stroke::new(2.0, Color32::WHITE));
                        painter.line_segment([a, b], egui::Stroke::new(1.0, accent));
                    }

                    // Corner handles
                    let screen_handle_r = 5.0;
                    for (i, &sp) in screen.iter().enumerate() {
                        let is_active = self.perspective_crop_state.dragging_corner == Some(i);
                        let fill = if is_active { accent } else { Color32::WHITE };
                        painter.circle_filled(sp, screen_handle_r, fill);
                        painter.circle_stroke(
                            sp,
                            screen_handle_r,
                            egui::Stroke::new(1.5, Color32::from_black_alpha(120)),
                        );
                    }
                }
            }
        }

        // Store stroke event for app.rs to pick up
        if stroke_event.is_some() {
            self.pending_stroke_event = stroke_event;
        }
    }
}
