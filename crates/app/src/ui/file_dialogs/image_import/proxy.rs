use super::*;

struct ProxyRebuild {
    hash: [u8; 32],
    page_index: u32,
    format: String,
    receiver: mpsc::Receiver<Result<plotx_io::image::ProxyImage, String>>,
}

#[derive(Default)]
struct ProxyRebuilds {
    active: Option<ProxyRebuild>,
    failed: std::collections::BTreeSet<([u8; 32], u32)>,
}

thread_local! {
    static REBUILDS: RefCell<ProxyRebuilds> = RefCell::new(ProxyRebuilds::default());
}

pub(super) fn poll_rebuilds(app: &mut PlotxApp, ctx: &egui::Context) {
    REBUILDS.with(|rebuilds| {
        let mut rebuilds = rebuilds.borrow_mut();
        if let Some(active) = &rebuilds.active {
            match active.receiver.try_recv() {
                Ok(Ok(proxy)) => {
                    let hash = active.hash;
                    let page_index = active.page_index;
                    insert_proxy(app, hash, page_index, proxy);
                    rebuilds.active = None;
                }
                Ok(Err(reason)) => {
                    let hash = active.hash;
                    let page_index = active.page_index;
                    let format = active.format.clone();
                    rebuilds.failed.insert((hash, page_index));
                    rebuilds.active = None;
                    let operation = app.session.begin_operation();
                    record_failures(
                        app,
                        operation,
                        0,
                        vec![failure(
                            "Embedded image",
                            &format,
                            "proxy_rebuild",
                            reason,
                            "Replace the image with a source below the 500 megapixel hard limit.",
                        )],
                    );
                }
                Err(mpsc::TryRecvError::Empty) => {
                    ctx.request_repaint_after(std::time::Duration::from_millis(30));
                    return;
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    let hash = active.hash;
                    let page_index = active.page_index;
                    rebuilds.failed.insert((hash, page_index));
                    rebuilds.active = None;
                }
            }
        }
        let existing: std::collections::BTreeSet<_> = app
            .session
            .ui
            .raster_proxies
            .iter()
            .map(|proxy| (proxy.hash, proxy.page_index))
            .collect();
        let candidate = app
            .doc
            .canvases
            .iter()
            .flat_map(|canvas| &canvas.objects)
            .find_map(|item| {
                let CanvasObjectKind::RasterImage(image) = &item.kind else {
                    return None;
                };
                let asset = app.doc.assets.get(&image.asset)?;
                let key = (asset.sha256, image.page_index);
                (!existing.contains(&key) && !rebuilds.failed.contains(&key))
                    .then_some((asset, image.page_index))
            });
        let Some((asset, page_index)) = candidate else {
            return;
        };
        let hash = asset.sha256;
        let format = asset.format.clone();
        let bytes = asset.bytes.clone();
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            let result = rebuild_preview(&bytes, page_index).map_err(|error| error.to_string());
            let _ = sender.send(result);
        });
        rebuilds.active = Some(ProxyRebuild {
            hash,
            page_index,
            format,
            receiver,
        });
        ctx.request_repaint_after(std::time::Duration::from_millis(30));
    });
}

pub(super) fn insert_proxy(
    app: &mut PlotxApp,
    hash: [u8; 32],
    page_index: u32,
    proxy: plotx_io::image::ProxyImage,
) {
    const PROXY_BUDGET: usize = 128 * 1024 * 1024;
    app.session
        .ui
        .raster_proxies
        .retain(|entry| entry.hash != hash || entry.page_index != page_index);
    app.session
        .ui
        .raster_proxies
        .push(plotx_core::state::RasterProxy {
            hash,
            page_index,
            pixel_size: proxy.pixel_size,
            rgba8: Arc::from(proxy.rgba8),
        });
    while app
        .session
        .ui
        .raster_proxies
        .iter()
        .map(|entry| entry.rgba8.len())
        .sum::<usize>()
        > PROXY_BUDGET
        && app.session.ui.raster_proxies.len() > 1
    {
        app.session.ui.raster_proxies.remove(0);
    }
}

pub(super) fn rebuild_preview(
    bytes: &[u8],
    page_index: u32,
) -> Result<plotx_io::image::ProxyImage, plotx_io::image::ImageError> {
    if page_index == 0 {
        let edge = if plotx_io::image::probe(bytes)?.class
            == plotx_io::image::ResourceClass::ProxyRequired
        {
            2048
        } else {
            4096
        };
        return plotx_io::image::decode_proxy_rgba8(bytes, edge);
    }
    let decoded = plotx_io::image::decode_rgba8_page(bytes, page_index, false)?;
    let source =
        image::RgbaImage::from_raw(decoded.probe.width, decoded.probe.height, decoded.rgba8)
            .ok_or(plotx_io::image::ImageError::InvalidDecodedSize)?;
    let preview = image::DynamicImage::ImageRgba8(source)
        .thumbnail(4096, 4096)
        .into_rgba8();
    Ok(plotx_io::image::ProxyImage {
        pixel_size: [preview.width(), preview.height()],
        rgba8: preview.into_raw(),
    })
}
