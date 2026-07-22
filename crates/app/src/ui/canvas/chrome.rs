use egui::{Color32, Stroke, Visuals};

/// Colours used by editor-only canvas chrome. Figure content and exports never
/// consume this table.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ChromeStyle {
    pub selection_fill: Color32,
    pub selection_stroke: Color32,
    pub selection_active: Color32,
    pub layout_grid: Color32,
    pub margin_guide: Color32,
    pub snap_guide: Color32,
    pub tile_existing_fill: Color32,
    pub tile_existing_stroke: Color32,
    pub tile_target_fill: Color32,
    pub tile_target_stroke: Color32,
    pub pivot: Color32,
    pub integral: Color32,
    pub peak: Color32,
}

impl ChromeStyle {
    pub fn from_visuals(visuals: &Visuals, accent: Option<[u8; 3]>) -> Self {
        let source = accent
            .map(|[r, g, b]| Color32::from_rgb(r, g, b))
            .unwrap_or(visuals.selection.bg_fill);
        let background = visuals.panel_fill;
        let normal = contrast_adjusted(source, background, 3.0);
        let weak = blend(normal, background, 0.42);
        let outline = contrast_adjusted(normal, background, 4.5);
        let alternate = contrast_adjusted(
            if visuals.dark_mode {
                Color32::from_rgb(0xff, 0x72, 0xb6)
            } else {
                Color32::from_rgb(0xa2, 0x00, 0x59)
            },
            background,
            3.0,
        );
        Self {
            selection_fill: with_alpha(normal, 32),
            selection_stroke: outline,
            selection_active: contrast_adjusted(
                Color32::from_rgb(0x1f, 0x9d, 0x74),
                background,
                3.0,
            ),
            layout_grid: weak,
            margin_guide: contrast_adjusted(blend(normal, background, 0.25), background, 2.0),
            snap_guide: alternate,
            tile_existing_fill: with_alpha(normal, 15),
            tile_existing_stroke: weak,
            tile_target_fill: with_alpha(normal, 26),
            tile_target_stroke: outline,
            pivot: contrast_adjusted(Color32::from_rgb(0xe0, 0x6c, 0x22), background, 3.0),
            integral: contrast_adjusted(Color32::from_rgb(0x2b, 0x6c, 0xb0), background, 3.0),
            peak: contrast_adjusted(Color32::from_rgb(0x8a, 0x1c, 0x1c), background, 3.0),
        }
    }

    pub fn tile_existing_stroke(self) -> Stroke {
        Stroke::new(1.0_f32, self.tile_existing_stroke)
    }

    pub fn tile_target_stroke(self) -> Stroke {
        Stroke::new(2.0_f32, self.tile_target_stroke)
    }
}

fn with_alpha(color: Color32, alpha: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), alpha)
}

fn blend(foreground: Color32, background: Color32, amount: f32) -> Color32 {
    let mix = |a: u8, b: u8| (a as f32 * (1.0 - amount) + b as f32 * amount).round() as u8;
    Color32::from_rgb(
        mix(foreground.r(), background.r()),
        mix(foreground.g(), background.g()),
        mix(foreground.b(), background.b()),
    )
}

fn contrast_adjusted(mut color: Color32, background: Color32, minimum: f32) -> Color32 {
    let target = if relative_luminance(background) > 0.45 {
        Color32::BLACK
    } else {
        Color32::WHITE
    };
    for _ in 0..12 {
        if contrast_ratio(color, background) >= minimum {
            break;
        }
        color = blend(color, target, 0.12);
    }
    color
}

fn contrast_ratio(a: Color32, b: Color32) -> f32 {
    let (lighter, darker) = if relative_luminance(a) >= relative_luminance(b) {
        (relative_luminance(a), relative_luminance(b))
    } else {
        (relative_luminance(b), relative_luminance(a))
    };
    (lighter + 0.05) / (darker + 0.05)
}

fn relative_luminance(color: Color32) -> f32 {
    let channel = |value: u8| {
        let value = value as f32 / 255.0;
        if value <= 0.04045 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * channel(color.r()) + 0.7152 * channel(color.g()) + 0.0722 * channel(color.b())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn translucent_roles_keep_unmultiplied_rgb() {
        let style = ChromeStyle::from_visuals(&Visuals::light(), Some([120, 80, 40]));
        let [r, g, b, alpha] = style.tile_existing_fill.to_srgba_unmultiplied();
        assert_eq!(alpha, 15);
        assert_eq!(style.tile_target_fill.to_srgba_unmultiplied()[3], 26);
        assert_eq!(
            style.tile_existing_fill,
            Color32::from_rgba_unmultiplied(r, g, b, alpha)
        );
        assert_ne!(
            style.tile_existing_fill,
            Color32::from_rgba_premultiplied(r, g, b, alpha)
        );
    }
}
