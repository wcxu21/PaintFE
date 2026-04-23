impl PaintFEApp {
    fn process_active_dialog(&mut self, ctx: &egui::Context) {
        // We need to take ownership temporarily to satisfy the borrow checker,
        // since dialogs need &mut self (via active_project_mut) on completion.
        let mut dialog = std::mem::take(&mut self.active_dialog);

        match &mut dialog {
            ActiveDialog::None => {}

            ActiveDialog::ResizeImage(dlg) => match dlg.show(ctx) {
                DialogResult::Ok((w, h, interp)) => {
                    self.settings.persist_resize_lock_aspect = dlg.lock_aspect;
                    self.active_dialog = ActiveDialog::None;
                    if let Some(project) = self.active_project_mut() {
                        // Rasterize text layers before extracting pixels for resize
                        project.canvas_state.ensure_all_text_layers_rasterized();
                        for layer in &mut project.canvas_state.layers {
                            if layer.is_text_layer() {
                                layer.content = crate::canvas::LayerContent::Raster;
                            }
                        }
                        let before = CanvasSnapshot::capture(&project.canvas_state);
                        let flat_layers: Vec<RgbaImage> = project
                            .canvas_state
                            .layers
                            .iter()
                            .map(|l| l.pixels.to_rgba_image())
                            .collect();
                        let sender = self.canvas_op_sender.clone();
                        let project_index = self.active_project_index;
                        let current_time = ctx.input(|i| i.time);
                        if self.pending_filter_jobs == 0 {
                            self.filter_ops_start_time = Some(current_time);
                        }
                        self.filter_status_description = "Resize Image".to_string();
                        self.pending_filter_jobs += 1;
                        rayon::spawn(move || {
                            let result_layers =
                                crate::ops::transform::resize_layers(flat_layers, w, h, interp);
                            let _ = sender.send(CanvasOpResult {
                                project_index,
                                before,
                                result_layers,
                                new_width: w,
                                new_height: h,
                                description: "Resize Image".to_string(),
                            });
                        });
                    }
                    return;
                }
                DialogResult::Cancel => {
                    self.settings.persist_resize_lock_aspect = dlg.lock_aspect;
                    self.active_dialog = ActiveDialog::None;
                    return;
                }
                _ => {}
            },

            ActiveDialog::ResizeCanvas(dlg) => {
                let secondary = self.colors_panel.get_secondary_color_f32();
                match dlg.show(ctx, secondary) {
                    DialogResult::Ok((w, h, anchor, fill)) => {
                        self.settings.persist_resize_lock_aspect = dlg.lock_aspect;
                        self.active_dialog = ActiveDialog::None;
                        let fill = if dlg.fill_transparent {
                            image::Rgba([0, 0, 0, 0])
                        } else {
                            fill
                        };
                        if let Some(project) = self.active_project_mut() {
                            // Rasterize text layers before extracting pixels for canvas resize
                            project.canvas_state.ensure_all_text_layers_rasterized();
                            for layer in &mut project.canvas_state.layers {
                                if layer.is_text_layer() {
                                    layer.content = crate::canvas::LayerContent::Raster;
                                }
                            }
                            let before = CanvasSnapshot::capture(&project.canvas_state);
                            let old_w = project.canvas_state.width;
                            let old_h = project.canvas_state.height;
                            let flat_layers: Vec<RgbaImage> = project
                                .canvas_state
                                .layers
                                .iter()
                                .map(|l| l.pixels.to_rgba_image())
                                .collect();
                            let sender = self.canvas_op_sender.clone();
                            let project_index = self.active_project_index;
                            let current_time = ctx.input(|i| i.time);
                            if self.pending_filter_jobs == 0 {
                                self.filter_ops_start_time = Some(current_time);
                            }
                            self.filter_status_description = "Resize Canvas".to_string();
                            self.pending_filter_jobs += 1;
                            rayon::spawn(move || {
                                let result_layers = crate::ops::transform::resize_canvas_layers(
                                    flat_layers,
                                    old_w,
                                    old_h,
                                    w,
                                    h,
                                    anchor,
                                    fill,
                                );
                                let _ = sender.send(CanvasOpResult {
                                    project_index,
                                    before,
                                    result_layers,
                                    new_width: w,
                                    new_height: h,
                                    description: "Resize Canvas".to_string(),
                                });
                            });
                        }
                        return;
                    }
                    DialogResult::Cancel => {
                        self.settings.persist_resize_lock_aspect = dlg.lock_aspect;
                        self.active_dialog = ActiveDialog::None;
                        return;
                    }
                    _ => {}
                }
            }

            ActiveDialog::AlignLayer(dlg) => match dlg.show(ctx) {
                DialogResult::Changed => {
                    let idx = dlg.layer_idx;
                    if let Some(flat) = &dlg.original_flat
                        && let Some(project) = self.active_project_mut()
                        && idx < project.canvas_state.layers.len()
                        && !project.canvas_state.layers[idx].is_text_layer()
                    {
                        let target_bounds = if dlg.align_to_selection {
                            project.canvas_state.selection_mask_bounds()
                        } else {
                            None
                        };
                        crate::ops::transform::align_layer_to_anchor_from_flat(
                            &mut project.canvas_state,
                            idx,
                            (dlg.anchor_x, dlg.anchor_y),
                            flat,
                            target_bounds,
                        );
                    }
                }
                DialogResult::Ok((_ax, _ay, _selection_target)) => {
                    self.active_dialog = ActiveDialog::None;
                    if let Some(project) = self.active_project_mut() {
                        let idx = dlg.layer_idx;
                        if let Some(original) = &dlg.original_pixels
                            && idx < project.canvas_state.layers.len()
                        {
                            let aligned = project.canvas_state.layers[idx].pixels.clone();
                            project.canvas_state.layers[idx].pixels = original.clone();
                            let mut cmd = SingleLayerSnapshotCommand::new_for_layer(
                                "Align Layer".to_string(),
                                &project.canvas_state,
                                idx,
                            );
                            project.canvas_state.layers[idx].pixels = aligned;
                            cmd.set_after(&project.canvas_state);
                            project.history.push(Box::new(cmd));
                        }
                        project.mark_dirty();
                    }
                    return;
                }
                DialogResult::Cancel => {
                    self.filter_cancel
                        .store(true, std::sync::atomic::Ordering::Relaxed);
                    let idx = dlg.layer_idx;
                    if let Some(original) = &dlg.original_pixels
                        && let Some(project) = self.active_project_mut()
                    {
                        if let Some(layer) = project.canvas_state.layers.get_mut(idx) {
                            layer.pixels = original.clone();
                        }
                        project.canvas_state.mark_dirty(None);
                    }
                    self.active_dialog = ActiveDialog::None;
                    return;
                }
                _ => {}
            },

            ActiveDialog::GaussianBlur(dlg) => {
                match dlg.show(ctx) {
                    DialogResult::Changed => {
                        // Spawn async preview job (token-tracked, interrupts stale previews)
                        let idx = dlg.layer_idx;
                        if let (Some(original), Some(flat)) =
                            (&dlg.original_pixels, &dlg.original_flat)
                        {
                            let sigma = dlg.sigma;
                            let sel_mask = self
                                .active_project()
                                .and_then(|p| p.canvas_state.selection_mask.clone());
                            self.spawn_preview_job(
                                ctx.input(|i| i.time),
                                "Gaussian Blur".to_string(),
                                idx,
                                original.clone(),
                                flat.clone(),
                                move |img| {
                                    crate::ops::filters::blur_with_selection_pub(
                                        img,
                                        sigma,
                                        sel_mask.as_ref(),
                                    )
                                },
                            );
                        }
                    }
                    DialogResult::Ok(_sigma) => {
                        self.preview_job_token = self.preview_job_token.wrapping_add(1);
                        // Spawn full-resolution blur on background thread.
                        self.filter_cancel
                            .store(true, std::sync::atomic::Ordering::Relaxed);
                        let sigma = dlg.sigma;
                        let idx = dlg.layer_idx;
                        self.active_dialog = ActiveDialog::None;
                        if let (Some(original_pixels), Some(original_flat)) =
                            (&dlg.original_pixels, &dlg.original_flat)
                        {
                            // Restore originals so the layer isn't left with preview data
                            if let Some(project) = self.active_project_mut()
                                && idx < project.canvas_state.layers.len()
                            {
                                project.canvas_state.layers[idx].pixels = original_pixels.clone();
                                project.canvas_state.mark_dirty(None);
                            }
                            let orig_clone = original_pixels.clone();
                            let flat_clone = original_flat.clone();
                            let sel_mask = self
                                .active_project()
                                .and_then(|p| p.canvas_state.selection_mask.clone());
                            self.spawn_filter_job(
                                ctx.input(|i| i.time),
                                "Gaussian Blur".to_string(),
                                idx,
                                orig_clone,
                                flat_clone,
                                move |flat| {
                                    crate::ops::filters::blur_with_selection_pub(
                                        flat,
                                        sigma,
                                        sel_mask.as_ref(),
                                    )
                                },
                            );
                        }
                        return;
                    }
                    DialogResult::Cancel => {
                        self.preview_job_token = self.preview_job_token.wrapping_add(1);
                        // Restore original pixels.
                        let idx = dlg.layer_idx;
                        self.filter_cancel
                            .store(true, std::sync::atomic::Ordering::Relaxed);
                        if let Some(original) = &dlg.original_pixels
                            && let Some(project) = self.active_project_mut()
                        {
                            if let Some(layer) = project.canvas_state.layers.get_mut(idx) {
                                layer.pixels = original.clone();
                            }
                            project.canvas_state.mark_dirty(None);
                        }
                        self.active_dialog = ActiveDialog::None;
                        return;
                    }
                    _ => {}
                }
            }

            ActiveDialog::LayerTransform(dlg) => {
                match dlg.show(ctx) {
                    DialogResult::Changed => {
                        // Live preview: use pre-flattened original to skip
                        // the expensive clone ÔåÆ flatten round-trip.
                        let rz = dlg.rotation_z;
                        let rx = dlg.rotation_x;
                        let ry = dlg.rotation_y;
                        let scale = dlg.scale_percent / 100.0;
                        let offset = (dlg.offset_x, dlg.offset_y);
                        let idx = dlg.layer_idx;
                        if let Some(flat) = &dlg.original_flat
                            && let Some(project) = self.active_project_mut()
                        {
                            crate::ops::transform::affine_transform_layer_from_flat(
                                &mut project.canvas_state,
                                idx,
                                rz,
                                rx,
                                ry,
                                scale,
                                offset,
                                flat,
                            );
                        }
                    }
                    DialogResult::Ok((_rot_z, _rot_x, _rot_y, _scale, _offset)) => {
                        // Accept current preview ÔÇö push undo.
                        self.active_dialog = ActiveDialog::None;
                        if let Some(project) = self.active_project_mut() {
                            let idx = dlg.layer_idx;
                            if let Some(original) = &dlg.original_pixels
                                && idx < project.canvas_state.layers.len()
                            {
                                let transformed = project.canvas_state.layers[idx].pixels.clone();
                                project.canvas_state.layers[idx].pixels = original.clone();
                                let mut cmd = SingleLayerSnapshotCommand::new_for_layer(
                                    "Layer Transform".to_string(),
                                    &project.canvas_state,
                                    idx,
                                );
                                project.canvas_state.layers[idx].pixels = transformed;
                                cmd.set_after(&project.canvas_state);
                                project.history.push(Box::new(cmd));
                            }
                            project.mark_dirty();
                        }
                        return;
                    }
                    DialogResult::Cancel => {
                        // Restore original pixels.
                        self.filter_cancel
                            .store(true, std::sync::atomic::Ordering::Relaxed);
                        let idx = dlg.layer_idx;
                        if let Some(original) = &dlg.original_pixels
                            && let Some(project) = self.active_project_mut()
                        {
                            if let Some(layer) = project.canvas_state.layers.get_mut(idx) {
                                layer.pixels = original.clone();
                            }
                            project.canvas_state.mark_dirty(None);
                        }
                        self.active_dialog = ActiveDialog::None;
                        return;
                    }
                    _ => {}
                }
            }

            // ================================================================
            // BRIGHTNESS / CONTRAST
            // ================================================================
            ActiveDialog::BrightnessContrast(dlg) => match dlg.show(ctx) {
                DialogResult::Changed => {
                    let brightness = dlg.brightness;
                    let contrast = dlg.contrast;
                    let idx = dlg.layer_idx;
                    if let Some(flat) = &dlg.original_flat {
                        let active_idx = self.active_project_index;
                        if let Some(project) = self.projects.get_mut(active_idx) {
                            crate::ops::adjustments::brightness_contrast_from_flat_gpu(
                                &mut project.canvas_state,
                                idx,
                                brightness,
                                contrast,
                                flat,
                                &self.canvas.gpu_renderer,
                            );
                        }
                    }
                }
                DialogResult::Ok(_) => {
                    self.active_dialog = ActiveDialog::None;
                    if let Some(project) = self.active_project_mut() {
                        let idx = dlg.layer_idx;
                        if let Some(original) = &dlg.original_pixels
                            && idx < project.canvas_state.layers.len()
                        {
                            let adjusted = project.canvas_state.layers[idx].pixels.clone();
                            project.canvas_state.layers[idx].pixels = original.clone();
                            let mut cmd = SingleLayerSnapshotCommand::new_for_layer(
                                "Brightness/Contrast".to_string(),
                                &project.canvas_state,
                                idx,
                            );
                            project.canvas_state.layers[idx].pixels = adjusted;
                            cmd.set_after(&project.canvas_state);
                            project.history.push(Box::new(cmd));
                        }
                        project.mark_dirty();
                    }
                    return;
                }
                DialogResult::Cancel => {
                    self.filter_cancel
                        .store(true, std::sync::atomic::Ordering::Relaxed);
                    let idx = dlg.layer_idx;
                    if let Some(original) = &dlg.original_pixels
                        && let Some(project) = self.active_project_mut()
                    {
                        if let Some(layer) = project.canvas_state.layers.get_mut(idx) {
                            layer.pixels = original.clone();
                        }
                        project.canvas_state.mark_dirty(None);
                    }
                    self.active_dialog = ActiveDialog::None;
                    return;
                }
                _ => {}
            },

            // ================================================================
            // HUE / SATURATION
            // ================================================================
            ActiveDialog::HueSaturation(dlg) => match dlg.show(ctx) {
                DialogResult::Changed => {
                    let hue = dlg.hue;
                    let sat = dlg.saturation;
                    let light = dlg.lightness;
                    let idx = dlg.layer_idx;
                    let per_band = dlg.per_band;
                    let bands = dlg.bands;
                    if let Some(flat) = &dlg.original_flat {
                        let active_idx = self.active_project_index;
                        if let Some(project) = self.projects.get_mut(active_idx) {
                            if per_band {
                                crate::ops::adjustments::hue_saturation_per_band_from_flat(
                                    &mut project.canvas_state,
                                    idx,
                                    hue,
                                    sat,
                                    light,
                                    &bands,
                                    flat,
                                );
                            } else {
                                crate::ops::adjustments::hue_saturation_lightness_from_flat_gpu(
                                    &mut project.canvas_state,
                                    idx,
                                    hue,
                                    sat,
                                    light,
                                    flat,
                                    &self.canvas.gpu_renderer,
                                );
                            }
                        }
                    }
                }
                DialogResult::Ok(_) => {
                    self.active_dialog = ActiveDialog::None;
                    if let Some(project) = self.active_project_mut() {
                        let idx = dlg.layer_idx;
                        let label = if dlg.per_band {
                            "Hue/Saturation (Per Band)"
                        } else {
                            "Hue/Saturation"
                        };
                        if let Some(original) = &dlg.original_pixels
                            && idx < project.canvas_state.layers.len()
                        {
                            let adjusted = project.canvas_state.layers[idx].pixels.clone();
                            project.canvas_state.layers[idx].pixels = original.clone();
                            let mut cmd = SingleLayerSnapshotCommand::new_for_layer(
                                label.to_string(),
                                &project.canvas_state,
                                idx,
                            );
                            project.canvas_state.layers[idx].pixels = adjusted;
                            cmd.set_after(&project.canvas_state);
                            project.history.push(Box::new(cmd));
                        }
                        project.mark_dirty();
                    }
                    return;
                }
                DialogResult::Cancel => {
                    self.filter_cancel
                        .store(true, std::sync::atomic::Ordering::Relaxed);
                    let idx = dlg.layer_idx;
                    if let Some(original) = &dlg.original_pixels
                        && let Some(project) = self.active_project_mut()
                    {
                        if let Some(layer) = project.canvas_state.layers.get_mut(idx) {
                            layer.pixels = original.clone();
                        }
                        project.canvas_state.mark_dirty(None);
                    }
                    self.active_dialog = ActiveDialog::None;
                    return;
                }
                _ => {}
            },

            // ================================================================
            // EXPOSURE
            // ================================================================
            ActiveDialog::Exposure(dlg) => match dlg.show(ctx) {
                DialogResult::Changed => {
                    let exposure = dlg.exposure;
                    let idx = dlg.layer_idx;
                    if let Some(flat) = &dlg.original_flat
                        && let Some(project) = self.active_project_mut()
                    {
                        crate::ops::adjustments::exposure_from_flat(
                            &mut project.canvas_state,
                            idx,
                            exposure,
                            flat,
                        );
                    }
                }
                DialogResult::Ok(_) => {
                    self.active_dialog = ActiveDialog::None;
                    if let Some(project) = self.active_project_mut() {
                        let idx = dlg.layer_idx;
                        if let Some(original) = &dlg.original_pixels
                            && idx < project.canvas_state.layers.len()
                        {
                            let adjusted = project.canvas_state.layers[idx].pixels.clone();
                            project.canvas_state.layers[idx].pixels = original.clone();
                            let mut cmd = SingleLayerSnapshotCommand::new_for_layer(
                                "Exposure".to_string(),
                                &project.canvas_state,
                                idx,
                            );
                            project.canvas_state.layers[idx].pixels = adjusted;
                            cmd.set_after(&project.canvas_state);
                            project.history.push(Box::new(cmd));
                        }
                        project.mark_dirty();
                    }
                    return;
                }
                DialogResult::Cancel => {
                    self.filter_cancel
                        .store(true, std::sync::atomic::Ordering::Relaxed);
                    let idx = dlg.layer_idx;
                    if let Some(original) = &dlg.original_pixels
                        && let Some(project) = self.active_project_mut()
                    {
                        if let Some(layer) = project.canvas_state.layers.get_mut(idx) {
                            layer.pixels = original.clone();
                        }
                        project.canvas_state.mark_dirty(None);
                    }
                    self.active_dialog = ActiveDialog::None;
                    return;
                }
                _ => {}
            },

            // ================================================================
            // HIGHLIGHTS / SHADOWS
            // ================================================================
            ActiveDialog::HighlightsShadows(dlg) => match dlg.show(ctx) {
                DialogResult::Changed => {
                    let shadows = dlg.shadows;
                    let highlights = dlg.highlights;
                    let idx = dlg.layer_idx;
                    if let Some(flat) = &dlg.original_flat
                        && let Some(project) = self.active_project_mut()
                    {
                        crate::ops::adjustments::highlights_shadows_from_flat(
                            &mut project.canvas_state,
                            idx,
                            shadows,
                            highlights,
                            flat,
                        );
                    }
                }
                DialogResult::Ok(_) => {
                    self.active_dialog = ActiveDialog::None;
                    if let Some(project) = self.active_project_mut() {
                        let idx = dlg.layer_idx;
                        if let Some(original) = &dlg.original_pixels
                            && idx < project.canvas_state.layers.len()
                        {
                            let adjusted = project.canvas_state.layers[idx].pixels.clone();
                            project.canvas_state.layers[idx].pixels = original.clone();
                            let mut cmd = SingleLayerSnapshotCommand::new_for_layer(
                                "Highlights/Shadows".to_string(),
                                &project.canvas_state,
                                idx,
                            );
                            project.canvas_state.layers[idx].pixels = adjusted;
                            cmd.set_after(&project.canvas_state);
                            project.history.push(Box::new(cmd));
                        }
                        project.mark_dirty();
                    }
                    return;
                }
                DialogResult::Cancel => {
                    self.filter_cancel
                        .store(true, std::sync::atomic::Ordering::Relaxed);
                    let idx = dlg.layer_idx;
                    if let Some(original) = &dlg.original_pixels
                        && let Some(project) = self.active_project_mut()
                    {
                        if let Some(layer) = project.canvas_state.layers.get_mut(idx) {
                            layer.pixels = original.clone();
                        }
                        project.canvas_state.mark_dirty(None);
                    }
                    self.active_dialog = ActiveDialog::None;
                    return;
                }
                _ => {}
            },

            // ================================================================
            // LEVELS
            // ================================================================
            ActiveDialog::Levels(dlg) => match dlg.show(ctx) {
                DialogResult::Changed => {
                    let res = dlg.as_result();
                    let idx = dlg.layer_idx;
                    if let Some(flat) = &dlg.original_flat
                        && let Some(project) = self.active_project_mut()
                    {
                        crate::ops::adjustments::levels_from_flat_per_channel(
                            &mut project.canvas_state,
                            idx,
                            res.master,
                            res.r_ch,
                            res.g_ch,
                            res.b_ch,
                            flat,
                        );
                    }
                }
                DialogResult::Ok(_) => {
                    self.active_dialog = ActiveDialog::None;
                    if let Some(project) = self.active_project_mut() {
                        let idx = dlg.layer_idx;
                        if let Some(original) = &dlg.original_pixels
                            && idx < project.canvas_state.layers.len()
                        {
                            let adjusted = project.canvas_state.layers[idx].pixels.clone();
                            project.canvas_state.layers[idx].pixels = original.clone();
                            let mut cmd = SingleLayerSnapshotCommand::new_for_layer(
                                "Levels".to_string(),
                                &project.canvas_state,
                                idx,
                            );
                            project.canvas_state.layers[idx].pixels = adjusted;
                            cmd.set_after(&project.canvas_state);
                            project.history.push(Box::new(cmd));
                        }
                        project.mark_dirty();
                    }
                    return;
                }
                DialogResult::Cancel => {
                    self.filter_cancel
                        .store(true, std::sync::atomic::Ordering::Relaxed);
                    let idx = dlg.layer_idx;
                    if let Some(original) = &dlg.original_pixels
                        && let Some(project) = self.active_project_mut()
                    {
                        if let Some(layer) = project.canvas_state.layers.get_mut(idx) {
                            layer.pixels = original.clone();
                        }
                        project.canvas_state.mark_dirty(None);
                    }
                    self.active_dialog = ActiveDialog::None;
                    return;
                }
                _ => {}
            },

            // ================================================================
            // CURVES
            // ================================================================
            ActiveDialog::Curves(dlg) => match dlg.show(ctx) {
                DialogResult::Changed => {
                    let ch_data: [(&[(f32, f32)], bool); 5] = [
                        (&dlg.channels[0].points, dlg.channels[0].enabled),
                        (&dlg.channels[1].points, dlg.channels[1].enabled),
                        (&dlg.channels[2].points, dlg.channels[2].enabled),
                        (&dlg.channels[3].points, dlg.channels[3].enabled),
                        (&dlg.channels[4].points, dlg.channels[4].enabled),
                    ];
                    let idx = dlg.layer_idx;
                    if let Some(flat) = &dlg.original_flat
                        && let Some(project) = self.active_project_mut()
                    {
                        crate::ops::adjustments::curves_from_flat_multi(
                            &mut project.canvas_state,
                            idx,
                            &ch_data,
                            flat,
                        );
                    }
                }
                DialogResult::Ok(_) => {
                    self.active_dialog = ActiveDialog::None;
                    if let Some(project) = self.active_project_mut() {
                        let idx = dlg.layer_idx;
                        if let Some(original) = &dlg.original_pixels
                            && idx < project.canvas_state.layers.len()
                        {
                            let adjusted = project.canvas_state.layers[idx].pixels.clone();
                            project.canvas_state.layers[idx].pixels = original.clone();
                            let mut cmd = SingleLayerSnapshotCommand::new_for_layer(
                                "Curves".to_string(),
                                &project.canvas_state,
                                idx,
                            );
                            project.canvas_state.layers[idx].pixels = adjusted;
                            cmd.set_after(&project.canvas_state);
                            project.history.push(Box::new(cmd));
                        }
                        project.mark_dirty();
                    }
                    return;
                }
                DialogResult::Cancel => {
                    self.filter_cancel
                        .store(true, std::sync::atomic::Ordering::Relaxed);
                    let idx = dlg.layer_idx;
                    if let Some(original) = &dlg.original_pixels
                        && let Some(project) = self.active_project_mut()
                    {
                        if let Some(layer) = project.canvas_state.layers.get_mut(idx) {
                            layer.pixels = original.clone();
                        }
                        project.canvas_state.mark_dirty(None);
                    }
                    self.active_dialog = ActiveDialog::None;
                    return;
                }
                _ => {}
            },

            // ================================================================
            // TEMPERATURE / TINT
            // ================================================================
            ActiveDialog::TemperatureTint(dlg) => match dlg.show(ctx) {
                DialogResult::Changed => {
                    let temperature = dlg.temperature;
                    let tint = dlg.tint;
                    let idx = dlg.layer_idx;
                    if let Some(flat) = &dlg.original_flat
                        && let Some(project) = self.active_project_mut()
                    {
                        crate::ops::adjustments::temperature_tint_from_flat(
                            &mut project.canvas_state,
                            idx,
                            temperature,
                            tint,
                            flat,
                        );
                    }
                }
                DialogResult::Ok(_) => {
                    self.active_dialog = ActiveDialog::None;
                    if let Some(project) = self.active_project_mut() {
                        let idx = dlg.layer_idx;
                        if let Some(original) = &dlg.original_pixels
                            && idx < project.canvas_state.layers.len()
                        {
                            let adjusted = project.canvas_state.layers[idx].pixels.clone();
                            project.canvas_state.layers[idx].pixels = original.clone();
                            let mut cmd = SingleLayerSnapshotCommand::new_for_layer(
                                "Temperature/Tint".to_string(),
                                &project.canvas_state,
                                idx,
                            );
                            project.canvas_state.layers[idx].pixels = adjusted;
                            cmd.set_after(&project.canvas_state);
                            project.history.push(Box::new(cmd));
                        }
                        project.mark_dirty();
                    }
                    return;
                }
                DialogResult::Cancel => {
                    self.filter_cancel
                        .store(true, std::sync::atomic::Ordering::Relaxed);
                    let idx = dlg.layer_idx;
                    if let Some(original) = &dlg.original_pixels
                        && let Some(project) = self.active_project_mut()
                    {
                        if let Some(layer) = project.canvas_state.layers.get_mut(idx) {
                            layer.pixels = original.clone();
                        }
                        project.canvas_state.mark_dirty(None);
                    }
                    self.active_dialog = ActiveDialog::None;
                    return;
                }
                _ => {}
            },

            // ================================================================
            // EFFECT DIALOGS ÔÇö macroified common patterns
            // ================================================================

            // Helper closure-like pattern: all effect dialogs follow the same
            // Changed / Ok / Cancel structure, just with different apply functions.
            ActiveDialog::BokehBlur(dlg) => {
                match dlg.show(ctx) {
                    DialogResult::Changed => {
                        // Spawn async preview job (token-tracked, interrupts stale previews)
                        dlg.poll_flat();
                        let idx = dlg.layer_idx;
                        if let (Some(original), Some(flat)) =
                            (&dlg.original_pixels, &dlg.original_flat)
                        {
                            let radius = dlg.radius;
                            self.spawn_preview_job(
                                ctx.input(|i| i.time),
                                "Bokeh Blur".to_string(),
                                idx,
                                original.clone(),
                                flat.clone(),
                                move |img| crate::ops::effects::bokeh_blur_core(img, radius, None),
                            );
                        }
                    }
                    DialogResult::Ok(_) => {
                        self.preview_job_token = self.preview_job_token.wrapping_add(1);
                        // Apply at full resolution
                        let idx = dlg.layer_idx;
                        if let Some(flat) = &dlg.original_flat {
                            let radius = dlg.radius;
                            if let Some(project) = self.active_project_mut() {
                                Self::apply_fullres_effect(
                                    &mut project.canvas_state,
                                    idx,
                                    flat,
                                    |img| crate::ops::effects::bokeh_blur_core(img, radius, None),
                                );
                            }
                        }
                        self.active_dialog = ActiveDialog::None;
                        if let Some(project) = self.active_project_mut() {
                            let idx = dlg.layer_idx;
                            if let Some(original) = &dlg.original_pixels
                                && idx < project.canvas_state.layers.len()
                            {
                                let adjusted = project.canvas_state.layers[idx].pixels.clone();
                                project.canvas_state.layers[idx].pixels = original.clone();
                                let mut cmd = SingleLayerSnapshotCommand::new_for_layer(
                                    "Bokeh Blur".to_string(),
                                    &project.canvas_state,
                                    idx,
                                );
                                project.canvas_state.layers[idx].pixels = adjusted;
                                cmd.set_after(&project.canvas_state);
                                project.history.push(Box::new(cmd));
                            }
                            project.mark_dirty();
                        }
                        return;
                    }
                    DialogResult::Cancel => {
                        self.preview_job_token = self.preview_job_token.wrapping_add(1);
                        self.filter_cancel
                            .store(true, std::sync::atomic::Ordering::Relaxed);
                        let idx = dlg.layer_idx;
                        if let Some(original) = &dlg.original_pixels
                            && let Some(project) = self.active_project_mut()
                        {
                            if let Some(layer) = project.canvas_state.layers.get_mut(idx) {
                                layer.pixels = original.clone();
                            }
                            project.canvas_state.mark_dirty(None);
                        }
                        self.active_dialog = ActiveDialog::None;
                        return;
                    }
                    _ => {
                        dlg.poll_flat();
                    }
                }
            }

            ActiveDialog::MotionBlur(dlg) => {
                match dlg.show(ctx) {
                    DialogResult::Changed => {
                        // Spawn async preview job (token-tracked, interrupts stale previews)
                        dlg.poll_flat();
                        let idx = dlg.layer_idx;
                        if let (Some(original), Some(flat)) =
                            (&dlg.original_pixels, &dlg.original_flat)
                        {
                            let (angle, distance) = (dlg.angle, dlg.distance);
                            self.spawn_preview_job(
                                ctx.input(|i| i.time),
                                "Motion Blur".to_string(),
                                idx,
                                original.clone(),
                                flat.clone(),
                                move |img| {
                                    crate::ops::effects::motion_blur_core(
                                        img, angle, distance, None,
                                    )
                                },
                            );
                        }
                    }
                    DialogResult::Ok(_) => {
                        self.preview_job_token = self.preview_job_token.wrapping_add(1);
                        let idx = dlg.layer_idx;
                        if let Some(flat) = &dlg.original_flat {
                            let (angle, distance) = (dlg.angle, dlg.distance);
                            if let Some(project) = self.active_project_mut() {
                                Self::apply_fullres_effect(
                                    &mut project.canvas_state,
                                    idx,
                                    flat,
                                    |img| {
                                        crate::ops::effects::motion_blur_core(
                                            img, angle, distance, None,
                                        )
                                    },
                                );
                            }
                        }
                        self.active_dialog = ActiveDialog::None;
                        if let Some(project) = self.active_project_mut() {
                            let idx = dlg.layer_idx;
                            if let Some(original) = &dlg.original_pixels
                                && idx < project.canvas_state.layers.len()
                            {
                                let adjusted = project.canvas_state.layers[idx].pixels.clone();
                                project.canvas_state.layers[idx].pixels = original.clone();
                                let mut cmd = SingleLayerSnapshotCommand::new_for_layer(
                                    "Motion Blur".to_string(),
                                    &project.canvas_state,
                                    idx,
                                );
                                project.canvas_state.layers[idx].pixels = adjusted;
                                cmd.set_after(&project.canvas_state);
                                project.history.push(Box::new(cmd));
                            }
                            project.mark_dirty();
                        }
                        return;
                    }
                    DialogResult::Cancel => {
                        self.preview_job_token = self.preview_job_token.wrapping_add(1);
                        self.filter_cancel
                            .store(true, std::sync::atomic::Ordering::Relaxed);
                        let idx = dlg.layer_idx;
                        if let Some(original) = &dlg.original_pixels
                            && let Some(project) = self.active_project_mut()
                        {
                            if let Some(layer) = project.canvas_state.layers.get_mut(idx) {
                                layer.pixels = original.clone();
                            }
                            project.canvas_state.mark_dirty(None);
                        }
                        self.active_dialog = ActiveDialog::None;
                        return;
                    }
                    _ => {
                        dlg.poll_flat();
                    }
                }
            }

            ActiveDialog::BoxBlur(dlg) => {
                match dlg.show(ctx) {
                    DialogResult::Changed => {
                        // Spawn async preview job (token-tracked, interrupts stale previews)
                        dlg.poll_flat();
                        let idx = dlg.layer_idx;
                        if let (Some(original), Some(flat)) =
                            (&dlg.original_pixels, &dlg.original_flat)
                        {
                            let radius = dlg.radius;
                            self.spawn_preview_job(
                                ctx.input(|i| i.time),
                                "Box Blur".to_string(),
                                idx,
                                original.clone(),
                                flat.clone(),
                                move |img| crate::ops::effects::box_blur_core(img, radius, None),
                            );
                        }
                    }
                    DialogResult::Ok(_) => {
                        self.preview_job_token = self.preview_job_token.wrapping_add(1);
                        let idx = dlg.layer_idx;
                        if let Some(flat) = &dlg.original_flat {
                            let radius = dlg.radius;
                            if let Some(project) = self.active_project_mut() {
                                Self::apply_fullres_effect(
                                    &mut project.canvas_state,
                                    idx,
                                    flat,
                                    |img| crate::ops::effects::box_blur_core(img, radius, None),
                                );
                            }
                        }
                        self.active_dialog = ActiveDialog::None;
                        if let Some(project) = self.active_project_mut() {
                            let idx = dlg.layer_idx;
                            if let Some(original) = &dlg.original_pixels
                                && idx < project.canvas_state.layers.len()
                            {
                                let adjusted = project.canvas_state.layers[idx].pixels.clone();
                                project.canvas_state.layers[idx].pixels = original.clone();
                                let mut cmd = SingleLayerSnapshotCommand::new_for_layer(
                                    "Box Blur".to_string(),
                                    &project.canvas_state,
                                    idx,
                                );
                                project.canvas_state.layers[idx].pixels = adjusted;
                                cmd.set_after(&project.canvas_state);
                                project.history.push(Box::new(cmd));
                            }
                            project.mark_dirty();
                        }
                        return;
                    }
                    DialogResult::Cancel => {
                        self.preview_job_token = self.preview_job_token.wrapping_add(1);
                        self.filter_cancel
                            .store(true, std::sync::atomic::Ordering::Relaxed);
                        let idx = dlg.layer_idx;
                        if let Some(original) = &dlg.original_pixels
                            && let Some(project) = self.active_project_mut()
                        {
                            if let Some(layer) = project.canvas_state.layers.get_mut(idx) {
                                layer.pixels = original.clone();
                            }
                            project.canvas_state.mark_dirty(None);
                        }
                        self.active_dialog = ActiveDialog::None;
                        return;
                    }
                    _ => {
                        dlg.poll_flat();
                    }
                }
            }

            ActiveDialog::ZoomBlur(dlg) => match dlg.show(ctx) {
                DialogResult::Changed => {
                    dlg.poll_flat();
                    let idx = dlg.layer_idx;
                    if let (Some(original), Some(flat)) = (&dlg.original_pixels, &dlg.original_flat)
                    {
                        let (cx, cy, strength, samples, tint, ts) = dlg.current_params();
                        self.spawn_preview_job(
                            ctx.input(|i| i.time),
                            "Zoom Blur".to_string(),
                            idx,
                            original.clone(),
                            flat.clone(),
                            move |img| {
                                crate::ops::effects::zoom_blur_core(
                                    img, cx, cy, strength, samples, tint, ts, None,
                                )
                            },
                        );
                    }
                }
                DialogResult::Ok(_) => {
                    self.preview_job_token = self.preview_job_token.wrapping_add(1);
                    let idx = dlg.layer_idx;
                    if let Some(flat) = &dlg.original_flat {
                        let (cx, cy, strength, samples, tint, ts) = dlg.current_params();
                        if let Some(project) = self.active_project_mut() {
                            Self::apply_fullres_effect(
                                &mut project.canvas_state,
                                idx,
                                flat,
                                |img| {
                                    crate::ops::effects::zoom_blur_core(
                                        img, cx, cy, strength, samples, tint, ts, None,
                                    )
                                },
                            );
                        }
                    }
                    self.active_dialog = ActiveDialog::None;
                    if let Some(project) = self.active_project_mut() {
                        let idx = dlg.layer_idx;
                        if let Some(original) = &dlg.original_pixels
                            && idx < project.canvas_state.layers.len()
                        {
                            let adjusted = project.canvas_state.layers[idx].pixels.clone();
                            project.canvas_state.layers[idx].pixels = original.clone();
                            let mut cmd = SingleLayerSnapshotCommand::new_for_layer(
                                "Zoom Blur".to_string(),
                                &project.canvas_state,
                                idx,
                            );
                            project.canvas_state.layers[idx].pixels = adjusted;
                            cmd.set_after(&project.canvas_state);
                            project.history.push(Box::new(cmd));
                        }
                        project.mark_dirty();
                    }
                    return;
                }
                DialogResult::Cancel => {
                    self.preview_job_token = self.preview_job_token.wrapping_add(1);
                    self.filter_cancel
                        .store(true, std::sync::atomic::Ordering::Relaxed);
                    let idx = dlg.layer_idx;
                    if let Some(original) = &dlg.original_pixels
                        && let Some(project) = self.active_project_mut()
                    {
                        if let Some(layer) = project.canvas_state.layers.get_mut(idx) {
                            layer.pixels = original.clone();
                        }
                        project.canvas_state.mark_dirty(None);
                    }
                    self.active_dialog = ActiveDialog::None;
                    return;
                }
                _ => {
                    dlg.poll_flat();
                }
            },

            ActiveDialog::Crystallize(dlg) => match dlg.show(ctx) {
                DialogResult::Changed => {
                    dlg.first_open = false;
                    let idx = dlg.layer_idx;
                    if let (Some(original), Some(flat)) = (&dlg.original_pixels, &dlg.original_flat)
                    {
                        let (cell_size, seed) = (dlg.cell_size, dlg.seed);
                        self.spawn_preview_job(
                            ctx.input(|i| i.time),
                            "Crystallize".to_string(),
                            idx,
                            original.clone(),
                            flat.clone(),
                            move |img| {
                                crate::ops::effects::crystallize_core(img, cell_size, seed, None)
                            },
                        );
                    }
                }
                DialogResult::Ok(_) => {
                    self.preview_job_token = self.preview_job_token.wrapping_add(1);
                    let idx = dlg.layer_idx;
                    if let Some(flat) = &dlg.original_flat {
                        let (cell_size, seed) = (dlg.cell_size, dlg.seed);
                        if let Some(project) = self.active_project_mut() {
                            Self::apply_fullres_effect(
                                &mut project.canvas_state,
                                idx,
                                flat,
                                |img| {
                                    crate::ops::effects::crystallize_core(
                                        img, cell_size, seed, None,
                                    )
                                },
                            );
                        }
                    }
                    self.active_dialog = ActiveDialog::None;
                    if let Some(project) = self.active_project_mut() {
                        let idx = dlg.layer_idx;
                        if let Some(original) = &dlg.original_pixels
                            && idx < project.canvas_state.layers.len()
                        {
                            let adjusted = project.canvas_state.layers[idx].pixels.clone();
                            project.canvas_state.layers[idx].pixels = original.clone();
                            let mut cmd = SingleLayerSnapshotCommand::new_for_layer(
                                "Crystallize".to_string(),
                                &project.canvas_state,
                                idx,
                            );
                            project.canvas_state.layers[idx].pixels = adjusted;
                            cmd.set_after(&project.canvas_state);
                            project.history.push(Box::new(cmd));
                        }
                        project.mark_dirty();
                    }
                    return;
                }
                DialogResult::Cancel => {
                    self.preview_job_token = self.preview_job_token.wrapping_add(1);
                    self.filter_cancel
                        .store(true, std::sync::atomic::Ordering::Relaxed);
                    let idx = dlg.layer_idx;
                    if let Some(original) = &dlg.original_pixels
                        && let Some(project) = self.active_project_mut()
                    {
                        if let Some(layer) = project.canvas_state.layers.get_mut(idx) {
                            layer.pixels = original.clone();
                        }
                        project.canvas_state.mark_dirty(None);
                    }
                    self.active_dialog = ActiveDialog::None;
                    return;
                }
                _ => {
                    dlg.poll_flat();
                    if dlg.first_open && dlg.live_preview && dlg.original_flat.is_some() {
                        dlg.first_open = false;
                        let idx = dlg.layer_idx;
                        if let (Some(original), Some(flat)) =
                            (&dlg.original_pixels, &dlg.original_flat)
                        {
                            let (cell_size, seed) = (dlg.cell_size, dlg.seed);
                            self.spawn_preview_job(
                                ctx.input(|i| i.time),
                                "Crystallize".to_string(),
                                idx,
                                original.clone(),
                                flat.clone(),
                                move |img| {
                                    crate::ops::effects::crystallize_core(
                                        img, cell_size, seed, None,
                                    )
                                },
                            );
                        }
                    }
                }
            },

            ActiveDialog::Dents(dlg) => match dlg.show(ctx) {
                DialogResult::Changed => {
                    dlg.first_open = false;
                    let idx = dlg.layer_idx;
                    if let (Some(original), Some(flat)) = (&dlg.original_pixels, &dlg.original_flat)
                    {
                        let (scale_p, amount, seed) = (dlg.scale, dlg.amount, dlg.seed);
                        let (octaves, roughness) = (dlg.octaves as u32, dlg.roughness);
                        let (pinch, wrap) = (dlg.pinch, dlg.wrap);
                        self.spawn_preview_job(
                            ctx.input(|i| i.time),
                            "Dents".to_string(),
                            idx,
                            original.clone(),
                            flat.clone(),
                            move |img| {
                                crate::ops::effects::dents_core(
                                    img, scale_p, amount, seed, octaves, roughness, pinch, wrap,
                                    None,
                                )
                            },
                        );
                    }
                }
                DialogResult::Ok(_) => {
                    self.preview_job_token = self.preview_job_token.wrapping_add(1);
                    let idx = dlg.layer_idx;
                    if let Some(flat) = &dlg.original_flat {
                        let (scale_p, amount, seed) = (dlg.scale, dlg.amount, dlg.seed);
                        let (octaves, roughness) = (dlg.octaves as u32, dlg.roughness);
                        let (pinch, wrap) = (dlg.pinch, dlg.wrap);
                        if let Some(project) = self.active_project_mut() {
                            Self::apply_fullres_effect(
                                &mut project.canvas_state,
                                idx,
                                flat,
                                |img| {
                                    crate::ops::effects::dents_core(
                                        img, scale_p, amount, seed, octaves, roughness, pinch,
                                        wrap, None,
                                    )
                                },
                            );
                        }
                    }
                    self.active_dialog = ActiveDialog::None;
                    if let Some(project) = self.active_project_mut() {
                        let idx = dlg.layer_idx;
                        if let Some(original) = &dlg.original_pixels
                            && idx < project.canvas_state.layers.len()
                        {
                            let adjusted = project.canvas_state.layers[idx].pixels.clone();
                            project.canvas_state.layers[idx].pixels = original.clone();
                            let mut cmd = SingleLayerSnapshotCommand::new_for_layer(
                                "Dents".to_string(),
                                &project.canvas_state,
                                idx,
                            );
                            project.canvas_state.layers[idx].pixels = adjusted;
                            cmd.set_after(&project.canvas_state);
                            project.history.push(Box::new(cmd));
                        }
                        project.mark_dirty();
                    }
                    return;
                }
                DialogResult::Cancel => {
                    self.preview_job_token = self.preview_job_token.wrapping_add(1);
                    self.filter_cancel
                        .store(true, std::sync::atomic::Ordering::Relaxed);
                    let idx = dlg.layer_idx;
                    if let Some(original) = &dlg.original_pixels
                        && let Some(project) = self.active_project_mut()
                    {
                        if let Some(layer) = project.canvas_state.layers.get_mut(idx) {
                            layer.pixels = original.clone();
                        }
                        project.canvas_state.mark_dirty(None);
                    }
                    self.active_dialog = ActiveDialog::None;
                    return;
                }
                _ => {
                    dlg.poll_flat();
                    if dlg.first_open && dlg.live_preview && dlg.original_flat.is_some() {
                        dlg.first_open = false;
                        let idx = dlg.layer_idx;
                        if let (Some(original), Some(flat)) =
                            (&dlg.original_pixels, &dlg.original_flat)
                        {
                            let (scale_p, amount, seed) = (dlg.scale, dlg.amount, dlg.seed);
                            let (octaves, roughness) = (dlg.octaves as u32, dlg.roughness);
                            let (pinch, wrap) = (dlg.pinch, dlg.wrap);
                            self.spawn_preview_job(
                                ctx.input(|i| i.time),
                                "Dents".to_string(),
                                idx,
                                original.clone(),
                                flat.clone(),
                                move |img| {
                                    crate::ops::effects::dents_core(
                                        img, scale_p, amount, seed, octaves, roughness, pinch,
                                        wrap, None,
                                    )
                                },
                            );
                        }
                    }
                }
            },

            ActiveDialog::Pixelate(dlg) => match dlg.show(ctx) {
                DialogResult::Changed => {
                    dlg.first_open = false;
                    let idx = dlg.layer_idx;
                    if let (Some(original), Some(flat)) = (&dlg.original_pixels, &dlg.original_flat)
                    {
                        let block_size = dlg.block_size as u32;
                        self.spawn_preview_job(
                            ctx.input(|i| i.time),
                            "Pixelate".to_string(),
                            idx,
                            original.clone(),
                            flat.clone(),
                            move |img| crate::ops::effects::pixelate_core(img, block_size, None),
                        );
                    }
                }
                DialogResult::Ok(_) => {
                    self.preview_job_token = self.preview_job_token.wrapping_add(1);
                    let idx = dlg.layer_idx;
                    if let Some(flat) = &dlg.original_flat {
                        let block_size = dlg.block_size as u32;
                        if let Some(project) = self.active_project_mut() {
                            Self::apply_fullres_effect(
                                &mut project.canvas_state,
                                idx,
                                flat,
                                |img| crate::ops::effects::pixelate_core(img, block_size, None),
                            );
                        }
                    }
                    self.active_dialog = ActiveDialog::None;
                    if let Some(project) = self.active_project_mut() {
                        let idx = dlg.layer_idx;
                        if let Some(original) = &dlg.original_pixels
                            && idx < project.canvas_state.layers.len()
                        {
                            let adjusted = project.canvas_state.layers[idx].pixels.clone();
                            project.canvas_state.layers[idx].pixels = original.clone();
                            let mut cmd = SingleLayerSnapshotCommand::new_for_layer(
                                "Pixelate".to_string(),
                                &project.canvas_state,
                                idx,
                            );
                            project.canvas_state.layers[idx].pixels = adjusted;
                            cmd.set_after(&project.canvas_state);
                            project.history.push(Box::new(cmd));
                        }
                        project.mark_dirty();
                    }
                    return;
                }
                DialogResult::Cancel => {
                    self.preview_job_token = self.preview_job_token.wrapping_add(1);
                    self.filter_cancel
                        .store(true, std::sync::atomic::Ordering::Relaxed);
                    let idx = dlg.layer_idx;
                    if let Some(original) = &dlg.original_pixels
                        && let Some(project) = self.active_project_mut()
                    {
                        if let Some(layer) = project.canvas_state.layers.get_mut(idx) {
                            layer.pixels = original.clone();
                        }
                        project.canvas_state.mark_dirty(None);
                    }
                    self.active_dialog = ActiveDialog::None;
                    return;
                }
                _ => {
                    dlg.poll_flat();
                    if dlg.first_open && dlg.live_preview && dlg.original_flat.is_some() {
                        dlg.first_open = false;
                        let idx = dlg.layer_idx;
                        if let (Some(original), Some(flat)) =
                            (&dlg.original_pixels, &dlg.original_flat)
                        {
                            let block_size = dlg.block_size as u32;
                            self.spawn_preview_job(
                                ctx.input(|i| i.time),
                                "Pixelate".to_string(),
                                idx,
                                original.clone(),
                                flat.clone(),
                                move |img| {
                                    crate::ops::effects::pixelate_core(img, block_size, None)
                                },
                            );
                        }
                    }
                }
            },

            ActiveDialog::Bulge(dlg) => match dlg.show(ctx) {
                DialogResult::Changed => {
                    dlg.first_open = false;
                    let idx = dlg.layer_idx;
                    if let (Some(original), Some(flat)) = (&dlg.original_pixels, &dlg.original_flat)
                    {
                        let amount = dlg.amount;
                        self.spawn_preview_job(
                            ctx.input(|i| i.time),
                            "Bulge".to_string(),
                            idx,
                            original.clone(),
                            flat.clone(),
                            move |img| crate::ops::effects::bulge_core(img, amount, None),
                        );
                    }
                }
                DialogResult::Ok(_) => {
                    self.preview_job_token = self.preview_job_token.wrapping_add(1);
                    let idx = dlg.layer_idx;
                    if let Some(flat) = &dlg.original_flat {
                        let amount = dlg.amount;
                        if let Some(project) = self.active_project_mut() {
                            Self::apply_fullres_effect(
                                &mut project.canvas_state,
                                idx,
                                flat,
                                |img| crate::ops::effects::bulge_core(img, amount, None),
                            );
                        }
                    }
                    self.active_dialog = ActiveDialog::None;
                    if let Some(project) = self.active_project_mut() {
                        let idx = dlg.layer_idx;
                        if let Some(original) = &dlg.original_pixels
                            && idx < project.canvas_state.layers.len()
                        {
                            let adjusted = project.canvas_state.layers[idx].pixels.clone();
                            project.canvas_state.layers[idx].pixels = original.clone();
                            let mut cmd = SingleLayerSnapshotCommand::new_for_layer(
                                "Bulge".to_string(),
                                &project.canvas_state,
                                idx,
                            );
                            project.canvas_state.layers[idx].pixels = adjusted;
                            cmd.set_after(&project.canvas_state);
                            project.history.push(Box::new(cmd));
                        }
                        project.mark_dirty();
                    }
                    return;
                }
                DialogResult::Cancel => {
                    self.preview_job_token = self.preview_job_token.wrapping_add(1);
                    self.filter_cancel
                        .store(true, std::sync::atomic::Ordering::Relaxed);
                    let idx = dlg.layer_idx;
                    if let Some(original) = &dlg.original_pixels
                        && let Some(project) = self.active_project_mut()
                    {
                        if let Some(layer) = project.canvas_state.layers.get_mut(idx) {
                            layer.pixels = original.clone();
                        }
                        project.canvas_state.mark_dirty(None);
                    }
                    self.active_dialog = ActiveDialog::None;
                    return;
                }
                _ => {
                    dlg.poll_flat();
                    if dlg.first_open && dlg.live_preview && dlg.original_flat.is_some() {
                        dlg.first_open = false;
                        let idx = dlg.layer_idx;
                        if let (Some(original), Some(flat)) =
                            (&dlg.original_pixels, &dlg.original_flat)
                        {
                            let amount = dlg.amount;
                            self.spawn_preview_job(
                                ctx.input(|i| i.time),
                                "Bulge".to_string(),
                                idx,
                                original.clone(),
                                flat.clone(),
                                move |img| crate::ops::effects::bulge_core(img, amount, None),
                            );
                        }
                    }
                }
            },

            ActiveDialog::Twist(dlg) => match dlg.show(ctx) {
                DialogResult::Changed => {
                    dlg.first_open = false;
                    let idx = dlg.layer_idx;
                    if let (Some(original), Some(flat)) = (&dlg.original_pixels, &dlg.original_flat)
                    {
                        let angle = dlg.angle;
                        self.spawn_preview_job(
                            ctx.input(|i| i.time),
                            "Twist".to_string(),
                            idx,
                            original.clone(),
                            flat.clone(),
                            move |img| crate::ops::effects::twist_core(img, angle, None),
                        );
                    }
                }
                DialogResult::Ok(_) => {
                    self.preview_job_token = self.preview_job_token.wrapping_add(1);
                    let idx = dlg.layer_idx;
                    if let Some(flat) = &dlg.original_flat {
                        let angle = dlg.angle;
                        if let Some(project) = self.active_project_mut() {
                            Self::apply_fullres_effect(
                                &mut project.canvas_state,
                                idx,
                                flat,
                                |img| crate::ops::effects::twist_core(img, angle, None),
                            );
                        }
                    }
                    self.active_dialog = ActiveDialog::None;
                    if let Some(project) = self.active_project_mut() {
                        let idx = dlg.layer_idx;
                        if let Some(original) = &dlg.original_pixels
                            && idx < project.canvas_state.layers.len()
                        {
                            let adjusted = project.canvas_state.layers[idx].pixels.clone();
                            project.canvas_state.layers[idx].pixels = original.clone();
                            let mut cmd = SingleLayerSnapshotCommand::new_for_layer(
                                "Twist".to_string(),
                                &project.canvas_state,
                                idx,
                            );
                            project.canvas_state.layers[idx].pixels = adjusted;
                            cmd.set_after(&project.canvas_state);
                            project.history.push(Box::new(cmd));
                        }
                        project.mark_dirty();
                    }
                    return;
                }
                DialogResult::Cancel => {
                    self.preview_job_token = self.preview_job_token.wrapping_add(1);
                    self.filter_cancel
                        .store(true, std::sync::atomic::Ordering::Relaxed);
                    let idx = dlg.layer_idx;
                    if let Some(original) = &dlg.original_pixels
                        && let Some(project) = self.active_project_mut()
                    {
                        if let Some(layer) = project.canvas_state.layers.get_mut(idx) {
                            layer.pixels = original.clone();
                        }
                        project.canvas_state.mark_dirty(None);
                    }
                    self.active_dialog = ActiveDialog::None;
                    return;
                }
                _ => {
                    dlg.poll_flat();
                    if dlg.first_open && dlg.live_preview && dlg.original_flat.is_some() {
                        dlg.first_open = false;
                        let idx = dlg.layer_idx;
                        if let (Some(original), Some(flat)) =
                            (&dlg.original_pixels, &dlg.original_flat)
                        {
                            let angle = dlg.angle;
                            self.spawn_preview_job(
                                ctx.input(|i| i.time),
                                "Twist".to_string(),
                                idx,
                                original.clone(),
                                flat.clone(),
                                move |img| crate::ops::effects::twist_core(img, angle, None),
                            );
                        }
                    }
                }
            },

            ActiveDialog::AddNoise(dlg) => match dlg.show(ctx) {
                DialogResult::Changed => {
                    dlg.first_open = false;
                    let idx = dlg.layer_idx;
                    if let (Some(original), Some(flat)) = (&dlg.original_pixels, &dlg.original_flat)
                    {
                        let amount = dlg.amount;
                        let noise_type = dlg.noise_type();
                        let monochrome = dlg.monochrome;
                        let seed = dlg.seed;
                        let noise_scale = dlg.scale;
                        let octaves = dlg.octaves as u32;
                        self.spawn_preview_job(
                            ctx.input(|i| i.time),
                            "Add Noise".to_string(),
                            idx,
                            original.clone(),
                            flat.clone(),
                            move |img| {
                                crate::ops::effects::add_noise_core(
                                    img,
                                    amount,
                                    noise_type,
                                    monochrome,
                                    seed,
                                    noise_scale,
                                    octaves,
                                    None,
                                )
                            },
                        );
                    }
                }
                DialogResult::Ok(_) => {
                    self.preview_job_token = self.preview_job_token.wrapping_add(1);
                    let idx = dlg.layer_idx;
                    if let Some(flat) = &dlg.original_flat {
                        let amount = dlg.amount;
                        let noise_type = dlg.noise_type();
                        let monochrome = dlg.monochrome;
                        let seed = dlg.seed;
                        let noise_scale = dlg.scale;
                        let octaves = dlg.octaves as u32;
                        if let Some(project) = self.active_project_mut() {
                            Self::apply_fullres_effect(
                                &mut project.canvas_state,
                                idx,
                                flat,
                                |img| {
                                    crate::ops::effects::add_noise_core(
                                        img,
                                        amount,
                                        noise_type,
                                        monochrome,
                                        seed,
                                        noise_scale,
                                        octaves,
                                        None,
                                    )
                                },
                            );
                        }
                    }
                    self.active_dialog = ActiveDialog::None;
                    if let Some(project) = self.active_project_mut() {
                        let idx = dlg.layer_idx;
                        if let Some(original) = &dlg.original_pixels
                            && idx < project.canvas_state.layers.len()
                        {
                            let adjusted = project.canvas_state.layers[idx].pixels.clone();
                            project.canvas_state.layers[idx].pixels = original.clone();
                            let mut cmd = SingleLayerSnapshotCommand::new_for_layer(
                                "Add Noise".to_string(),
                                &project.canvas_state,
                                idx,
                            );
                            project.canvas_state.layers[idx].pixels = adjusted;
                            cmd.set_after(&project.canvas_state);
                            project.history.push(Box::new(cmd));
                        }
                        project.mark_dirty();
                    }
                    return;
                }
                DialogResult::Cancel => {
                    self.preview_job_token = self.preview_job_token.wrapping_add(1);
                    self.filter_cancel
                        .store(true, std::sync::atomic::Ordering::Relaxed);
                    let idx = dlg.layer_idx;
                    if let Some(original) = &dlg.original_pixels
                        && let Some(project) = self.active_project_mut()
                    {
                        if let Some(layer) = project.canvas_state.layers.get_mut(idx) {
                            layer.pixels = original.clone();
                        }
                        project.canvas_state.mark_dirty(None);
                    }
                    self.active_dialog = ActiveDialog::None;
                    return;
                }
                _ => {
                    dlg.poll_flat();
                    if dlg.first_open && dlg.live_preview && dlg.original_flat.is_some() {
                        dlg.first_open = false;
                        let idx = dlg.layer_idx;
                        if let (Some(original), Some(flat)) =
                            (&dlg.original_pixels, &dlg.original_flat)
                        {
                            let amount = dlg.amount;
                            let noise_type = dlg.noise_type();
                            let monochrome = dlg.monochrome;
                            let seed = dlg.seed;
                            let noise_scale = dlg.scale;
                            let octaves = dlg.octaves as u32;
                            self.spawn_preview_job(
                                ctx.input(|i| i.time),
                                "Add Noise".to_string(),
                                idx,
                                original.clone(),
                                flat.clone(),
                                move |img| {
                                    crate::ops::effects::add_noise_core(
                                        img,
                                        amount,
                                        noise_type,
                                        monochrome,
                                        seed,
                                        noise_scale,
                                        octaves,
                                        None,
                                    )
                                },
                            );
                        }
                    }
                }
            },

            ActiveDialog::ReduceNoise(dlg) => match dlg.show(ctx) {
                DialogResult::Changed => {
                    dlg.first_open = false;
                    let idx = dlg.layer_idx;
                    if let (Some(original), Some(flat)) = (&dlg.original_pixels, &dlg.original_flat)
                    {
                        let (strength, radius) = (dlg.strength, dlg.radius as u32);
                        self.spawn_preview_job(
                            ctx.input(|i| i.time),
                            "Reduce Noise".to_string(),
                            idx,
                            original.clone(),
                            flat.clone(),
                            move |img| {
                                crate::ops::effects::reduce_noise_core(img, strength, radius, None)
                            },
                        );
                    }
                }
                DialogResult::Ok(_) => {
                    self.preview_job_token = self.preview_job_token.wrapping_add(1);
                    let idx = dlg.layer_idx;
                    if let Some(flat) = &dlg.original_flat {
                        let strength = dlg.strength;
                        let radius = dlg.radius as u32;
                        if let Some(project) = self.active_project_mut() {
                            Self::apply_fullres_effect(
                                &mut project.canvas_state,
                                idx,
                                flat,
                                |img| {
                                    crate::ops::effects::reduce_noise_core(
                                        img, strength, radius, None,
                                    )
                                },
                            );
                        }
                    }
                    self.active_dialog = ActiveDialog::None;
                    if let Some(project) = self.active_project_mut() {
                        let idx = dlg.layer_idx;
                        if let Some(original) = &dlg.original_pixels
                            && idx < project.canvas_state.layers.len()
                        {
                            let adjusted = project.canvas_state.layers[idx].pixels.clone();
                            project.canvas_state.layers[idx].pixels = original.clone();
                            let mut cmd = SingleLayerSnapshotCommand::new_for_layer(
                                "Reduce Noise".to_string(),
                                &project.canvas_state,
                                idx,
                            );
                            project.canvas_state.layers[idx].pixels = adjusted;
                            cmd.set_after(&project.canvas_state);
                            project.history.push(Box::new(cmd));
                        }
                        project.mark_dirty();
                    }
                    return;
                }
                DialogResult::Cancel => {
                    self.preview_job_token = self.preview_job_token.wrapping_add(1);
                    self.filter_cancel
                        .store(true, std::sync::atomic::Ordering::Relaxed);
                    let idx = dlg.layer_idx;
                    if let Some(original) = &dlg.original_pixels
                        && let Some(project) = self.active_project_mut()
                    {
                        if let Some(layer) = project.canvas_state.layers.get_mut(idx) {
                            layer.pixels = original.clone();
                        }
                        project.canvas_state.mark_dirty(None);
                    }
                    self.active_dialog = ActiveDialog::None;
                    return;
                }
                _ => {
                    dlg.poll_flat();
                    if dlg.first_open && dlg.live_preview && dlg.original_flat.is_some() {
                        dlg.first_open = false;
                        let idx = dlg.layer_idx;
                        if let (Some(original), Some(flat)) =
                            (&dlg.original_pixels, &dlg.original_flat)
                        {
                            let (strength, radius) = (dlg.strength, dlg.radius as u32);
                            self.spawn_preview_job(
                                ctx.input(|i| i.time),
                                "Reduce Noise".to_string(),
                                idx,
                                original.clone(),
                                flat.clone(),
                                move |img| {
                                    crate::ops::effects::reduce_noise_core(
                                        img, strength, radius, None,
                                    )
                                },
                            );
                        }
                    }
                }
            },

            ActiveDialog::Median(dlg) => {
                match dlg.show(ctx) {
                    DialogResult::Changed => {
                        // Spawn async preview job (token-tracked, interrupts stale previews)
                        let idx = dlg.layer_idx;
                        if let (Some(original), Some(flat)) =
                            (&dlg.original_pixels, &dlg.original_flat)
                        {
                            let radius = dlg.radius as u32;
                            self.spawn_preview_job(
                                ctx.input(|i| i.time),
                                "Median Filter".to_string(),
                                idx,
                                original.clone(),
                                flat.clone(),
                                move |img| crate::ops::effects::median_core(img, radius, None),
                            );
                        }
                    }
                    DialogResult::Ok(_) => {
                        self.preview_job_token = self.preview_job_token.wrapping_add(1);
                        // Use GPU-accelerated median at full resolution
                        let idx = dlg.layer_idx;
                        let radius = dlg.radius as u32;
                        if let Some(flat) = &dlg.original_flat {
                            let active_idx = self.active_project_index;
                            if let Some(project) = self.projects.get_mut(active_idx) {
                                crate::ops::effects::median_filter_from_flat_gpu(
                                    &mut project.canvas_state,
                                    idx,
                                    radius,
                                    flat,
                                    &self.canvas.gpu_renderer,
                                );
                            }
                        }
                        self.active_dialog = ActiveDialog::None;
                        if let Some(project) = self.active_project_mut() {
                            let idx = dlg.layer_idx;
                            if let Some(original) = &dlg.original_pixels
                                && idx < project.canvas_state.layers.len()
                            {
                                let adjusted = project.canvas_state.layers[idx].pixels.clone();
                                project.canvas_state.layers[idx].pixels = original.clone();
                                let mut cmd = SingleLayerSnapshotCommand::new_for_layer(
                                    "Median Filter".to_string(),
                                    &project.canvas_state,
                                    idx,
                                );
                                project.canvas_state.layers[idx].pixels = adjusted;
                                cmd.set_after(&project.canvas_state);
                                project.history.push(Box::new(cmd));
                            }
                            project.mark_dirty();
                        }
                        return;
                    }
                    DialogResult::Cancel => {
                        self.preview_job_token = self.preview_job_token.wrapping_add(1);
                        self.filter_cancel
                            .store(true, std::sync::atomic::Ordering::Relaxed);
                        let idx = dlg.layer_idx;
                        if let Some(original) = &dlg.original_pixels
                            && let Some(project) = self.active_project_mut()
                        {
                            if let Some(layer) = project.canvas_state.layers.get_mut(idx) {
                                layer.pixels = original.clone();
                            }
                            project.canvas_state.mark_dirty(None);
                        }
                        self.active_dialog = ActiveDialog::None;
                        return;
                    }
                    _ => {}
                }
            }

            ActiveDialog::Glow(dlg) => match dlg.show(ctx) {
                DialogResult::Changed => {
                    dlg.first_open = false;
                    let idx = dlg.layer_idx;
                    if let (Some(original), Some(flat)) = (&dlg.original_pixels, &dlg.original_flat)
                    {
                        let (radius, intensity) = (dlg.radius, dlg.intensity);
                        self.spawn_preview_job(
                            ctx.input(|i| i.time),
                            "Glow".to_string(),
                            idx,
                            original.clone(),
                            flat.clone(),
                            move |img| crate::ops::effects::glow_core(img, radius, intensity, None),
                        );
                    }
                }
                DialogResult::Ok(_) => {
                    self.preview_job_token = self.preview_job_token.wrapping_add(1);
                    let idx = dlg.layer_idx;
                    if let Some(flat) = &dlg.original_flat {
                        let (radius, intensity) = (dlg.radius, dlg.intensity);
                        if let Some(project) = self.active_project_mut() {
                            Self::apply_fullres_effect(
                                &mut project.canvas_state,
                                idx,
                                flat,
                                |img| crate::ops::effects::glow_core(img, radius, intensity, None),
                            );
                        }
                    }
                    self.active_dialog = ActiveDialog::None;
                    if let Some(project) = self.active_project_mut() {
                        let idx = dlg.layer_idx;
                        if let Some(original) = &dlg.original_pixels
                            && idx < project.canvas_state.layers.len()
                        {
                            let adjusted = project.canvas_state.layers[idx].pixels.clone();
                            project.canvas_state.layers[idx].pixels = original.clone();
                            let mut cmd = SingleLayerSnapshotCommand::new_for_layer(
                                "Glow".to_string(),
                                &project.canvas_state,
                                idx,
                            );
                            project.canvas_state.layers[idx].pixels = adjusted;
                            cmd.set_after(&project.canvas_state);
                            project.history.push(Box::new(cmd));
                        }
                        project.mark_dirty();
                    }
                    return;
                }
                DialogResult::Cancel => {
                    self.preview_job_token = self.preview_job_token.wrapping_add(1);
                    self.filter_cancel
                        .store(true, std::sync::atomic::Ordering::Relaxed);
                    let idx = dlg.layer_idx;
                    if let Some(original) = &dlg.original_pixels
                        && let Some(project) = self.active_project_mut()
                    {
                        if let Some(layer) = project.canvas_state.layers.get_mut(idx) {
                            layer.pixels = original.clone();
                        }
                        project.canvas_state.mark_dirty(None);
                    }
                    self.active_dialog = ActiveDialog::None;
                    return;
                }
                _ => {
                    dlg.poll_flat();
                    if dlg.first_open && dlg.live_preview && dlg.original_flat.is_some() {
                        dlg.first_open = false;
                        let idx = dlg.layer_idx;
                        if let (Some(original), Some(flat)) =
                            (&dlg.original_pixels, &dlg.original_flat)
                        {
                            let (radius, intensity) = (dlg.radius, dlg.intensity);
                            self.spawn_preview_job(
                                ctx.input(|i| i.time),
                                "Glow".to_string(),
                                idx,
                                original.clone(),
                                flat.clone(),
                                move |img| {
                                    crate::ops::effects::glow_core(img, radius, intensity, None)
                                },
                            );
                        }
                    }
                }
            },

            ActiveDialog::Sharpen(dlg) => match dlg.show(ctx) {
                DialogResult::Changed => {
                    dlg.first_open = false;
                    let idx = dlg.layer_idx;
                    if let (Some(original), Some(flat)) = (&dlg.original_pixels, &dlg.original_flat)
                    {
                        let (amount, radius) = (dlg.amount, dlg.radius);
                        self.spawn_preview_job(
                            ctx.input(|i| i.time),
                            "Sharpen".to_string(),
                            idx,
                            original.clone(),
                            flat.clone(),
                            move |img| crate::ops::effects::sharpen_core(img, amount, radius, None),
                        );
                    }
                }
                DialogResult::Ok(_) => {
                    self.preview_job_token = self.preview_job_token.wrapping_add(1);
                    let idx = dlg.layer_idx;
                    if let Some(flat) = &dlg.original_flat {
                        let (amount, radius) = (dlg.amount, dlg.radius);
                        if let Some(project) = self.active_project_mut() {
                            Self::apply_fullres_effect(
                                &mut project.canvas_state,
                                idx,
                                flat,
                                |img| crate::ops::effects::sharpen_core(img, amount, radius, None),
                            );
                        }
                    }
                    self.active_dialog = ActiveDialog::None;
                    if let Some(project) = self.active_project_mut() {
                        let idx = dlg.layer_idx;
                        if let Some(original) = &dlg.original_pixels
                            && idx < project.canvas_state.layers.len()
                        {
                            let adjusted = project.canvas_state.layers[idx].pixels.clone();
                            project.canvas_state.layers[idx].pixels = original.clone();
                            let mut cmd = SingleLayerSnapshotCommand::new_for_layer(
                                "Sharpen".to_string(),
                                &project.canvas_state,
                                idx,
                            );
                            project.canvas_state.layers[idx].pixels = adjusted;
                            cmd.set_after(&project.canvas_state);
                            project.history.push(Box::new(cmd));
                        }
                        project.mark_dirty();
                    }
                    return;
                }
                DialogResult::Cancel => {
                    self.preview_job_token = self.preview_job_token.wrapping_add(1);
                    self.filter_cancel
                        .store(true, std::sync::atomic::Ordering::Relaxed);
                    let idx = dlg.layer_idx;
                    if let Some(original) = &dlg.original_pixels
                        && let Some(project) = self.active_project_mut()
                    {
                        if let Some(layer) = project.canvas_state.layers.get_mut(idx) {
                            layer.pixels = original.clone();
                        }
                        project.canvas_state.mark_dirty(None);
                    }
                    self.active_dialog = ActiveDialog::None;
                    return;
                }
                _ => {
                    dlg.poll_flat();
                    if dlg.first_open && dlg.live_preview && dlg.original_flat.is_some() {
                        dlg.first_open = false;
                        let idx = dlg.layer_idx;
                        if let (Some(original), Some(flat)) =
                            (&dlg.original_pixels, &dlg.original_flat)
                        {
                            let (amount, radius) = (dlg.amount, dlg.radius);
                            self.spawn_preview_job(
                                ctx.input(|i| i.time),
                                "Sharpen".to_string(),
                                idx,
                                original.clone(),
                                flat.clone(),
                                move |img| {
                                    crate::ops::effects::sharpen_core(img, amount, radius, None)
                                },
                            );
                        }
                    }
                }
            },

            ActiveDialog::Vignette(dlg) => match dlg.show(ctx) {
                DialogResult::Changed => {
                    dlg.first_open = false;
                    let idx = dlg.layer_idx;
                    if let (Some(original), Some(flat)) = (&dlg.original_pixels, &dlg.original_flat)
                    {
                        let (amount, softness) = (dlg.amount, dlg.softness);
                        self.spawn_preview_job(
                            ctx.input(|i| i.time),
                            "Vignette".to_string(),
                            idx,
                            original.clone(),
                            flat.clone(),
                            move |img| {
                                crate::ops::effects::vignette_core(img, amount, softness, None)
                            },
                        );
                    }
                }
                DialogResult::Ok(_) => {
                    self.preview_job_token = self.preview_job_token.wrapping_add(1);
                    let idx = dlg.layer_idx;
                    if let Some(flat) = &dlg.original_flat {
                        let (amount, softness) = (dlg.amount, dlg.softness);
                        if let Some(project) = self.active_project_mut() {
                            Self::apply_fullres_effect(
                                &mut project.canvas_state,
                                idx,
                                flat,
                                |img| {
                                    crate::ops::effects::vignette_core(img, amount, softness, None)
                                },
                            );
                        }
                    }
                    self.active_dialog = ActiveDialog::None;
                    if let Some(project) = self.active_project_mut() {
                        let idx = dlg.layer_idx;
                        if let Some(original) = &dlg.original_pixels
                            && idx < project.canvas_state.layers.len()
                        {
                            let adjusted = project.canvas_state.layers[idx].pixels.clone();
                            project.canvas_state.layers[idx].pixels = original.clone();
                            let mut cmd = SingleLayerSnapshotCommand::new_for_layer(
                                "Vignette".to_string(),
                                &project.canvas_state,
                                idx,
                            );
                            project.canvas_state.layers[idx].pixels = adjusted;
                            cmd.set_after(&project.canvas_state);
                            project.history.push(Box::new(cmd));
                        }
                        project.mark_dirty();
                    }
                    return;
                }
                DialogResult::Cancel => {
                    self.preview_job_token = self.preview_job_token.wrapping_add(1);
                    self.filter_cancel
                        .store(true, std::sync::atomic::Ordering::Relaxed);
                    let idx = dlg.layer_idx;
                    if let Some(original) = &dlg.original_pixels
                        && let Some(project) = self.active_project_mut()
                    {
                        if let Some(layer) = project.canvas_state.layers.get_mut(idx) {
                            layer.pixels = original.clone();
                        }
                        project.canvas_state.mark_dirty(None);
                    }
                    self.active_dialog = ActiveDialog::None;
                    return;
                }
                _ => {
                    dlg.poll_flat();
                    if dlg.first_open && dlg.live_preview && dlg.original_flat.is_some() {
                        dlg.first_open = false;
                        let idx = dlg.layer_idx;
                        if let (Some(original), Some(flat)) =
                            (&dlg.original_pixels, &dlg.original_flat)
                        {
                            let (amount, softness) = (dlg.amount, dlg.softness);
                            self.spawn_preview_job(
                                ctx.input(|i| i.time),
                                "Vignette".to_string(),
                                idx,
                                original.clone(),
                                flat.clone(),
                                move |img| {
                                    crate::ops::effects::vignette_core(img, amount, softness, None)
                                },
                            );
                        }
                    }
                }
            },

            ActiveDialog::Halftone(dlg) => match dlg.show(ctx) {
                DialogResult::Changed => {
                    dlg.first_open = false;
                    let idx = dlg.layer_idx;
                    if let (Some(original), Some(flat)) = (&dlg.original_pixels, &dlg.original_flat)
                    {
                        let (dot_size, angle) = (dlg.dot_size, dlg.angle);
                        let shape = dlg.halftone_shape();
                        self.spawn_preview_job(
                            ctx.input(|i| i.time),
                            "Halftone".to_string(),
                            idx,
                            original.clone(),
                            flat.clone(),
                            move |img| {
                                crate::ops::effects::halftone_core(
                                    img, dot_size, angle, shape, None,
                                )
                            },
                        );
                    }
                }
                DialogResult::Ok(_) => {
                    self.preview_job_token = self.preview_job_token.wrapping_add(1);
                    let idx = dlg.layer_idx;
                    if let Some(flat) = &dlg.original_flat {
                        let (dot_size, angle) = (dlg.dot_size, dlg.angle);
                        let shape = dlg.halftone_shape();
                        if let Some(project) = self.active_project_mut() {
                            Self::apply_fullres_effect(
                                &mut project.canvas_state,
                                idx,
                                flat,
                                |img| {
                                    crate::ops::effects::halftone_core(
                                        img, dot_size, angle, shape, None,
                                    )
                                },
                            );
                        }
                    }
                    self.active_dialog = ActiveDialog::None;
                    if let Some(project) = self.active_project_mut() {
                        let idx = dlg.layer_idx;
                        if let Some(original) = &dlg.original_pixels
                            && idx < project.canvas_state.layers.len()
                        {
                            let adjusted = project.canvas_state.layers[idx].pixels.clone();
                            project.canvas_state.layers[idx].pixels = original.clone();
                            let mut cmd = SingleLayerSnapshotCommand::new_for_layer(
                                "Halftone".to_string(),
                                &project.canvas_state,
                                idx,
                            );
                            project.canvas_state.layers[idx].pixels = adjusted;
                            cmd.set_after(&project.canvas_state);
                            project.history.push(Box::new(cmd));
                        }
                        project.mark_dirty();
                    }
                    return;
                }
                DialogResult::Cancel => {
                    self.preview_job_token = self.preview_job_token.wrapping_add(1);
                    self.filter_cancel
                        .store(true, std::sync::atomic::Ordering::Relaxed);
                    let idx = dlg.layer_idx;
                    if let Some(original) = &dlg.original_pixels
                        && let Some(project) = self.active_project_mut()
                    {
                        if let Some(layer) = project.canvas_state.layers.get_mut(idx) {
                            layer.pixels = original.clone();
                        }
                        project.canvas_state.mark_dirty(None);
                    }
                    self.active_dialog = ActiveDialog::None;
                    return;
                }
                _ => {
                    dlg.poll_flat();
                    if dlg.first_open && dlg.live_preview && dlg.original_flat.is_some() {
                        dlg.first_open = false;
                        let idx = dlg.layer_idx;
                        if let (Some(original), Some(flat)) =
                            (&dlg.original_pixels, &dlg.original_flat)
                        {
                            let (dot_size, angle) = (dlg.dot_size, dlg.angle);
                            let shape = dlg.halftone_shape();
                            self.spawn_preview_job(
                                ctx.input(|i| i.time),
                                "Halftone".to_string(),
                                idx,
                                original.clone(),
                                flat.clone(),
                                move |img| {
                                    crate::ops::effects::halftone_core(
                                        img, dot_size, angle, shape, None,
                                    )
                                },
                            );
                        }
                    }
                }
            },

            ActiveDialog::Grid(dlg) => match dlg.show(ctx) {
                DialogResult::Changed => {
                    dlg.first_open = false;
                    let idx = dlg.layer_idx;
                    if let (Some(original), Some(flat)) = (&dlg.original_pixels, &dlg.original_flat)
                    {
                        let cw = dlg.cell_w as u32;
                        let ch = dlg.cell_h as u32;
                        let lw = dlg.line_width as u32;
                        let c = [
                            (dlg.color[0] * 255.0) as u8,
                            (dlg.color[1] * 255.0) as u8,
                            (dlg.color[2] * 255.0) as u8,
                            255,
                        ];
                        let style = dlg.grid_style();
                        let opacity = dlg.opacity;
                        self.spawn_preview_job(
                            ctx.input(|i| i.time),
                            "Grid".to_string(),
                            idx,
                            original.clone(),
                            flat.clone(),
                            move |img| {
                                crate::ops::effects::grid_core(
                                    img, cw, ch, lw, c, style, opacity, None,
                                )
                            },
                        );
                    }
                }
                DialogResult::Ok(_) => {
                    self.preview_job_token = self.preview_job_token.wrapping_add(1);
                    let idx = dlg.layer_idx;
                    if let Some(flat) = &dlg.original_flat {
                        let cw = dlg.cell_w as u32;
                        let ch = dlg.cell_h as u32;
                        let lw = dlg.line_width as u32;
                        let c = [
                            (dlg.color[0] * 255.0) as u8,
                            (dlg.color[1] * 255.0) as u8,
                            (dlg.color[2] * 255.0) as u8,
                            255,
                        ];
                        let style = dlg.grid_style();
                        let opacity = dlg.opacity;
                        if let Some(project) = self.active_project_mut() {
                            Self::apply_fullres_effect(
                                &mut project.canvas_state,
                                idx,
                                flat,
                                |img| {
                                    crate::ops::effects::grid_core(
                                        img, cw, ch, lw, c, style, opacity, None,
                                    )
                                },
                            );
                        }
                    }
                    self.active_dialog = ActiveDialog::None;
                    if let Some(project) = self.active_project_mut() {
                        let idx = dlg.layer_idx;
                        if let Some(original) = &dlg.original_pixels
                            && idx < project.canvas_state.layers.len()
                        {
                            let adjusted = project.canvas_state.layers[idx].pixels.clone();
                            project.canvas_state.layers[idx].pixels = original.clone();
                            let mut cmd = SingleLayerSnapshotCommand::new_for_layer(
                                "Grid".to_string(),
                                &project.canvas_state,
                                idx,
                            );
                            project.canvas_state.layers[idx].pixels = adjusted;
                            cmd.set_after(&project.canvas_state);
                            project.history.push(Box::new(cmd));
                        }
                        project.mark_dirty();
                    }
                    return;
                }
                DialogResult::Cancel => {
                    self.preview_job_token = self.preview_job_token.wrapping_add(1);
                    self.filter_cancel
                        .store(true, std::sync::atomic::Ordering::Relaxed);
                    let idx = dlg.layer_idx;
                    if let Some(original) = &dlg.original_pixels
                        && let Some(project) = self.active_project_mut()
                    {
                        if let Some(layer) = project.canvas_state.layers.get_mut(idx) {
                            layer.pixels = original.clone();
                        }
                        project.canvas_state.mark_dirty(None);
                    }
                    self.active_dialog = ActiveDialog::None;
                    return;
                }
                _ => {
                    dlg.poll_flat();
                    if dlg.first_open && dlg.live_preview && dlg.original_flat.is_some() {
                        dlg.first_open = false;
                        let idx = dlg.layer_idx;
                        if let (Some(original), Some(flat)) =
                            (&dlg.original_pixels, &dlg.original_flat)
                        {
                            let cw = dlg.cell_w as u32;
                            let ch = dlg.cell_h as u32;
                            let lw = dlg.line_width as u32;
                            let c = [
                                (dlg.color[0] * 255.0) as u8,
                                (dlg.color[1] * 255.0) as u8,
                                (dlg.color[2] * 255.0) as u8,
                                255,
                            ];
                            let style = dlg.grid_style();
                            let opacity = dlg.opacity;
                            self.spawn_preview_job(
                                ctx.input(|i| i.time),
                                "Grid".to_string(),
                                idx,
                                original.clone(),
                                flat.clone(),
                                move |img| {
                                    crate::ops::effects::grid_core(
                                        img, cw, ch, lw, c, style, opacity, None,
                                    )
                                },
                            );
                        }
                    }
                }
            },

            ActiveDialog::DropShadow(dlg) => match dlg.show(ctx) {
                DialogResult::Changed => {
                    dlg.first_open = false;
                    let idx = dlg.layer_idx;
                    if let (Some(original), Some(flat)) = (&dlg.original_pixels, &dlg.original_flat)
                    {
                        let ox = dlg.offset_x as i32;
                        let oy = dlg.offset_y as i32;
                        let br = dlg.blur_radius;
                        let widen = dlg.widen_radius;
                        let c = [
                            (dlg.color[0] * 255.0) as u8,
                            (dlg.color[1] * 255.0) as u8,
                            (dlg.color[2] * 255.0) as u8,
                            255,
                        ];
                        let opacity = dlg.opacity;
                        self.spawn_preview_job(
                            ctx.input(|i| i.time),
                            "Drop Shadow".to_string(),
                            idx,
                            original.clone(),
                            flat.clone(),
                            move |img| {
                                crate::ops::effects::shadow_core(
                                    img, ox, oy, br, widen, c, opacity, None,
                                )
                            },
                        );
                    }
                }
                DialogResult::Ok(_) => {
                    self.preview_job_token = self.preview_job_token.wrapping_add(1);
                    let idx = dlg.layer_idx;
                    if let Some(flat) = &dlg.original_flat {
                        let ox = dlg.offset_x as i32;
                        let oy = dlg.offset_y as i32;
                        let br = dlg.blur_radius;
                        let widen = dlg.widen_radius;
                        let c = [
                            (dlg.color[0] * 255.0) as u8,
                            (dlg.color[1] * 255.0) as u8,
                            (dlg.color[2] * 255.0) as u8,
                            255,
                        ];
                        let opacity = dlg.opacity;
                        if let Some(project) = self.active_project_mut() {
                            Self::apply_fullres_effect(
                                &mut project.canvas_state,
                                idx,
                                flat,
                                |img| {
                                    crate::ops::effects::shadow_core(
                                        img, ox, oy, br, widen, c, opacity, None,
                                    )
                                },
                            );
                        }
                    }
                    self.active_dialog = ActiveDialog::None;
                    if let Some(project) = self.active_project_mut() {
                        let idx = dlg.layer_idx;
                        if let Some(original) = &dlg.original_pixels
                            && idx < project.canvas_state.layers.len()
                        {
                            let adjusted = project.canvas_state.layers[idx].pixels.clone();
                            project.canvas_state.layers[idx].pixels = original.clone();
                            let mut cmd = SingleLayerSnapshotCommand::new_for_layer(
                                "Drop Shadow".to_string(),
                                &project.canvas_state,
                                idx,
                            );
                            project.canvas_state.layers[idx].pixels = adjusted;
                            cmd.set_after(&project.canvas_state);
                            project.history.push(Box::new(cmd));
                        }
                        project.mark_dirty();
                    }
                    return;
                }
                DialogResult::Cancel => {
                    self.preview_job_token = self.preview_job_token.wrapping_add(1);
                    self.filter_cancel
                        .store(true, std::sync::atomic::Ordering::Relaxed);
                    let idx = dlg.layer_idx;
                    if let Some(original) = &dlg.original_pixels
                        && let Some(project) = self.active_project_mut()
                    {
                        if let Some(layer) = project.canvas_state.layers.get_mut(idx) {
                            layer.pixels = original.clone();
                        }
                        project.canvas_state.mark_dirty(None);
                    }
                    self.active_dialog = ActiveDialog::None;
                    return;
                }
                _ => {
                    dlg.poll_flat();
                    if dlg.first_open && dlg.live_preview && dlg.original_flat.is_some() {
                        dlg.first_open = false;
                        let idx = dlg.layer_idx;
                        if let (Some(original), Some(flat)) =
                            (&dlg.original_pixels, &dlg.original_flat)
                        {
                            let ox = dlg.offset_x as i32;
                            let oy = dlg.offset_y as i32;
                            let br = dlg.blur_radius;
                            let widen = dlg.widen_radius;
                            let c = [
                                (dlg.color[0] * 255.0) as u8,
                                (dlg.color[1] * 255.0) as u8,
                                (dlg.color[2] * 255.0) as u8,
                                255,
                            ];
                            let opacity = dlg.opacity;
                            self.spawn_preview_job(
                                ctx.input(|i| i.time),
                                "Drop Shadow".to_string(),
                                idx,
                                original.clone(),
                                flat.clone(),
                                move |img| {
                                    crate::ops::effects::shadow_core(
                                        img, ox, oy, br, widen, c, opacity, None,
                                    )
                                },
                            );
                        }
                    }
                }
            },

            ActiveDialog::Outline(dlg) => match dlg.show(ctx) {
                DialogResult::Changed => {
                    dlg.first_open = false;
                    let idx = dlg.layer_idx;
                    if let (Some(original), Some(flat)) = (&dlg.original_pixels, &dlg.original_flat)
                    {
                        let width = dlg.width as u32;
                        let c = [
                            (dlg.color[0] * 255.0) as u8,
                            (dlg.color[1] * 255.0) as u8,
                            (dlg.color[2] * 255.0) as u8,
                            255,
                        ];
                        let mode = dlg.outline_mode();
                        let anti_alias = dlg.anti_alias;
                        self.spawn_preview_job(
                            ctx.input(|i| i.time),
                            "Outline".to_string(),
                            idx,
                            original.clone(),
                            flat.clone(),
                            move |img| {
                                crate::ops::effects::outline_core(
                                    img, width, c, mode, anti_alias, None,
                                )
                            },
                        );
                    }
                }
                DialogResult::Ok(_) => {
                    self.preview_job_token = self.preview_job_token.wrapping_add(1);
                    self.filter_cancel
                        .store(true, std::sync::atomic::Ordering::Relaxed);
                    let idx = dlg.layer_idx;
                    if let (Some(original), Some(flat)) = (&dlg.original_pixels, &dlg.original_flat)
                    {
                        let width = dlg.width as u32;
                        let c = [
                            (dlg.color[0] * 255.0) as u8,
                            (dlg.color[1] * 255.0) as u8,
                            (dlg.color[2] * 255.0) as u8,
                            255,
                        ];
                        let mode = dlg.outline_mode();
                        let anti_alias = dlg.anti_alias;
                        self.spawn_filter_job(
                            ctx.input(|i| i.time),
                            "Outline".to_string(),
                            idx,
                            original.clone(),
                            flat.clone(),
                            move |img| {
                                crate::ops::effects::outline_core(
                                    img, width, c, mode, anti_alias, None,
                                )
                            },
                        );
                    }
                    self.active_dialog = ActiveDialog::None;
                    return;
                }
                DialogResult::Cancel => {
                    self.preview_job_token = self.preview_job_token.wrapping_add(1);
                    self.filter_cancel
                        .store(true, std::sync::atomic::Ordering::Relaxed);
                    let idx = dlg.layer_idx;
                    if let Some(original) = &dlg.original_pixels
                        && let Some(project) = self.active_project_mut()
                    {
                        if let Some(layer) = project.canvas_state.layers.get_mut(idx) {
                            layer.pixels = original.clone();
                        }
                        project.canvas_state.mark_dirty(None);
                    }
                    self.active_dialog = ActiveDialog::None;
                    return;
                }
                _ => {
                    dlg.poll_flat();
                    if dlg.first_open && dlg.live_preview && dlg.original_flat.is_some() {
                        dlg.first_open = false;
                        let idx = dlg.layer_idx;
                        if let (Some(original), Some(flat)) =
                            (&dlg.original_pixels, &dlg.original_flat)
                        {
                            let width = dlg.width as u32;
                            let c = [
                                (dlg.color[0] * 255.0) as u8,
                                (dlg.color[1] * 255.0) as u8,
                                (dlg.color[2] * 255.0) as u8,
                                255,
                            ];
                            let mode = dlg.outline_mode();
                            let anti_alias = dlg.anti_alias;
                            self.spawn_preview_job(
                                ctx.input(|i| i.time),
                                "Outline".to_string(),
                                idx,
                                original.clone(),
                                flat.clone(),
                                move |img| {
                                    crate::ops::effects::outline_core(
                                        img, width, c, mode, anti_alias, None,
                                    )
                                },
                            );
                        }
                    }
                }
            },

            ActiveDialog::CanvasBorder(dlg) => match dlg.show(ctx) {
                DialogResult::Changed => {
                    dlg.first_open = false;
                    let idx = dlg.layer_idx;
                    if let (Some(original), Some(flat)) = (&dlg.original_pixels, &dlg.original_flat)
                    {
                        let width = dlg.width as u32;
                        let primary = self.colors_panel.get_primary_color_f32();
                        let c = [
                            (primary[0] * 255.0) as u8,
                            (primary[1] * 255.0) as u8,
                            (primary[2] * 255.0) as u8,
                            255,
                        ];
                        self.spawn_preview_job(
                            ctx.input(|i| i.time),
                            "Canvas Border".to_string(),
                            idx,
                            original.clone(),
                            flat.clone(),
                            move |img| crate::ops::effects::canvas_border_core(img, width, c, None),
                        );
                    }
                }
                DialogResult::Ok(_) => {
                    self.preview_job_token = self.preview_job_token.wrapping_add(1);
                    let idx = dlg.layer_idx;
                    if let Some(flat) = &dlg.original_flat {
                        let width = dlg.width as u32;
                        let primary = self.colors_panel.get_primary_color_f32();
                        let c = [
                            (primary[0] * 255.0) as u8,
                            (primary[1] * 255.0) as u8,
                            (primary[2] * 255.0) as u8,
                            255,
                        ];
                        if let Some(project) = self.active_project_mut() {
                            Self::apply_fullres_effect(
                                &mut project.canvas_state,
                                idx,
                                flat,
                                |img| crate::ops::effects::canvas_border_core(img, width, c, None),
                            );
                        }
                    }
                    self.active_dialog = ActiveDialog::None;
                    if let Some(project) = self.active_project_mut() {
                        let idx = dlg.layer_idx;
                        if let Some(original) = &dlg.original_pixels
                            && idx < project.canvas_state.layers.len()
                        {
                            let adjusted = project.canvas_state.layers[idx].pixels.clone();
                            project.canvas_state.layers[idx].pixels = original.clone();
                            let mut cmd = SingleLayerSnapshotCommand::new_for_layer(
                                "Canvas Border".to_string(),
                                &project.canvas_state,
                                idx,
                            );
                            project.canvas_state.layers[idx].pixels = adjusted;
                            cmd.set_after(&project.canvas_state);
                            project.history.push(Box::new(cmd));
                        }
                        project.mark_dirty();
                    }
                    return;
                }
                DialogResult::Cancel => {
                    self.preview_job_token = self.preview_job_token.wrapping_add(1);
                    self.filter_cancel
                        .store(true, std::sync::atomic::Ordering::Relaxed);
                    let idx = dlg.layer_idx;
                    if let Some(original) = &dlg.original_pixels
                        && let Some(project) = self.active_project_mut()
                    {
                        if let Some(layer) = project.canvas_state.layers.get_mut(idx) {
                            layer.pixels = original.clone();
                        }
                        project.canvas_state.mark_dirty(None);
                    }
                    self.active_dialog = ActiveDialog::None;
                    return;
                }
                _ => {
                    dlg.poll_flat();
                    if dlg.first_open && dlg.live_preview && dlg.original_flat.is_some() {
                        dlg.first_open = false;
                        let idx = dlg.layer_idx;
                        if let (Some(original), Some(flat)) =
                            (&dlg.original_pixels, &dlg.original_flat)
                        {
                            let width = dlg.width as u32;
                            let primary = self.colors_panel.get_primary_color_f32();
                            let c = [
                                (primary[0] * 255.0) as u8,
                                (primary[1] * 255.0) as u8,
                                (primary[2] * 255.0) as u8,
                                255,
                            ];
                            self.spawn_preview_job(
                                ctx.input(|i| i.time),
                                "Canvas Border".to_string(),
                                idx,
                                original.clone(),
                                flat.clone(),
                                move |img| {
                                    crate::ops::effects::canvas_border_core(img, width, c, None)
                                },
                            );
                        }
                    }
                }
            },

            ActiveDialog::PixelDrag(dlg) => match dlg.show(ctx) {
                DialogResult::Changed => {
                    dlg.first_open = false;
                    let idx = dlg.layer_idx;
                    if let (Some(original), Some(flat)) = (&dlg.original_pixels, &dlg.original_flat)
                    {
                        let (seed, amount) = (dlg.seed, dlg.amount);
                        let distance = dlg.distance as u32;
                        let direction = dlg.direction;
                        self.spawn_preview_job(
                            ctx.input(|i| i.time),
                            "Pixel Drag".to_string(),
                            idx,
                            original.clone(),
                            flat.clone(),
                            move |img| {
                                crate::ops::effects::pixel_drag_core(
                                    img, seed, amount, distance, direction, None,
                                )
                            },
                        );
                    }
                }
                DialogResult::Ok(_) => {
                    self.preview_job_token = self.preview_job_token.wrapping_add(1);
                    let idx = dlg.layer_idx;
                    if let Some(flat) = &dlg.original_flat {
                        let (seed, amount) = (dlg.seed, dlg.amount);
                        let distance = dlg.distance as u32;
                        let direction = dlg.direction;
                        if let Some(project) = self.active_project_mut() {
                            Self::apply_fullres_effect(
                                &mut project.canvas_state,
                                idx,
                                flat,
                                |img| {
                                    crate::ops::effects::pixel_drag_core(
                                        img, seed, amount, distance, direction, None,
                                    )
                                },
                            );
                        }
                    }
                    self.active_dialog = ActiveDialog::None;
                    if let Some(project) = self.active_project_mut() {
                        let idx = dlg.layer_idx;
                        if let Some(original) = &dlg.original_pixels
                            && idx < project.canvas_state.layers.len()
                        {
                            let adjusted = project.canvas_state.layers[idx].pixels.clone();
                            project.canvas_state.layers[idx].pixels = original.clone();
                            let mut cmd = SingleLayerSnapshotCommand::new_for_layer(
                                "Pixel Drag".to_string(),
                                &project.canvas_state,
                                idx,
                            );
                            project.canvas_state.layers[idx].pixels = adjusted;
                            cmd.set_after(&project.canvas_state);
                            project.history.push(Box::new(cmd));
                        }
                        project.mark_dirty();
                    }
                    return;
                }
                DialogResult::Cancel => {
                    self.preview_job_token = self.preview_job_token.wrapping_add(1);
                    self.filter_cancel
                        .store(true, std::sync::atomic::Ordering::Relaxed);
                    let idx = dlg.layer_idx;
                    if let Some(original) = &dlg.original_pixels
                        && let Some(project) = self.active_project_mut()
                    {
                        if let Some(layer) = project.canvas_state.layers.get_mut(idx) {
                            layer.pixels = original.clone();
                        }
                        project.canvas_state.mark_dirty(None);
                    }
                    self.active_dialog = ActiveDialog::None;
                    return;
                }
                _ => {
                    dlg.poll_flat();
                    if dlg.first_open && dlg.live_preview && dlg.original_flat.is_some() {
                        dlg.first_open = false;
                        let idx = dlg.layer_idx;
                        if let (Some(original), Some(flat)) =
                            (&dlg.original_pixels, &dlg.original_flat)
                        {
                            let (seed, amount) = (dlg.seed, dlg.amount);
                            let distance = dlg.distance as u32;
                            let direction = dlg.direction;
                            self.spawn_preview_job(
                                ctx.input(|i| i.time),
                                "Pixel Drag".to_string(),
                                idx,
                                original.clone(),
                                flat.clone(),
                                move |img| {
                                    crate::ops::effects::pixel_drag_core(
                                        img, seed, amount, distance, direction, None,
                                    )
                                },
                            );
                        }
                    }
                }
            },

            ActiveDialog::RgbDisplace(dlg) => match dlg.show(ctx) {
                DialogResult::Changed => {
                    dlg.first_open = false;
                    let idx = dlg.layer_idx;
                    if let (Some(original), Some(flat)) = (&dlg.original_pixels, &dlg.original_flat)
                    {
                        let r_off = (dlg.r_x as i32, dlg.r_y as i32);
                        let g_off = (dlg.g_x as i32, dlg.g_y as i32);
                        let b_off = (dlg.b_x as i32, dlg.b_y as i32);
                        self.spawn_preview_job(
                            ctx.input(|i| i.time),
                            "RGB Displace".to_string(),
                            idx,
                            original.clone(),
                            flat.clone(),
                            move |img| {
                                crate::ops::effects::rgb_displace_core(
                                    img, r_off, g_off, b_off, None,
                                )
                            },
                        );
                    }
                }
                DialogResult::Ok(_) => {
                    self.preview_job_token = self.preview_job_token.wrapping_add(1);
                    let idx = dlg.layer_idx;
                    if let Some(flat) = &dlg.original_flat {
                        let r_off = (dlg.r_x as i32, dlg.r_y as i32);
                        let g_off = (dlg.g_x as i32, dlg.g_y as i32);
                        let b_off = (dlg.b_x as i32, dlg.b_y as i32);
                        if let Some(project) = self.active_project_mut() {
                            Self::apply_fullres_effect(
                                &mut project.canvas_state,
                                idx,
                                flat,
                                |img| {
                                    crate::ops::effects::rgb_displace_core(
                                        img, r_off, g_off, b_off, None,
                                    )
                                },
                            );
                        }
                    }
                    self.active_dialog = ActiveDialog::None;
                    if let Some(project) = self.active_project_mut() {
                        let idx = dlg.layer_idx;
                        if let Some(original) = &dlg.original_pixels
                            && idx < project.canvas_state.layers.len()
                        {
                            let adjusted = project.canvas_state.layers[idx].pixels.clone();
                            project.canvas_state.layers[idx].pixels = original.clone();
                            let mut cmd = SingleLayerSnapshotCommand::new_for_layer(
                                "RGB Displace".to_string(),
                                &project.canvas_state,
                                idx,
                            );
                            project.canvas_state.layers[idx].pixels = adjusted;
                            cmd.set_after(&project.canvas_state);
                            project.history.push(Box::new(cmd));
                        }
                        project.mark_dirty();
                    }
                    return;
                }
                DialogResult::Cancel => {
                    self.preview_job_token = self.preview_job_token.wrapping_add(1);
                    self.filter_cancel
                        .store(true, std::sync::atomic::Ordering::Relaxed);
                    let idx = dlg.layer_idx;
                    if let Some(original) = &dlg.original_pixels
                        && let Some(project) = self.active_project_mut()
                    {
                        if let Some(layer) = project.canvas_state.layers.get_mut(idx) {
                            layer.pixels = original.clone();
                        }
                        project.canvas_state.mark_dirty(None);
                    }
                    self.active_dialog = ActiveDialog::None;
                    return;
                }
                _ => {
                    dlg.poll_flat();
                    if dlg.first_open && dlg.live_preview && dlg.original_flat.is_some() {
                        dlg.first_open = false;
                        let idx = dlg.layer_idx;
                        if let (Some(original), Some(flat)) =
                            (&dlg.original_pixels, &dlg.original_flat)
                        {
                            let r_off = (dlg.r_x as i32, dlg.r_y as i32);
                            let g_off = (dlg.g_x as i32, dlg.g_y as i32);
                            let b_off = (dlg.b_x as i32, dlg.b_y as i32);
                            self.spawn_preview_job(
                                ctx.input(|i| i.time),
                                "RGB Displace".to_string(),
                                idx,
                                original.clone(),
                                flat.clone(),
                                move |img| {
                                    crate::ops::effects::rgb_displace_core(
                                        img, r_off, g_off, b_off, None,
                                    )
                                },
                            );
                        }
                    }
                }
            },

            ActiveDialog::Ink(dlg) => match dlg.show(ctx) {
                DialogResult::Changed => {
                    dlg.first_open = false;
                    let idx = dlg.layer_idx;
                    if let (Some(original), Some(flat)) = (&dlg.original_pixels, &dlg.original_flat)
                    {
                        let (edge_strength, threshold) = (dlg.edge_strength, dlg.threshold);
                        self.spawn_preview_job(
                            ctx.input(|i| i.time),
                            "Ink".to_string(),
                            idx,
                            original.clone(),
                            flat.clone(),
                            move |img| {
                                crate::ops::effects::ink_core(img, edge_strength, threshold, None)
                            },
                        );
                    }
                }
                DialogResult::Ok(_) => {
                    self.preview_job_token = self.preview_job_token.wrapping_add(1);
                    let idx = dlg.layer_idx;
                    if let Some(flat) = &dlg.original_flat {
                        let (edge_strength, threshold) = (dlg.edge_strength, dlg.threshold);
                        if let Some(project) = self.active_project_mut() {
                            Self::apply_fullres_effect(
                                &mut project.canvas_state,
                                idx,
                                flat,
                                |img| {
                                    crate::ops::effects::ink_core(
                                        img,
                                        edge_strength,
                                        threshold,
                                        None,
                                    )
                                },
                            );
                        }
                    }
                    self.active_dialog = ActiveDialog::None;
                    if let Some(project) = self.active_project_mut() {
                        let idx = dlg.layer_idx;
                        if let Some(original) = &dlg.original_pixels
                            && idx < project.canvas_state.layers.len()
                        {
                            let adjusted = project.canvas_state.layers[idx].pixels.clone();
                            project.canvas_state.layers[idx].pixels = original.clone();
                            let mut cmd = SingleLayerSnapshotCommand::new_for_layer(
                                "Ink".to_string(),
                                &project.canvas_state,
                                idx,
                            );
                            project.canvas_state.layers[idx].pixels = adjusted;
                            cmd.set_after(&project.canvas_state);
                            project.history.push(Box::new(cmd));
                        }
                        project.mark_dirty();
                    }
                    return;
                }
                DialogResult::Cancel => {
                    self.preview_job_token = self.preview_job_token.wrapping_add(1);
                    self.filter_cancel
                        .store(true, std::sync::atomic::Ordering::Relaxed);
                    let idx = dlg.layer_idx;
                    if let Some(original) = &dlg.original_pixels
                        && let Some(project) = self.active_project_mut()
                    {
                        if let Some(layer) = project.canvas_state.layers.get_mut(idx) {
                            layer.pixels = original.clone();
                        }
                        project.canvas_state.mark_dirty(None);
                    }
                    self.active_dialog = ActiveDialog::None;
                    return;
                }
                _ => {
                    dlg.poll_flat();
                    if dlg.first_open && dlg.live_preview && dlg.original_flat.is_some() {
                        dlg.first_open = false;
                        let idx = dlg.layer_idx;
                        if let (Some(original), Some(flat)) =
                            (&dlg.original_pixels, &dlg.original_flat)
                        {
                            let (edge_strength, threshold) = (dlg.edge_strength, dlg.threshold);
                            self.spawn_preview_job(
                                ctx.input(|i| i.time),
                                "Ink".to_string(),
                                idx,
                                original.clone(),
                                flat.clone(),
                                move |img| {
                                    crate::ops::effects::ink_core(
                                        img,
                                        edge_strength,
                                        threshold,
                                        None,
                                    )
                                },
                            );
                        }
                    }
                }
            },

            ActiveDialog::OilPainting(dlg) => match dlg.show(ctx) {
                DialogResult::Changed => {
                    dlg.first_open = false;
                    let idx = dlg.layer_idx;
                    if let (Some(original), Some(flat)) = (&dlg.original_pixels, &dlg.original_flat)
                    {
                        let (radius, levels) = (dlg.radius as u32, dlg.levels as u32);
                        self.spawn_preview_job(
                            ctx.input(|i| i.time),
                            "Oil Painting".to_string(),
                            idx,
                            original.clone(),
                            flat.clone(),
                            move |img| {
                                crate::ops::effects::oil_painting_core(img, radius, levels, None)
                            },
                        );
                    }
                }
                DialogResult::Ok(_) => {
                    self.preview_job_token = self.preview_job_token.wrapping_add(1);
                    let idx = dlg.layer_idx;
                    if let Some(flat) = &dlg.original_flat {
                        let (radius, levels) = (dlg.radius as u32, dlg.levels as u32);
                        if let Some(project) = self.active_project_mut() {
                            Self::apply_fullres_effect(
                                &mut project.canvas_state,
                                idx,
                                flat,
                                |img| {
                                    crate::ops::effects::oil_painting_core(
                                        img, radius, levels, None,
                                    )
                                },
                            );
                        }
                    }
                    self.active_dialog = ActiveDialog::None;
                    if let Some(project) = self.active_project_mut() {
                        let idx = dlg.layer_idx;
                        if let Some(original) = &dlg.original_pixels
                            && idx < project.canvas_state.layers.len()
                        {
                            let adjusted = project.canvas_state.layers[idx].pixels.clone();
                            project.canvas_state.layers[idx].pixels = original.clone();
                            let mut cmd = SingleLayerSnapshotCommand::new_for_layer(
                                "Oil Painting".to_string(),
                                &project.canvas_state,
                                idx,
                            );
                            project.canvas_state.layers[idx].pixels = adjusted;
                            cmd.set_after(&project.canvas_state);
                            project.history.push(Box::new(cmd));
                        }
                        project.mark_dirty();
                    }
                    return;
                }
                DialogResult::Cancel => {
                    self.preview_job_token = self.preview_job_token.wrapping_add(1);
                    self.filter_cancel
                        .store(true, std::sync::atomic::Ordering::Relaxed);
                    let idx = dlg.layer_idx;
                    if let Some(original) = &dlg.original_pixels
                        && let Some(project) = self.active_project_mut()
                    {
                        if let Some(layer) = project.canvas_state.layers.get_mut(idx) {
                            layer.pixels = original.clone();
                        }
                        project.canvas_state.mark_dirty(None);
                    }
                    self.active_dialog = ActiveDialog::None;
                    return;
                }
                _ => {
                    dlg.poll_flat();
                    if dlg.first_open && dlg.live_preview && dlg.original_flat.is_some() {
                        dlg.first_open = false;
                        let idx = dlg.layer_idx;
                        if let (Some(original), Some(flat)) =
                            (&dlg.original_pixels, &dlg.original_flat)
                        {
                            let (radius, levels) = (dlg.radius as u32, dlg.levels as u32);
                            self.spawn_preview_job(
                                ctx.input(|i| i.time),
                                "Oil Painting".to_string(),
                                idx,
                                original.clone(),
                                flat.clone(),
                                move |img| {
                                    crate::ops::effects::oil_painting_core(
                                        img, radius, levels, None,
                                    )
                                },
                            );
                        }
                    }
                }
            },

            ActiveDialog::ColorFilter(dlg) => match dlg.show(ctx) {
                DialogResult::Changed => {
                    dlg.first_open = false;
                    let idx = dlg.layer_idx;
                    if let (Some(original), Some(flat)) = (&dlg.original_pixels, &dlg.original_flat)
                    {
                        let fc = [
                            (dlg.color[0] * 255.0) as u8,
                            (dlg.color[1] * 255.0) as u8,
                            (dlg.color[2] * 255.0) as u8,
                            255,
                        ];
                        let intensity = dlg.intensity;
                        let mode = dlg.filter_mode();
                        self.spawn_preview_job(
                            ctx.input(|i| i.time),
                            "Color Filter".to_string(),
                            idx,
                            original.clone(),
                            flat.clone(),
                            move |img| {
                                crate::ops::effects::color_filter_core(
                                    img, fc, intensity, mode, None,
                                )
                            },
                        );
                    }
                }
                DialogResult::Ok(_) => {
                    self.preview_job_token = self.preview_job_token.wrapping_add(1);
                    let idx = dlg.layer_idx;
                    if let Some(flat) = &dlg.original_flat {
                        let fc = [
                            (dlg.color[0] * 255.0) as u8,
                            (dlg.color[1] * 255.0) as u8,
                            (dlg.color[2] * 255.0) as u8,
                            255,
                        ];
                        let intensity = dlg.intensity;
                        let mode = dlg.filter_mode();
                        if let Some(project) = self.active_project_mut() {
                            Self::apply_fullres_effect(
                                &mut project.canvas_state,
                                idx,
                                flat,
                                |img| {
                                    crate::ops::effects::color_filter_core(
                                        img, fc, intensity, mode, None,
                                    )
                                },
                            );
                        }
                    }
                    self.active_dialog = ActiveDialog::None;
                    if let Some(project) = self.active_project_mut() {
                        let idx = dlg.layer_idx;
                        if let Some(original) = &dlg.original_pixels
                            && idx < project.canvas_state.layers.len()
                        {
                            let adjusted = project.canvas_state.layers[idx].pixels.clone();
                            project.canvas_state.layers[idx].pixels = original.clone();
                            let mut cmd = SingleLayerSnapshotCommand::new_for_layer(
                                "Color Filter".to_string(),
                                &project.canvas_state,
                                idx,
                            );
                            project.canvas_state.layers[idx].pixels = adjusted;
                            cmd.set_after(&project.canvas_state);
                            project.history.push(Box::new(cmd));
                        }
                        project.mark_dirty();
                    }
                    return;
                }
                DialogResult::Cancel => {
                    self.preview_job_token = self.preview_job_token.wrapping_add(1);
                    self.filter_cancel
                        .store(true, std::sync::atomic::Ordering::Relaxed);
                    let idx = dlg.layer_idx;
                    if let Some(original) = &dlg.original_pixels
                        && let Some(project) = self.active_project_mut()
                    {
                        if let Some(layer) = project.canvas_state.layers.get_mut(idx) {
                            layer.pixels = original.clone();
                        }
                        project.canvas_state.mark_dirty(None);
                    }
                    self.active_dialog = ActiveDialog::None;
                    return;
                }
                _ => {
                    dlg.poll_flat();
                    if dlg.first_open && dlg.live_preview && dlg.original_flat.is_some() {
                        dlg.first_open = false;
                        let idx = dlg.layer_idx;
                        if let (Some(original), Some(flat)) =
                            (&dlg.original_pixels, &dlg.original_flat)
                        {
                            let fc = [
                                (dlg.color[0] * 255.0) as u8,
                                (dlg.color[1] * 255.0) as u8,
                                (dlg.color[2] * 255.0) as u8,
                                255,
                            ];
                            let intensity = dlg.intensity;
                            let mode = dlg.filter_mode();
                            self.spawn_preview_job(
                                ctx.input(|i| i.time),
                                "Color Filter".to_string(),
                                idx,
                                original.clone(),
                                flat.clone(),
                                move |img| {
                                    crate::ops::effects::color_filter_core(
                                        img, fc, intensity, mode, None,
                                    )
                                },
                            );
                        }
                    }
                }
            },

            ActiveDialog::Contours(dlg) => match dlg.show(ctx) {
                DialogResult::Changed => {
                    dlg.first_open = false;
                    let idx = dlg.layer_idx;
                    if let (Some(original), Some(flat)) = (&dlg.original_pixels, &dlg.original_flat)
                    {
                        let (contour_scale, frequency, line_width) =
                            (dlg.scale, dlg.frequency, dlg.line_width);
                        let lc = [
                            (dlg.line_color[0] * 255.0) as u8,
                            (dlg.line_color[1] * 255.0) as u8,
                            (dlg.line_color[2] * 255.0) as u8,
                            255u8,
                        ];
                        let (seed, octaves, blend) = (dlg.seed, dlg.octaves as u32, dlg.blend);
                        self.spawn_preview_job(
                            ctx.input(|i| i.time),
                            "Contours".to_string(),
                            idx,
                            original.clone(),
                            flat.clone(),
                            move |img| {
                                crate::ops::effects::contours_core(
                                    img,
                                    contour_scale,
                                    frequency,
                                    line_width,
                                    lc,
                                    seed,
                                    octaves,
                                    blend,
                                    None,
                                )
                            },
                        );
                    }
                }
                DialogResult::Ok(_) => {
                    self.preview_job_token = self.preview_job_token.wrapping_add(1);
                    let idx = dlg.layer_idx;
                    if let Some(flat) = &dlg.original_flat {
                        let (contour_scale, frequency, line_width) =
                            (dlg.scale, dlg.frequency, dlg.line_width);
                        let lc = [
                            (dlg.line_color[0] * 255.0) as u8,
                            (dlg.line_color[1] * 255.0) as u8,
                            (dlg.line_color[2] * 255.0) as u8,
                            255u8,
                        ];
                        let (seed, octaves, blend) = (dlg.seed, dlg.octaves as u32, dlg.blend);
                        if let Some(project) = self.active_project_mut() {
                            Self::apply_fullres_effect(
                                &mut project.canvas_state,
                                idx,
                                flat,
                                |img| {
                                    crate::ops::effects::contours_core(
                                        img,
                                        contour_scale,
                                        frequency,
                                        line_width,
                                        lc,
                                        seed,
                                        octaves,
                                        blend,
                                        None,
                                    )
                                },
                            );
                        }
                    }
                    self.active_dialog = ActiveDialog::None;
                    if let Some(project) = self.active_project_mut() {
                        let idx = dlg.layer_idx;
                        if let Some(original) = &dlg.original_pixels
                            && idx < project.canvas_state.layers.len()
                        {
                            let adjusted = project.canvas_state.layers[idx].pixels.clone();
                            project.canvas_state.layers[idx].pixels = original.clone();
                            let mut cmd = SingleLayerSnapshotCommand::new_for_layer(
                                "Contours".to_string(),
                                &project.canvas_state,
                                idx,
                            );
                            project.canvas_state.layers[idx].pixels = adjusted;
                            cmd.set_after(&project.canvas_state);
                            project.history.push(Box::new(cmd));
                        }
                        project.mark_dirty();
                    }
                    return;
                }
                DialogResult::Cancel => {
                    self.preview_job_token = self.preview_job_token.wrapping_add(1);
                    self.filter_cancel
                        .store(true, std::sync::atomic::Ordering::Relaxed);
                    let idx = dlg.layer_idx;
                    if let Some(original) = &dlg.original_pixels
                        && let Some(project) = self.active_project_mut()
                    {
                        if let Some(layer) = project.canvas_state.layers.get_mut(idx) {
                            layer.pixels = original.clone();
                        }
                        project.canvas_state.mark_dirty(None);
                    }
                    self.active_dialog = ActiveDialog::None;
                    return;
                }
                _ => {
                    dlg.poll_flat();
                    if dlg.first_open && dlg.live_preview && dlg.original_flat.is_some() {
                        dlg.first_open = false;
                        let idx = dlg.layer_idx;
                        if let (Some(original), Some(flat)) =
                            (&dlg.original_pixels, &dlg.original_flat)
                        {
                            let (contour_scale, frequency, line_width) =
                                (dlg.scale, dlg.frequency, dlg.line_width);
                            let lc = [
                                (dlg.line_color[0] * 255.0) as u8,
                                (dlg.line_color[1] * 255.0) as u8,
                                (dlg.line_color[2] * 255.0) as u8,
                                255u8,
                            ];
                            let (seed, octaves, blend) = (dlg.seed, dlg.octaves as u32, dlg.blend);
                            self.spawn_preview_job(
                                ctx.input(|i| i.time),
                                "Contours".to_string(),
                                idx,
                                original.clone(),
                                flat.clone(),
                                move |img| {
                                    crate::ops::effects::contours_core(
                                        img,
                                        contour_scale,
                                        frequency,
                                        line_width,
                                        lc,
                                        seed,
                                        octaves,
                                        blend,
                                        None,
                                    )
                                },
                            );
                        }
                    }
                }
            },

            ActiveDialog::RemoveBackground(dlg) => match dlg.show(ctx) {
                DialogResult::Ok(settings) => {
                    self.active_dialog = ActiveDialog::None;
                    if let Some(project) = self.active_project() {
                        let layer_idx = project.canvas_state.active_layer_index;
                        let original_pixels = project.canvas_state.layers[layer_idx].pixels.clone();
                        let original_flat = original_pixels.to_rgba_image();
                        let dll_path = self.settings.onnx_runtime_path.clone();
                        let model_path = self.settings.birefnet_model_path.clone();

                        self.filter_status_description = t!("status.remove_background");
                        self.spawn_filter_job(
                            ctx.input(|i| i.time),
                            "Remove Background".to_string(),
                            layer_idx,
                            original_pixels,
                            original_flat,
                            move |input_img| match crate::ops::ai::remove_background(
                                &dll_path,
                                &model_path,
                                input_img,
                                &settings,
                            ) {
                                Ok(result) => result,
                                Err(e) => {
                                    eprintln!("Remove Background failed: {}", e);
                                    input_img.clone()
                                }
                            },
                        );
                    }
                    return;
                }
                DialogResult::Cancel => {
                    self.active_dialog = ActiveDialog::None;
                    return;
                }
                _ => {}
            },

            // ================================================================
            // THRESHOLD
            // ================================================================
            ActiveDialog::Threshold(dlg) => match dlg.show(ctx) {
                DialogResult::Changed => {
                    let level = dlg.level;
                    let idx = dlg.layer_idx;
                    if let Some(flat) = &dlg.original_flat
                        && let Some(project) = self.active_project_mut()
                    {
                        crate::ops::adjustments::threshold_from_flat(
                            &mut project.canvas_state,
                            idx,
                            level,
                            flat,
                        );
                    }
                }
                DialogResult::Ok(_) => {
                    self.active_dialog = ActiveDialog::None;
                    if let Some(project) = self.active_project_mut() {
                        let idx = dlg.layer_idx;
                        if let Some(original) = &dlg.original_pixels
                            && idx < project.canvas_state.layers.len()
                        {
                            let adjusted = project.canvas_state.layers[idx].pixels.clone();
                            project.canvas_state.layers[idx].pixels = original.clone();
                            let mut cmd = SingleLayerSnapshotCommand::new_for_layer(
                                "Threshold".to_string(),
                                &project.canvas_state,
                                idx,
                            );
                            project.canvas_state.layers[idx].pixels = adjusted;
                            cmd.set_after(&project.canvas_state);
                            project.history.push(Box::new(cmd));
                        }
                        project.mark_dirty();
                    }
                    return;
                }
                DialogResult::Cancel => {
                    self.filter_cancel
                        .store(true, std::sync::atomic::Ordering::Relaxed);
                    let idx = dlg.layer_idx;
                    if let Some(original) = &dlg.original_pixels
                        && let Some(project) = self.active_project_mut()
                    {
                        if let Some(layer) = project.canvas_state.layers.get_mut(idx) {
                            layer.pixels = original.clone();
                        }
                        project.canvas_state.mark_dirty(None);
                    }
                    self.active_dialog = ActiveDialog::None;
                    return;
                }
                _ => {}
            },

            // ================================================================
            // POSTERIZE
            // ================================================================
            ActiveDialog::Posterize(dlg) => match dlg.show(ctx) {
                DialogResult::Changed => {
                    let levels = dlg.levels;
                    let idx = dlg.layer_idx;
                    if let Some(flat) = &dlg.original_flat
                        && let Some(project) = self.active_project_mut()
                    {
                        crate::ops::adjustments::posterize_from_flat(
                            &mut project.canvas_state,
                            idx,
                            levels,
                            flat,
                        );
                    }
                }
                DialogResult::Ok(_) => {
                    self.active_dialog = ActiveDialog::None;
                    if let Some(project) = self.active_project_mut() {
                        let idx = dlg.layer_idx;
                        if let Some(original) = &dlg.original_pixels
                            && idx < project.canvas_state.layers.len()
                        {
                            let adjusted = project.canvas_state.layers[idx].pixels.clone();
                            project.canvas_state.layers[idx].pixels = original.clone();
                            let mut cmd = SingleLayerSnapshotCommand::new_for_layer(
                                "Posterize".to_string(),
                                &project.canvas_state,
                                idx,
                            );
                            project.canvas_state.layers[idx].pixels = adjusted;
                            cmd.set_after(&project.canvas_state);
                            project.history.push(Box::new(cmd));
                        }
                        project.mark_dirty();
                    }
                    return;
                }
                DialogResult::Cancel => {
                    self.filter_cancel
                        .store(true, std::sync::atomic::Ordering::Relaxed);
                    let idx = dlg.layer_idx;
                    if let Some(original) = &dlg.original_pixels
                        && let Some(project) = self.active_project_mut()
                    {
                        if let Some(layer) = project.canvas_state.layers.get_mut(idx) {
                            layer.pixels = original.clone();
                        }
                        project.canvas_state.mark_dirty(None);
                    }
                    self.active_dialog = ActiveDialog::None;
                    return;
                }
                _ => {}
            },

            // ================================================================
            // COLOR BALANCE
            // ================================================================
            ActiveDialog::ColorBalance(dlg) => match dlg.show(ctx) {
                DialogResult::Changed => {
                    let (shadows, midtones, highlights) =
                        (dlg.shadows, dlg.midtones, dlg.highlights);
                    let idx = dlg.layer_idx;
                    if let Some(flat) = &dlg.original_flat
                        && let Some(project) = self.active_project_mut()
                    {
                        crate::ops::adjustments::color_balance_from_flat(
                            &mut project.canvas_state,
                            idx,
                            shadows,
                            midtones,
                            highlights,
                            flat,
                        );
                    }
                }
                DialogResult::Ok(_) => {
                    self.active_dialog = ActiveDialog::None;
                    if let Some(project) = self.active_project_mut() {
                        let idx = dlg.layer_idx;
                        if let Some(original) = &dlg.original_pixels
                            && idx < project.canvas_state.layers.len()
                        {
                            let adjusted = project.canvas_state.layers[idx].pixels.clone();
                            project.canvas_state.layers[idx].pixels = original.clone();
                            let mut cmd = SingleLayerSnapshotCommand::new_for_layer(
                                "Color Balance".to_string(),
                                &project.canvas_state,
                                idx,
                            );
                            project.canvas_state.layers[idx].pixels = adjusted;
                            cmd.set_after(&project.canvas_state);
                            project.history.push(Box::new(cmd));
                        }
                        project.mark_dirty();
                    }
                    return;
                }
                DialogResult::Cancel => {
                    self.filter_cancel
                        .store(true, std::sync::atomic::Ordering::Relaxed);
                    let idx = dlg.layer_idx;
                    if let Some(original) = &dlg.original_pixels
                        && let Some(project) = self.active_project_mut()
                    {
                        if let Some(layer) = project.canvas_state.layers.get_mut(idx) {
                            layer.pixels = original.clone();
                        }
                        project.canvas_state.mark_dirty(None);
                    }
                    self.active_dialog = ActiveDialog::None;
                    return;
                }
                _ => {}
            },

            // ================================================================
            // GRADIENT MAP
            // ================================================================
            ActiveDialog::GradientMap(dlg) => match dlg.show(ctx) {
                DialogResult::Changed => {
                    let lut = dlg.build_lut();
                    let idx = dlg.layer_idx;
                    if let Some(flat) = &dlg.original_flat
                        && let Some(project) = self.active_project_mut()
                    {
                        crate::ops::adjustments::gradient_map_from_flat(
                            &mut project.canvas_state,
                            idx,
                            &lut,
                            flat,
                        );
                    }
                }
                DialogResult::Ok(lut) => {
                    self.active_dialog = ActiveDialog::None;
                    if let Some(project) = self.active_project_mut() {
                        let idx = dlg.layer_idx;
                        if let Some(original) = &dlg.original_pixels
                            && idx < project.canvas_state.layers.len()
                        {
                            let adjusted = project.canvas_state.layers[idx].pixels.clone();
                            project.canvas_state.layers[idx].pixels = original.clone();
                            let mut cmd = SingleLayerSnapshotCommand::new_for_layer(
                                "Gradient Map".to_string(),
                                &project.canvas_state,
                                idx,
                            );
                            project.canvas_state.layers[idx].pixels = adjusted;
                            cmd.set_after(&project.canvas_state);
                            project.history.push(Box::new(cmd));
                        }
                        project.mark_dirty();
                    }
                    let _ = lut;
                    return;
                }
                DialogResult::Cancel => {
                    self.filter_cancel
                        .store(true, std::sync::atomic::Ordering::Relaxed);
                    let idx = dlg.layer_idx;
                    if let Some(original) = &dlg.original_pixels
                        && let Some(project) = self.active_project_mut()
                    {
                        if let Some(layer) = project.canvas_state.layers.get_mut(idx) {
                            layer.pixels = original.clone();
                        }
                        project.canvas_state.mark_dirty(None);
                    }
                    self.active_dialog = ActiveDialog::None;
                    return;
                }
                _ => {}
            },

            // ================================================================
            // BLACK AND WHITE
            // ================================================================
            ActiveDialog::BlackAndWhite(dlg) => match dlg.show(ctx) {
                DialogResult::Changed => {
                    let (r, g, b) = (dlg.r_weight, dlg.g_weight, dlg.b_weight);
                    let idx = dlg.layer_idx;
                    if let Some(flat) = &dlg.original_flat
                        && let Some(project) = self.active_project_mut()
                    {
                        crate::ops::adjustments::black_and_white_from_flat(
                            &mut project.canvas_state,
                            idx,
                            r,
                            g,
                            b,
                            flat,
                        );
                    }
                }
                DialogResult::Ok(_) => {
                    self.active_dialog = ActiveDialog::None;
                    if let Some(project) = self.active_project_mut() {
                        let idx = dlg.layer_idx;
                        if let Some(original) = &dlg.original_pixels
                            && idx < project.canvas_state.layers.len()
                        {
                            let adjusted = project.canvas_state.layers[idx].pixels.clone();
                            project.canvas_state.layers[idx].pixels = original.clone();
                            let mut cmd = SingleLayerSnapshotCommand::new_for_layer(
                                "Black & White".to_string(),
                                &project.canvas_state,
                                idx,
                            );
                            project.canvas_state.layers[idx].pixels = adjusted;
                            cmd.set_after(&project.canvas_state);
                            project.history.push(Box::new(cmd));
                        }
                        project.mark_dirty();
                    }
                    return;
                }
                DialogResult::Cancel => {
                    self.filter_cancel
                        .store(true, std::sync::atomic::Ordering::Relaxed);
                    let idx = dlg.layer_idx;
                    if let Some(original) = &dlg.original_pixels
                        && let Some(project) = self.active_project_mut()
                    {
                        if let Some(layer) = project.canvas_state.layers.get_mut(idx) {
                            layer.pixels = original.clone();
                        }
                        project.canvas_state.mark_dirty(None);
                    }
                    self.active_dialog = ActiveDialog::None;
                    return;
                }
                _ => {}
            },

            // ================================================================
            // VIBRANCE
            // ================================================================
            ActiveDialog::Vibrance(dlg) => match dlg.show(ctx) {
                DialogResult::Changed => {
                    let amount = dlg.amount;
                    let idx = dlg.layer_idx;
                    if let Some(flat) = &dlg.original_flat
                        && let Some(project) = self.active_project_mut()
                    {
                        crate::ops::adjustments::vibrance_from_flat(
                            &mut project.canvas_state,
                            idx,
                            amount,
                            flat,
                        );
                    }
                }
                DialogResult::Ok(_) => {
                    self.active_dialog = ActiveDialog::None;
                    if let Some(project) = self.active_project_mut() {
                        let idx = dlg.layer_idx;
                        if let Some(original) = &dlg.original_pixels
                            && idx < project.canvas_state.layers.len()
                        {
                            let adjusted = project.canvas_state.layers[idx].pixels.clone();
                            project.canvas_state.layers[idx].pixels = original.clone();
                            let mut cmd = SingleLayerSnapshotCommand::new_for_layer(
                                "Vibrance".to_string(),
                                &project.canvas_state,
                                idx,
                            );
                            project.canvas_state.layers[idx].pixels = adjusted;
                            cmd.set_after(&project.canvas_state);
                            project.history.push(Box::new(cmd));
                        }
                        project.mark_dirty();
                    }
                    return;
                }
                DialogResult::Cancel => {
                    self.filter_cancel
                        .store(true, std::sync::atomic::Ordering::Relaxed);
                    let idx = dlg.layer_idx;
                    if let Some(original) = &dlg.original_pixels
                        && let Some(project) = self.active_project_mut()
                    {
                        if let Some(layer) = project.canvas_state.layers.get_mut(idx) {
                            layer.pixels = original.clone();
                        }
                        project.canvas_state.mark_dirty(None);
                    }
                    self.active_dialog = ActiveDialog::None;
                    return;
                }
                _ => {}
            },
            ActiveDialog::ColorRange(dlg) => {
                match dlg.show(ctx) {
                    DialogResult::Changed => {
                        // Restore original selection then re-run so live preview is correct
                        if let Some(project) = self.active_project_mut() {
                            project.canvas_state.selection_mask = dlg.original_selection.clone();
                            crate::ops::adjustments::select_color_range(
                                &mut project.canvas_state,
                                dlg.hue_center,
                                dlg.hue_tolerance,
                                dlg.sat_min,
                                dlg.fuzziness,
                                dlg.mode,
                            );
                            project.canvas_state.invalidate_selection_overlay();
                            project.canvas_state.mark_dirty(None);
                        }
                    }
                    DialogResult::Ok(_) => {
                        // Selection already applied from last Changed event; just commit
                        self.active_dialog = ActiveDialog::None;
                        return;
                    }
                    DialogResult::Cancel => {
                        self.filter_cancel
                            .store(true, std::sync::atomic::Ordering::Relaxed);
                        // Restore original selection
                        if let Some(project) = self.active_project_mut() {
                            project.canvas_state.selection_mask = dlg.original_selection.clone();
                            project.canvas_state.invalidate_selection_overlay();
                            project.canvas_state.mark_dirty(None);
                        }
                        self.active_dialog = ActiveDialog::None;
                        return;
                    }
                    _ => {}
                }
            }
        }

        // If we reach here the dialog is still open ÔÇö put it back
        self.active_dialog = dialog;
    }
}

// --- Floating Panel Methods ---
impl PaintFEApp {
}
