pub struct ResizeImageDialog {
    pub width: f32,
    pub height: f32,
    width_input: String,
    height_input: String,
    pub scale_percent: f32,
    pub lock_aspect: bool,
    aspect_ratio: f32,
    pub interpolation: Interpolation,
    pub preset: ResizePreset,
    original_w: u32,
    original_h: u32,
    focus_width_on_open: bool,
    replace_width_on_first_edit: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum ResizePreset {
    #[default]
    Custom,
    Hd1920x1080,
    Square1080,
    Uhd4K,
    A4At300Dpi,
}

impl ResizePreset {
    pub fn label(&self) -> String {
        match self {
            ResizePreset::Custom => t!("resize_preset.custom"),
            ResizePreset::Hd1920x1080 => t!("resize_preset.hd"),
            ResizePreset::Square1080 => t!("resize_preset.square"),
            ResizePreset::Uhd4K => t!("resize_preset.uhd4k"),
            ResizePreset::A4At300Dpi => t!("resize_preset.a4_300dpi"),
        }
    }
    pub fn dims(&self) -> Option<(u32, u32)> {
        match self {
            ResizePreset::Custom => None,
            ResizePreset::Hd1920x1080 => Some((1920, 1080)),
            ResizePreset::Square1080 => Some((1080, 1080)),
            ResizePreset::Uhd4K => Some((3840, 2160)),
            ResizePreset::A4At300Dpi => Some((2480, 3508)),
        }
    }
    pub fn all() -> &'static [ResizePreset] {
        &[
            ResizePreset::Custom,
            ResizePreset::Hd1920x1080,
            ResizePreset::Square1080,
            ResizePreset::Uhd4K,
            ResizePreset::A4At300Dpi,
        ]
    }
}

impl ResizeImageDialog {
    pub fn new(state: &CanvasState) -> Self {
        Self {
            width: state.width as f32,
            height: state.height as f32,
            width_input: format_dimension_value(state.width as f32),
            height_input: format_dimension_value(state.height as f32),
            scale_percent: 100.0,
            lock_aspect: true,
            aspect_ratio: state.width as f32 / state.height.max(1) as f32,
            interpolation: Interpolation::default(),
            preset: ResizePreset::default(),
            original_w: state.width,
            original_h: state.height,
            focus_width_on_open: true,
            replace_width_on_first_edit: true,
        }
    }

    fn sync_inputs_from_values(&mut self) {
        self.width_input = format_dimension_value(self.width);
        self.height_input = format_dimension_value(self.height);
    }

    fn commit_width_input(&mut self) {
        let old_width = self.width;
        if let Some(new_width) = evaluate_dimension_expression(&self.width_input) {
            self.width = new_width.round().clamp(1.0, 20000.0);
            self.width_input = format_dimension_value(self.width);
            self.preset = ResizePreset::Custom;
            if self.lock_aspect && old_width > 0.0 {
                self.height = (self.width / self.aspect_ratio).round().clamp(1.0, 20000.0);
                self.height_input = format_dimension_value(self.height);
            } else {
                self.aspect_ratio = self.width / self.height.max(1.0);
            }
            self.scale_percent = self.width / self.original_w.max(1) as f32 * 100.0;
        } else {
            self.width_input = format_dimension_value(self.width);
        }
    }

    fn commit_height_input(&mut self) {
        let old_height = self.height;
        if let Some(new_height) = evaluate_dimension_expression(&self.height_input) {
            self.height = new_height.round().clamp(1.0, 20000.0);
            self.height_input = format_dimension_value(self.height);
            self.preset = ResizePreset::Custom;
            if self.lock_aspect && old_height > 0.0 {
                self.width = (self.height * self.aspect_ratio)
                    .round()
                    .clamp(1.0, 20000.0);
                self.width_input = format_dimension_value(self.width);
            } else {
                self.aspect_ratio = self.width / self.height.max(1.0);
            }
            self.scale_percent = self.height / self.original_h.max(1) as f32 * 100.0;
        } else {
            self.height_input = format_dimension_value(self.height);
        }
    }

    fn commit_inputs(&mut self) {
        self.commit_width_input();
        self.commit_height_input();
    }

