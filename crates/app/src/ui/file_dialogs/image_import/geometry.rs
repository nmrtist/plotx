pub(super) fn initial_image_size(page_size: [f32; 2], aspect: f32) -> [f32; 2] {
    let aspect = if aspect.is_finite() && aspect > 0.0 {
        aspect
    } else {
        1.0
    };
    let bounds = [
        (page_size[0] * 0.65).max(12.0),
        (page_size[1] * 0.65).max(12.0),
    ];
    if bounds[0] / bounds[1] > aspect {
        [bounds[1] * aspect, bounds[1]]
    } else {
        [bounds[0], bounds[0] / aspect]
    }
}

pub(super) fn physical_image_size_mm(pixel_size: [u32; 2], dpi: Option<[f32; 2]>) -> [f32; 2] {
    const DEFAULT_DPI: f32 = 96.0;
    const MM_PER_INCH: f32 = 25.4;
    let source_dpi = dpi
        .map(|value| value[0])
        .filter(|value| value.is_finite() && *value > 0.0)
        .unwrap_or(DEFAULT_DPI);
    let aspect = pixel_size[0].max(1) as f32 / pixel_size[1].max(1) as f32;
    let natural_width = pixel_size[0].max(1) as f32 / source_dpi * MM_PER_INCH;
    let width = natural_width.min(plotx_core::state::NATURE_SINGLE_COLUMN.width_mm);
    [width, width / aspect]
}
