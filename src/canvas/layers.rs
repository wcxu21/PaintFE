#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum BlendMode {
    #[default]
    Normal,
    Multiply,
    Screen,
    Additive,
    Reflect,
    Glow,
    ColorBurn,
    ColorDodge,
    Overlay,
    Difference,
    Negation,
    Lighten,
    Darken,
    Xor,
    Overwrite,
    HardLight,
    SoftLight,
    Exclusion,
    Subtract,
    Divide,
    LinearBurn,
    VividLight,
    LinearLight,
    PinLight,
    HardMix,
}

impl BlendMode {
    /// Returns all blend modes for UI display
    pub fn all() -> &'static [BlendMode] {
        &[
            BlendMode::Normal,
            BlendMode::Multiply,
            BlendMode::Screen,
            BlendMode::Additive,
            BlendMode::Overlay,
            BlendMode::HardLight,
            BlendMode::SoftLight,
            BlendMode::Lighten,
            BlendMode::Darken,
            BlendMode::ColorBurn,
            BlendMode::ColorDodge,
            BlendMode::Difference,
            BlendMode::Exclusion,
            BlendMode::Negation,
            BlendMode::Reflect,
            BlendMode::Glow,
            BlendMode::Subtract,
            BlendMode::Divide,
            BlendMode::LinearBurn,
            BlendMode::VividLight,
            BlendMode::LinearLight,
            BlendMode::PinLight,
            BlendMode::HardMix,
            BlendMode::Xor,
            BlendMode::Overwrite,
        ]
    }

    pub fn name(&self) -> &'static str {
        match self {
            BlendMode::Normal => "Normal",
            BlendMode::Multiply => "Multiply",
            BlendMode::Screen => "Screen",
            BlendMode::Additive => "Additive",
            BlendMode::Reflect => "Reflect",
            BlendMode::Glow => "Glow",
            BlendMode::ColorBurn => "Color Burn",
            BlendMode::ColorDodge => "Color Dodge",
            BlendMode::Overlay => "Overlay",
            BlendMode::Difference => "Difference",
            BlendMode::Negation => "Negation",
            BlendMode::Lighten => "Lighten",
            BlendMode::Darken => "Darken",
            BlendMode::Xor => "Xor",
            BlendMode::Overwrite => "Overwrite",
            BlendMode::HardLight => "Hard Light",
            BlendMode::SoftLight => "Soft Light",
            BlendMode::Exclusion => "Exclusion",
            BlendMode::Subtract => "Subtract",
            BlendMode::Divide => "Divide",
            BlendMode::LinearBurn => "Linear Burn",
            BlendMode::VividLight => "Vivid Light",
            BlendMode::LinearLight => "Linear Light",
            BlendMode::PinLight => "Pin Light",
            BlendMode::HardMix => "Hard Mix",
        }
    }

    /// Returns the localized display name for UI rendering
    pub fn display_name(&self) -> String {
        match self {
            BlendMode::Normal => t!("blend.normal"),
            BlendMode::Multiply => t!("blend.multiply"),
            BlendMode::Screen => t!("blend.screen"),
            BlendMode::Additive => t!("blend.additive"),
            BlendMode::Reflect => t!("blend.reflect"),
            BlendMode::Glow => t!("blend.glow"),
            BlendMode::ColorBurn => t!("blend.color_burn"),
            BlendMode::ColorDodge => t!("blend.color_dodge"),
            BlendMode::Overlay => t!("blend.overlay"),
            BlendMode::Difference => t!("blend.difference"),
            BlendMode::Negation => t!("blend.negation"),
            BlendMode::Lighten => t!("blend.lighten"),
            BlendMode::Darken => t!("blend.darken"),
            BlendMode::Xor => t!("blend.xor"),
            BlendMode::Overwrite => t!("blend.overwrite"),
            BlendMode::HardLight => t!("blend.hard_light"),
            BlendMode::SoftLight => t!("blend.soft_light"),
            BlendMode::Exclusion => t!("blend.exclusion"),
            BlendMode::Subtract => t!("blend.subtract"),
            BlendMode::Divide => t!("blend.divide"),
            BlendMode::LinearBurn => t!("blend.linear_burn"),
            BlendMode::VividLight => t!("blend.vivid_light"),
            BlendMode::LinearLight => t!("blend.linear_light"),
            BlendMode::PinLight => t!("blend.pin_light"),
            BlendMode::HardMix => t!("blend.hard_mix"),
        }
    }

    /// Convert to a stable u8 for binary serialization
    pub fn to_u8(self) -> u8 {
        match self {
            BlendMode::Normal => 0,
            BlendMode::Multiply => 1,
            BlendMode::Screen => 2,
            BlendMode::Additive => 3,
            BlendMode::Reflect => 4,
            BlendMode::Glow => 5,
            BlendMode::ColorBurn => 6,
            BlendMode::ColorDodge => 7,
            BlendMode::Overlay => 8,
            BlendMode::Difference => 9,
            BlendMode::Negation => 10,
            BlendMode::Lighten => 11,
            BlendMode::Darken => 12,
            BlendMode::Xor => 13,
            BlendMode::Overwrite => 14,
            BlendMode::HardLight => 15,
            BlendMode::SoftLight => 16,
            BlendMode::Exclusion => 17,
            BlendMode::Subtract => 18,
            BlendMode::Divide => 19,
            BlendMode::LinearBurn => 20,
            BlendMode::VividLight => 21,
            BlendMode::LinearLight => 22,
            BlendMode::PinLight => 23,
            BlendMode::HardMix => 24,
        }
    }

    /// Reconstruct from a u8 (defaults to Normal for unknown values)
    pub fn from_u8(v: u8) -> Self {
        match v {
            0 => BlendMode::Normal,
            1 => BlendMode::Multiply,
            2 => BlendMode::Screen,
            3 => BlendMode::Additive,
            4 => BlendMode::Reflect,
            5 => BlendMode::Glow,
            6 => BlendMode::ColorBurn,
            7 => BlendMode::ColorDodge,
            8 => BlendMode::Overlay,
            9 => BlendMode::Difference,
            10 => BlendMode::Negation,
            11 => BlendMode::Lighten,
            12 => BlendMode::Darken,
            13 => BlendMode::Xor,
            14 => BlendMode::Overwrite,
            15 => BlendMode::HardLight,
            16 => BlendMode::SoftLight,
            17 => BlendMode::Exclusion,
            18 => BlendMode::Subtract,
            19 => BlendMode::Divide,
            20 => BlendMode::LinearBurn,
            21 => BlendMode::VividLight,
            22 => BlendMode::LinearLight,
            23 => BlendMode::PinLight,
            24 => BlendMode::HardMix,
            _ => BlendMode::Normal,
        }
    }
}

