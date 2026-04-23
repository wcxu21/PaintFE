impl ToolsPanel {
    fn brush_resize_drag_modifier_held(&self, ui: &egui::Ui) -> bool {
        let binding = &self.brush_resize_drag_binding;
        ui.input(|i| {
            i.modifiers.command == binding.ctrl
                && i.modifiers.shift == binding.shift
                && i.modifiers.alt == binding.alt
        })
    }

    fn reset_brush_pointer_state(&mut self) {
        self.tool_state.last_pos = None;
        self.tool_state.last_precise_pos = None;
        self.tool_state.distance_remainder = 0.0;
        self.tool_state.using_secondary_color = false;
        self.tool_state.smooth_pos = None;
    }

    fn commit_brush_straight_line(
        &mut self,
        canvas_state: &mut CanvasState,
        last: (u32, u32),
        current: (u32, u32),
        primary_color_f32: [f32; 4],
        secondary_color_f32: [f32; 4],
    ) -> Option<StrokeEvent> {
        let is_eraser = self.active_tool == Tool::Eraser;
        let editing_mask = canvas_state.edit_layer_mask
            && canvas_state
                .layers
                .get(canvas_state.active_layer_index)
                .is_some_and(|l| l.has_live_mask());

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
            if let Some(layer) = canvas_state.layers.get(canvas_state.active_layer_index) {
                self.stroke_tracker.start_direct_tool(
                    canvas_state.active_layer_index,
                    "Eraser Line",
                    &layer.pixels,
                );
            }
        } else {
            let description = if self.active_tool == Tool::Pencil {
                "Pencil Line"
            } else {
                "Brush Line"
            };
            self.stroke_tracker
                .start_preview_tool(canvas_state.active_layer_index, description);
        }

        if !is_eraser || editing_mask {
            if canvas_state.preview_layer.is_none()
                || canvas_state.preview_layer.as_ref().unwrap().width() != canvas_state.width
                || canvas_state.preview_layer.as_ref().unwrap().height() != canvas_state.height
            {
                canvas_state.preview_layer =
                    Some(TiledImage::new(canvas_state.width, canvas_state.height));
            } else if let Some(ref mut preview) = canvas_state.preview_layer {
                preview.clear();
            }
            canvas_state.preview_blend_mode = self.properties.blending_mode;
        }

        let mirror = canvas_state.mirror_mode;
        let mw = canvas_state.width;
        let mh = canvas_state.height;
        let start_f = (last.0 as f32, last.1 as f32);
        let end_f = (current.0 as f32, current.1 as f32);
        let start_mirrors = mirror.mirror_positions(start_f.0, start_f.1, mw, mh);
        let end_mirrors = mirror.mirror_positions(end_f.0, end_f.1, mw, mh);
        let mut modified_rect = Rect::NOTHING;
        for i in 0..end_mirrors.len {
            let s = start_mirrors.data[i];
            let e = end_mirrors.data[i];
            let r = if self.active_tool == Tool::Pencil {
                self.draw_pixel_line_and_get_bounds(
                    canvas_state,
                    s,
                    e,
                    false,
                    primary_color_f32,
                    secondary_color_f32,
                )
            } else {
                self.draw_line_and_get_bounds(
                    canvas_state,
                    s,
                    e,
                    is_eraser,
                    false,
                    primary_color_f32,
                    secondary_color_f32,
                )
            };
            modified_rect = modified_rect.union(r);
        }

        self.stroke_tracker.expand_bounds(modified_rect);

        if editing_mask {
            let stroke_event = self.stroke_tracker.finish(canvas_state);
            self.commit_preview_to_layer_mask(canvas_state, is_eraser);
            canvas_state.clear_preview_state();
            canvas_state.mark_dirty(Some(modified_rect.expand(12.0)));
            stroke_event
        } else if !is_eraser {
            let stroke_event = self.stroke_tracker.finish(canvas_state);
            self.commit_bezier_to_layer(canvas_state, primary_color_f32);
            canvas_state.clear_preview_state();
            if let Some(ref ev) = stroke_event {
                canvas_state.mark_dirty(Some(ev.bounds.expand(12.0)));
            } else {
                self.mark_full_dirty(canvas_state);
            }
            stroke_event
        } else {
            let stroke_event = self.stroke_tracker.finish(canvas_state);
            canvas_state.mark_dirty(Some(modified_rect));
            stroke_event
        }
    }

    /// Auto-switch to Text tool when active layer is a text layer, and
    /// restore the previous tool when switching away. Called immediately
    /// after layer selection changes so there is no 1-frame delay.
    pub fn auto_switch_tool_for_layer(&mut self, canvas_state: &CanvasState) {
        let is_text = canvas_state
            .layers
            .get(canvas_state.active_layer_index)
            .is_some_and(|l| l.is_text_layer());
        if is_text && self.active_tool != Tool::Text {
            self.tool_before_text_layer = Some(self.active_tool);
            self.active_tool = Tool::Text;
        } else if !is_text
            && let Some(prev) = self.tool_before_text_layer.take()
            && self.active_tool == Tool::Text
        {
            self.active_tool = prev;
        }
    }

    /// Take the pending stroke event (if any) for processing
    pub fn take_stroke_event(&mut self) -> Option<StrokeEvent> {
        self.pending_stroke_event.take()
    }

    /// Take the pending color removal request (if any) for async dispatch
    pub fn take_pending_color_removal(&mut self) -> Option<ColorRemovalRequest> {
        self.pending_color_removal.take()
    }

    /// Take the pending inpaint request (if any) for async dispatch
    pub fn take_pending_inpaint(&mut self) -> Option<crate::ops::inpaint::InpaintRequest> {
        self.content_aware_state.pending_inpaint.take()
    }

    /// Cancel any active text editing session without committing.
    /// Call this whenever the layer being edited is rasterized or otherwise
    /// converted away from `LayerContent::Text`, so that the text tool no
    /// longer holds stale `is_editing` / `editing_text_layer` state that
    /// would otherwise absorb canvas clicks and prevent other tools from
    /// working.
    pub fn cancel_text_editing(&mut self, canvas_state: &mut crate::canvas::CanvasState) {
        if !self.text_state.is_editing && !self.text_state.editing_text_layer {
            return;
        }
        self.stroke_tracker.cancel();
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

    /// Returns true if the user is actively painting (brush/eraser stroke in progress).
    /// Useful for triggering continuous repaint during brush strokes.
    pub fn is_stroke_active(&self) -> bool {
        self.stroke_tracker.is_active || self.tool_state.last_pos.is_some()
    }

    /// Returns true if any tool has an uncommitted preview/in-progress state.
    pub fn has_active_tool_preview(&self) -> bool {
        match self.active_tool {
            Tool::Brush | Tool::Pencil | Tool::Eraser => {
                self.stroke_tracker.is_active || self.tool_state.last_pos.is_some()
            }
            Tool::Line => {
                matches!(
                    self.line_state.line_tool.stage,
                    LineStage::Dragging | LineStage::Editing
                )
            }
            Tool::Gradient => self.gradient_state.drag_start.is_some(),
            Tool::Shapes => self.shapes_state.placed.is_some() || self.shapes_state.is_drawing,
            Tool::Text => self.text_state.is_editing,
            Tool::Fill => self.fill_state.active_fill.is_some(),
            Tool::Liquify => self.liquify_state.is_active,
            Tool::MeshWarp => self.mesh_warp_state.is_active,
            _ => false,
        }
    }

    pub fn debug_operation_label(&self) -> Option<String> {
        if self.active_layer_rgba_prewarm_rx.is_some() {
            return Some(match self.active_tool {
                Tool::MagicWand => "Building: Magic Wand Map".to_string(),
                Tool::Fill => "Building: Fill Map".to_string(),
                _ => "Building: Tool Map".to_string(),
            });
        }

        if self.active_tool == Tool::MagicWand && self.magic_wand_state.computing {
            return Some("Building: Magic Wand Map".to_string());
        }

        None
    }

    /// Cancel the current tool's in-progress operation without committing.
    /// Returns true if something was cancelled, false if there was nothing to cancel.
    pub fn cancel_active_tool(&mut self, canvas_state: &mut CanvasState) -> bool {
        match self.active_tool {
            Tool::Brush | Tool::Pencil | Tool::Eraser
                if self.stroke_tracker.is_active || self.tool_state.last_pos.is_some() =>
            {
                self.stroke_tracker.cancel();
                self.tool_state.last_pos = None;
                self.tool_state.last_precise_pos = None;
                self.tool_state.smooth_pos = None;
                canvas_state.clear_preview_state();
                canvas_state.mark_dirty(None);
                true
            }
            Tool::Brush | Tool::Pencil | Tool::Eraser => false,
            Tool::Line => match self.line_state.line_tool.stage {
                LineStage::Dragging | LineStage::Editing => {
                    self.stroke_tracker.cancel();
                    canvas_state.clear_preview_state();
                    if let Some(bounds) = self.line_state.line_tool.last_bounds {
                        canvas_state.mark_dirty(Some(bounds));
                    } else {
                        canvas_state.mark_dirty(None);
                    }
                    self.line_state.line_tool.stage = LineStage::Idle;
                    self.line_state.line_tool.last_bounds = None;
                    self.line_state.line_tool.require_mouse_release = true;
                    self.line_state.line_tool.initial_mouse_pos = None;
                    self.line_state.line_tool.dragging_handle = None;
                    true
                }
                _ => false,
            },
            Tool::Gradient if self.gradient_state.drag_start.is_some() => {
                self.cancel_gradient(canvas_state);
                true
            }
            Tool::Gradient => false,
            Tool::Shapes => {
                if self.shapes_state.placed.is_some() {
                    self.shapes_state.placed = None;
                    canvas_state.clear_preview_state();
                    canvas_state.mark_dirty(None);
                    true
                } else if self.shapes_state.is_drawing {
                    self.shapes_state.is_drawing = false;
                    self.shapes_state.draw_start = None;
                    self.shapes_state.draw_end = None;
                    canvas_state.clear_preview_state();
                    canvas_state.mark_dirty(None);
                    true
                } else {
                    false
                }
            }
            Tool::Text if self.text_state.is_editing => {
                self.text_state.text.clear();
                self.text_state.cursor_pos = 0;
                self.text_state.is_editing = false;
                self.text_state.editing_text_layer = false;
                self.text_state.origin = None;
                self.text_state.dragging_handle = false;
                canvas_state.clear_preview_state();
                canvas_state.mark_dirty(None);
                true
            }
            Tool::Text => false,
            Tool::Fill if self.fill_state.active_fill.is_some() => {
                self.clear_fill_preview_state();
                canvas_state.clear_preview_state();
                self.stroke_tracker.cancel();
                true
            }
            Tool::Fill => false,
            Tool::Liquify if self.liquify_state.is_active => {
                self.liquify_state.displacement = None;
                self.liquify_state.is_active = false;
                self.liquify_state.source_snapshot = None;
                self.liquify_state.warp_buffer.clear();
                self.liquify_state.dirty_rect = None;
                canvas_state.clear_preview_state();
                canvas_state.mark_dirty(None);
                true
            }
            Tool::Liquify => false,
            Tool::MeshWarp if self.mesh_warp_state.is_active => {
                self.mesh_warp_state.is_active = false;
                self.mesh_warp_state.source_snapshot = None;
                self.mesh_warp_state.warp_buffer.clear();
                self.stroke_tracker.cancel();
                canvas_state.clear_preview_state();
                canvas_state.mark_dirty(None);
                true
            }
            Tool::MeshWarp => false,
            _ => false,
        }
    }

    /// Commit the current tool's in-progress preview immediately.
    /// Returns true if a commit occurred.
    pub fn commit_active_tool_preview(
        &mut self,
        canvas_state: &mut CanvasState,
        secondary_color_f32: [f32; 4],
    ) -> bool {
        match self.active_tool {
            Tool::Line if self.line_state.line_tool.stage == LineStage::Editing => {
                let line_editing_mask = canvas_state.edit_layer_mask
                    && canvas_state
                        .layers
                        .get(canvas_state.active_layer_index)
                        .is_some_and(|l| l.has_live_mask());
                if let Some(final_bounds) = self.line_state.line_tool.last_bounds {
                    self.stroke_tracker.expand_bounds(final_bounds);
                }
                let stroke_event = self.stroke_tracker.finish(canvas_state);
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
                if stroke_event.is_some() {
                    self.pending_stroke_event = stroke_event;
                }
                true
            }
            Tool::Fill if self.fill_state.active_fill.is_some() => {
                self.commit_fill_preview(canvas_state);
                true
            }
            Tool::Gradient if self.gradient_state.drag_start.is_some() => {
                self.commit_gradient(canvas_state);
                true
            }
            Tool::Text if self.text_state.is_editing => {
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
                true
            }
            Tool::Liquify if self.liquify_state.is_active => {
                self.commit_liquify(canvas_state);
                true
            }
            Tool::MeshWarp if self.mesh_warp_state.is_active => {
                self.commit_mesh_warp(canvas_state);
                true
            }
            Tool::Shapes if self.shapes_state.placed.is_some() => {
                self.commit_shape(canvas_state);
                true
            }
            _ => false,
        }
    }

    /// Compact vertical tool strip for floating window
    /// Returns an action indicating what the user clicked
    pub fn show_compact(
        &mut self,
        ui: &mut egui::Ui,
        assets: &Assets,
        primary_color: egui::Color32,
        secondary_color: egui::Color32,
        keybindings: &crate::assets::KeyBindings,
        is_text_layer: bool,
    ) -> ToolsPanelAction {
        let mut action = ToolsPanelAction::None;

        // Clear tool hint each frame ÔÇö only set when hovering a tool button
        self.tool_hint.clear();

        let btn_size = 26.0; // visual button size
        let cols = 3;
        let gap = 6.0; // 1px gap between buttons

        ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
        ui.spacing_mut().button_padding = egui::vec2(2.0, 2.0);

        ui.add_space(4.0);

        // Separator color for between-group dividers (accent-tinted for Signal Grid)
        let accent = ui.visuals().selection.bg_fill;
        let sep_color = if ui.visuals().dark_mode {
            egui::Color32::from_rgb(
                60u8.saturating_add(accent.r() / 8),
                60u8.saturating_add(accent.g() / 8),
                60u8.saturating_add(accent.b() / 8),
            )
        } else {
            egui::Color32::from_gray(200)
        };

        // Tool groups ÔÇö 4 groups of 6, each 2 rows ├ù 3 cols, with separators
        // PAINT: Core painting & fill tools
        let paint_tools: Vec<(Icon, Tool)> = vec![
            (Icon::Brush, Tool::Brush),
            (Icon::Pencil, Tool::Pencil),
            (Icon::Eraser, Tool::Eraser),
            (Icon::Line, Tool::Line),
            (Icon::Fill, Tool::Fill),
            (Icon::Gradient, Tool::Gradient),
        ];
        // SELECT: Region selection & movement
        let select_tools: Vec<(Icon, Tool)> = vec![
            (Icon::RectSelect, Tool::RectangleSelect),
            (Icon::EllipseSelect, Tool::EllipseSelect),
            (Icon::Lasso, Tool::Lasso),
            (Icon::MagicWand, Tool::MagicWand),
            (Icon::MovePixels, Tool::MovePixels),
            (Icon::MoveSelection, Tool::MoveSelection),
        ];
        // RETOUCH & WARP: Repair/clone + distort/transform
        let retouch_tools: Vec<(Icon, Tool)> = vec![
            (Icon::CloneStamp, Tool::CloneStamp),
            (Icon::ContentAwareBrush, Tool::ContentAwareBrush),
            (Icon::ColorRemover, Tool::ColorRemover),
            (Icon::Liquify, Tool::Liquify),
            (Icon::MeshWarp, Tool::MeshWarp),
            (Icon::PerspectiveCrop, Tool::PerspectiveCrop),
        ];
        // UTILITY: Sample, create, navigate
        let utility_tools: Vec<(Icon, Tool)> = vec![
            (Icon::ColorPicker, Tool::ColorPicker),
            (Icon::Text, Tool::Text),
            (Icon::Zoom, Tool::Zoom),
            (Icon::Pan, Tool::Pan),
        ];

        let groups: Vec<&Vec<(Icon, Tool)>> =
            vec![&paint_tools, &select_tools, &retouch_tools, &utility_tools];
        let sep_gap = 11.0; // vertical space for separator lines between groups (5px padding each side + 1px line)
        let grid_w = cols as f32 * btn_size + (cols - 1) as f32 * gap;

        // Calculate total height: all tool rows + separators between groups
        let total_tool_rows: usize = groups.iter().map(|g| g.len().div_ceil(cols)).sum();
        let num_separators = groups.len() - 1;
        let grid_h = total_tool_rows as f32 * btn_size
            + (total_tool_rows - 1) as f32 * gap
            + num_separators as f32 * sep_gap;

        // Allocate exact space for entire tool grid (all groups + separators)
        let (grid_rect, _) =
            ui.allocate_exact_size(egui::vec2(grid_w, grid_h), egui::Sense::hover());

        let mut current_y = grid_rect.min.y;
        let dark_mode = ui.visuals().dark_mode;
        let tool_btn_fill = crate::theme::Theme::icon_button_bg_for(ui);
        let tool_btn_active = crate::theme::Theme::icon_button_active_for(ui);
        let tool_btn_disabled = crate::theme::Theme::icon_button_disabled_for(ui);

        for (gi, group) in groups.iter().enumerate() {
            let group_rows = group.len().div_ceil(cols);

            for (i, (icon, tool)) in group.iter().enumerate() {
                let col = i % cols;
                let row = i / cols;
                let x = grid_rect.min.x + col as f32 * (btn_size + gap);
                let y = current_y + row as f32 * (btn_size + gap);
                let btn_rect =
                    egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(btn_size, btn_size));

                let selected = self.active_tool == *tool;

                // On text layers, only Text/Zoom/Pan are enabled
                let tool_disabled =
                    is_text_layer && !matches!(tool, Tool::Text | Tool::Zoom | Tool::Pan);

                // Manual painting (like Shapes button) for full control over fill/tint
                let resp = ui.allocate_rect(btn_rect, egui::Sense::click());
                let hovered = resp.hovered() && !tool_disabled;

                // Background fill ÔÇö selected > hovered > recessed default
                let fill = if tool_disabled {
                    tool_btn_disabled
                } else if selected {
                    tool_btn_active
                } else if hovered {
                    ui.visuals().widgets.hovered.bg_fill
                } else {
                    tool_btn_fill
                };

                // Accent glow behind active tool
                if selected {
                    let glow_expand = 3.0;
                    let glow_rect = btn_rect.expand(glow_expand);
                    let sel = ui.visuals().selection.bg_fill;
                    let glow_color =
                        egui::Color32::from_rgba_unmultiplied(sel.r(), sel.g(), sel.b(), 40);
                    ui.painter().rect_filled(glow_rect, 6.0, glow_color);
                }

                ui.painter().rect_filled(btn_rect, 4.0, fill);

                // Border
                if selected {
                    ui.painter().rect_stroke(
                        btn_rect,
                        2.0,
                        egui::Stroke::new(2.0, ui.visuals().selection.bg_fill),
                        egui::StrokeKind::Middle,
                    );
                } else if hovered {
                    ui.painter().rect_stroke(
                        btn_rect,
                        4.0,
                        ui.visuals().widgets.hovered.bg_stroke,
                        egui::StrokeKind::Middle,
                    );
                }

                // Draw icon image or emoji fallback
                if let Some(texture) = assets.get_texture(*icon) {
                    let sized_texture = egui::load::SizedTexture::from_handle(texture);
                    let img_size = egui::vec2(btn_size * 0.75, btn_size * 0.75);
                    // Dim disabled tools, tint white in dark mode for contrast
                    let tint = if tool_disabled {
                        egui::Color32::from_rgba_unmultiplied(128, 128, 128, 80)
                    } else {
                        egui::Color32::WHITE
                    };
                    let img = egui::Image::from_texture(sized_texture)
                        .fit_to_exact_size(img_size)
                        .tint(tint);
                    let img_rect = egui::Rect::from_center_size(btn_rect.center(), img_size);
                    img.paint_at(ui, img_rect);
                } else {
                    let text_color = if tool_disabled {
                        egui::Color32::from_rgba_unmultiplied(128, 128, 128, 80)
                    } else if selected {
                        egui::Color32::WHITE
                    } else {
                        ui.visuals().text_color()
                    };
                    ui.painter().text(
                        btn_rect.center(),
                        egui::Align2::CENTER_CENTER,
                        icon.emoji(),
                        egui::FontId::proportional(btn_size * 0.42),
                        text_color,
                    );
                }

                if !tool_disabled
                    && resp
                        .on_hover_text(icon.tooltip_with_keybind(keybindings))
                        .clicked()
                {
                    self.change_tool(*tool);
                }
                if hovered {
                    self.tool_hint = Self::tool_hint_for(*tool);
                }
            }

            // Advance Y past this group's rows
            current_y += group_rows as f32 * btn_size + (group_rows - 1) as f32 * gap;

            // Draw separator line between groups (not after the last group)
            if gi < groups.len() - 1 {
                let sep_y = current_y + sep_gap * 0.5;
                ui.painter().line_segment(
                    [
                        egui::pos2(grid_rect.min.x, sep_y),
                        egui::pos2(grid_rect.min.x + grid_w, sep_y),
                    ],
                    egui::Stroke::new(1.0, sep_color),
                );
                current_y += sep_gap;
            }
        }

        // Shapes ÔÇö double-wide, sits in the last row of the utility group
        {
            // The utility group has 4 tools ÔåÆ row0: 3 tools, row1: Pan only (col 0)
            // Shapes goes in col 1-2 of that last row
            let shape_x = grid_rect.min.x + 1.0 * (btn_size + gap);
            let shape_y = current_y - btn_size; // last row Y (Pan's row)
            let remaining_cols = cols - 1; // 2 columns
            let shape_w = remaining_cols as f32 * btn_size + (remaining_cols - 1) as f32 * gap;
            let shape_rect = egui::Rect::from_min_size(
                egui::pos2(shape_x, shape_y),
                egui::vec2(shape_w, btn_size),
            );

            let icon = Icon::Shapes;
            let is_shapes = self.active_tool == Tool::Shapes;
            let shapes_disabled = is_text_layer;

            let resp = ui.allocate_rect(shape_rect, egui::Sense::click());
            let hovered = resp.hovered() && !shapes_disabled;

            let fill = if shapes_disabled {
                tool_btn_disabled
            } else if is_shapes {
                tool_btn_active
            } else if hovered {
                ui.visuals().widgets.hovered.bg_fill
            } else {
                tool_btn_fill
            };

            // Accent glow behind active Shapes button
            if is_shapes {
                let glow_expand = 3.0;
                let glow_rect = shape_rect.expand(glow_expand);
                let sel = ui.visuals().selection.bg_fill;
                let glow_color =
                    egui::Color32::from_rgba_unmultiplied(sel.r(), sel.g(), sel.b(), 40);
                ui.painter().rect_filled(glow_rect, 6.0, glow_color);
            }

            ui.painter().rect_filled(shape_rect, 4.0, fill);

            if is_shapes {
                ui.painter().rect_stroke(
                    shape_rect,
                    2.0,
                    egui::Stroke::new(2.0, ui.visuals().selection.bg_fill),
                    egui::StrokeKind::Middle,
                );
            } else if hovered {
                ui.painter().rect_stroke(
                    shape_rect,
                    4.0,
                    ui.visuals().widgets.hovered.bg_stroke,
                    egui::StrokeKind::Middle,
                );
            }

            if let Some(texture) = assets.get_texture(icon) {
                let sized_texture = egui::load::SizedTexture::from_handle(texture);
                let img = egui::Image::from_texture(sized_texture)
                    .fit_to_exact_size(egui::vec2(shape_w * 0.75, btn_size * 0.75));
                let img = if shapes_disabled {
                    img.tint(egui::Color32::from_rgba_unmultiplied(128, 128, 128, 80))
                } else if dark_mode && !is_shapes {
                    img.tint(egui::Color32::WHITE)
                } else {
                    img
                };
                let img_rect = egui::Rect::from_center_size(
                    shape_rect.center(),
                    egui::vec2(shape_w * 0.75, btn_size * 0.75),
                );
                img.paint_at(ui, img_rect);
            } else {
                let text_color = if shapes_disabled {
                    egui::Color32::from_rgba_unmultiplied(128, 128, 128, 80)
                } else if is_shapes {
                    egui::Color32::WHITE
                } else {
                    ui.visuals().text_color()
                };
                ui.painter().text(
                    shape_rect.center(),
                    egui::Align2::CENTER_CENTER,
                    icon.emoji(),
                    egui::FontId::proportional(btn_size * 0.42),
                    text_color,
                );
            }

            if !shapes_disabled
                && resp
                    .on_hover_text(icon.tooltip_with_keybind(keybindings))
                    .clicked()
            {
                self.change_tool(Tool::Shapes);
            }
            if hovered {
                self.tool_hint = Self::tool_hint_for(Tool::Shapes);
            }
        }

        ui.add_space(8.0);

        // Separator line before color swatches
        let sep_width = grid_w;
        let (sep_rect, _) =
            ui.allocate_exact_size(egui::vec2(sep_width, 1.0), egui::Sense::hover());
        ui.painter().line_segment(
            [sep_rect.left_center(), sep_rect.right_center()],
            egui::Stroke::new(1.0, sep_color),
        );
        ui.add_space(10.0);

        // Color swatches ÔÇö centered in panel
        ui.horizontal(|ui| {
            ui.add_space(28.0);
            if Self::draw_color_swatch_compact(ui, primary_color, secondary_color) {
                action = ToolsPanelAction::OpenColors;
            }
        });

        ui.add_space(-8.0);

        // Swap button ÔÇö centered in panel (frameless)
        ui.horizontal(|ui| {
            ui.add_space(28.0);
            let clicked = if let Some(texture) = assets.get_texture(Icon::SwapColors) {
                let sized_texture = egui::load::SizedTexture::from_handle(texture);
                let img = egui::Image::from_texture(sized_texture)
                    .fit_to_exact_size(egui::vec2(16.0, 16.0));
                ui.add(egui::Button::image(img).frame(false))
                    .on_hover_text(Icon::SwapColors.tooltip())
                    .clicked()
            } else {
                assets.small_icon_button(ui, Icon::SwapColors).clicked()
            };
            if clicked {
                action = ToolsPanelAction::SwapColors;
            }
        });

        ui.add_space(4.0);

        action
    }

    /// Draw compact overlapping primary/secondary color swatch
    fn draw_color_swatch_compact(
        ui: &mut egui::Ui,
        primary: egui::Color32,
        secondary: egui::Color32,
    ) -> bool {
        let swatch_size = 24.0;
        let offset = 8.0;
        let total_size = egui::vec2(swatch_size + offset, swatch_size + offset);

        let (rect, response) = ui.allocate_exact_size(total_size, egui::Sense::click());

        if ui.is_rect_visible(rect) {
            let painter = ui.painter();

            // Secondary color (back, offset down-right)
            let secondary_rect = egui::Rect::from_min_size(
                rect.min + egui::vec2(offset, offset),
                egui::vec2(swatch_size, swatch_size),
            );
            painter.rect_filled(secondary_rect, 2.0, secondary);
            painter.rect_stroke(
                secondary_rect,
                2.0,
                egui::Stroke::new(1.0, egui::Color32::from_gray(100)),
                egui::StrokeKind::Middle,
            );

            // Primary color (front, top-left)
            let primary_rect =
                egui::Rect::from_min_size(rect.min, egui::vec2(swatch_size, swatch_size));
            painter.rect_filled(primary_rect, 2.0, primary);
            painter.rect_stroke(
                primary_rect,
                2.0,
                egui::Stroke::new(1.0, egui::Color32::from_gray(100)),
                egui::StrokeKind::Middle,
            );
        }

        response.on_hover_text("Click to open Colors").clicked()
    }

    /// Draw larger overlapping primary/secondary color swatch (centered)
    /// Returns true if clicked (to open colors panel)
    fn draw_color_swatch_large(
        ui: &mut egui::Ui,
        primary: egui::Color32,
        secondary: egui::Color32,
    ) -> bool {
        let swatch_size = 32.0;
        let offset = 10.0;
        let total_size = egui::vec2(swatch_size + offset, swatch_size + offset);

        let (rect, response) = ui.allocate_exact_size(total_size, egui::Sense::click());

        if ui.is_rect_visible(rect) {
            let painter = ui.painter();

            // Secondary color (back, offset down-right)
            let secondary_rect = egui::Rect::from_min_size(
                rect.min + egui::vec2(offset, offset),
                egui::vec2(swatch_size, swatch_size),
            );
            painter.rect_filled(secondary_rect, 3.0, secondary);
            painter.rect_stroke(
                secondary_rect,
                3.0,
                egui::Stroke::new(1.0, egui::Color32::from_gray(80)),
                egui::StrokeKind::Middle,
            );

            // Primary color (front, top-left)
            let primary_rect =
                egui::Rect::from_min_size(rect.min, egui::vec2(swatch_size, swatch_size));
            painter.rect_filled(primary_rect, 3.0, primary);
            painter.rect_stroke(
                primary_rect,
                3.0,
                egui::Stroke::new(1.0, egui::Color32::from_gray(80)),
                egui::StrokeKind::Middle,
            );
        }

        response.on_hover_text("Click to open Colors").clicked()
    }

    /// Draw overlapping primary/secondary color swatch
    /// Returns true if clicked (to open colors panel)
    pub fn draw_color_swatch(
        ui: &mut egui::Ui,
        primary: egui::Color32,
        secondary: egui::Color32,
    ) -> bool {
        let swatch_size = 24.0;
        let offset = 8.0;
        let total_size = egui::vec2(swatch_size + offset, swatch_size + offset);

        let (rect, response) = ui.allocate_exact_size(total_size, egui::Sense::click());

        if ui.is_rect_visible(rect) {
            let painter = ui.painter();

            // Secondary color (back, offset down-right)
            let secondary_rect = egui::Rect::from_min_size(
                rect.min + egui::vec2(offset, offset),
                egui::vec2(swatch_size, swatch_size),
            );
            painter.rect_filled(secondary_rect, 2.0, secondary);
            painter.rect_stroke(
                secondary_rect,
                2.0,
                egui::Stroke::new(1.0, egui::Color32::from_gray(80)),
                egui::StrokeKind::Middle,
            );

            // Primary color (front, top-left)
            let primary_rect =
                egui::Rect::from_min_size(rect.min, egui::vec2(swatch_size, swatch_size));
            painter.rect_filled(primary_rect, 2.0, primary);
            painter.rect_stroke(
                primary_rect,
                2.0,
                egui::Stroke::new(1.0, egui::Color32::from_gray(80)),
                egui::StrokeKind::Middle,
            );
        }

        response.on_hover_text("Click to open Colors").clicked()
    }

    /// Original full show method for sidebar (kept for compatibility)
    pub fn show(&mut self, ui: &mut egui::Ui, assets: &Assets) {
        ui.vertical(|ui| {
            ui.heading("Tools");
            ui.separator();

            // Large icon-style tool buttons
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;

                let is_brush = self.active_tool == Tool::Brush;
                if assets.icon_selectable(ui, Icon::Brush, is_brush) {
                    self.change_tool(Tool::Brush);
                }

                let is_eraser = self.active_tool == Tool::Eraser;
                if assets.icon_selectable(ui, Icon::Eraser, is_eraser) {
                    self.change_tool(Tool::Eraser);
                }

                let is_line = self.active_tool == Tool::Line;
                if assets.icon_selectable(ui, Icon::Line, is_line) {
                    self.change_tool(Tool::Line);
                }

                let is_rect_sel = self.active_tool == Tool::RectangleSelect;
                if assets.icon_selectable(ui, Icon::RectSelect, is_rect_sel) {
                    self.change_tool(Tool::RectangleSelect);
                }

                let is_ellipse_sel = self.active_tool == Tool::EllipseSelect;
                if assets.icon_selectable(ui, Icon::EllipseSelect, is_ellipse_sel) {
                    self.change_tool(Tool::EllipseSelect);
                }

                let is_move_px = self.active_tool == Tool::MovePixels;
                if assets.icon_selectable(ui, Icon::MovePixels, is_move_px) {
                    self.change_tool(Tool::MovePixels);
                }

                let is_move_sel = self.active_tool == Tool::MoveSelection;
                if assets.icon_selectable(ui, Icon::MoveSelection, is_move_sel) {
                    self.change_tool(Tool::MoveSelection);
                }
            });

            ui.separator();

            // Tool name label
            let tool_name = match self.active_tool {
                Tool::Brush => t!("tool.brush"),
                Tool::Eraser => t!("tool.eraser"),
                Tool::Pencil => t!("tool.pencil"),
                Tool::Line => t!("tool.line"),
                Tool::RectangleSelect => t!("tool.rectangle_select"),
                Tool::EllipseSelect => t!("tool.ellipse_select"),
                Tool::MovePixels => t!("tool.move_pixels"),
                Tool::MoveSelection => t!("tool.move_selection"),
                Tool::MagicWand => t!("tool.magic_wand"),
                Tool::Fill => t!("tool.fill"),
                Tool::ColorPicker => t!("tool.color_picker"),
                Tool::Gradient => t!("tool.gradient"),
                Tool::ContentAwareBrush => t!("tool.content_aware_fill"),
                Tool::Liquify => t!("tool.liquify"),
                Tool::MeshWarp => t!("tool.mesh_warp"),
                Tool::ColorRemover => t!("tool.color_remover"),
                Tool::Smudge => "Smudge".to_string(),
                Tool::CloneStamp => t!("tool.clone_stamp"),
                Tool::Text => t!("tool.text"),
                Tool::PerspectiveCrop => t!("tool.perspective_crop"),
                Tool::Lasso => t!("tool.lasso"),
                Tool::Zoom => t!("tool.zoom"),
                Tool::Pan => t!("tool.pan"),
                Tool::Shapes => t!("tool.shapes"),
            };
            ui.label(egui::RichText::new(tool_name).strong());
        });
    }

    /// Change tool and handle any cleanup needed (like committing active B├®zier line)
    pub fn change_tool(&mut self, new_tool: Tool) {
        if self.active_tool != new_tool {
            // Deactivate perspective crop when switching away
            if self.active_tool == Tool::PerspectiveCrop {
                self.perspective_crop_state.active = false;
                self.perspective_crop_state.dragging_corner = None;
            }
            self.active_tool = new_tool;
            // Auto-init perspective crop ÔÇö need canvas dims, so flag for
            // lazy init in handle_input on next frame.
            if new_tool == Tool::PerspectiveCrop {
                self.perspective_crop_state.needs_auto_init = true;
            }
            // Note: Actual commitment will be handled in handle_input with canvas_state access
        }
    }

    /// Get the name of the active tool for display in context bar
    pub fn active_tool_name(&self) -> String {
        match self.active_tool {
            Tool::Brush => t!("tool.brush"),
            Tool::Eraser => t!("tool.eraser"),
            Tool::Pencil => t!("tool.pencil"),
            Tool::Line => t!("tool.line"),
            Tool::RectangleSelect => t!("tool.rectangle_select"),
            Tool::EllipseSelect => t!("tool.ellipse_select"),
            Tool::MovePixels => t!("tool.move_pixels"),
            Tool::MoveSelection => t!("tool.move_selection"),
            Tool::MagicWand => t!("tool.magic_wand"),
            Tool::Fill => t!("tool.fill"),
            Tool::ColorPicker => t!("tool.color_picker"),
            Tool::Gradient => t!("tool.gradient"),
            Tool::ContentAwareBrush => t!("tool.content_aware_fill"),
            Tool::Liquify => t!("tool.liquify"),
            Tool::MeshWarp => t!("tool.mesh_warp"),
            Tool::ColorRemover => t!("tool.color_remover"),
            Tool::Smudge => "Smudge".to_string(),
            Tool::CloneStamp => t!("tool.clone_stamp"),
            Tool::Text => t!("tool.text"),
            Tool::PerspectiveCrop => t!("tool.perspective_crop"),
            Tool::Lasso => t!("tool.lasso"),
            Tool::Zoom => t!("tool.zoom"),
            Tool::Pan => t!("tool.pan"),
            Tool::Shapes => t!("tool.shapes"),
        }
    }

    /// Short usage hint for a given tool ÔÇö displayed at bottom-left of the app on hover.
    pub fn tool_hint_for(tool: Tool) -> String {
        match tool {
            Tool::Brush => "Left-click to paint. Right-click for secondary color. Hold Shift for straight lines.".into(),
            Tool::Pencil => "Left-click to draw 1px aliased lines. Hold Shift for straight lines.".into(),
            Tool::Eraser => "Left-click to erase. Removes pixels from the active layer.".into(),
            Tool::Line => "Click and drag to draw a straight line. Adjust width in options.".into(),
            Tool::RectangleSelect => "Click and drag to create a rectangular selection.".into(),
            Tool::EllipseSelect => "Click and drag to create an elliptical selection.".into(),
            Tool::MovePixels => "Click + drag to move selected pixels. No selection = move entire layer.".into(),
            Tool::MoveSelection => "Click + drag to move the selection boundary without affecting pixels.".into(),
            Tool::MagicWand => "Click to select contiguous areas of similar color. Adjust tolerance in options.".into(),
            Tool::Fill => "Click to flood-fill an area with the primary color.".into(),
            Tool::ColorPicker => "Left-click to pick primary color. Right-click for secondary color.".into(),
            Tool::Gradient => "Click and drag to draw a gradient on the active layer.".into(),
            Tool::Lasso => "Click to place points, or drag freehand, to create an irregular selection.".into(),
            Tool::Zoom => "Click to zoom in. Drag a rectangle to zoom to area. Hold Alt to zoom out.".into(),
            Tool::Pan => "Click and drag to pan the canvas viewport.".into(),
            Tool::CloneStamp => "Ctrl+click to set source. Then paint to clone from source area.".into(),
            Tool::ContentAwareBrush => "Paint over an area to remove it using content-aware fill.".into(),
            Tool::Liquify => "Click and drag to push/warp pixels in the brush direction.".into(),
            Tool::MeshWarp => "Drag control points to warp the image with a smooth mesh grid.".into(),
            Tool::ColorRemover => "Paint over a color to remove it, making those pixels transparent.".into(),
            Tool::Smudge => "Click and drag to smudge/blend colors in the stroke direction.".into(),
            Tool::Text => "Click to place text. Configure font, size, and color in options.".into(),
            Tool::PerspectiveCrop => "Drag the four corners to define a perspective crop region.".into(),
            Tool::Shapes => "Click and drag to draw shapes. Hold Shift for constrained proportions.".into(),
        }
    }

    /// Dynamic context bar that shows options based on active tool
    /// This replaces show_properties_toolbar for the floating window layout
    pub fn show_context_bar(
        &mut self,
        ui: &mut egui::Ui,
        assets: &Assets,
        primary_color: Color32,
        secondary_color: Color32,
    ) {
        ui.horizontal(|ui| {
            // Tool name tag badge (Signal Grid style)
            crate::signal_widgets::tool_shelf_tag(
                ui,
                &self.active_tool_name().to_uppercase(),
                ui.visuals().widgets.active.bg_stroke.color,
            );
            ui.add_space(6.0);

            // Rebuild tip mask cache for brush/eraser tools
            // Called both before AND after tool options UI so picker changes
            // take effect in the same frame (picker runs inside show_*_options).
            match self.active_tool {
                Tool::Brush | Tool::Eraser => {
                    self.rebuild_tip_mask(assets);
                }
                _ => {}
            }

            match self.active_tool {
                Tool::Brush | Tool::Pencil => {
                    self.show_brush_options(ui, assets);
                }
                Tool::Line => {
                    self.show_line_options(ui, assets);
                }
                Tool::Eraser => {
                    self.show_eraser_options(ui, assets);
                }
                _ => {}
            }
            // Re-run rebuild_tip_mask after options UI so that picker changes
            // (tip/size/hardness) take effect this frame, not next frame.
            match self.active_tool {
                Tool::Brush | Tool::Eraser => {
                    self.rebuild_tip_mask(assets);
                }
                _ => {}
            }
            match self.active_tool {
                // Already handled above
                Tool::Brush | Tool::Pencil | Tool::Line | Tool::Eraser => {}
                Tool::RectangleSelect | Tool::EllipseSelect => {
                    self.show_selection_options(ui);
                }
                Tool::MovePixels => {
                    // Hint only ÔÇö no options
                }
                Tool::MoveSelection => {
                    // Hint only ÔÇö no options
                }
                Tool::MagicWand => {
                    self.show_magic_wand_options(ui);
                }
                Tool::Fill => {
                    self.show_fill_options(ui);
                }
                Tool::ColorPicker => {
                    // Hint only ÔÇö no options
                }
                Tool::Lasso => {
                    self.show_lasso_options(ui);
                }
                Tool::Zoom => {
                    // Toggle button for zoom direction (touch-friendly)
                    let label = if self.zoom_tool_state.zoom_out_mode {
                        "\u{1F50D}\u{2796} Zoom Out"
                    } else {
                        "\u{1F50D}\u{2795} Zoom In"
                    };
                    if ui
                        .selectable_label(self.zoom_tool_state.zoom_out_mode, label)
                        .clicked()
                    {
                        self.zoom_tool_state.zoom_out_mode = !self.zoom_tool_state.zoom_out_mode;
                    }
                    // Removed inline label ÔÇö zoom hint is in tool_hint
                }
                Tool::Pan => {
                    // Hint only ÔÇö no options
                }
                Tool::PerspectiveCrop => {
                    self.show_perspective_crop_options(ui);
                }
                Tool::Gradient => {
                    self.show_gradient_options(ui, assets, primary_color, secondary_color);
                }
                Tool::Liquify => {
                    self.show_liquify_options(ui);
                }
                Tool::MeshWarp => {
                    self.show_mesh_warp_options(ui);
                }
                Tool::ColorRemover => {
                    self.show_color_remover_options(ui);
                }
                Tool::Smudge => {
                    self.show_smudge_options(ui);
                }
                Tool::Text => {
                    self.show_text_options(ui, assets);
                }
                Tool::Shapes => {
                    self.show_shapes_options(ui, assets);
                }
                Tool::CloneStamp => {
                    self.show_clone_stamp_options(ui);
                }
                Tool::ContentAwareBrush => {
                    self.show_content_aware_options(ui);
                }
            }
        });
    }

    /// Show brush tip picker dropdown (grid popup with categories, matching shapes tool pattern)
    fn show_brush_tip_picker(&mut self, ui: &mut egui::Ui, assets: &Assets) {
        ui.label("Tip:");

        let popup_id = ui.make_persistent_id("brush_tip_grid_popup");
        let display_name = self.properties.brush_tip.display_name().to_string();

        // Button showing current tip icon + name
        let btn_response = {
            if let BrushTip::Image(ref name) = self.properties.brush_tip {
                if let Some(tex) = assets.get_brush_tip_texture(name) {
                    let sized = egui::load::SizedTexture::from_handle(tex);
                    let img =
                        egui::Image::from_texture(sized).fit_to_exact_size(egui::Vec2::splat(16.0));
                    let btn = egui::Button::image_and_text(img, &display_name);
                    ui.add(btn)
                } else {
                    ui.button(&display_name)
                }
            } else {
                // Circle ÔÇö draw a small filled circle icon on the button
                let btn = ui.button(format!("      {}", display_name));
                let rect = btn.rect;
                let circle_x = rect.left() + 14.0;
                let circle_y = rect.center().y;
                ui.painter().circle_filled(
                    egui::Pos2::new(circle_x, circle_y),
                    5.0,
                    ui.visuals().text_color(),
                );
                btn
            }
        };
        if btn_response.clicked() {
            egui::Popup::toggle_id(ui.ctx(), popup_id);
        }

        egui::Popup::new(
            popup_id,
            ui.ctx().clone(),
            egui::PopupAnchor::from(&btn_response),
            ui.layer_id(),
        )
        .open_memory(None::<egui::SetOpenCommand>)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .show(|ui| {
            ui.set_min_width(240.0);

            let cols = 5;
            let icon_size = egui::Vec2::splat(36.0);
            let accent = ui.visuals().hyperlink_color;

            // "Basic" category header always first, with Circle as first item
            ui.label(egui::RichText::new("Basic").strong().size(11.0));
            egui::Grid::new("brush_tip_basic_grid")
                .spacing(egui::Vec2::splat(2.0))
                .show(ui, |ui| {
                    // Circle (built-in, always first)
                    let selected = self.properties.brush_tip.is_circle();
                    let (rect, response) = ui.allocate_exact_size(icon_size, egui::Sense::click());
                    if selected {
                        ui.painter().rect_filled(
                            rect,
                            4.0,
                            egui::Color32::from_rgba_premultiplied(
                                accent.r(),
                                accent.g(),
                                accent.b(),
                                60,
                            ),
                        );
                    }
                    if response.hovered() {
                        ui.painter()
                            .rect_filled(rect, 4.0, ui.visuals().widgets.hovered.bg_fill);
                    }
                    // Draw a circle icon
                    let center = rect.center();
                    let r = icon_size.x * 0.3;
                    let stroke_color = ui.visuals().text_color();
                    ui.painter().circle_filled(center, r, stroke_color);
                    if response.clicked() {
                        self.properties.brush_tip = BrushTip::Circle;
                    }
                    response.on_hover_text("Circle");

                    // Other tips in the "Basic" category
                    let mut col = 1;
                    let categories = assets.brush_tip_categories();
                    if let Some(basic_cat) = categories.iter().find(|c| c.name == "Basic") {
                        for tip_name in &basic_cat.tips {
                            let is_selected =
                                self.properties.brush_tip == BrushTip::Image(tip_name.clone());
                            let (rect, response) =
                                ui.allocate_exact_size(icon_size, egui::Sense::click());
                            if is_selected {
                                ui.painter().rect_filled(
                                    rect,
                                    4.0,
                                    egui::Color32::from_rgba_premultiplied(
                                        accent.r(),
                                        accent.g(),
                                        accent.b(),
                                        60,
                                    ),
                                );
                            }
                            if response.hovered() {
                                ui.painter().rect_filled(
                                    rect,
                                    4.0,
                                    ui.visuals().widgets.hovered.bg_fill,
                                );
                            }
                            if let Some(tex) = assets.get_brush_tip_texture(tip_name) {
                                let sized = egui::load::SizedTexture::from_handle(tex);
                                let img = egui::Image::from_texture(sized)
                                    .fit_to_exact_size(icon_size * 0.8);
                                let inner_rect = rect.shrink(icon_size.x * 0.1);
                                img.paint_at(ui, inner_rect);
                            } else {
                                ui.painter().text(
                                    rect.center(),
                                    egui::Align2::CENTER_CENTER,
                                    &tip_name[..2.min(tip_name.len())],
                                    egui::FontId::proportional(11.0),
                                    ui.visuals().text_color(),
                                );
                            }
                            if response.clicked() {
                                self.properties.brush_tip = BrushTip::Image(tip_name.clone());
                            }
                            response.on_hover_text(tip_name);
                            col += 1;
                            if col % cols == 0 {
                                ui.end_row();
                            }
                        }
                    }
                });

            // Remaining categories
            let categories = assets.brush_tip_categories();
            for cat in categories.iter().filter(|c| c.name != "Basic") {
                ui.add_space(4.0);
                ui.label(egui::RichText::new(&cat.name).strong().size(11.0));
                egui::Grid::new(format!("brush_tip_{}_grid", cat.name))
                    .spacing(egui::Vec2::splat(2.0))
                    .show(ui, |ui| {
                        for (i, tip_name) in cat.tips.iter().enumerate() {
                            let is_selected =
                                self.properties.brush_tip == BrushTip::Image(tip_name.clone());
                            let (rect, response) =
                                ui.allocate_exact_size(icon_size, egui::Sense::click());
                            if is_selected {
                                ui.painter().rect_filled(
                                    rect,
                                    4.0,
                                    egui::Color32::from_rgba_premultiplied(
                                        accent.r(),
                                        accent.g(),
                                        accent.b(),
                                        60,
                                    ),
                                );
                            }
                            if response.hovered() {
                                ui.painter().rect_filled(
                                    rect,
                                    4.0,
                                    ui.visuals().widgets.hovered.bg_fill,
                                );
                            }
                            if let Some(tex) = assets.get_brush_tip_texture(tip_name) {
                                let sized = egui::load::SizedTexture::from_handle(tex);
                                let img = egui::Image::from_texture(sized)
                                    .fit_to_exact_size(icon_size * 0.8);
                                let inner_rect = rect.shrink(icon_size.x * 0.1);
                                img.paint_at(ui, inner_rect);
                            } else {
                                ui.painter().text(
                                    rect.center(),
                                    egui::Align2::CENTER_CENTER,
                                    &tip_name[..2.min(tip_name.len())],
                                    egui::FontId::proportional(11.0),
                                    ui.visuals().text_color(),
                                );
                            }
                            if response.clicked() {
                                self.properties.brush_tip = BrushTip::Image(tip_name.clone());
                            }
                            response.on_hover_text(tip_name);
                            if (i + 1) % cols == 0 {
                                ui.end_row();
                            }
                        }
                    });
            }
        });
    }

    /// Size widget with +/- buttons, drag value, and preset dropdown.
    /// The DragValue and dropdown arrow are merged into a single bordered control.
    fn show_size_widget(&mut self, ui: &mut egui::Ui, combo_id: &str, assets: &Assets) {
        ui.label(t!("ctx.size"));
        let popup_id = ui.make_persistent_id(combo_id);

        if ui.small_button("\u{2212}").clicked() {
            self.properties.size = (self.properties.size - 1.0).max(1.0);
        }

        // Merged DragValue + dropdown arrow in one frame
        let inactive = ui.visuals().widgets.inactive;
        let frame_resp = egui::Frame::NONE
            .fill(inactive.bg_fill)
            .stroke(inactive.bg_stroke)
            .corner_radius(inactive.corner_radius)
            .inner_margin(egui::Margin::same(0))
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                // Make inner widgets frameless ÔÇö the outer Frame provides the border
                let vis = ui.visuals_mut();
                vis.widgets.inactive.bg_fill = Color32::TRANSPARENT;
                vis.widgets.inactive.bg_stroke = egui::Stroke::NONE;
                vis.widgets.hovered.bg_fill = Color32::TRANSPARENT;
                vis.widgets.hovered.bg_stroke = egui::Stroke::NONE;
                vis.widgets.active.bg_fill = Color32::TRANSPARENT;
                vis.widgets.active.bg_stroke = egui::Stroke::NONE;

                let dv_resp = ui.add(
                    egui::DragValue::new(&mut self.properties.size)
                        .speed(0.5)
                        .range(1.0..=256.0)
                        .suffix("px"),
                );
                let dv_rect = dv_resp.rect;
                let dv_height = dv_rect.height();
                dv_resp.on_hover_text(t!("ctx.size_drag_tooltip"));

                // Thin internal divider
                let sep_x = ui.cursor().left();
                ui.painter().vline(
                    sep_x,
                    dv_rect.top() + 3.0..=dv_rect.bottom() - 3.0,
                    egui::Stroke::new(1.0, inactive.bg_stroke.color.linear_multiply(0.4)),
                );

                if let Some(tex) = assets.get_texture(Icon::DropDown) {
                    let sized = egui::load::SizedTexture::from_handle(tex);
                    let img =
                        egui::Image::from_texture(sized).fit_to_exact_size(egui::vec2(12.0, 12.0));
                    ui.add(egui::Button::image(img).min_size(egui::vec2(14.0, dv_height)))
                } else {
                    ui.add(
                        egui::Button::new(egui::RichText::new("\u{25BE}").size(9.0))
                            .min_size(egui::vec2(14.0, dv_height)),
                    )
                }
            });

        let arrow_resp = frame_resp.inner;
        if arrow_resp.clicked() {
            egui::Popup::toggle_id(ui.ctx(), popup_id);
        }
        // Anchor popup below the whole merged control, not just the arrow
        egui::Popup::new(
            popup_id,
            ui.ctx().clone(),
            egui::PopupAnchor::from(&frame_resp.response),
            ui.layer_id(),
        )
        .open_memory(None::<egui::SetOpenCommand>)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .show(|ui| {
            ui.set_min_width(80.0);
            for &preset in BRUSH_SIZE_PRESETS.iter() {
                let label = format!("{:.0} px", preset);
                if ui
                    .selectable_label((self.properties.size - preset).abs() < 0.1, &label)
                    .clicked()
                {
                    self.properties.size = preset;
                    egui::Popup::close_id(ui.ctx(), popup_id);
                }
            }
        });
        if ui.small_button("+").clicked() {
            self.properties.size = (self.properties.size + 1.0).min(256.0);
        }
    }

    /// Show text font-size widget: merged DragValue + dropdown (same as brush size).
    fn show_text_size_widget(&mut self, ui: &mut egui::Ui, combo_id: &str, assets: &Assets) {
        ui.label(t!("ctx.size"));
        let popup_id = ui.make_persistent_id(combo_id);

        if ui.small_button("\u{2212}").clicked() {
            self.text_state.font_size = (self.text_state.font_size - 1.0).max(6.0);
            self.text_state.preview_dirty = true;
            self.text_state.glyph_cache.clear();
            self.text_state.ctx_bar_style_dirty = true;
        }

        let inactive = ui.visuals().widgets.inactive;
        let frame_resp = egui::Frame::NONE
            .fill(inactive.bg_fill)
            .stroke(inactive.bg_stroke)
            .corner_radius(inactive.corner_radius)
            .inner_margin(egui::Margin::same(0))
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                let vis = ui.visuals_mut();
                vis.widgets.inactive.bg_fill = Color32::TRANSPARENT;
                vis.widgets.inactive.bg_stroke = egui::Stroke::NONE;
                vis.widgets.hovered.bg_fill = Color32::TRANSPARENT;
                vis.widgets.hovered.bg_stroke = egui::Stroke::NONE;
                vis.widgets.active.bg_fill = Color32::TRANSPARENT;
                vis.widgets.active.bg_stroke = egui::Stroke::NONE;

                let dv_resp = ui.add(
                    egui::DragValue::new(&mut self.text_state.font_size)
                        .speed(0.5)
                        .range(6.0..=f32::MAX)
                        .suffix("px"),
                );
                if dv_resp.changed() {
                    self.text_state.preview_dirty = true;
                    self.text_state.glyph_cache.clear();
                    self.text_state.ctx_bar_style_dirty = true;
                }
                let dv_rect = dv_resp.rect;
                let dv_height = dv_rect.height();
                dv_resp.on_hover_text(t!("ctx.size_drag_tooltip"));

                let sep_x = ui.cursor().left();
                ui.painter().vline(
                    sep_x,
                    dv_rect.top() + 3.0..=dv_rect.bottom() - 3.0,
                    egui::Stroke::new(1.0, inactive.bg_stroke.color.linear_multiply(0.4)),
                );

                if let Some(tex) = assets.get_texture(Icon::DropDown) {
                    let sized = egui::load::SizedTexture::from_handle(tex);
                    let img =
                        egui::Image::from_texture(sized).fit_to_exact_size(egui::vec2(12.0, 12.0));
                    ui.add(egui::Button::image(img).min_size(egui::vec2(14.0, dv_height)))
                } else {
                    ui.add(
                        egui::Button::new(egui::RichText::new("\u{25BE}").size(9.0))
                            .min_size(egui::vec2(14.0, dv_height)),
                    )
                }
            });

        let arrow_resp = frame_resp.inner;
        if arrow_resp.clicked() {
            egui::Popup::toggle_id(ui.ctx(), popup_id);
        }
        egui::Popup::new(
            popup_id,
            ui.ctx().clone(),
            egui::PopupAnchor::from(&frame_resp.response),
            ui.layer_id(),
        )
        .open_memory(None::<egui::SetOpenCommand>)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .show(|ui| {
            ui.set_min_width(80.0);
            for &preset in TEXT_SIZE_PRESETS.iter() {
                let label = format!("{:.0} px", preset);
                if ui
                    .selectable_label((self.text_state.font_size - preset).abs() < 0.1, &label)
                    .clicked()
                {
                    self.text_state.font_size = preset;
                    self.text_state.preview_dirty = true;
                    self.text_state.glyph_cache.clear();
                    self.text_state.ctx_bar_style_dirty = true;
                    egui::Popup::close_id(ui.ctx(), popup_id);
                }
            }
        });
        if ui.small_button("+").clicked() {
            self.text_state.font_size += 1.0;
            self.text_state.preview_dirty = true;
            self.text_state.glyph_cache.clear();
            self.text_state.ctx_bar_style_dirty = true;
        }
    }

    /// Show brush-specific options (size, hardness, blend mode)
    /// For Pencil tool, skip size and hardness since it always paints single pixels
    fn show_brush_options(&mut self, ui: &mut egui::Ui, assets: &Assets) {
        // Brush tip picker (skip for Pencil ÔÇö always pixel)
        if self.active_tool != Tool::Pencil {
            self.show_brush_tip_picker(ui, assets);
            ui.separator();
        }

        // Size - skip for Pencil tool since it always paints single pixels
        if self.active_tool != Tool::Pencil {
            self.show_size_widget(ui, "ctx_brush_size", assets);

            ui.separator();
        }

        // Hardness - skip for Pencil tool since it always paints single pixels and Line tool
        if self.active_tool != Tool::Pencil && self.active_tool != Tool::Line {
            ui.label(t!("ctx.hardness"));
            let mut hardness_pct = (self.properties.hardness * 100.0).round();
            if ui
                .add(
                    egui::DragValue::new(&mut hardness_pct)
                        .speed(1.0)
                        .range(0.0..=100.0)
                        .suffix("%"),
                )
                .on_hover_text(t!("ctx.hardness_brush_tooltip"))
                .changed()
            {
                self.properties.hardness = hardness_pct / 100.0;
            }

            ui.separator();
        }

        // Blend Mode
        ui.label(t!("ctx.blend"));
        let current_mode = self.properties.blending_mode;
        egui::ComboBox::from_id_salt("ctx_blend_mode")
            .selected_text(current_mode.name())
            .width(90.0)
            .show_ui(ui, |ui| {
                for &mode in BlendMode::all() {
                    if ui
                        .selectable_label(mode == current_mode, mode.name())
                        .clicked()
                    {
                        self.properties.blending_mode = mode;
                    }
                }
            });

        ui.separator();

        // Anti-aliasing toggle (compact "AA" matching Text tool)
        let aa_resp = ui.selectable_label(self.properties.anti_aliased, t!("ctx.anti_alias"));
        if aa_resp.clicked() {
            self.properties.anti_aliased = !self.properties.anti_aliased;
        }
        aa_resp.on_hover_text(t!("ctx.anti_alias_tooltip"));

        // Brush Mode (Normal/Dodge/Burn/Sponge) - only for Brush tool (disabled for now)
        if self.active_tool == Tool::Brush {
            ui.separator();
            ui.add_enabled_ui(false, |ui| {
                ui.label("Mode:");
                let current_bm = self.properties.brush_mode;
                egui::ComboBox::from_id_salt("ctx_brush_mode")
                    .selected_text(current_bm.label())
                    .width(70.0)
                    .show_ui(ui, |ui| {
                        for &mode in BrushMode::all() {
                            let _ = ui.selectable_label(mode == current_bm, mode.label());
                        }
                    });
            });
        }

        // Dynamics popup: Scatter, Color Jitter
        ui.separator();
        let dyn_active = self.properties.scatter > 0.01
            || self.properties.hue_jitter > 0.01
            || self.properties.brightness_jitter > 0.01
            || self.properties.pressure_size
            || self.properties.pressure_opacity;
        let dyn_popup_id = ui.make_persistent_id("brush_dyn_popup");
        let dyn_resp = assets.icon_button(ui, Icon::UiBrushDynamics, egui::Vec2::splat(20.0));
        if dyn_active {
            let dot = egui::pos2(dyn_resp.rect.max.x - 3.5, dyn_resp.rect.min.y + 3.5);
            ui.painter()
                .circle_filled(dot, 2.5, ui.visuals().hyperlink_color);
        }
        if dyn_resp.clicked() {
            egui::Popup::toggle_id(ui.ctx(), dyn_popup_id);
        }
        egui::Popup::new(
            dyn_popup_id,
            ui.ctx().clone(),
            egui::PopupAnchor::from(&dyn_resp),
            ui.layer_id(),
        )
        .open_memory(None::<egui::SetOpenCommand>)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .show(|ui| {
            ui.set_min_width(220.0);
            egui::Grid::new("brush_dynamics_popup")
                .num_columns(2)
                .spacing([8.0, 4.0])
                .show(ui, |ui| {
                    ui.label("Scatter");
                    let mut scatter_pct = (self.properties.scatter * 200.0).round();
                    if ui
                        .add(
                            egui::Slider::new(&mut scatter_pct, 0.0..=200.0)
                                .suffix("%")
                                .max_decimals(0),
                        )
                        .changed()
                    {
                        self.properties.scatter = scatter_pct / 200.0;
                    }
                    ui.end_row();
                    ui.label("Hue Jitter");
                    let mut hj_pct = (self.properties.hue_jitter * 100.0).round();
                    if ui
                        .add(
                            egui::Slider::new(&mut hj_pct, 0.0..=100.0)
                                .suffix("%")
                                .max_decimals(0),
                        )
                        .changed()
                    {
                        self.properties.hue_jitter = hj_pct / 100.0;
                    }
                    ui.end_row();
                    ui.label("Brightness");
                    let mut bj_pct = (self.properties.brightness_jitter * 100.0).round();
                    if ui
                        .add(
                            egui::Slider::new(&mut bj_pct, 0.0..=100.0)
                                .suffix("%")
                                .max_decimals(0),
                        )
                        .changed()
                    {
                        self.properties.brightness_jitter = bj_pct / 100.0;
                    }
                    ui.end_row();

                    // Pen pressure sensitivity
                    ui.separator();
                    ui.separator();
                    ui.end_row();

                    ui.label("Pen Pressure");
                    ui.label("");
                    ui.end_row();

                    ui.checkbox(&mut self.properties.pressure_size, "Size")
                        .on_hover_text("Pen pressure controls brush size");
                    let mut min_size_pct = (self.properties.pressure_min_size * 100.0).round();
                    if ui
                        .add_enabled(
                            self.properties.pressure_size,
                            egui::Slider::new(&mut min_size_pct, 1.0..=100.0)
                                .suffix("% min")
                                .max_decimals(0),
                        )
                        .on_hover_text("Minimum brush size at zero pressure")
                        .changed()
                    {
                        self.properties.pressure_min_size = min_size_pct / 100.0;
                    }
                    ui.end_row();

                    ui.checkbox(&mut self.properties.pressure_opacity, "Opacity")
                        .on_hover_text("Pen pressure controls brush opacity");
                    let mut min_opacity_pct =
                        (self.properties.pressure_min_opacity * 100.0).round();
                    if ui
                        .add_enabled(
                            self.properties.pressure_opacity,
                            egui::Slider::new(&mut min_opacity_pct, 1.0..=100.0)
                                .suffix("% min")
                                .max_decimals(0),
                        )
                        .on_hover_text("Minimum opacity at zero pressure")
                        .changed()
                    {
                        self.properties.pressure_min_opacity = min_opacity_pct / 100.0;
                    }
                    ui.end_row();
                });
        });

        // Spacing slider (only for image tips, not circle)
        if !self.properties.brush_tip.is_circle() && self.active_tool != Tool::Pencil {
            ui.separator();
            ui.label(t!("ctx.spacing"));
            let mut spacing_pct = (self.properties.spacing * 100.0).round();
            if ui
                .add(
                    egui::DragValue::new(&mut spacing_pct)
                        .speed(1.0)
                        .range(1.0..=200.0)
                        .suffix("%"),
                )
                .on_hover_text(t!("ctx.spacing_tooltip"))
                .changed()
            {
                self.properties.spacing = spacing_pct / 100.0;
            }

            // Rotation controls
            self.show_tip_rotation_controls(ui);
        }
    }

    /// Show rotation controls for non-circle brush tips.
    /// Provides a fixed-angle slider OR a random-range double-slider, plus a checkbox to toggle.
    fn show_tip_rotation_controls(&mut self, ui: &mut egui::Ui) {
        ui.separator();

        // Always show "Angle:" label first
        ui.label(t!("ctx.angle"));

        if self.properties.tip_random_rotation {
            // --- Random rotation mode: range controls ---
            let mut lo = self.properties.tip_rotation_range.0;
            let mut hi = self.properties.tip_rotation_range.1;

            // Min handle
            if ui
                .add(
                    egui::DragValue::new(&mut lo)
                        .speed(1.0)
                        .range(0.0..=360.0)
                        .suffix("┬░"),
                )
                .changed()
                && lo > hi
            {
                hi = lo;
            }

            // Painted range bar
            let bar_width = 100.0;
            let bar_height = 14.0;
            let (bar_rect, _bar_resp) = ui
                .allocate_exact_size(egui::Vec2::new(bar_width, bar_height), egui::Sense::hover());

            // Background track
            let track_color = ui.visuals().widgets.inactive.bg_fill;
            ui.painter().rect_filled(bar_rect, 3.0, track_color);

            // Highlighted portion
            let frac_lo = lo / 360.0;
            let frac_hi = hi / 360.0;
            let fill_left = bar_rect.left() + frac_lo * bar_width;
            let fill_right = bar_rect.left() + frac_hi * bar_width;
            if fill_right > fill_left + 1.0 {
                let fill_rect = egui::Rect::from_min_max(
                    egui::Pos2::new(fill_left, bar_rect.top()),
                    egui::Pos2::new(fill_right, bar_rect.bottom()),
                );
                let accent = ui.visuals().hyperlink_color;
                let fill_color =
                    egui::Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), 140);
                ui.painter().rect_filled(fill_rect, 3.0, fill_color);
            }

            // Handle markers at the edges of the range
            let handle_r = 4.0;
            let handle_color = ui.visuals().text_color();
            ui.painter().circle_filled(
                egui::Pos2::new(fill_left, bar_rect.center().y),
                handle_r,
                handle_color,
            );
            ui.painter().circle_filled(
                egui::Pos2::new(fill_right, bar_rect.center().y),
                handle_r,
                handle_color,
            );

            // Max handle
            if ui
                .add(
                    egui::DragValue::new(&mut hi)
                        .speed(1.0)
                        .range(0.0..=360.0)
                        .suffix("┬░"),
                )
                .changed()
                && hi < lo
            {
                lo = hi;
            }

            self.properties.tip_rotation_range = (lo, hi);
            // In random mode, the cursor doesn't show a specific rotation
            self.active_tip_rotation_deg = 0.0;
        } else {
            // --- Fixed rotation mode: single angle value ---
            ui.add(
                egui::DragValue::new(&mut self.properties.tip_rotation)
                    .speed(1.0)
                    .range(0.0..=359.0)
                    .suffix("┬░"),
            )
            .on_hover_text(t!("ctx.rotation_tooltip"));
            self.active_tip_rotation_deg = self.properties.tip_rotation;
        }

        // Random checkbox ÔÇö to the right of the angle controls
        let rnd_resp = ui.selectable_label(self.properties.tip_random_rotation, t!("ctx.random"));
        if rnd_resp.clicked() {
            self.properties.tip_random_rotation = !self.properties.tip_random_rotation;
        }
        rnd_resp.on_hover_text(t!("ctx.random_tooltip"));
    }

    /// Show line tool-specific options (size, cap style, pattern, blend mode)
    fn show_line_options(&mut self, ui: &mut egui::Ui, assets: &Assets) {
        // Size
        self.show_size_widget(ui, "ctx_line_size", assets);

        ui.separator();

        // Line Pattern
        ui.label(t!("ctx.pattern"));
        let current_pattern = self.line_state.line_tool.options.pattern;
        egui::ComboBox::from_id_salt("ctx_line_pattern")
            .selected_text(current_pattern.label())
            .width(70.0)
            .show_ui(ui, |ui| {
                for &pattern in LinePattern::all() {
                    if ui
                        .selectable_label(pattern == current_pattern, pattern.label())
                        .clicked()
                    {
                        self.line_state.line_tool.options.pattern = pattern;
                    }
                }
            });

        ui.separator();

        // End Shape
        ui.label(t!("ctx.ends"));
        let current_end_shape = self.line_state.line_tool.options.end_shape;
        egui::ComboBox::from_id_salt("ctx_line_end_shape")
            .selected_text(current_end_shape.label())
            .width(60.0)
            .show_ui(ui, |ui| {
                for &shape in LineEndShape::all() {
                    if ui
                        .selectable_label(shape == current_end_shape, shape.label())
                        .clicked()
                    {
                        self.line_state.line_tool.options.end_shape = shape;
                    }
                }
            });

        // Arrow Side (only visible when Arrow is selected)
        if self.line_state.line_tool.options.end_shape == LineEndShape::Arrow {
            let current_arrow_side = self.line_state.line_tool.options.arrow_side;
            egui::ComboBox::from_id_salt("ctx_line_arrow_side")
                .selected_text(current_arrow_side.label())
                .width(55.0)
                .show_ui(ui, |ui| {
                    for &side in ArrowSide::all() {
                        if ui
                            .selectable_label(side == current_arrow_side, side.label())
                            .clicked()
                        {
                            self.line_state.line_tool.options.arrow_side = side;
                        }
                    }
                });
        }

        ui.separator();

        // Anti-aliasing toggle (matches brush/eraser style)
        let aa_resp = ui.selectable_label(
            self.line_state.line_tool.options.anti_alias,
            t!("ctx.anti_alias"),
        );
        if aa_resp.clicked() {
            self.line_state.line_tool.options.anti_alias =
                !self.line_state.line_tool.options.anti_alias;
        }
        aa_resp.on_hover_text(t!("ctx.anti_alias_tooltip"));

        ui.separator();

        // Blend Mode
        ui.label(t!("ctx.blend"));
        let current_mode = self.properties.blending_mode;
        egui::ComboBox::from_id_salt("ctx_line_blend_mode")
            .selected_text(current_mode.name())
            .width(90.0)
            .show_ui(ui, |ui| {
                for &mode in BlendMode::all() {
                    if ui
                        .selectable_label(mode == current_mode, mode.name())
                        .clicked()
                    {
                        self.properties.blending_mode = mode;
                    }
                }
            });
    }

    /// Show eraser-specific options (size, hardness - opacity from color alpha)
    fn show_eraser_options(&mut self, ui: &mut egui::Ui, assets: &Assets) {
        // Brush tip picker
        self.show_brush_tip_picker(ui, assets);
        ui.separator();

        // Size
        self.show_size_widget(ui, "ctx_eraser_size", assets);

        ui.separator();

        // Hardness
        ui.label(t!("ctx.hardness"));
        let mut hardness_pct = (self.properties.hardness * 100.0).round();
        if ui
            .add(
                egui::DragValue::new(&mut hardness_pct)
                    .speed(1.0)
                    .range(0.0..=100.0)
                    .suffix("%"),
            )
            .on_hover_text(t!("ctx.hardness_eraser_tooltip"))
            .changed()
        {
            self.properties.hardness = hardness_pct / 100.0;
        }

        ui.separator();

        // Anti-aliasing toggle (compact "AA" matching Text tool)
        let aa_resp = ui.selectable_label(self.properties.anti_aliased, t!("ctx.anti_alias"));
        if aa_resp.clicked() {
            self.properties.anti_aliased = !self.properties.anti_aliased;
        }
        aa_resp.on_hover_text(t!("ctx.anti_alias_tooltip"));

        // Dynamics popup for eraser: Scatter only
        ui.separator();
        let dyn_active_eraser = self.properties.scatter > 0.01;
        let dyn_popup_id_e = ui.make_persistent_id("eraser_dyn_popup");
        let dyn_resp_e = assets.icon_button(ui, Icon::UiBrushDynamics, egui::Vec2::splat(20.0));
        if dyn_active_eraser {
            let dot_e = egui::pos2(dyn_resp_e.rect.max.x - 3.5, dyn_resp_e.rect.min.y + 3.5);
            ui.painter()
                .circle_filled(dot_e, 2.5, ui.visuals().hyperlink_color);
        }
        if dyn_resp_e.clicked() {
            egui::Popup::toggle_id(ui.ctx(), dyn_popup_id_e);
        }
        egui::Popup::new(
            dyn_popup_id_e,
            ui.ctx().clone(),
            egui::PopupAnchor::from(&dyn_resp_e),
            ui.layer_id(),
        )
        .open_memory(None::<egui::SetOpenCommand>)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .show(|ui| {
            ui.set_min_width(180.0);
            egui::Grid::new("eraser_dynamics_popup")
                .num_columns(2)
                .spacing([8.0, 4.0])
                .show(ui, |ui| {
                    ui.label("Scatter");
                    let mut scatter_pct = (self.properties.scatter * 200.0).round();
                    if ui
                        .add(
                            egui::Slider::new(&mut scatter_pct, 0.0..=200.0)
                                .suffix("%")
                                .max_decimals(0),
                        )
                        .changed()
                    {
                        self.properties.scatter = scatter_pct / 200.0;
                    }
                    ui.end_row();
                });
        });

        ui.separator();

        // Show current eraser opacity from color alpha
        let opacity_pct = (self.properties.color.a() as f32 / 255.0 * 100.0).round();
        ui.label(t!("ctx.opacity_label").replace("{0}", &format!("{:.0}", opacity_pct)))
            .on_hover_text(t!("ctx.eraser_opacity_tooltip"));

        // Spacing slider (only for image tips)
        if !self.properties.brush_tip.is_circle() {
            ui.separator();
            ui.label(t!("ctx.spacing"));
            let mut spacing_pct = (self.properties.spacing * 100.0).round();
            if ui
                .add(
                    egui::DragValue::new(&mut spacing_pct)
                        .speed(1.0)
                        .range(1.0..=200.0)
                        .suffix("%"),
                )
                .on_hover_text(t!("ctx.spacing_tooltip"))
                .changed()
            {
                self.properties.spacing = spacing_pct / 100.0;
            }

            // Rotation controls (shared helper)
            self.show_tip_rotation_controls(ui);
        }
    }

    /// Show selection-tool-specific options (mode dropdown)
    fn show_selection_options(&mut self, ui: &mut egui::Ui) {
        ui.label(t!("ctx.mode"));
        let current = self.selection_state.mode;
        egui::ComboBox::from_id_salt("ctx_sel_mode")
            .selected_text(current.label())
            .width(90.0)
            .show_ui(ui, |ui| {
                for &mode in SelectionMode::all() {
                    if ui.selectable_label(mode == current, mode.label()).clicked() {
                        self.selection_state.mode = mode;
                    }
                }
            });

        ui.separator();

        self.show_sel_modify_controls(ui);

        ui.separator();
        ui.label(t!("ctx.selection_hint"));
    }

    /// Show lasso-tool-specific options (mode dropdown)
    fn show_lasso_options(&mut self, ui: &mut egui::Ui) {
        ui.label(t!("ctx.mode"));
        let current = self.lasso_state.mode;
        egui::ComboBox::from_id_salt("ctx_lasso_mode")
            .selected_text(current.label())
            .width(90.0)
            .show_ui(ui, |ui| {
                for &mode in SelectionMode::all() {
                    if ui.selectable_label(mode == current, mode.label()).clicked() {
                        self.lasso_state.mode = mode;
                    }
                }
            });
        ui.separator();
        self.show_sel_modify_controls(ui);
        ui.separator();
        ui.label(t!("ctx.lasso_hint"));
    }

    /// Inline Feather / Expand / Contract controls for all selection tool context bars.
    fn show_sel_modify_controls(&mut self, ui: &mut egui::Ui) {
        ui.label("Modify:");
        ui.add(
            egui::DragValue::new(&mut self.sel_modify_radius)
                .range(1.0..=200.0)
                .speed(0.5)
                .suffix("px")
                .max_decimals(0),
        )
        .on_hover_text("Radius in pixels for Feather / Expand / Contract");
        if ui
            .button("Feather")
            .on_hover_text("Blur (soften) selection edge by radius")
            .clicked()
        {
            self.pending_sel_modify = Some(SelectionModifyOp::Feather(self.sel_modify_radius));
        }
        if ui
            .button("Expand")
            .on_hover_text("Grow selection by radius")
            .clicked()
        {
            self.pending_sel_modify = Some(SelectionModifyOp::Expand(
                self.sel_modify_radius.round() as i32,
            ));
        }
        if ui
            .button("Contract")
            .on_hover_text("Shrink selection by radius")
            .clicked()
        {
            self.pending_sel_modify = Some(SelectionModifyOp::Contract(
                self.sel_modify_radius.round() as i32,
            ));
        }
    }

    /// Show perspective crop options
    fn show_perspective_crop_options(&mut self, ui: &mut egui::Ui) {
        if self.perspective_crop_state.active {
            ui.label(t!("ctx.perspective_crop_active"));
            if ui.button(t!("ctx.apply")).clicked() {
                // Set a flag ÔÇö actual cropping happens in handle_input
                // which has canvas_state access
            }
            if ui.button(t!("ctx.cancel")).clicked() {
                self.perspective_crop_state.active = false;
            }
        } else {
            ui.label(t!("ctx.perspective_crop_inactive"));
        }
    }

    fn show_clone_stamp_options(&mut self, ui: &mut egui::Ui) {
        // Size
        ui.label(t!("ctx.size"));
        ui.add(
            egui::DragValue::new(&mut self.properties.size)
                .range(1.0..=256.0)
                .speed(0.5)
                .suffix("px"),
        );
        ui.separator();

        // Hardness
        ui.label(t!("ctx.hardness"));
        let mut hardness_pct = self.properties.hardness * 100.0;
        if ui
            .add(
                egui::DragValue::new(&mut hardness_pct)
                    .range(0.0..=100.0)
                    .speed(0.5)
                    .suffix("%"),
            )
            .changed()
        {
            self.properties.hardness = hardness_pct / 100.0;
        }
        ui.separator();

        // Source indicator
        if let Some(src) = self.clone_stamp_state.source {
            ui.label(
                t!("ctx.clone_stamp.source")
                    .replace("{0}", &format!("{:.0}", src.x))
                    .replace("{1}", &format!("{:.0}", src.y)),
            );
        } else {
            ui.label(t!("ctx.clone_stamp.set_source"));
        }
    }

    fn show_content_aware_options(&mut self, ui: &mut egui::Ui) {
        use crate::ops::inpaint::ContentAwareQuality;

        // Size
        ui.label(t!("ctx.size"));
        ui.add(
            egui::DragValue::new(&mut self.properties.size)
                .range(1.0..=256.0)
                .speed(0.5)
                .suffix("px"),
        );
        ui.separator();

        // Hardness
        ui.label(t!("ctx.hardness"));
        let mut hardness_pct = self.properties.hardness * 100.0;
        if ui
            .add(
                egui::DragValue::new(&mut hardness_pct)
                    .range(0.0..=100.0)
                    .speed(0.5)
                    .suffix("%"),
            )
            .changed()
        {
            self.properties.hardness = hardness_pct / 100.0;
        }
        ui.separator();

        // Quality dropdown
        ui.label("Quality:");
        let cur_q = self.content_aware_state.quality;
        egui::ComboBox::from_id_salt("ca_quality")
            .selected_text(cur_q.label())
            .width(100.0)
            .show_ui(ui, |ui| {
                for &q in ContentAwareQuality::all() {
                    if ui.selectable_label(q == cur_q, q.label()).clicked() {
                        self.content_aware_state.quality = q;
                    }
                }
            });
        ui.separator();

        // Sample radius (Instant only)
        if cur_q == ContentAwareQuality::Instant {
            ui.label(t!("ctx.content_aware.sample"));
            ui.add(
                egui::DragValue::new(&mut self.content_aware_state.sample_radius)
                    .range(10.0..=150.0)
                    .speed(0.5)
                    .suffix("px"),
            );
            ui.separator();
        }

        // Patch size (Balanced / HQ only)
        if cur_q.is_async() {
            ui.label("Patch:");
            ui.add(
                egui::DragValue::new(&mut self.content_aware_state.patch_size)
                    .range(3_u32..=11_u32)
                    .speed(0.5)
                    .suffix("px"),
            );
            // Keep patch_size odd
            if self.content_aware_state.patch_size.is_multiple_of(2) {
                self.content_aware_state.patch_size += 1;
            }
            ui.separator();
        }

        if cur_q == ContentAwareQuality::Instant {
            ui.label(t!("ctx.content_aware.hint"));
        } else {
            ui.label("Paint to preview, then release to run inpaint.");
        }
    }

    /// Draw a custom tolerance slider with +/- buttons, wide track, and vertical handle.
    /// Returns `Some(new_value)` if changed, `None` otherwise.
    fn tolerance_slider(ui: &mut egui::Ui, id_salt: &str, value: f32) -> Option<f32> {
        let mut new_value = value;
        let mut changed = false;

        let vis = ui.visuals().clone();
        let is_dark = vis.dark_mode;

        // Colors that work in both dark and light modes
        let track_bg = if is_dark {
            Color32::from_gray(50)
        } else {
            Color32::from_gray(190)
        };
        // Use theme accent color for the filled portion
        let track_fill = vis.selection.bg_fill;
        let handle_color = if is_dark {
            Color32::from_gray(220)
        } else {
            Color32::from_gray(255)
        };
        let handle_border = if is_dark {
            Color32::from_gray(140)
        } else {
            Color32::from_gray(80)
        };
        let btn_bg = if is_dark {
            Color32::from_gray(60)
        } else {
            Color32::from_gray(210)
        };
        let btn_hover = if is_dark {
            Color32::from_gray(80)
        } else {
            Color32::from_gray(225)
        };
        let btn_text = if is_dark {
            Color32::from_gray(220)
        } else {
            Color32::from_gray(30)
        };
        let value_text_color = if is_dark {
            Color32::from_gray(200)
        } else {
            Color32::from_gray(40)
        };

        ui.horizontal(|ui| {
            // Minus button
            let btn_size = egui::vec2(20.0, 20.0);
            let (minus_rect, minus_resp) = ui.allocate_exact_size(btn_size, egui::Sense::click());
            let minus_bg = if minus_resp.hovered() {
                btn_hover
            } else {
                btn_bg
            };
            ui.painter().rect_filled(minus_rect, 3.0, minus_bg);
            ui.painter().text(
                minus_rect.center(),
                egui::Align2::CENTER_CENTER,
                "ÔêÆ",
                egui::FontId::proportional(14.0),
                btn_text,
            );
            if minus_resp.clicked() {
                new_value = (new_value - 1.0).max(0.0);
                changed = true;
            }

            ui.add_space(4.0);

            // Slider track
            let slider_width = 140.0;
            let slider_height = 20.0;
            let (slider_rect, slider_resp) = ui.allocate_exact_size(
                egui::vec2(slider_width, slider_height),
                egui::Sense::click_and_drag(),
            );

            let rounding = slider_height / 2.0;

            // Draw track background
            ui.painter().rect_filled(slider_rect, rounding, track_bg);

            // Draw filled portion
            let fill_fraction = new_value / 100.0;
            let fill_width = slider_rect.width() * fill_fraction;
            if fill_width > 0.5 {
                let fill_rect =
                    Rect::from_min_size(slider_rect.min, egui::vec2(fill_width, slider_height));
                ui.painter().rect_filled(fill_rect, rounding, track_fill);
            }

            // Draw vertical handle
            let handle_x = slider_rect.min.x + fill_width;
            let handle_width = 4.0;
            let handle_height = slider_height + 2.0;
            let handle_rect = Rect::from_center_size(
                egui::pos2(handle_x, slider_rect.center().y),
                egui::vec2(handle_width, handle_height),
            );
            ui.painter().rect_filled(handle_rect, 2.0, handle_color);
            ui.painter().rect_stroke(
                handle_rect,
                2.0,
                (1.0, handle_border),
                egui::StrokeKind::Middle,
            );

            // Draw value text centered on track
            let text = format!("{}%", new_value.round() as i32);
            ui.painter().text(
                slider_rect.center(),
                egui::Align2::CENTER_CENTER,
                &text,
                egui::FontId::proportional(11.0),
                value_text_color,
            );

            // Handle interaction
            if (slider_resp.dragged() || slider_resp.clicked())
                && let Some(pos) = slider_resp.interact_pointer_pos()
            {
                let frac = ((pos.x - slider_rect.min.x) / slider_rect.width()).clamp(0.0, 1.0);
                new_value = (frac * 100.0).round();
                changed = true;
            }

            // Tooltip
            if slider_resp.hovered() {
                slider_resp.on_hover_text(t!("ctx.tolerance_tooltip"));
            }

            ui.add_space(4.0);

            // Plus button
            let (plus_rect, plus_resp) = ui.allocate_exact_size(btn_size, egui::Sense::click());
            let plus_bg = if plus_resp.hovered() {
                btn_hover
            } else {
                btn_bg
            };
            ui.painter().rect_filled(plus_rect, 3.0, plus_bg);
            ui.painter().text(
                plus_rect.center(),
                egui::Align2::CENTER_CENTER,
                "+",
                egui::FontId::proportional(14.0),
                btn_text,
            );
            if plus_resp.clicked() {
                new_value = (new_value + 1.0).min(100.0);
                changed = true;
            }
        });

        // Ensure we allocate a unique id for internal egui state
        let _ = ui.id().with(id_salt);

        if changed { Some(new_value) } else { None }
    }

    fn show_magic_wand_options(&mut self, ui: &mut egui::Ui) {
        ui.label(t!("ctx.tolerance"));
        if let Some(new_tolerance) =
            Self::tolerance_slider(ui, "mw_tol", self.magic_wand_state.tolerance)
        {
            self.magic_wand_state.tolerance = new_tolerance;
            self.magic_wand_state.tolerance_changed_at = Some(Instant::now());
            self.magic_wand_state.preview_pending = true;
            // Instant re-threshold ÔÇö no debounce needed with distance-map approach
            ui.ctx().request_repaint();
        }

        ui.separator();

        let old_aa = self.magic_wand_state.anti_aliased;
        let aa_resp = ui.selectable_label(self.magic_wand_state.anti_aliased, t!("ctx.anti_alias"));
        if aa_resp.clicked() {
            self.magic_wand_state.anti_aliased = !self.magic_wand_state.anti_aliased;
        }
        aa_resp.on_hover_text(t!("ctx.anti_alias_tooltip"));
        if self.magic_wand_state.anti_aliased != old_aa {
            self.magic_wand_state.tolerance_changed_at = Some(Instant::now());
            self.magic_wand_state.preview_pending = true;
            ui.ctx().request_repaint();
        }

        ui.separator();

        ui.label("Compare");
        let prev_distance_mode = self.magic_wand_state.distance_mode;
        egui::ComboBox::from_id_salt("ctx_magic_wand_distance_mode")
            .selected_text(self.magic_wand_state.distance_mode.label())
            .width(120.0)
            .show_ui(ui, |ui| {
                for &mode in WandDistanceMode::all() {
                    if ui
                        .selectable_label(mode == self.magic_wand_state.distance_mode, mode.label())
                        .clicked()
                    {
                        self.magic_wand_state.distance_mode = mode;
                    }
                }
            });
        if self.magic_wand_state.distance_mode != prev_distance_mode {
            self.clear_magic_wand_async_state();
            ui.ctx().request_repaint();
        }

        ui.separator();

        ui.label("Connectivity");
        let prev_connectivity = self.magic_wand_state.connectivity;
        egui::ComboBox::from_id_salt("ctx_magic_wand_connectivity")
            .selected_text(self.magic_wand_state.connectivity.label())
            .width(120.0)
            .show_ui(ui, |ui| {
                for &mode in FloodConnectivity::all() {
                    if ui
                        .selectable_label(mode == self.magic_wand_state.connectivity, mode.label())
                        .clicked()
                    {
                        self.magic_wand_state.connectivity = mode;
                    }
                }
            });
        if self.magic_wand_state.connectivity != prev_connectivity {
            self.clear_magic_wand_async_state();
            ui.ctx().request_repaint();
        }

        ui.separator();

        ui.label(t!("ctx.mode"));
        let current = self.selection_state.mode;
        egui::ComboBox::from_id_salt("ctx_magic_wand_mode")
            .selected_text(current.label())
            .width(90.0)
            .show_ui(ui, |ui| {
                for &mode in SelectionMode::all() {
                    if ui.selectable_label(mode == current, mode.label()).clicked() {
                        self.selection_state.mode = mode;
                    }
                }
            });

        ui.separator();
        self.show_sel_modify_controls(ui);
    }

    fn show_fill_options(&mut self, ui: &mut egui::Ui) {
        ui.label(t!("ctx.tolerance"));
        if let Some(new_val) = Self::tolerance_slider(ui, "fill_tol", self.fill_state.tolerance) {
            self.fill_state.tolerance = new_val;
            self.fill_state.tolerance_changed_at = Some(Instant::now());
            self.fill_state.recalc_pending = true;
            ui.ctx().request_repaint();
        }

        ui.separator();

        let old_aa = self.fill_state.anti_aliased;
        let aa_resp = ui.selectable_label(self.fill_state.anti_aliased, t!("ctx.anti_alias"));
        if aa_resp.clicked() {
            self.fill_state.anti_aliased = !self.fill_state.anti_aliased;
        }
        aa_resp.on_hover_text(t!("ctx.anti_alias_tooltip"));
        if self.fill_state.anti_aliased != old_aa {
            // Anti-alias setting changed, trigger preview refresh
            self.fill_state.tolerance_changed_at = Some(Instant::now());
            self.fill_state.recalc_pending = true;
            ui.ctx().request_repaint();
        }
    }

    pub fn show_properties_toolbar(&mut self, ui: &mut egui::Ui, assets: &Assets) {
        // Size
        self.show_size_widget(ui, "brush_size_presets", assets);

        ui.separator();

        // Hardness as percentage
        ui.label(t!("ctx.hardness"));
        let mut hardness_pct = (self.properties.hardness * 100.0).round();
        if ui
            .add(
                egui::DragValue::new(&mut hardness_pct)
                    .speed(1.0)
                    .range(0.0..=100.0)
                    .suffix("%"),
            )
            .on_hover_text(t!("ctx.hardness_brush_tooltip"))
            .changed()
        {
            self.properties.hardness = hardness_pct / 100.0;
        }

        ui.separator();

        // Blend Mode Dropdown (for brush painting)
        ui.label(t!("ctx.blend"));
        let current_mode = self.properties.blending_mode;
        egui::ComboBox::from_id_salt("tool_blend_mode")
            .selected_text(current_mode.name())
            .width(90.0)
            .show_ui(ui, |ui: &mut egui::Ui| {
                for &mode in BlendMode::all() {
                    if ui
                        .selectable_label(mode == current_mode, mode.name())
                        .clicked()
                    {
                        self.properties.blending_mode = mode;
                    }
                }
            });
    }

}