    pub fn show(&mut self, ctx: &egui::Context) -> DialogResult<(u32, u32, Interpolation)> {
        let mut result = DialogResult::Open;
        let colors = DialogColors::from_ctx(ctx);
        let mut ok_pressed = false;

        egui::Window::new("dialog_resize_image")
            .title_bar(false)
            .collapsible(false)
            .resizable(false)
            .default_pos(egui::pos2(
                ctx.content_rect().center().x - 175.0,
                ctx.content_rect().center().y - 160.0,
            ))
            .show(ctx, |ui| {
                ui.set_min_width(350.0);

                paint_dialog_header(ui, &colors, "\u{1F4D0}", &t!("dialog.resize_image"));
                ui.add_space(4.0);

                // -- Preset (own grid so it doesn't misalign the dims columns) --
                section_label(ui, &colors, &t!("dialog.resize_image.dimensions"));

                egui::Grid::new("resize_img_preset")
                    .num_columns(2)
                    .min_col_width(80.0)
                    .spacing([8.0, 4.0])
                    .show(ui, |ui| {
                        ui.label(t!("dialog.resize_image.preset"));
                        egui::ComboBox::from_id_salt("resize_preset")
                            .width(210.0)
                            .selected_text(self.preset.label())
                            .show_ui(ui, |ui| {
                                for p in ResizePreset::all() {
                                    if ui
                                        .selectable_value(&mut self.preset, *p, p.label())
                                        .clicked()
                                        && let Some((w, h)) = p.dims()
                                    {
                                        self.width = w as f32;
                                        self.height = h as f32;
                                        self.scale_percent =
                                            self.width / self.original_w.max(1) as f32 * 100.0;
                                        self.aspect_ratio = self.width / self.height.max(1.0);
                                        self.sync_inputs_from_values();
                                    }
                                }
                            });
                        ui.end_row();
                    });

                ui.add_space(2.0);

                // -- Width / Height / Lock / Scale grid --
                egui::Grid::new("resize_img_dims")
                    .num_columns(3)
                    .min_col_width(80.0)
                    .spacing([8.0, 5.0])
                    .show(ui, |ui| {
                        // Width row
                        ui.label(t!("dialog.resize_image.width"));
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 4.0;
                            let previous_input = self.width_input.clone();
                            let width_response = ui.add(
                                egui::TextEdit::singleline(&mut self.width_input)
                                    .desired_width(96.0),
                            );
                            if self.focus_width_on_open {
                                width_response.request_focus();
                                self.focus_width_on_open = false;
                            }
                            if self.replace_width_on_first_edit && width_response.changed() {
                                if self.width_input.starts_with(&previous_input)
                                    && self.width_input.len() > previous_input.len()
                                {
                                    let suffix = &self.width_input[previous_input.len()..];
                                    let suffix_trimmed = suffix.trim_start();
                                    if !matches!(
                                        suffix_trimmed.chars().next(),
                                        Some('+') | Some('-') | Some('*') | Some('/')
                                    ) {
                                        self.width_input = suffix.to_string();
                                    }
                                }
                                self.replace_width_on_first_edit = false;
                            }
                            let width_commit = width_response.lost_focus()
                                || (width_response.has_focus()
                                    && ui.input(|i| i.key_pressed(egui::Key::Tab)));
                            if width_commit {
                                self.commit_width_input();
                            }
                            ui.label("px");
                        });
                        ui.end_row();

                        // Height row
                        ui.label(t!("dialog.resize_image.height"));
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 4.0;
                            let height_response = ui.add(
                                egui::TextEdit::singleline(&mut self.height_input)
                                    .desired_width(96.0),
                            );
                            let height_commit = height_response.lost_focus()
                                || (height_response.has_focus()
                                    && ui.input(|i| i.key_pressed(egui::Key::Tab)));
                            if height_commit {
                                self.commit_height_input();
                            }
                            ui.label("px");
                        });
                        ui.end_row();

                        // Lock aspect ratio (inline between H and Scale)
                        ui.label("");
                        let lock_resp = ui
                            .checkbox(&mut self.lock_aspect, t!("dialog.resize_image.lock_aspect"));
                        if lock_resp.changed() && self.lock_aspect {
                            self.aspect_ratio = self.width / self.height.max(1.0);
                        }
                        ui.label("");
                        ui.end_row();

                        // Scale row
                        ui.label(t!("dialog.resize_image.scale"));
                        let s_changed = numeric_field_with_buttons(
                            ui,
                            &mut self.scale_percent,
                            0.5,
                            1.0..=10000.0,
                            "%",
                            5.0,
                        );
                        ui.label("");
                        ui.end_row();

                        if s_changed {
                            self.preset = ResizePreset::Custom;
                            self.width =
                                (self.original_w as f32 * self.scale_percent / 100.0).round();
                            self.height =
                                (self.original_h as f32 * self.scale_percent / 100.0).round();
                            self.aspect_ratio = self.width / self.height.max(1.0);
                            self.sync_inputs_from_values();
                        }
                    });

                // -- Quality section --
                accent_separator(ui, &colors);
                section_label(ui, &colors, &t!("dialog.resize_image.quality"));

                egui::Grid::new("resize_img_quality")
                    .num_columns(2)
                    .spacing([8.0, 4.0])
                    .show(ui, |ui| {
                        ui.label(t!("dialog.resize_image.interpolation"));
                        egui::ComboBox::from_id_salt("resize_interp")
                            .width(160.0)
                            .selected_text(self.interpolation.label())
                            .show_ui(ui, |ui| {
                                for i in Interpolation::all() {
                                    ui.selectable_value(&mut self.interpolation, *i, i.label());
                                }
                            });
                        ui.end_row();
                    });

                // -- Info bar --
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.add_space(4.0);
                    let new_w = self.width.round() as u32;
                    let new_h = self.height.round() as u32;
                    let info = format!(
                        "{}x{} \u{2192} {}x{}",
                        self.original_w, self.original_h, new_w, new_h
                    );
                    ui.label(
                        egui::RichText::new(info)
                            .size(11.0)
                            .color(colors.text_muted),
                    );
                });

                // -- Footer --
                let (ok, cancel) = dialog_footer(ui, &colors);
                if ok {
                    ok_pressed = true;
                }
                if cancel {
                    result = DialogResult::Cancel;
                }
            });

        // Keyboard shortcuts
        if matches!(result, DialogResult::Open) {
            if ctx.input(|i| i.key_pressed(egui::Key::Enter)) {
                self.commit_inputs();
                ok_pressed = true;
            }
            if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
                result = DialogResult::Cancel;
            }
        }
        if ok_pressed && matches!(result, DialogResult::Open) {
            let w = (self.width.round() as u32).max(1);
            let h = (self.height.round() as u32).max(1);
            result = DialogResult::Ok((w, h, self.interpolation));
        }

        result
    }
}

