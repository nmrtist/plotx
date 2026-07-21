//! "Copy figure": puts the selected frame (else the active canvas) on the OS
//! clipboard — multi-format on Windows, egui's image clipboard elsewhere.

use std::fmt;

use egui::Context;
use plotx_core::export::{RasterError, RasterImage, RasterOptions, rasterize_canvas};
use plotx_core::operation::{Diagnostic, DiagnosticCode, OperationKind, OperationReport, Severity};
use plotx_core::state::{FrameRef, PlotxApp};

pub(super) fn copy_figure_to_clipboard(app: &mut PlotxApp, ctx: &Context) {
    match resolve_copy_target(app) {
        Some(canvas_index) => copy_canvas_figure(app, ctx, canvas_index),
        None => {
            let operation_id = app.session.begin_operation();
            app.session.record_operation(OperationReport::<()>::failure(
                operation_id,
                OperationKind::ClipboardFigureCopy,
                "Nothing to copy; open a canvas first.",
                Diagnostic::new(
                    Severity::Error,
                    DiagnosticCode::ClipboardImageUnavailable,
                    "No frame is selected and no canvas is active.",
                )
                .with_source("app.clipboard_figure")
                .with_context("category", "no_target"),
            ));
        }
    }
}

/// Selected page frame wins over the active canvas, matching what the user
/// perceives as "the selected figure".
pub(super) fn resolve_copy_target(app: &PlotxApp) -> Option<usize> {
    app.session
        .ui
        .frame_selection
        .iter()
        .find_map(|frame| match frame {
            FrameRef::Page(ci) => Some(*ci),
            FrameRef::Sheet(_) => None,
        })
        .or(app.session.active_canvas)
        .filter(|ci| *ci < app.doc.canvases.len())
}

pub(super) fn copy_canvas_figure(app: &mut PlotxApp, ctx: &Context, canvas_index: usize) {
    let operation_id = app.session.begin_operation();
    let report = match build_payload(app, canvas_index) {
        Ok(payload) => publish(ctx, payload, operation_id),
        Err(error) => {
            let code = clipboard_error_code(&error);
            let summary = if code == DiagnosticCode::ClipboardImageUnavailable {
                "Nothing to copy; open a canvas first."
            } else {
                "Could not copy the figure."
            };
            OperationReport::<()>::failure(
                operation_id,
                OperationKind::ClipboardFigureCopy,
                summary,
                Diagnostic::new(
                    Severity::Error,
                    code,
                    "The figure could not be rendered for the clipboard.",
                )
                .with_source("app.clipboard_figure")
                .with_context("category", clipboard_error_category(&error))
                .with_context("error", error.to_string()),
            )
        }
    };
    app.session.record_operation(report);
}

struct FigurePayload {
    raster: RasterImage,
    dpi: u16,
    #[cfg(windows)]
    png: Vec<u8>,
    #[cfg(windows)]
    svg: String,
    #[cfg(windows)]
    emf: Result<Vec<u8>, String>,
}

fn build_payload(
    app: &PlotxApp,
    canvas_index: usize,
) -> Result<FigurePayload, ClipboardFigureError> {
    let canvas = app
        .doc
        .canvases
        .get(canvas_index)
        .ok_or(ClipboardFigureError::NoTarget)?;
    let dpi = plotx_core::settings::load().export.dpi;
    let raster =
        rasterize_canvas(canvas, RasterOptions::new(dpi)).map_err(ClipboardFigureError::Raster)?;
    #[cfg(windows)]
    {
        let png = raster
            .to_png()
            .map_err(|error| ClipboardFigureError::PngEncode(error.into()))?;
        let document = plotx_core::state::build_render_document(canvas);
        let svg = plotx_render::svg::export_document(&document);
        let emf =
            plotx_render::emf::export_document_emf(&document).map_err(|error| error.to_string());
        Ok(FigurePayload {
            raster,
            dpi,
            png,
            svg,
            emf,
        })
    }
    #[cfg(not(windows))]
    Ok(FigurePayload { raster, dpi })
}

