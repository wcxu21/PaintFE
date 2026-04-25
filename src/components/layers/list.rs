impl LayersPanel {
    /// Main show method - renders the entire layers panel
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        canvas_state: &mut CanvasState,
        assets: &Assets,
        history: &mut HistoryManager,
    ) {
        let layer_count = canvas_state.layers.len();

        // Invalidate cache if layer count changed
        if layer_count != self.last_layer_count {
            self.thumbnail_cache.clear();
            self.last_layer_count = layer_count;
        }

        // Check if we should end peek (no longer pressing)
        self.update_peek_state(ui, canvas_state);

        ui.vertical(|ui| {
            // Search bar — only shown when there are more than 2 layers
            if layer_count > 2 {
                self.show_search_bar(ui);
                ui.add_space(2.0);
            } else {
                // Clear search when layer count drops to 2 or less
                self.search_query.clear();
            }

            // Layer list with scroll area (takes most of the space)
            self.show_layer_list(ui, canvas_state, assets, history);

            ui.add_space(4.0);

            // Fixed footer toolbar
            self.show_footer_toolbar(ui, canvas_state, assets, history);
        });

        // Show layer settings popup if active
        self.show_layer_settings_popup(ui, canvas_state);
    }

    /// Show the layer search/filter bar
    fn show_search_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            ui.label("🔍");
            let resp = ui.add(
                egui::TextEdit::singleline(&mut self.search_query)
                    .hint_text("Filter layers…")
                    .desired_width(ui.available_width() - 20.0),
            );
            // Clear button — only show when there's text
            if !self.search_query.is_empty() && ui.small_button("×").clicked() {
                self.search_query.clear();
                resp.request_focus();
            }
        });
    }

    /// Render the scrollable layer list with drag-and-drop reordering
    fn show_layer_list(
        &mut self,
        ui: &mut egui::Ui,
        canvas_state: &mut CanvasState,
        assets: &Assets,
        history: &mut HistoryManager,
    ) {
        let layer_count = canvas_state.layers.len();
        if layer_count == 0 {
            return;
        }

        let row_height = 48.0;
        let row_gap = 3.0;
        let row_stride = row_height + row_gap;

        // Build filtered display list: display_idx → layer_idx mapping
        // When searching, only include layers whose name matches the query
        let is_filtering = !self.search_query.is_empty();
        let query_lower = self.search_query.to_lowercase();

        let visible_entries: Vec<(usize, usize)> = (0..layer_count)
            .map(|display_idx| (display_idx, layer_count - 1 - display_idx))
            .filter(|&(_, layer_idx)| {
                if !is_filtering {
                    return true;
                }
                canvas_state.layers[layer_idx]
                    .name
                    .to_lowercase()
                    .contains(&query_lower)
            })
            .collect();

        let visible_count = visible_entries.len();
        if visible_count == 0 {
            ui.vertical_centered(|ui| {
                ui.add_space(20.0);
                ui.label(egui::RichText::new("No layers match filter").weak());
            });
            return;
        }

        // Ensure anim_offsets vec is the right size
        self.drag_state.anim_offsets.resize(layer_count, 0.0);

        // Disable drag-and-drop when filtering (reordering filtered list is confusing)
        if is_filtering {
            self.drag_state.dragging_display_idx = None;
            self.drag_state.drag_offset_y = 0.0;
        }

        // Scroll area — pinned to a fixed height that leaves room for the
        // footer toolbar, spacing, and item_spacing gaps that follow.
        //
        // We measure available_height() HERE (after the search bar was drawn)
        // and subtract a generous 60px for the footer (~36px) + add_space(4)
        // + egui item_spacing between elements (~20px combined).
        //
        // auto_shrink([false, false]) makes the scroll area always fill its
        // max_height regardless of content, so the window stays the same size
        // whether there are 1 or 100 layers.  Because the deduction (60px)
        // is larger than the actual overhead (~44px), total content is always
        // ~16px LESS than the window — a resizable egui::Window only grows
        // when content EXCEEDS stored size, so the window never grows.
        let scroll_h = (ui.available_height() - 60.0).max(80.0);
        egui::ScrollArea::vertical()
            .id_salt("layer_scroll")
            .max_height(scroll_h)
            .auto_shrink([false, false])
            .show(ui, |ui: &mut egui::Ui| {
                // Reserve total space for visible rows only
                let total_h = visible_count as f32 * row_stride;
                let available_w = ui.available_width();
                let (total_rect, _) =
                    ui.allocate_exact_size(Vec2::new(available_w, total_h), Sense::hover());

                // --- Drag logic (frame-level) ---
                let pointer_pos = ui.input(|i| i.pointer.latest_pos());
                let pointer_down = ui.input(|i| i.pointer.primary_down());
                let drag_delta_y = ui.input(|i| i.pointer.delta().y);

                // Determine which display_idx the pointer would be over
                let _hover_display_idx: Option<usize> = pointer_pos.map(|p| {
                    let relative_y = p.y - total_rect.top();
                    ((relative_y / row_stride).floor() as usize).min(layer_count.saturating_sub(1))
                });

                // If dragging, update offset and compute target slot
                let mut drop_target: Option<usize> = None; // display index to drop before/at

                if let Some(drag_didx) = self.drag_state.dragging_display_idx {
                    if pointer_down {
                        // Accumulate drag offset
                        self.drag_state.drag_offset_y += drag_delta_y;

                        // Compute target display index from current drag position
                        let origin_center_y = self.drag_state.origin_display_idx as f32
                            * row_stride
                            + row_stride * 0.5;
                        let dragged_center_y = origin_center_y + self.drag_state.drag_offset_y;
                        let target = ((dragged_center_y / row_stride).floor() as usize)
                            .min(layer_count.saturating_sub(1));
                        drop_target = Some(target);

                        // Set grab cursor
                        ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
                        // Keep repainting during drag for smooth animation
                        ui.ctx().request_repaint();
                    } else {
                        // Mouse released — commit the reorder
                        let origin_center_y = self.drag_state.origin_display_idx as f32
                            * row_stride
                            + row_stride * 0.5;
                        let dragged_center_y = origin_center_y + self.drag_state.drag_offset_y;
                        let target = ((dragged_center_y / row_stride).floor() as usize)
                            .min(layer_count.saturating_sub(1));

                        if target != drag_didx {
                            // Convert display indices to layer indices (display is reversed)
                            let from_layer_idx = layer_count - 1 - drag_didx;
                            let to_layer_idx = layer_count - 1 - target;
                            self.move_layer(from_layer_idx, to_layer_idx, canvas_state, history);
                        }

                        // Reset drag state
                        self.drag_state.dragging_display_idx = None;
                        self.drag_state.drag_offset_y = 0.0;
                        for v in self.drag_state.anim_offsets.iter_mut() {
                            *v = 0.0;
                        }
                    }
                }

                // Compute animated offsets for non-dragged rows (elastic slide)
                if let (Some(drag_didx), Some(target)) =
                    (self.drag_state.dragging_display_idx, drop_target)
                {
                    let direction = if target > drag_didx { 1 } else { -1i32 };
                    let range_lo = drag_didx.min(target);
                    let range_hi = drag_didx.max(target);

                    for i in 0..layer_count {
                        let target_offset = if i == drag_didx {
                            0.0 // dragged row uses raw drag_offset_y
                        } else if i > range_lo && i <= range_hi && direction == 1 {
                            // Rows between origin and drop target slide up
                            -row_stride
                        } else if i >= range_lo && i < range_hi && direction == -1 {
                            // Rows between drop target and origin slide down
                            row_stride
                        } else {
                            0.0
                        };
                        // Smooth interpolation toward target
                        let speed = 0.25;
                        self.drag_state.anim_offsets[i] +=
                            (target_offset - self.drag_state.anim_offsets[i]) * speed;
                        // Snap when close
                        if (self.drag_state.anim_offsets[i] - target_offset).abs() < 0.5 {
                            self.drag_state.anim_offsets[i] = target_offset;
                        }
                    }
                } else {
                    // No drag — decay all offsets to 0
                    for v in self.drag_state.anim_offsets.iter_mut() {
                        *v *= 0.7;
                        if v.abs() < 0.5 {
                            *v = 0.0;
                        }
                    }
                }

                // --- Collect deferred actions ---
                let mut layer_to_merge: Option<usize> = None;
                let mut layer_to_flatten = false;
                let mut layer_to_add = false;
                let mut layer_to_add_text = false;
                let mut layer_to_duplicate: Option<usize> = None;
                let mut layer_to_delete: Option<usize> = None;
                let mut layer_to_rasterize: Option<usize> = None;
                let mut new_active: Option<usize> = None;
                let mut swap_layers: Option<(usize, usize)> = None;

                // --- Draw rows ---
                let is_dragging = self.drag_state.dragging_display_idx.is_some();

                for (vis_idx, &(display_idx, layer_idx)) in visible_entries.iter().enumerate() {
                    let is_dragged = self.drag_state.dragging_display_idx == Some(display_idx);

                    // Compute visual Y position — use vis_idx for layout when filtering
                    let base_y = total_rect.top() + vis_idx as f32 * row_stride;
                    let visual_y = if is_dragged {
                        base_y + self.drag_state.drag_offset_y
                    } else if !is_filtering {
                        base_y + self.drag_state.anim_offsets[display_idx]
                    } else {
                        base_y
                    };

                    let row_rect = Rect::from_min_size(
                        Pos2::new(total_rect.left(), visual_y),
                        Vec2::new(available_w, row_height),
                    );

                    // Draw the row with an overlay for the dragged item
                    let paint_layer = if is_dragged {
                        // Paint dragged row on a higher layer so it renders on top
                        ui.painter().clone().with_layer_id(egui::LayerId::new(
                            egui::Order::Tooltip,
                            Id::new("drag_layer"),
                        ))
                    } else {
                        ui.painter().clone()
                    };

                    let (action, context_action) = self.show_layer_row_at(
                        ui,
                        &paint_layer,
                        row_rect,
                        layer_idx,
                        display_idx,
                        canvas_state,
                        assets,
                        is_dragged,
                        is_dragging,
                    );

                    // Handle row click actions
                    if let Some(act) = action {
                        match act {
                            LayerAction::Select => new_active = Some(layer_idx),
                            LayerAction::StartRename => {
                                self.rename_state.renaming_layer = Some(layer_idx);
                                self.rename_state.rename_text =
                                    canvas_state.layers[layer_idx].name.clone();
                                self.rename_state.focus_requested = true;
                            }
                            LayerAction::FinishRename => {
                                if let Some(rename_idx) = self.rename_state.renaming_layer
                                    && !self.rename_state.rename_text.is_empty()
                                {
                                    let old_name = canvas_state.layers[rename_idx].name.clone();
                                    let new_name = self.rename_state.rename_text.clone();
                                    if old_name != new_name {
                                        canvas_state.layers[rename_idx].name = new_name.clone();
                                        history.push(Box::new(LayerOpCommand::new(
                                            LayerOperation::Rename {
                                                index: rename_idx,
                                                old_name,
                                                new_name,
                                            },
                                        )));
                                    }
                                }
                                self.rename_state.renaming_layer = None;
                            }
                            LayerAction::CancelRename => {
                                self.rename_state.renaming_layer = None;
                            }
                            LayerAction::ToggleVisibility => {
                                let was_visible = canvas_state.layers[layer_idx].visible;
                                canvas_state.layers[layer_idx].visible = !was_visible;
                                history.push(Box::new(LayerOpCommand::new(
                                    LayerOperation::Visibility {
                                        index: layer_idx,
                                        was_visible,
                                    },
                                )));
                                self.mark_full_dirty(canvas_state);
                            }
                            LayerAction::BeginDrag => {
                                self.drag_state.dragging_display_idx = Some(display_idx);
                                self.drag_state.origin_display_idx = display_idx;
                                self.drag_state.drag_offset_y = 0.0;
                                for v in self.drag_state.anim_offsets.iter_mut() {
                                    *v = 0.0;
                                }
                            }
                        }
                    }

                    // Handle context menu actions
                    if let Some(ctx_act) = context_action {
                        match ctx_act {
                            ContextAction::AddNew => layer_to_add = true,
                            ContextAction::MergeDown => layer_to_merge = Some(layer_idx),
                            ContextAction::MergeDownAsMask => {
                                self.pending_app_action =
                                    Some(LayerAppAction::MergeDownAsMask(layer_idx));
                            }
                            ContextAction::AddLayerMaskRevealAll => {
                                self.pending_app_action =
                                    Some(LayerAppAction::AddLayerMaskRevealAll(layer_idx));
                            }
                            ContextAction::AddLayerMaskFromSelection => {
                                self.pending_app_action =
                                    Some(LayerAppAction::AddLayerMaskFromSelection(layer_idx));
                            }
                            ContextAction::ToggleLayerMaskEdit => {
                                self.pending_app_action =
                                    Some(LayerAppAction::ToggleLayerMaskEdit(layer_idx));
                            }
                            ContextAction::ToggleLayerMask => {
                                self.pending_app_action =
                                    Some(LayerAppAction::ToggleLayerMask(layer_idx));
                            }
                            ContextAction::InvertLayerMask => {
                                self.pending_app_action =
                                    Some(LayerAppAction::InvertLayerMask(layer_idx));
                            }
                            ContextAction::ApplyLayerMask => {
                                self.pending_app_action =
                                    Some(LayerAppAction::ApplyLayerMask(layer_idx));
                            }
                            ContextAction::DeleteLayerMask => {
                                self.pending_app_action =
                                    Some(LayerAppAction::DeleteLayerMask(layer_idx));
                            }
                            ContextAction::FlattenImage => layer_to_flatten = true,
                            ContextAction::Duplicate => layer_to_duplicate = Some(layer_idx),
                            ContextAction::Delete => layer_to_delete = Some(layer_idx),
                            ContextAction::OpenSettings => {
                                self.open_settings_for_layer(
                                    layer_idx,
                                    canvas_state,
                                    LayerSettingsTab::General,
                                );
                            }
                            ContextAction::MoveToTop => {
                                let top = canvas_state.layers.len().saturating_sub(1);
                                if layer_idx < top {
                                    // Use move_layer which handles remove+insert, active index, and history
                                    swap_layers = Some((layer_idx, top));
                                }
                            }
                            ContextAction::MoveUp => {
                                if layer_idx + 1 < canvas_state.layers.len() {
                                    swap_layers = Some((layer_idx, layer_idx + 1));
                                }
                            }
                            ContextAction::MoveDown => {
                                if layer_idx > 0 {
                                    swap_layers = Some((layer_idx, layer_idx - 1));
                                }
                            }
                            ContextAction::MoveToBottom => {
                                if layer_idx > 0 {
                                    // Use move_layer which handles remove+insert, active index, and history
                                    swap_layers = Some((layer_idx, 0));
                                }
                            }
                            ContextAction::Rename => {
                                self.rename_state.renaming_layer = Some(layer_idx);
                                self.rename_state.rename_text =
                                    canvas_state.layers[layer_idx].name.clone();
                                self.rename_state.focus_requested = true;
                            }
                            ContextAction::ImportFromFile => {
                                self.pending_app_action = Some(LayerAppAction::ImportFromFile);
                            }
                            ContextAction::FlipHorizontal => {
                                self.pending_app_action = Some(LayerAppAction::FlipHorizontal);
                            }
                            ContextAction::FlipVertical => {
                                self.pending_app_action = Some(LayerAppAction::FlipVertical);
                            }
                            ContextAction::RotateScale => {
                                self.pending_app_action = Some(LayerAppAction::RotateScale);
                            }
                            ContextAction::AlignLayer => {
                                self.pending_app_action = Some(LayerAppAction::AlignLayer);
                            }
                            ContextAction::SoloLayer => {
                                self.solo_layer(layer_idx, canvas_state);
                            }
                            ContextAction::HideAll => {
                                self.hide_all_layers(canvas_state);
                            }
                            ContextAction::ShowAll => {
                                self.show_all_layers(canvas_state);
                            }
                            ContextAction::AddNewTextLayer => {
                                layer_to_add_text = true;
                            }
                            ContextAction::RasterizeTextLayer => {
                                layer_to_rasterize = Some(layer_idx);
                            }
                            ContextAction::TextLayerEffects => {
                                self.open_settings_for_layer(
                                    layer_idx,
                                    canvas_state,
                                    LayerSettingsTab::Effects,
                                );
                            }
                            ContextAction::TextLayerWarp => {
                                self.open_settings_for_layer(
                                    layer_idx,
                                    canvas_state,
                                    LayerSettingsTab::Warp,
                                );
                            }
                        }
                    }
                }

                // Draw drop indicator line
                if let (Some(_drag_didx), Some(target)) =
                    (self.drag_state.dragging_display_idx, drop_target)
                {
                    let indicator_y =
                        total_rect.top() + target as f32 * row_stride + row_stride * 0.5;
                    let accent = ui.visuals().selection.bg_fill;
                    ui.painter().line_segment(
                        [
                            Pos2::new(total_rect.left() + 4.0, indicator_y),
                            Pos2::new(total_rect.right() - 4.0, indicator_y),
                        ],
                        egui::Stroke::new(2.0, accent),
                    );
                }

                // Process deferred actions
                if let Some(idx) = new_active {
                    canvas_state.active_layer_index = idx;
                    if !canvas_state.layers[idx].has_live_mask() {
                        canvas_state.edit_layer_mask = false;
                    }
                }
                if let Some(merge_idx) = layer_to_merge {
                    self.merge_down(merge_idx, canvas_state, history);
                }
                if layer_to_flatten {
                    self.flatten_image(canvas_state, history);
                }
                if layer_to_add {
                    self.add_new_layer(canvas_state, history);
                }
                if layer_to_add_text {
                    self.add_new_text_layer(canvas_state, history);
                }
                if let Some(dup_idx) = layer_to_duplicate {
                    self.duplicate_layer(dup_idx, canvas_state, history);
                }
                if let Some(del_idx) = layer_to_delete {
                    self.delete_layer(del_idx, canvas_state, history);
                }
                if let Some(rast_idx) = layer_to_rasterize {
                    self.rasterize_text_layer(rast_idx, canvas_state, history);
                }
                if let Some((from, to)) = swap_layers {
                    self.move_layer(from, to, canvas_state, history);
                }
            });
    }

    /// Render a single layer row at an explicit rect (supports drag offset)
    fn show_layer_row_at(
        &mut self,
        ui: &mut egui::Ui,
        painter: &egui::Painter,
        row_rect: Rect,
        layer_idx: usize,
        display_idx: usize,
        canvas_state: &mut CanvasState,
        assets: &Assets,
        is_dragged: bool,
        is_any_dragging: bool,
    ) -> (Option<LayerAction>, Option<ContextAction>) {
        // Copy the values we need from the layer to avoid borrow conflicts
        let layer_visible = canvas_state.layers[layer_idx].visible;
        let layer_name = canvas_state.layers[layer_idx].name.clone();
        let is_active = layer_idx == canvas_state.active_layer_index;
        let is_renaming = self.rename_state.renaming_layer == Some(layer_idx);

        let mut action: Option<LayerAction> = None;
        let mut context_action: Option<ContextAction> = None;
        let mut should_peek = false;

        let row_height = row_rect.height();
        let selection_color = ui.visuals().selection.bg_fill;
        let row_bg = if is_active {
            selection_color
        } else {
            Color32::TRANSPARENT
        };

        // Interact with the row at its visual position
        let row_id = Id::new("layer_row").with(display_idx);
        let row_response = ui.interact(row_rect, row_id, Sense::click_and_drag());

        // Drag initiation — only when not already dragging and not renaming
        if !is_any_dragging && !is_renaming && row_response.drag_started() {
            action = Some(LayerAction::BeginDrag);
        }

        // Hover cursor: show grab hand when hovering a row (but not when dragging)
        if !is_any_dragging && row_response.hovered() {
            ui.ctx().set_cursor_icon(CursorIcon::Grab);
        }

        // Paint row background
        if ui.is_rect_visible(row_rect) {
            // Row background - shrink vertically to avoid overlap
            let shrink = row_height * 0.09;
            let bg_rect = row_rect.shrink2(Vec2::new(0.0, shrink));

            if is_dragged {
                // Dragged item: elevated shadow + accent border
                let shadow_color = Color32::from_black_alpha(60);
                let shadow_rect = bg_rect.translate(Vec2::new(0.0, 2.0));
                painter.rect_filled(shadow_rect, 6.0, shadow_color);

                // Slightly brighter background for dragged item
                let drag_bg = if is_active {
                    selection_color
                } else {
                    ui.visuals().widgets.active.bg_fill
                };
                painter.rect_filled(bg_rect, 4.0, drag_bg);
                painter.rect_stroke(
                    bg_rect,
                    4.0,
                    egui::Stroke::new(1.5, selection_color),
                    egui::StrokeKind::Middle,
                );
            } else {
                painter.rect_filled(bg_rect, 4.0, row_bg);
            }

            // Layout: [Eye] [Thumbnail] [Name]
            let mut x = row_rect.left() + 4.0;
            let center_y = row_rect.center().y;

            // Pre-calculate all rects
            let eye_rect = Rect::from_center_size(Pos2::new(x + 10.0, center_y), Vec2::splat(20.0));
            x += 24.0;

            let thumb_size = 36.0;
            let thumb_rect = Rect::from_min_size(
                Pos2::new(x + 2.0, center_y - thumb_size / 2.0),
                Vec2::splat(thumb_size),
            );
            x += thumb_size + 8.0;

            let is_text_layer = matches!(
                canvas_state.layers[layer_idx].content,
                LayerContent::Text(_)
            );
            let gear_width = if is_text_layer { 20.0 } else { 0.0 };
            let name_rect = Rect::from_min_max(
                Pos2::new(x, row_rect.top() + 4.0),
                Pos2::new(row_rect.right() - 6.0 - gear_width, row_rect.bottom() - 4.0),
            );

            // Small gear icon for text layer settings (right side of row)
            if is_text_layer {
                let gear_rect = Rect::from_center_size(
                    Pos2::new(row_rect.right() - 6.0 - gear_width / 2.0, center_y),
                    Vec2::splat(16.0),
                );
                let gear_color = if gear_rect
                    .contains(ui.input(|i| i.pointer.hover_pos().unwrap_or_default()))
                {
                    ui.visuals().strong_text_color()
                } else {
                    ui.visuals().text_color()
                };
                ui.painter().text(
                    gear_rect.center(),
                    egui::Align2::CENTER_CENTER,
                    "\u{2699}",
                    egui::FontId::proportional(14.0),
                    gear_color,
                );
                let gear_resp =
                    ui.interact(gear_rect, Id::new(("text_gear", layer_idx)), Sense::click());
                if gear_resp.hovered() {
                    ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
                }
                if gear_resp.clicked() {
                    context_action = Some(ContextAction::TextLayerEffects);
                }
                gear_resp.on_hover_text(t!("layer.text_settings"));
            }

            // Draw thumbnail (needs mutable self for cache)
            self.draw_thumbnail(ui, thumb_rect, layer_idx, canvas_state, 1.0);

            // Draw eye icon (left-click: toggle visibility, right-click hold: peek layer)
            let icon_color = ui.visuals().strong_text_color();
            let muted_color = ui.visuals().text_color();
            let is_this_soloed =
                self.peek_state.is_soloed && self.peek_state.solo_layer_index == Some(layer_idx);
            let eye_icon = if layer_visible {
                Icon::Visible
            } else {
                Icon::Hidden
            };
            let eye_tint = if is_this_soloed {
                Color32::from_rgb(255, 180, 60) // Orange tint when soloed
            } else if layer_visible {
                icon_color
            } else {
                muted_color
            };
            let eye_response = assets.icon_in_rect(ui, eye_icon, eye_rect, eye_tint);
            if eye_response.clicked() {
                action = Some(LayerAction::ToggleVisibility);
            }
            // Right-click hold: temporary peek (show only this layer while held)
            if eye_response.is_pointer_button_down_on()
                && ui.input(|i| i.pointer.button_down(egui::PointerButton::Secondary))
            {
                should_peek = true;
            }
            // Right-click release (secondary_clicked): toggle solo —
            // but only if we weren't just peeking (peek_just_ended suppresses
            // the solo toggle on the same frame the hold was released).
            if eye_response.secondary_clicked() && !self.peek_state.peek_just_ended {
                if is_this_soloed {
                    context_action = Some(ContextAction::ShowAll);
                } else {
                    context_action = Some(ContextAction::SoloLayer);
                }
            }
            if is_this_soloed {
                eye_response.on_hover_text("Soloed — right-click to unsolo");
            } else {
                eye_response.on_hover_text(if layer_visible {
                    "Hide layer · Right-click hold to peek"
                } else {
                    "Show layer · Right-click hold to peek"
                });
            }

            // Draw name or rename field
            if is_renaming {
                let text_edit = egui::TextEdit::singleline(&mut self.rename_state.rename_text)
                    .font(egui::TextStyle::Body)
                    .desired_width(name_rect.width());

                let response = ui.put(name_rect, text_edit);

                if self.rename_state.focus_requested {
                    response.request_focus();
                    self.rename_state.focus_requested = false;
                }

                if response.lost_focus() {
                    if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        action = Some(LayerAction::FinishRename);
                    } else if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                        action = Some(LayerAction::CancelRename);
                    } else {
                        action = Some(LayerAction::FinishRename);
                    }
                }
            } else {
                let is_text = matches!(
                    canvas_state.layers[layer_idx].content,
                    LayerContent::Text(_)
                );
                let mut name_text =
                    egui::RichText::new(&layer_name)
                        .size(13.0)
                        .color(if is_active {
                            ui.visuals().strong_text_color()
                        } else {
                            icon_color
                        });
                if is_text {
                    name_text = name_text.strong();
                }

                let mut child_ui = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(name_rect)
                        .layout(egui::Layout::top_down(egui::Align::LEFT)),
                );
                child_ui.spacing_mut().item_spacing.y = 0.0;
                // Vertically center the content block within the row
                let content_h = if is_text { 13.0 + 9.0 + 1.0 } else { 13.0 };
                let pad = ((name_rect.height() - content_h) / 2.0).max(0.0);
                if pad > 0.0 {
                    child_ui.add_space(pad);
                }
                child_ui.add(
                    egui::Label::new(name_text)
                        .selectable(false)
                        .truncate()
                        .sense(egui::Sense::hover()),
                );
                if is_text {
                    let accent = child_ui.visuals().selection.stroke.color;
                    child_ui.add(
                        egui::Label::new(
                            egui::RichText::new("TEXT LAYER")
                                .size(9.0)
                                .strong()
                                .color(accent),
                        )
                        .selectable(false)
                        .sense(egui::Sense::hover()),
                    );
                }
            }

            // Now handle peek (after all layer borrows are done)
            if should_peek {
                self.start_peek(layer_idx, canvas_state);
            }
        }

        // Row click handling (select layer) — only when not dragging
        if !is_any_dragging && row_response.clicked() && action.is_none() {
            action = Some(LayerAction::Select);
        }

        // Double-click to rename
        if !is_any_dragging && row_response.double_clicked() {
            action = Some(LayerAction::StartRename);
        }

        // Right-click context menu
        row_response.context_menu(|ui| {
            if assets
                .menu_item(ui, Icon::LayerAdd, &t!("layer.add_layer"))
                .clicked()
            {
                context_action = Some(ContextAction::AddNew);
                ui.close();
            }
            if assets
                .menu_item(ui, Icon::Rename, &t!("layer.add_text_layer"))
                .clicked()
            {
                context_action = Some(ContextAction::AddNewTextLayer);
                ui.close();
            }
            if assets
                .menu_item(ui, Icon::LayerDuplicate, &t!("layer.duplicate_layer"))
                .clicked()
            {
                context_action = Some(ContextAction::Duplicate);
                ui.close();
            }
            if canvas_state.layers.len() > 1
                && assets
                    .menu_item(ui, Icon::LayerDelete, &t!("layer.delete_layer"))
                    .clicked()
            {
                context_action = Some(ContextAction::Delete);
                ui.close();
            }
            ui.separator();
            if layer_idx > 0 {
                if assets
                    .menu_item(ui, Icon::MergeDown, &t!("layer.merge_down"))
                    .clicked()
                {
                    context_action = Some(ContextAction::MergeDown);
                    ui.close();
                }
                if assets
                    .menu_item(ui, Icon::MergeDownAsMask, &t!("layer.merge_down_as_mask"))
                    .clicked()
                {
                    context_action = Some(ContextAction::MergeDownAsMask);
                    ui.close();
                }
            }
            ui.separator();
            let has_mask = canvas_state.layers[layer_idx].has_live_mask();
            if !has_mask
                && assets
                    .menu_item(
                        ui,
                        Icon::AddLayerMaskRevealAll,
                        "Add Layer Mask (Reveal All)",
                    )
                    .clicked()
            {
                context_action = Some(ContextAction::AddLayerMaskRevealAll);
                ui.close();
            }
            if !has_mask
                && assets
                    .menu_item(
                        ui,
                        Icon::AddLayerMaskFromSelection,
                        "Add Layer Mask (From Selection)",
                    )
                    .clicked()
            {
                context_action = Some(ContextAction::AddLayerMaskFromSelection);
                ui.close();
            }
            if has_mask
                && assets
                    .menu_item(
                        ui,
                        Icon::LayerProperties,
                        if canvas_state.edit_layer_mask
                            && canvas_state.active_layer_index == layer_idx
                        {
                            "Edit Layer Pixels"
                        } else {
                            "Edit Layer Mask"
                        },
                    )
                    .clicked()
            {
                context_action = Some(ContextAction::ToggleLayerMaskEdit);
                ui.close();
            }
            if has_mask
                && assets
                    .menu_item(
                        ui,
                        Icon::ToggleLayerMask,
                        if canvas_state.layers[layer_idx].mask_enabled {
                            "Disable Layer Mask"
                        } else {
                            "Enable Layer Mask"
                        },
                    )
                    .clicked()
            {
                context_action = Some(ContextAction::ToggleLayerMask);
                ui.close();
            }
            if has_mask
                && assets
                    .menu_item(ui, Icon::InvertLayerMask, "Invert Layer Mask")
                    .clicked()
            {
                context_action = Some(ContextAction::InvertLayerMask);
                ui.close();
            }
            if has_mask
                && assets
                    .menu_item(ui, Icon::ApplyLayerMask, "Apply Layer Mask")
                    .clicked()
            {
                context_action = Some(ContextAction::ApplyLayerMask);
                ui.close();
            }
            if has_mask
                && assets
                    .menu_item(ui, Icon::DeleteLayerMask, "Delete Layer Mask")
                    .clicked()
            {
                context_action = Some(ContextAction::DeleteLayerMask);
                ui.close();
            }
            if canvas_state.layers.len() > 1
                && assets
                    .menu_item(ui, Icon::Flatten, &t!("layer.flatten_all"))
                    .clicked()
            {
                context_action = Some(ContextAction::FlattenImage);
                ui.close();
            }
            ui.separator();
            // Move submenu
            let layer_count = canvas_state.layers.len();
            let can_up = layer_idx + 1 < layer_count;
            let can_down = layer_idx > 0;
            if assets
                .menu_item_enabled(ui, Icon::MoveTop, &t!("layer.move_to_top"), can_up)
                .clicked()
            {
                context_action = Some(ContextAction::MoveToTop);
                ui.close();
            }
            if assets
                .menu_item_enabled(ui, Icon::MoveUp, &t!("layer.move_up"), can_up)
                .clicked()
            {
                context_action = Some(ContextAction::MoveUp);
                ui.close();
            }
            if assets
                .menu_item_enabled(ui, Icon::MoveDown, &t!("layer.move_down"), can_down)
                .clicked()
            {
                context_action = Some(ContextAction::MoveDown);
                ui.close();
            }
            if assets
                .menu_item_enabled(ui, Icon::MoveBottom, &t!("layer.move_to_bottom"), can_down)
                .clicked()
            {
                context_action = Some(ContextAction::MoveToBottom);
                ui.close();
            }
            ui.separator();
            if assets
                .menu_item(ui, Icon::ImportLayer, &t!("layer.import_from_file"))
                .clicked()
            {
                context_action = Some(ContextAction::ImportFromFile);
                ui.close();
            }
            ui.separator();
            ui.menu_button(t!("layer.transform"), |ui| {
                if assets
                    .menu_item(ui, Icon::LayerFlipH, &t!("layer.transform.flip_horizontal"))
                    .clicked()
                {
                    context_action = Some(ContextAction::FlipHorizontal);
                    ui.close();
                }
                if assets
                    .menu_item(ui, Icon::LayerFlipV, &t!("layer.transform.flip_vertical"))
                    .clicked()
                {
                    context_action = Some(ContextAction::FlipVertical);
                    ui.close();
                }
                ui.separator();
                if assets
                    .menu_item(ui, Icon::MenuCanvasAlign, &t!("menu.canvas.align"))
                    .clicked()
                {
                    context_action = Some(ContextAction::AlignLayer);
                    ui.close();
                }
                ui.separator();
                if assets
                    .menu_item(ui, Icon::LayerRotate, &t!("layer.transform.rotate_scale"))
                    .clicked()
                {
                    context_action = Some(ContextAction::RotateScale);
                    ui.close();
                }
            });
            ui.separator();
            // Visibility group: Solo / Hide All / Show All
            {
                let is_this_soloed = self.peek_state.is_soloed
                    && self.peek_state.solo_layer_index == Some(layer_idx);
                let solo_label = if is_this_soloed {
                    t!("layer.unsolo_layer")
                } else {
                    t!("layer.solo_layer")
                };
                if assets.menu_item(ui, Icon::SoloLayer, &solo_label).clicked() {
                    if is_this_soloed {
                        context_action = Some(ContextAction::ShowAll);
                    } else {
                        context_action = Some(ContextAction::SoloLayer);
                    }
                    ui.close();
                }
                if assets
                    .menu_item(ui, Icon::HideAll, &t!("layer.hide_all"))
                    .clicked()
                {
                    context_action = Some(ContextAction::HideAll);
                    ui.close();
                }
                if assets
                    .menu_item(ui, Icon::ShowAll, &t!("layer.show_all"))
                    .clicked()
                {
                    context_action = Some(ContextAction::ShowAll);
                    ui.close();
                }
            }
            ui.separator();
            if assets
                .menu_item(ui, Icon::Rename, &t!("layer.rename_layer"))
                .clicked()
            {
                context_action = Some(ContextAction::Rename);
                ui.close();
            }
            if assets
                .menu_item(ui, Icon::LayerProperties, &t!("layer.layer_properties"))
                .clicked()
            {
                context_action = Some(ContextAction::OpenSettings);
                ui.close();
            }
            // Rasterize option for text layers + effects/warp
            if matches!(
                canvas_state.layers[layer_idx].content,
                LayerContent::Text(_)
            ) {
                ui.separator();
                if assets
                    .menu_item(ui, Icon::LayerProperties, &t!("layer.text_effects"))
                    .clicked()
                {
                    context_action = Some(ContextAction::TextLayerEffects);
                    ui.close();
                }
                if assets
                    .menu_item(ui, Icon::LayerProperties, &t!("layer.text_warp"))
                    .clicked()
                {
                    context_action = Some(ContextAction::TextLayerWarp);
                    ui.close();
                }
                ui.separator();
                if ui
                    .add(egui::Button::new(t!("layer.rasterize_text_layer")))
                    .clicked()
                {
                    context_action = Some(ContextAction::RasterizeTextLayer);
                    ui.close();
                }
            }
        });

        (action, context_action)
    }

    /// Draw layer thumbnail with checkerboard background
    fn draw_thumbnail(
        &mut self,
        ui: &mut egui::Ui,
        rect: Rect,
        layer_idx: usize,
        canvas_state: &CanvasState,
        alpha: f32,
    ) {
        // Get or create cached thumbnail texture first (requires mutable ui borrow)
        let texture = self.get_or_create_thumbnail(ui, layer_idx, canvas_state);

        // Now get painter for drawing (immutable borrow)
        let painter = ui.painter();

        // Draw checkerboard background for transparency
        let is_dark = ui.visuals().dark_mode;
        let grid_size = 6.0;
        let (light, dark) = if is_dark {
            (Color32::from_gray(60), Color32::from_gray(40))
        } else {
            (Color32::from_gray(240), Color32::from_gray(200))
        };

        let mut y = rect.top();
        let mut row = 0;
        while y < rect.bottom() {
            let mut x = rect.left();
            let mut col = 0;
            while x < rect.right() {
                let cell_rect = Rect::from_min_size(
                    Pos2::new(x, y),
                    Vec2::new(
                        (rect.right() - x).min(grid_size),
                        (rect.bottom() - y).min(grid_size),
                    ),
                );
                let color = if (row + col) % 2 == 0 { light } else { dark };
                painter.rect_filled(cell_rect, 0.0, color);
                x += grid_size;
                col += 1;
            }
            y += grid_size;
            row += 1;
        }

        // Draw border
        let border_color = if is_dark {
            Color32::from_gray(80)
        } else {
            Color32::from_gray(180)
        };
        painter.rect_stroke(
            rect,
            2.0,
            egui::Stroke::new(1.0, border_color),
            egui::StrokeKind::Middle,
        );

        // Draw cached thumbnail texture
        if let Some(texture) = texture {
            let tint = Color32::from_rgba_unmultiplied(255, 255, 255, (alpha * 255.0) as u8);
            painter.image(
                texture.id(),
                rect.shrink(1.0),
                Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                tint,
            );
        }
    }

    /// Get or create a cached thumbnail texture for a layer
    /// Uses canvas dirty_generation + time throttle (500ms) to avoid excessive recomputation
    fn get_or_create_thumbnail(
        &mut self,
        ui: &mut egui::Ui,
        layer_idx: usize,
        canvas_state: &CanvasState,
    ) -> Option<TextureHandle> {
        let current_gen = canvas_state.dirty_generation;
        let now = Instant::now();

        // Check cache: reuse if generation hasn't changed, or if changed but too recently updated
        if let Some(cache) = self.thumbnail_cache.get(&layer_idx) {
            if cache.last_generation == current_gen {
                // Nothing changed since last thumbnail
                return cache.texture.clone();
            }
            // Generation changed — only refresh if at least 500ms since last update
            if now.duration_since(cache.last_update).as_millis() < 500 {
                // Too soon, request a repaint later and return stale thumbnail
                ui.ctx()
                    .request_repaint_after(std::time::Duration::from_millis(500));
                return cache.texture.clone();
            }
        }

        // Generate new thumbnail
        let layer = &canvas_state.layers[layer_idx];

        // Include preview layer if this is the active layer and preview exists
        let is_active = layer_idx == canvas_state.active_layer_index;
        let thumbnail_image = if is_active && canvas_state.preview_layer.is_some() {
            self.generate_thumbnail_with_preview(
                &layer.pixels,
                canvas_state.preview_layer.as_ref().unwrap(),
                canvas_state.preview_blend_mode,
            )
        } else {
            self.generate_thumbnail(&layer.pixels)
        };

        let color_image = ColorImage::from_rgba_unmultiplied(
            [THUMBNAIL_SIZE as usize, THUMBNAIL_SIZE as usize],
            &thumbnail_image,
        );

        let texture = ui.ctx().load_texture(
            format!("layer_thumb_{}", layer_idx),
            color_image,
            TextureOptions::LINEAR,
        );

        // Cache it
        self.thumbnail_cache.insert(
            layer_idx,
            ThumbnailCache {
                texture: Some(texture.clone()),
                last_generation: current_gen,
                last_update: now,
            },
        );

        Some(texture)
    }

    /// Generate a downscaled thumbnail from full-size layer.
    /// Uses uniform scaling to preserve aspect ratio (letterbox/pillarbox).
    fn generate_thumbnail(&self, source: &TiledImage) -> Vec<u8> {
        let src_w = source.width() as f32;
        let src_h = source.height() as f32;
        let dst_size = THUMBNAIL_SIZE as f32;

        // Uniform scale: fit the largest dimension into THUMBNAIL_SIZE
        let scale = dst_size / src_w.max(src_h);
        let fit_w = (src_w * scale).round() as u32;
        let fit_h = (src_h * scale).round() as u32;
        // Offset to center the image within the square thumbnail
        let off_x = (THUMBNAIL_SIZE.saturating_sub(fit_w)) / 2;
        let off_y = (THUMBNAIL_SIZE.saturating_sub(fit_h)) / 2;

        let mut result = vec![0u8; (THUMBNAIL_SIZE * THUMBNAIL_SIZE * 4) as usize];

        for y in 0..THUMBNAIL_SIZE {
            for x in 0..THUMBNAIL_SIZE {
                // Only sample within the fitted region
                if x >= off_x && x < off_x + fit_w && y >= off_y && y < off_y + fit_h {
                    let local_x = x - off_x;
                    let local_y = y - off_y;
                    let src_x = ((local_x as f32 / fit_w as f32) * src_w) as u32;
                    let src_y = ((local_y as f32 / fit_h as f32) * src_h) as u32;

                    let src_x = src_x.min(source.width() - 1);
                    let src_y = src_y.min(source.height() - 1);

                    let pixel = source.get_pixel(src_x, src_y);
                    let idx = ((y * THUMBNAIL_SIZE + x) * 4) as usize;
                    result[idx] = pixel[0];
                    result[idx + 1] = pixel[1];
                    result[idx + 2] = pixel[2];
                    result[idx + 3] = pixel[3];
                }
                // Outside fitted region: stays transparent (0,0,0,0)
            }
        }

        result
    }

    /// Generate a thumbnail with preview layer composited on top
    /// Used for showing fill/brush previews in the active layer thumbnail
    fn generate_thumbnail_with_preview(
        &self,
        base_layer: &TiledImage,
        preview_layer: &TiledImage,
        blend_mode: BlendMode,
    ) -> Vec<u8> {
        let src_w = base_layer.width() as f32;
        let src_h = base_layer.height() as f32;
        let dst_size = THUMBNAIL_SIZE as f32;

        // Uniform scale: fit the largest dimension into THUMBNAIL_SIZE
        let scale = dst_size / src_w.max(src_h);
        let fit_w = (src_w * scale).round() as u32;
        let fit_h = (src_h * scale).round() as u32;
        // Offset to center the image within the square thumbnail
        let off_x = (THUMBNAIL_SIZE.saturating_sub(fit_w)) / 2;
        let off_y = (THUMBNAIL_SIZE.saturating_sub(fit_h)) / 2;

        let mut result = vec![0u8; (THUMBNAIL_SIZE * THUMBNAIL_SIZE * 4) as usize];

        for y in 0..THUMBNAIL_SIZE {
            for x in 0..THUMBNAIL_SIZE {
                // Only sample within the fitted region
                if x >= off_x && x < off_x + fit_w && y >= off_y && y < off_y + fit_h {
                    let local_x = x - off_x;
                    let local_y = y - off_y;
                    let src_x = ((local_x as f32 / fit_w as f32) * src_w) as u32;
                    let src_y = ((local_y as f32 / fit_h as f32) * src_h) as u32;

                    let src_x = src_x.min(base_layer.width() - 1);
                    let src_y = src_y.min(base_layer.height() - 1);

                    // Get base pixel
                    let base = base_layer.get_pixel(src_x, src_y);
                    // Get preview pixel
                    let preview = preview_layer.get_pixel(src_x, src_y);

                    // Composite preview on top of base using the blend mode
                    let composited = self.blend_pixels(*base, *preview, blend_mode);

                    let idx = ((y * THUMBNAIL_SIZE + x) * 4) as usize;
                    result[idx] = composited[0];
                    result[idx + 1] = composited[1];
                    result[idx + 2] = composited[2];
                    result[idx + 3] = composited[3];
                }
                // Outside fitted region: stays transparent (0,0,0,0)
            }
        }

        result
    }

    /// Simple alpha blend for thumbnail preview (Normal blend mode only for simplicity)
    /// Both inputs are in straight alpha format (RGB not premultiplied)
    fn blend_pixels(&self, base: Rgba<u8>, overlay: Rgba<u8>, _blend_mode: BlendMode) -> Rgba<u8> {
        // If overlay is fully transparent, return base
        if overlay[3] == 0 {
            return base;
        }

        // If base is fully transparent, return overlay
        if base[3] == 0 {
            return overlay;
        }

        // Convert to float for blending
        let base_a = base[3] as f32 / 255.0;
        let overlay_a = overlay[3] as f32 / 255.0;

        // Straight alpha "over" compositing formula:
        // result_rgb = overlay_rgb * overlay_a + base_rgb * (1 - overlay_a)
        // result_a = overlay_a + base_a * (1 - overlay_a)
        let one_minus_overlay_a = 1.0 - overlay_a;

        let result_r = (overlay[0] as f32 * overlay_a + base[0] as f32 * one_minus_overlay_a)
            .clamp(0.0, 255.0) as u8;
        let result_g = (overlay[1] as f32 * overlay_a + base[1] as f32 * one_minus_overlay_a)
            .clamp(0.0, 255.0) as u8;
        let result_b = (overlay[2] as f32 * overlay_a + base[2] as f32 * one_minus_overlay_a)
            .clamp(0.0, 255.0) as u8;
        let result_a = (overlay_a + base_a * one_minus_overlay_a).clamp(0.0, 1.0);
        let result_a_u8 = (result_a * 255.0) as u8;

        Rgba([result_r, result_g, result_b, result_a_u8])
    }

    /// Show the footer toolbar with layer action buttons
    fn show_footer_toolbar(
        &mut self,
        ui: &mut egui::Ui,
        canvas_state: &mut CanvasState,
        assets: &Assets,
        history: &mut HistoryManager,
    ) {
        ui.separator();

        // All actions in one row: New, Delete, Duplicate, Merge, Flatten, Options + count
        ui.horizontal(|ui| {
            // New Layer
            if assets.small_icon_button(ui, Icon::NewLayer).clicked() {
                self.add_new_layer(canvas_state, history);
            }

            // Delete Layer
            let can_delete = canvas_state.layers.len() > 1;
            if assets
                .icon_button_enabled(ui, Icon::Delete, can_delete)
                .clicked()
                && can_delete
            {
                self.delete_active_layer(canvas_state, history);
            }

            // Duplicate Layer
            if assets.small_icon_button(ui, Icon::Duplicate).clicked() {
                self.duplicate_layer(canvas_state.active_layer_index, canvas_state, history);
            }

            // Merge Down
            let can_merge = canvas_state.active_layer_index > 0;
            if assets
                .icon_button_enabled(ui, Icon::MergeDown, can_merge)
                .clicked()
                && can_merge
            {
                self.merge_down(canvas_state.active_layer_index, canvas_state, history);
            }

            // Flatten Image
            let can_flatten = canvas_state.layers.len() > 1;
            if assets
                .icon_button_enabled(ui, Icon::Flatten, can_flatten)
                .clicked()
                && can_flatten
            {
                self.flatten_image(canvas_state, history);
            }

            // Layer Options (settings)
            if assets.small_icon_button(ui, Icon::Settings).clicked() {
                let idx = canvas_state.active_layer_index;
                self.open_settings_for_layer(idx, canvas_state, LayerSettingsTab::General);
            }
        });
    }

    /// Open the layer settings dialog for a given layer, starting on the given tab.
    fn open_settings_for_layer(
        &mut self,
        idx: usize,
        canvas_state: &CanvasState,
        tab: LayerSettingsTab,
    ) {
        if idx >= canvas_state.layers.len() {
            return;
        }
        let layer = &canvas_state.layers[idx];
        self.settings_state.editing_layer = Some(idx);
        self.settings_state.editing_name = layer.name.clone();
        self.settings_state.editing_opacity = layer.opacity;
        self.settings_state.editing_blend_mode = layer.blend_mode;
        self.settings_state.tab = tab;
        self.settings_state.texture_load_rx = None;

        // Load text effects / warp if this is a text layer
        if let LayerContent::Text(ref td) = layer.content {
            self.settings_state.text_effects = td.effects.clone();
            // Load warp from the first block (or default)
            if let Some(block) = td.blocks.first() {
                self.settings_state.text_warp = block.warp.clone();
            } else {
                self.settings_state.text_warp = TextWarp::None;
            }
        } else {
            self.settings_state.text_effects = TextEffects::default();
            self.settings_state.text_warp = TextWarp::None;
        }
    }
}