// ============================================================================
// RESIZE CANVAS DIALOG
// ============================================================================

pub struct ResizeCanvasDialog {
    pub width: f32,
    pub height: f32,
    width_input: String,
    height_input: String,
    pub scale_percent: f32,
    pub lock_aspect: bool,
    aspect_ratio: f32,
    /// Anchor as (col, row) each 0..=2.
    pub anchor: (u32, u32),
    pub fill_transparent: bool,
    original_w: u32,
    original_h: u32,
    focus_width_on_open: bool,
    replace_width_on_first_edit: bool,
}

impl ResizeCanvasDialog {
    pub fn new(state: &CanvasState) -> Self {
        Self {
            width: state.width as f32,
            height: state.height as f32,
            width_input: format_dimension_value(state.width as f32),
            height_input: format_dimension_value(state.height as f32),
            scale_percent: 100.0,
            lock_aspect: true,
            aspect_ratio: state.width as f32 / state.height.max(1) as f32,
            anchor: (1, 1), // center
            fill_transparent: true,
            original_w: state.width,
            original_h: state.height,
            focus_width_on_open: true,
            replace_width_on_first_edit: true,
        }
    }

    fn sync_inputs_from_values(&mut self) {
        self.width_input = format_dimension_value(self.width);
        self.height_input = format_dimension_value(self.height);
    }

    fn commit_width_input(&mut self) {
        let old_width = self.width;
        if let Some(new_width) = evaluate_dimension_expression(&self.width_input) {
            self.width = new_width.round().clamp(1.0, 20000.0);
            self.width_input = format_dimension_value(self.width);
            if self.lock_aspect && old_width > 0.0 {
                self.height = (self.width / self.aspect_ratio).round().clamp(1.0, 20000.0);
                self.height_input = format_dimension_value(self.height);
            } else {
                self.aspect_ratio = self.width / self.height.max(1.0);
            }
            self.scale_percent = self.width / self.original_w.max(1) as f32 * 100.0;
        } else {
            self.width_input = format_dimension_value(self.width);
        }
    }

