impl PaintFEApp {
    fn process_brush_tip_dialog(&mut self, ctx: &egui::Context, dialog: &mut ActiveDialog) -> bool {
        if !matches!(dialog, ActiveDialog::AddBrushTip(_)) {
            return false;
        }

        match dialog {
            ActiveDialog::AddBrushTip(dlg) => {
                match dlg.show(ctx) {
                    Some(result) => {
                        // Load the brush tip into assets
                        self.assets.load_brush_tip(
                            ctx,
                            &result.name,
                            &result.category,
                            &result.png_data,
                        );
                        // Persist to settings (base64-encoded)
                        use base64::Engine;
                        let b64 = base64::engine::general_purpose::STANDARD.encode(&result.png_data);
                        self.settings.custom_brush_tips.push((
                            result.name.clone(),
                            result.category.clone(),
                            b64,
                        ));
                        self.settings.save();
                        // If the current brush tip was circle, switch to the new one
                        if self.tools_panel.properties.brush_tip.is_circle() {
                            self.tools_panel.properties.brush_tip =
                                crate::components::tools::BrushTip::Image(result.name.clone());
                        }
                        self.active_dialog = ActiveDialog::None;
                        true
                    }
                    None => {
                        // Dialog still open
                        false
                    }
                }
            }
            _ => false,
        }
    }
}
