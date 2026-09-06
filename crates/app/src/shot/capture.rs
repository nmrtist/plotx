use std::path::Path;

pub(super) fn save_png(path: &Path, image: &egui::ColorImage) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("create {}: {error}", parent.display()))?;
    }
    let [width, height] = image.size;
    // egui screenshots are opaque RGBA8, so straight-alpha encoding is exact.
    image::save_buffer_with_format(
        path,
        image.as_raw(),
        width as u32,
        height as u32,
        image::ColorType::Rgba8,
        image::ImageFormat::Png,
    )
    .map_err(|error| format!("encode {}: {error}", path.display()))
}