    fn commit_height_input(&mut self) {
        let old_height = self.height;
        if let Some(new_height) = evaluate_dimension_expression(&self.height_input) {
            self.height = new_height.round().clamp(1.0, 20000.0);
            self.height_input = format_dimension_value(self.height);
            if self.lock_aspect && old_height > 0.0 {
                self.width = (self.height * self.aspect_ratio)
                    .round()
                    .clamp(1.0, 20000.0);
                self.width_input = format_dimension_value(self.width);
            } else {
                self.aspect_ratio = self.width / self.height.max(1.0);
            }
            self.scale_percent = self.height / self.original_h.max(1) as f32 * 100.0;
        } else {
            self.height_input = format_dimension_value(self.height);
        }
    }

    fn commit_inputs(&mut self) {
        self.commit_width_input();
        self.commit_height_input();
    }

    pub fn show(
        &mut self,
        ctx: &egui::Context,
        secondary_color: [f32; 4],
    ) -> DialogResult<(u32, u32, (u32, u32), Rgba<u8>)> {
        let mut result = DialogResult::Open;
        let colors = DialogColors::from_ctx(ctx);
        let mut ok_pressed = false;

        egui::Window::new("dialog_resize_canvas")
            .title_bar(false)
            .collapsible(false)
            .resizable(false)
            .default_pos(egui::pos2(
                ctx.content_rect().center().x - 170.0,
                ctx.content_rect().center().y - 180.0,
            ))
            .show(ctx, |ui| {
                ui.set_min_width(340.0);

                paint_dialog_header(ui, &colors, "\u{1F532}", &t!("dialog.resize_canvas"));
                ui.add_space(4.0);

                // -- Dimensions section --
                section_label(ui, &colors, &t!("dialog.resize_canvas.canvas_size"));

                egui::Grid::new("resize_canvas_dims")
                    .num_columns(3)
                    .spacing([8.0, 4.0])
                    .show(ui, |ui| {
                        ui.label(t!("dialog.resize_image.width"));
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 4.0;
                            let previous_input = self.width_input.clone();
                            let width_response = ui.add(
                                egui::TextEdit::singleline(&mut self.width_input)
                                    .desired_width(96.0),
                            );
                            if self.focus_width_on_open {
                                width_response.request_focus();
                                self.focus_width_on_open = false;
                            }
                            if self.replace_width_on_first_edit && width_response.changed() {
                                if self.width_input.starts_with(&previous_input)
                                    && self.width_input.len() > previous_input.len()
                                {
                                    let suffix = &self.width_input[previous_input.len()..];
                                    let suffix_trimmed = suffix.trim_start();
                                    if !matches!(
                                        suffix_trimmed.chars().next(),
                                        Some('+') | Some('-') | Some('*') | Some('/')
                                    ) {
                                        self.width_input = suffix.to_string();
                                    }
                                }
                                self.replace_width_on_first_edit = false;
                            }
                            let width_commit = width_response.lost_focus()
                                || (width_response.has_focus()
                                    && ui.input(|i| i.key_pressed(egui::Key::Tab)));
                            if width_commit {
                                self.commit_width_input();
                            }
                            ui.label("px");
                        });
                        ui.end_row();

                        ui.label(t!("dialog.resize_image.height"));
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 4.0;
                            let height_response = ui.add(
                                egui::TextEdit::singleline(&mut self.height_input)
                                    .desired_width(96.0),
                            );
                            let height_commit = height_response.lost_focus()
                                || (height_response.has_focus()
                                    && ui.input(|i| i.key_pressed(egui::Key::Tab)));
                            if height_commit {
                                self.commit_height_input();
                            }
                            ui.label("px");
                        });
                        ui.end_row();

                        ui.label("");
                        let lock_resp = ui
                            .checkbox(&mut self.lock_aspect, t!("dialog.resize_image.lock_aspect"));
                        if lock_resp.changed() && self.lock_aspect {
                            self.aspect_ratio = self.width / self.height.max(1.0);
                        }
                        ui.label("");
                        ui.end_row();

                        ui.label(t!("dialog.resize_image.scale"));
                        let s_changed = numeric_field_with_buttons(
                            ui,
                            &mut self.scale_percent,
                            0.5,
                            1.0..=10000.0,
                            "%",
                            5.0,
                        );
                        ui.label("");
                        ui.end_row();

                        if s_changed {
                            self.width =
                                (self.original_w as f32 * self.scale_percent / 100.0).round();
                            self.height =
                                (self.original_h as f32 * self.scale_percent / 100.0).round();
                            self.aspect_ratio = self.width / self.height.max(1.0);
                            self.sync_inputs_from_values();
                        }
                    });

                // -- Anchor section --
                accent_separator(ui, &colors);
                section_label(ui, &colors, &t!("dialog.resize_canvas.anchor_position"));

                ui.add_space(2.0);

                // Visual anchor grid with canvas preview
                let new_w = self.width.round() as u32;
                let new_h = self.height.round() as u32;
                self.draw_anchor_grid(ui, &colors, new_w, new_h);

                ui.add_space(4.0);

                // -- Fill section --
                accent_separator(ui, &colors);
                section_label(ui, &colors, &t!("dialog.resize_canvas.fill"));
                ui.horizontal(|ui| {
                    ui.add_space(4.0);
                    ui.checkbox(
                        &mut self.fill_transparent,
                        t!("dialog.resize_canvas.fill_transparent"),
                    );
                });
                if !self.fill_transparent {
                    ui.horizontal(|ui| {
                        ui.add_space(4.0);
                        ui.label(
                            egui::RichText::new(t!("dialog.resize_canvas.uses_secondary_color"))
                                .size(11.0)
                                .color(colors.text_muted),
                        );
                    });
                }

                // -- Info bar --
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.add_space(4.0);
                    let dw = new_w as i32 - self.original_w as i32;
                    let dh = new_h as i32 - self.original_h as i32;
                    let sign_w = if dw >= 0 { "+" } else { "" };
                    let sign_h = if dh >= 0 { "+" } else { "" };
                    let info = format!(
                        "{}x{} \u{2192} {}x{}  ({}{}px, {}{}px)",
                        self.original_w, self.original_h, new_w, new_h, sign_w, dw, sign_h, dh,
                    );
                    ui.label(
                        egui::RichText::new(info)
                            .size(11.0)
                            .color(colors.text_muted),
                    );
                });

                // -- Footer --
                let (ok, cancel) = dialog_footer(ui, &colors);
                if ok {
                    ok_pressed = true;
                }
                if cancel {
                    result = DialogResult::Cancel;
                }
            });

        // Keyboard shortcuts
        if matches!(result, DialogResult::Open) {
            if ctx.input(|i| i.key_pressed(egui::Key::Enter)) {
                self.commit_inputs();
                ok_pressed = true;
            }
            if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
                result = DialogResult::Cancel;
            }
        }
        if ok_pressed && matches!(result, DialogResult::Open) {
            let w = (self.width.round() as u32).max(1);
            let h = (self.height.round() as u32).max(1);
            let fill = if self.fill_transparent {
                Rgba([0, 0, 0, 0])
            } else {
                Rgba([
                    (secondary_color[0] * 255.0) as u8,
                    (secondary_color[1] * 255.0) as u8,
                    (secondary_color[2] * 255.0) as u8,
                    (secondary_color[3] * 255.0) as u8,
                ])
            };
            result = DialogResult::Ok((w, h, self.anchor, fill));
        }

        result
    }

    /// Draw an interactive 3x3 anchor grid with a mini canvas preview.
    fn draw_anchor_grid(
        &mut self,
        ui: &mut egui::Ui,
        colors: &DialogColors,
        new_w: u32,
        new_h: u32,
    ) {
        let grid_size = 120.0f32;
        let cell_size = grid_size / 3.0;

        // Center the grid
        ui.horizontal(|ui| {
            let avail = ui.available_width();
            let pad = ((avail - grid_size) / 2.0).max(0.0);
            ui.add_space(pad);

            let (grid_rect, response) =
                ui.allocate_exact_size(Vec2::splat(grid_size), Sense::click());
            let painter = ui.painter();

            // Background
            let grid_bg = if colors.is_dark {
                Color32::from_gray(25)
            } else {
                Color32::from_gray(245)
            };
            painter.rect_filled(grid_rect, 4.0, grid_bg);
            painter.rect_stroke(
                grid_rect,
                4.0,
                Stroke::new(1.0, colors.separator),
                egui::StrokeKind::Middle,
            );

            // Compute canvas preview rect (showing where the original sits in the new canvas)
            let max_dim = (new_w as f32).max(new_h as f32).max(1.0);
            let preview_w = (new_w as f32 / max_dim) * (grid_size - 8.0);
            let preview_h = (new_h as f32 / max_dim) * (grid_size - 8.0);
            let offset_x = (grid_size - preview_w) / 2.0;
            let offset_y = (grid_size - preview_h) / 2.0;

            let canvas_w = (self.original_w as f32 / max_dim) * (grid_size - 8.0);
            let canvas_h = (self.original_h as f32 / max_dim) * (grid_size - 8.0);

            // Where canvas sits based on anchor
            let cx = match self.anchor.0 {
                0 => grid_rect.min.x + offset_x + 4.0,
                2 => grid_rect.min.x + offset_x + 4.0 + (preview_w - canvas_w),
                _ => grid_rect.min.x + offset_x + 4.0 + (preview_w - canvas_w) / 2.0,
            };
            let cy = match self.anchor.1 {
                0 => grid_rect.min.y + offset_y + 4.0,
                2 => grid_rect.min.y + offset_y + 4.0 + (preview_h - canvas_h),
                _ => grid_rect.min.y + offset_y + 4.0 + (preview_h - canvas_h) / 2.0,
            };

            // Draw new canvas area (faint)
            let new_canvas_rect = Rect::from_min_size(
                Pos2::new(
                    grid_rect.min.x + offset_x + 4.0,
                    grid_rect.min.y + offset_y + 4.0,
                ),
                Vec2::new(preview_w, preview_h),
            );
            let new_area_color = if colors.is_dark {
                Color32::from_gray(40)
            } else {
                Color32::from_gray(225)
            };
            painter.rect_filled(new_canvas_rect, 2.0, new_area_color);

            // Draw original canvas position (accent colored)
            let orig_rect = Rect::from_min_size(Pos2::new(cx, cy), Vec2::new(canvas_w, canvas_h));
            painter.rect_filled(orig_rect, 1.0, colors.accent_faint);
            painter.rect_stroke(
                orig_rect,
                1.0,
                Stroke::new(1.5, colors.accent),
                egui::StrokeKind::Middle,
            );

            // Handle click on the grid to set anchor
            if response.clicked()
                && let Some(pos) = response.interact_pointer_pos()
            {
                let local_x = pos.x - grid_rect.min.x;
                let local_y = pos.y - grid_rect.min.y;
                let col = ((local_x / cell_size).floor() as u32).min(2);
                let row = ((local_y / cell_size).floor() as u32).min(2);
                self.anchor = (col, row);
            }

            // Draw anchor dots aligned to the corners/edges of the new-canvas preview rect.
            // Previously these used uniform cell-centre positions which did not match the
            // accent-bordered original-canvas rect when the canvas was non-square.
            let dot_xs = [
                new_canvas_rect.min.x,
                new_canvas_rect.center().x,
                new_canvas_rect.max.x,
            ];
            let dot_ys = [
                new_canvas_rect.min.y,
                new_canvas_rect.center().y,
                new_canvas_rect.max.y,
            ];
            for row in 0..3u32 {
                for col in 0..3u32 {
                    let center = Pos2::new(dot_xs[col as usize], dot_ys[row as usize]);
                    let selected = self.anchor == (col, row);

                    let dot_radius = if selected { 6.0 } else { 3.5 };
                    let dot_color = if selected {
                        colors.accent
                    } else {
                        colors.text_muted
                    };

                    if selected {
                        // Ring around selected dot
                        painter.circle_stroke(
                            center,
                            dot_radius + 2.0,
                            Stroke::new(1.5, colors.accent),
                        );
                    }
                    painter.circle_filled(center, dot_radius, dot_color);
                }
            }
        });
    }
}