/// Discriminant for heterogeneous layer types.
#[derive(Clone, Debug, Default)]
pub enum LayerContent {
    /// Standard raster layer (current behaviour). Pixel data lives in `Layer::pixels`.
    #[default]
    Raster,
    /// Editable text layer. Vector data + cached rasterisation in `Layer::pixels`.
    Text(TextLayerData),
}

pub struct Layer {
    pub name: String,
    pub visible: bool,
    pub opacity: f32,
    pub blend_mode: BlendMode,
    pub pixels: TiledImage,
    /// Optional live (non-destructive) layer mask.
    /// We encode concealment in alpha: 0 = reveal, 255 = fully hidden.
    pub mask: Option<TiledImage>,
    /// Whether the live mask participates in compositing.
    pub mask_enabled: bool,
    /// Downscaled cache (max 1024px longest edge) for zoomed-out rendering.
    /// Not serialized — rebuilt on demand.
    pub lod_cache: Option<Arc<RgbaImage>>,
    /// Per-layer generation counter for GPU texture synchronisation.
    /// Bumped only when THIS layer's pixels are modified, so unchanged
    /// layers are never re-uploaded to the GPU.
    pub gpu_generation: u64,
    /// Layer type discriminant — `Raster` for normal layers, `Text(..)` for
    /// editable text layers. Default: `Raster`.
    pub content: LayerContent,
}