#[cfg(windows)]
fn publish(
    _ctx: &Context,
    payload: FigurePayload,
    operation_id: plotx_core::operation::OperationId,
) -> OperationReport<()> {
    use super::clipboard_native;

    let (width, height) = (payload.raster.width(), payload.raster.height());
    let dibv5 = clipboard_native::build_dibv5(width, height, payload.raster.rgba(), payload.dpi);
    let outcomes = match clipboard_native::set_clipboard_formats(
        &dibv5,
        &payload.png,
        &payload.svg,
        payload.emf.as_deref().ok(),
    ) {
        Ok(outcomes) => outcomes,
        Err(error) => {
            return OperationReport::<()>::failure(
                operation_id,
                OperationKind::ClipboardFigureCopy,
                "Could not copy the figure.",
                Diagnostic::new(
                    Severity::Error,
                    DiagnosticCode::ClipboardImageFailed,
                    "The system clipboard could not be opened.",
                )
                .with_source("app.clipboard_figure")
                .with_context("category", "clipboard_open")
                .with_context("error", error.to_string()),
            );
        }
    };

    let committed: Vec<&str> = outcomes.iter().filter(|o| o.ok).map(|o| o.name).collect();
    let mut failed: Vec<String> = outcomes
        .iter()
        .filter(|o| !o.ok)
        .map(|o| format!("{}: {}", o.name, o.error.as_deref().unwrap_or("failed")))
        .collect();
    if let Err(error) = &payload.emf {
        failed.push(format!("emf: {error}"));
    }
    let formats = committed.join(",");

    if committed.is_empty() {
        OperationReport::<()>::failure(
            operation_id,
            OperationKind::ClipboardFigureCopy,
            "Could not copy the figure.",
            Diagnostic::new(
                Severity::Error,
                DiagnosticCode::ClipboardImageFailed,
                "No clipboard format could be committed.",
            )
            .with_source("app.clipboard_figure")
            .with_context("category", "no_format_committed")
            .with_context("error", failed.join("; ")),
        )
    } else if failed.is_empty() {
        OperationReport::success(
            operation_id,
            OperationKind::ClipboardFigureCopy,
            format!(
                "Copied figure to the clipboard ({width}x{height} px, {} dpi; bitmap + vector).",
                payload.dpi
            ),
            (),
        )
        .with_diagnostic(
            Diagnostic::new(
                Severity::Info,
                DiagnosticCode::ClipboardImageSucceeded,
                "The figure was copied under every clipboard format.",
            )
            .with_source("app.clipboard_figure")
            .with_context("formats", formats)
            .with_context("width_px", width.to_string())
            .with_context("height_px", height.to_string())
            .with_context("dpi", payload.dpi.to_string()),
        )
    } else {
        OperationReport::warning(
            operation_id,
            OperationKind::ClipboardFigureCopy,
            format!("Copied figure to the clipboard ({formats}); some formats failed."),
            (),
        )
        .with_diagnostic(
            Diagnostic::new(
                Severity::Warning,
                DiagnosticCode::ClipboardFigurePartial,
                "Some clipboard formats could not be committed.",
            )
            .with_source("app.clipboard_figure")
            .with_context("formats", formats)
            .with_context("failed", failed.join("; ")),
        )
    }
}

#[cfg(not(windows))]
fn publish(
    ctx: &Context,
    payload: FigurePayload,
    operation_id: plotx_core::operation::OperationId,
) -> OperationReport<()> {
    let (width, height) = (payload.raster.width(), payload.raster.height());
    match color_image_from_straight_rgba([width as usize, height as usize], payload.raster.rgba()) {
        Ok(image) => {
            ctx.copy_image(image);
            OperationReport::success(
                operation_id,
                OperationKind::ClipboardFigureCopy,
                format!(
                    "Copied {width}x{height} image to the clipboard ({} dpi; raster only on this platform).",
                    payload.dpi
                ),
                (),
            )
            .with_diagnostic(
                Diagnostic::new(
                    Severity::Info,
                    DiagnosticCode::ClipboardImageSucceeded,
                    "The figure image was copied to the clipboard.",
                )
                .with_source("app.clipboard_figure")
                .with_context("formats", "raster")
                .with_context("width_px", width.to_string())
                .with_context("height_px", height.to_string())
                .with_context("dpi", payload.dpi.to_string()),
            )
        }
        Err(error) => OperationReport::<()>::failure(
            operation_id,
            OperationKind::ClipboardFigureCopy,
            "Could not copy the figure.",
            Diagnostic::new(
                Severity::Error,
                DiagnosticCode::ClipboardImageFailed,
                "The figure could not be converted for the clipboard.",
            )
            .with_source("app.clipboard_figure")
            .with_context("category", clipboard_error_category(&error))
            .with_context("error", error.to_string()),
        ),
    }
}

/// Converts straight-alpha RGBA8 into egui's premultiplied `ColorImage` representation.
#[cfg(not(windows))]
fn color_image_from_straight_rgba(
    size: [usize; 2],
    rgba: &[u8],
) -> Result<egui::ColorImage, ClipboardFigureError> {
    let expected_len = size[0]
        .checked_mul(size[1])
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or(ClipboardFigureError::DimensionOverflow)?;
    if rgba.len() != expected_len {
        return Err(ClipboardFigureError::InvalidBufferLength {
            expected: expected_len,
            actual: rgba.len(),
        });
    }
    Ok(egui::ColorImage::from_rgba_unmultiplied(size, rgba))
}

fn clipboard_error_code(error: &ClipboardFigureError) -> DiagnosticCode {
    match error {
        ClipboardFigureError::NoTarget => DiagnosticCode::ClipboardImageUnavailable,
        _ => DiagnosticCode::ClipboardImageFailed,
    }
}