// ============================================================================
// GAUSSIAN BLUR DIALOG - with live preview
// ============================================================================

pub struct GaussianBlurDialog {
    pub sigma: f32,
    /// Snapshot of the original layer pixels before any preview blur.
    pub original_pixels: Option<TiledImage>,
    /// Pre-flattened original pixels (avoids re-flattening every frame).
    pub original_flat: Option<image::RgbaImage>,
    /// The sigma value currently applied to the preview (-1 = none yet).
    pub applied_sigma: f32,
    /// Layer index being blurred.
    pub layer_idx: usize,
    /// Whether live preview is enabled (vs manual preview button).
    pub live_preview: bool,
    /// Advanced mode: unlocks higher radius values (up to 100).
    pub advanced_blur: bool,
    /// Slider currently being dragged.
    pub dragging: bool,
}

impl GaussianBlurDialog {
    pub fn new(state: &CanvasState) -> Self {
        let idx = state.active_layer_index;
        let original = state.layers.get(idx).map(|l| l.pixels.clone());
        let flat = state.layers.get(idx).map(|l| l.pixels.to_rgba_image());
        Self {
            sigma: 0.0,
            original_pixels: original,
            original_flat: flat,
            applied_sigma: -1.0,
            layer_idx: idx,
            live_preview: true,
            advanced_blur: false,
            dragging: false,
        }
    }