impl Layer {
    pub fn new(name: String, width: u32, height: u32, fill_color: Rgba<u8>) -> Self {
        let pixels = TiledImage::new_filled(width, height, fill_color);

        Self {
            name,
            visible: true,
            opacity: 1.0,
            blend_mode: BlendMode::Normal,
            pixels,
            mask: None,
            mask_enabled: true,
            lod_cache: None,
            gpu_generation: 0,
            content: LayerContent::Raster,
        }
    }

    /// Create a new text layer with default empty text data.
    pub fn new_text(name: String, width: u32, height: u32) -> Self {
        Self {
            name,
            visible: true,
            opacity: 1.0,
            blend_mode: BlendMode::Normal,
            pixels: TiledImage::new(width, height),
            mask: None,
            mask_enabled: true,
            lod_cache: None,
            gpu_generation: 0,
            content: LayerContent::Text(TextLayerData::default()),
        }
    }

    /// Returns true if this is a text layer.
    pub fn is_text_layer(&self) -> bool {
        matches!(self.content, LayerContent::Text(_))
    }

    /// Invalidate the LOD cache (call after any pixel modification).
    pub fn invalidate_lod(&mut self) {
        self.lod_cache = None;
    }

    pub fn has_live_mask(&self) -> bool {
        self.mask.is_some()
    }

    pub fn ensure_mask(&mut self) {
        if self.mask.is_none() {
            self.mask = Some(TiledImage::new(self.pixels.width(), self.pixels.height()));
            self.mask_enabled = true;
        }
    }

    #[inline]
    pub fn apply_mask_alpha_at(&self, x: u32, y: u32, src_alpha: u8) -> u8 {
        if !self.mask_enabled {
            return src_alpha;
        }
        let conceal = self
            .mask
            .as_ref()
            .map(|m| m.get_pixel(x, y)[3])
            .unwrap_or(0);
        if conceal == 0 {
            src_alpha
        } else {
            ((src_alpha as u32 * (255 - conceal as u32)) / 255) as u8
        }
    }

    /// Flatten this layer to RGBA, applying the live mask to alpha when enabled.
    pub fn to_masked_rgba_image(&self) -> RgbaImage {
        let mut flat = self.pixels.to_rgba_image();
        if !self.mask_enabled {
            return flat;
        }
        let Some(mask) = &self.mask else {
            return flat;
        };
        let w = flat.width().min(mask.width());
        let h = flat.height().min(mask.height());
        for y in 0..h {
            for x in 0..w {
                let conceal = mask.get_pixel(x, y)[3];
                if conceal == 0 {
                    continue;
                }
                let mut p = *flat.get_pixel(x, y);
                p[3] = ((p[3] as u32 * (255 - conceal as u32)) / 255) as u8;
                flat.put_pixel(x, y, p);
            }
        }
        flat
    }

    /// Return a reference to the downscaled LOD image, generating it lazily.
    /// The thumbnail is at most `LOD_MAX_EDGE` pixels on its longest side.
    pub fn get_lod_image(&mut self) -> Arc<RgbaImage> {
        if let Some(ref cached) = self.lod_cache {
            return Arc::clone(cached);
        }
        let (w, h) = (self.pixels.width(), self.pixels.height());
        let longest = w.max(h);
        let (nw, nh) = if longest <= LOD_MAX_EDGE {
            (w, h) // Already small enough
        } else {
            let scale = LOD_MAX_EDGE as f32 / longest as f32;
            (
                ((w as f32 * scale).round() as u32).max(1),
                ((h as f32 * scale).round() as u32).max(1),
            )
        };
        let flat = self.pixels.to_rgba_image();
        let thumb = image::imageops::resize(&flat, nw, nh, image::imageops::FilterType::Triangle);
        let arc = Arc::new(thumb);
        self.lod_cache = Some(Arc::clone(&arc));
        arc
    }
}