fn clipboard_error_category(error: &ClipboardFigureError) -> &'static str {
    match error {
        ClipboardFigureError::NoTarget => "no_target",
        ClipboardFigureError::Raster(_) => "rasterization",
        #[cfg(windows)]
        ClipboardFigureError::PngEncode(_) => "png_encode",
        #[cfg(not(windows))]
        ClipboardFigureError::DimensionOverflow => "dimension_overflow",
        #[cfg(not(windows))]
        ClipboardFigureError::InvalidBufferLength { .. } => "invalid_buffer_length",
    }
}

#[derive(Debug)]
enum ClipboardFigureError {
    NoTarget,
    Raster(RasterError),
    #[cfg(windows)]
    PngEncode(Box<dyn std::error::Error + Send + Sync>),
    #[cfg(not(windows))]
    DimensionOverflow,
    #[cfg(not(windows))]
    InvalidBufferLength {
        expected: usize,
        actual: usize,
    },
}

impl fmt::Display for ClipboardFigureError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoTarget => formatter.write_str("no canvas is selected or active"),
            Self::Raster(error) => error.fmt(formatter),
            #[cfg(windows)]
            Self::PngEncode(error) => write!(formatter, "PNG encoding failed: {error}"),
            #[cfg(not(windows))]
            Self::DimensionOverflow => {
                formatter.write_str("image dimensions cannot be represented on this platform")
            }
            #[cfg(not(windows))]
            Self::InvalidBufferLength { expected, actual } => write!(
                formatter,
                "RGBA buffer length is {actual} bytes, expected {expected} bytes"
            ),
        }
    }
}

impl std::error::Error for ClipboardFigureError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Raster(error) => Some(error),
            _ => None,
        }
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    /// Renders a realistic canvas to %TEMP%\plotx_probe and drives the real
    /// clipboard, for manual inspection.
    #[test]
    #[ignore]
    fn manual_figure_probe() {
        use num_complex::Complex64;
        use plotx_core::actions::Action;
        use plotx_core::state::{DEFAULT_CANVAS_SIZE_MM, Dataset, NmrDataset};
        use plotx_io::{Domain, NmrData};

        let npoints = 4096;
        let (sw, obs, carrier) = (4000.0, 400.0, 5.0);
        let dt = 1.0 / sw;
        let peaks = [(1.8, 1.0, 0.15), (2.3, 0.6, 0.10), (2.6, 0.8, 0.20)];
        let points = (0..npoints)
            .map(|k| {
                let t = k as f64 * dt;
                peaks
                    .iter()
                    .map(|&(ppm, amp, t2): &(f64, f64, f64)| {
                        let freq_hz = (ppm - carrier) * obs;
                        Complex64::from_polar(
                            amp * (-t / t2).exp(),
                            std::f64::consts::TAU * freq_hz * t,
                        )
                    })
                    .sum()
            })
            .collect();
        let data = NmrData {
            points,
            domain: Domain::Time,
            spectral_width_hz: sw,
            observe_freq_mhz: obs,
            carrier_ppm: carrier,
            nucleus: "1H".to_owned(),
            source: "synthetic".to_owned(),
            group_delay: 0.0,
        };
        let mut app = plotx_core::state::PlotxApp::new();
        let action = Action::insert_dataset_with_default_canvas(
            &app,
            Dataset::Nmr(Box::new(NmrDataset::load(data))),
            "probe".to_owned(),
            DEFAULT_CANVAS_SIZE_MM,
        );
        app.execute_action(action);

        let payload = build_payload(&app, 0).expect("payload");
        println!(
            "raster: {}x{} px at {} dpi",
            payload.raster.width(),
            payload.raster.height(),
            payload.dpi
        );
        let dir = std::env::temp_dir().join("plotx_probe");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("probe.png"), &payload.png).unwrap();
        std::fs::write(dir.join("probe.svg"), payload.svg.as_bytes()).unwrap();
        if let Ok(emf) = &payload.emf {
            std::fs::write(dir.join("probe.emf"), emf).unwrap();
        }
        println!("saved to {}", dir.display());

        let ctx = egui::Context::default();
        copy_canvas_figure(&mut app, &ctx, 0);
        println!("status: {}", app.session.status);
        println!("{}", app.session.sanitized_diagnostics_text());
    }

    #[test]
    fn clipboard_errors_map_to_stable_operation_diagnostics() {
        let unavailable = ClipboardFigureError::NoTarget;
        assert_eq!(
            clipboard_error_code(&unavailable),
            DiagnosticCode::ClipboardImageUnavailable
        );
        assert_eq!(clipboard_error_category(&unavailable), "no_target");

        #[cfg(not(windows))]
        {
            let invalid = ClipboardFigureError::InvalidBufferLength {
                expected: 8,
                actual: 4,
            };
            assert_eq!(
                clipboard_error_code(&invalid),
                DiagnosticCode::ClipboardImageFailed
            );
            assert_eq!(clipboard_error_category(&invalid), "invalid_buffer_length");
        }
    }
}