    pub fn show(&mut self, ctx: &egui::Context) -> DialogResult<f32> {
        let mut result = DialogResult::Open;
        let colors = DialogColors::from_ctx(ctx);

        egui::Window::new("dialog_gaussian_blur")
            .title_bar(false)
            .collapsible(false)
            .resizable(false)
            .default_pos(egui::pos2(ctx.content_rect().center().x - 175.0, 60.0))
            .show(ctx, |ui| {
                ui.set_min_width(350.0);

                paint_dialog_header(ui, &colors, "\u{1F4A7}", &t!("dialog.gaussian_blur"));
                ui.add_space(4.0);

                // -- Parameters --
                section_label(ui, &colors, "BLUR SETTINGS");

                let mut sigma_changed = false;

                let slider_max = if self.advanced_blur { 100.0 } else { 10.0 };

                egui::Grid::new("blur_params")
                    .num_columns(2)
                    .spacing([8.0, 6.0])
                    .show(ui, |ui| {
                        ui.label("Radius");
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 4.0;
                            if self.advanced_blur {
                                // Advanced: editable DragValue (up to 100)
                                let r = ui.add(
                                    egui::DragValue::new(&mut self.sigma)
                                        .speed(0.2)
                                        .range(0.1..=100.0)
                                        .max_decimals(1),
                                );
                                if track_slider(&r, &mut self.dragging) {
                                    sigma_changed = true;
                                }
                            } else {
                                // Normal: slider capped at 10
                                if dialog_slider(ui, &mut self.sigma, 0.1..=slider_max, 0.1, "", 1)
                                {
                                    sigma_changed = true;
                                }
                            }
                        });
                        ui.end_row();

                        // Quick presets
                        ui.label("Quick");
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 4.0;
                            let presets: &[(&str, f32)] = if self.advanced_blur {
                                &[
                                    ("Subtle", 1.0),
                                    ("Light", 3.0),
                                    ("Medium", 8.0),
                                    ("Heavy", 25.0),
                                    ("Max", 80.0),
                                ]
                            } else {
                                &[
                                    ("Subtle", 0.5),
                                    ("Light", 1.5),
                                    ("Medium", 3.0),
                                    ("Strong", 6.0),
                                    ("Max", 10.0),
                                ]
                            };
                            for &(label, val) in presets {
                                let is_close = (self.sigma - val).abs() < 0.3;
                                let btn = if is_close {
                                    egui::Button::new(
                                        egui::RichText::new(label).strong().size(11.0),
                                    )
                                    .fill(colors.accent_faint)
                                } else {
                                    egui::Button::new(egui::RichText::new(label).size(11.0))
                                };
                                if ui.add(btn).clicked() {
                                    self.sigma = val;
                                    sigma_changed = true;
                                }
                            }
                        });
                        ui.end_row();

                        // Advanced blur toggle
                        ui.label("");
                        ui.horizontal(|ui| {
                            if ui
                                .checkbox(&mut self.advanced_blur, "Advanced (up to 100)")
                                .changed()
                            {
                                // Clamp sigma when switching back to normal mode
                                if !self.advanced_blur && self.sigma > 10.0 {
                                    self.sigma = 10.0;
                                    sigma_changed = true;
                                }
                            }
                        });
                        ui.end_row();
                    });

                // -- Preview controls --
                accent_separator(ui, &colors);
                let manual_preview = preview_controls(ui, &colors, &mut self.live_preview);

                if sigma_changed && self.live_preview {
                    result = DialogResult::Changed;
                }
                if manual_preview {
                    result = DialogResult::Changed;
                }

                // -- Footer --
                let (ok, cancel) = dialog_footer(ui, &colors);
                if ok {
                    result = DialogResult::Ok(self.sigma);
                }
                if cancel {
                    result = DialogResult::Cancel;
                }
            });

        result
    }
}

// ============================================================================
// LAYER TRANSFORM DIALOG - with interactive rotation gizmo
// ============================================================================

