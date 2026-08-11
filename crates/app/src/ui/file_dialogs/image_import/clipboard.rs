use super::*;

pub(crate) fn paste_clipboard_image(app: &mut PlotxApp) {
    #[cfg(windows)]
    let file_list_error = {
        match crate::ui::clipboard_native::get_file_list() {
            Ok(paths) if !paths.is_empty() => {
                enqueue(
                    app,
                    ImportImageRequest {
                        paths,
                        payloads: Vec::new(),
                        target: ImportImageTarget::NewPages,
                        allow_first_frame: false,
                        strip_metadata: false,
                        import_all_tiff_pages: false,
                    },
                );
                return;
            }
            Ok(_) => None,
            Err(error) => Some(error.to_string()),
        }
    };
    #[cfg(not(windows))]
    let file_list_error: Option<String> = None;
    let image = match read_clipboard_pixels() {
        Ok(Some(image)) => image,
        Ok(None) if file_list_error.is_none() => return,
        Ok(None) => {
            let Some(file_list_error) = file_list_error else {
                return;
            };
            let operation = app.session.begin_operation();
            record_failures(
                app,
                operation,
                0,
                vec![failure(
                    "Clipboard image",
                    "unknown",
                    "clipboard",
                    file_list_error,
                    "Copy image files or bitmap pixels, then choose Paste Image again.",
                )],
            );
            return;
        }
        Err(image_error) => {
            let reason = match file_list_error {
                Some(file_error) => format!(
                    "file-list read failed: {file_error}; pixel-image read failed: {image_error}"
                ),
                None => image_error,
            };
            let operation = app.session.begin_operation();
            record_failures(
                app,
                operation,
                0,
                vec![failure(
                    "Clipboard image",
                    "unknown",
                    "clipboard",
                    reason,
                    "Copy image files or bitmap pixels, then choose Paste Image again.",
                )],
            );
            return;
        }
    };
    let Some(buffer) = image::RgbaImage::from_raw(image.width, image.height, image.rgba) else {
        let operation = app.session.begin_operation();
        record_failures(
            app,
            operation,
            0,
            vec![failure(
                "Clipboard image",
                "RGBA",
                "clipboard_decode",
                "clipboard pixel dimensions did not match the returned byte count".to_owned(),
                "Copy the image again and retry.",
            )],
        );
        return;
    };
    let mut cursor = std::io::Cursor::new(Vec::new());
    if let Err(error) =
        image::DynamicImage::ImageRgba8(buffer).write_to(&mut cursor, image::ImageFormat::Png)
    {
        let operation = app.session.begin_operation();
        record_failures(
            app,
            operation,
            0,
            vec![failure(
                "Clipboard image",
                "RGBA",
                "encode",
                error.to_string(),
                "Copy the image again and retry.",
            )],
        );
        return;
    }
    let target = app
        .session
        .active_canvas
        .filter(|index| *index < app.doc.canvases.len())
        .map_or(ImportImageTarget::NewPages, |ci| {
            let canvas = app.doc.canvases[ci].resource_id;
            app.session
                .ui
                .hierarchical_selection
                .lead()
                .and_then(|path| (path.canvas == canvas).then_some(path.panel).flatten())
                .map_or(
                    ImportImageTarget::Canvas {
                        canvas,
                        position: None,
                    },
                    |panel| ImportImageTarget::Panel {
                        canvas,
                        panel,
                        position: None,
                    },
                )
        });
    enqueue(
        app,
        ImportImageRequest {
            paths: Vec::new(),
            payloads: vec![("Pasted image.png".to_owned(), cursor.into_inner())],
            target,
            allow_first_frame: false,
            strip_metadata: false,
            import_all_tiff_pages: false,
        },
    );
}

struct ClipboardPixels {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
}

fn read_clipboard_pixels() -> Result<Option<ClipboardPixels>, String> {
    let arboard_error =
        match arboard::Clipboard::new().and_then(|mut clipboard| clipboard.get_image()) {
            Ok(image) => {
                let width = u32::try_from(image.width)
                    .map_err(|_| "clipboard image width exceeds the supported range".to_owned())?;
                let height = u32::try_from(image.height)
                    .map_err(|_| "clipboard image height exceeds the supported range".to_owned())?;
                return Ok(Some(ClipboardPixels {
                    width,
                    height,
                    rgba: image.bytes.into_owned(),
                }));
            }
            Err(error) => arboard_image_error(error),
        };

    #[cfg(windows)]
    {
        match crate::ui::clipboard_native::get_dib_image() {
            Ok(Some(image)) => Ok(Some(ClipboardPixels {
                width: image.width(),
                height: image.height(),
                rgba: image.into_raw(),
            })),
            Ok(None) => arboard_error.map_or(Ok(None), |error| {
                Err(format!(
                    "arboard returned {error}; no native DIB format was available"
                ))
            }),
            Err(error) => Err(format!(
                "arboard returned {}; native DIB read failed: {error}",
                arboard_error.as_deref().unwrap_or("no image content")
            )),
        }
    }
    #[cfg(not(windows))]
    {
        arboard_error.map_or(Ok(None), Err)
    }
}

fn arboard_image_error(error: arboard::Error) -> Option<String> {
    match error {
        arboard::Error::ContentNotAvailable => None,
        error => Some(error.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::arboard_image_error;

    #[test]
    fn unavailable_clipboard_image_content_is_not_a_read_failure() {
        assert!(arboard_image_error(arboard::Error::ContentNotAvailable).is_none());
        assert!(arboard_image_error(arboard::Error::ClipboardOccupied).is_some());
    }
}
