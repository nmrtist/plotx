use super::*;

pub(crate) fn import_images(app: &mut PlotxApp) {
    pick_images(app, false, false, false);
}

pub(crate) fn import_images_first_frame(app: &mut PlotxApp) {
    pick_images(app, true, false, false);
}

pub(crate) fn import_images_without_metadata(app: &mut PlotxApp) {
    pick_images(app, false, true, false);
}

pub(crate) fn import_tiff_pages(app: &mut PlotxApp) {
    pick_images(app, false, false, true);
}

pub(crate) fn replace_selected_image(app: &mut PlotxApp) {
    let Some(ci) = app.session.active_canvas else {
        return;
    };
    let Some(content) = app
        .session
        .ui
        .hierarchical_selection
        .lead()
        .and_then(|path| path.content)
    else {
        return;
    };
    let Some(path) = rfd::FileDialog::new()
        .add_filter(
            "Images",
            &["png", "jpg", "jpeg", "tif", "tiff", "webp", "bmp"],
        )
        .set_title("Replace image")
        .pick_file()
    else {
        return;
    };
    enqueue(
        app,
        ImportImageRequest {
            paths: vec![path],
            payloads: Vec::new(),
            target: ImportImageTarget::Replace {
                canvas: app.doc.canvases[ci].resource_id,
                content,
            },
            allow_first_frame: false,
            strip_metadata: false,
            import_all_tiff_pages: false,
        },
    );
}

fn pick_images(
    app: &mut PlotxApp,
    allow_first_frame: bool,
    strip_metadata: bool,
    import_all_tiff_pages: bool,
) {
    let Some(paths) = rfd::FileDialog::new()
        .add_filter(
            "Images (*.png, *.jpg, *.jpeg, *.tif, *.tiff, *.webp, *.bmp)",
            &["png", "jpg", "jpeg", "tif", "tiff", "webp", "bmp"],
        )
        .add_filter("All files", &["*"])
        .set_title("Add images to the figure")
        .pick_files()
    else {
        return;
    };
    import_image_paths_with_options(
        app,
        &paths,
        allow_first_frame,
        strip_metadata,
        import_all_tiff_pages,
    );
}

pub(crate) fn import_image_paths(app: &mut PlotxApp, paths: &[PathBuf]) {
    import_image_paths_with_options(app, paths, false, false, false);
}

fn import_image_paths_with_options(
    app: &mut PlotxApp,
    paths: &[PathBuf],
    allow_first_frame: bool,
    strip_metadata: bool,
    import_all_tiff_pages: bool,
) {
    let ci = app
        .session
        .active_canvas
        .filter(|index| *index < app.doc.canvases.len());
    let target = ci.map_or(ImportImageTarget::NewPages, |ci| {
        let canvas = app.doc.canvases[ci].resource_id;
        match app
            .session
            .ui
            .hierarchical_selection
            .lead()
            .and_then(|path| (path.canvas == canvas).then_some(path.panel).flatten())
        {
            Some(panel) => ImportImageTarget::Panel {
                canvas,
                panel,
                position: None,
            },
            None => ImportImageTarget::Canvas {
                canvas,
                position: None,
            },
        }
    });
    enqueue(
        app,
        ImportImageRequest {
            paths: paths.to_vec(),
            payloads: Vec::new(),
            target,
            allow_first_frame,
            strip_metadata,
            import_all_tiff_pages,
        },
    );
}
