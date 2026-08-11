use super::ExportFormat;
use crate::state::{AssetId, AssetRecord, CanvasDocument, CanvasObjectKind, ImageFit, QuarterTurn};
use plotx_figure::AxisFrame;
use std::collections::BTreeMap;

/// The minimum rendered sizes a figure must meet for a target. Values are in
/// points, measured at the exported physical size (after any column downscale).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ComplianceThresholds {
    pub min_font_pt: f32,
    pub min_line_pt: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ComplianceStatus {
    Pass,
    Warn,
    Fail,
}

impl ComplianceStatus {
    fn rank(self) -> u8 {
        match self {
            Self::Pass => 0,
            Self::Warn => 1,
            Self::Fail => 2,
        }
    }
}

#[derive(Clone, Debug)]
pub struct ComplianceItem {
    pub status: ComplianceStatus,
    pub label: String,
    pub detail: String,
}

#[derive(Clone, Debug)]
pub struct PrecheckReport {
    pub items: Vec<ComplianceItem>,
}

impl PrecheckReport {
    pub fn worst(&self) -> ComplianceStatus {
        self.items
            .iter()
            .map(|item| item.status)
            .max_by_key(|status| status.rank())
            .unwrap_or(ComplianceStatus::Pass)
    }
}

/// The smallest authored font and line width on one page, in page points, plus
/// the page width used to derive the export downscale.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PageMetrics {
    pub width_mm: f32,
    pub min_font_pt: Option<f32>,
    pub min_line_pt: Option<f32>,
}

/// Scan a page for the smallest authored (user-controlled) font and line width.
/// Since figure typography became a document style, tick, axis-title, and
/// visible legend sizes count too; only fixed renderer chrome stays out.
pub fn page_metrics(canvas: &CanvasDocument) -> PageMetrics {
    let mut fonts: Vec<f32> = Vec::new();
    let mut lines: Vec<f32> = Vec::new();
    for object in &canvas.objects {
        if !object.visible {
            continue;
        }
        match &object.kind {
            CanvasObjectKind::RasterImage(_) => {}
            CanvasObjectKind::Text(t) => {
                if !t.text.trim().is_empty() {
                    fonts.push(t.font_size);
                }
            }
            CanvasObjectKind::Shape(s) => lines.push(s.stroke_width),
            CanvasObjectKind::Plot(plot) => {
                if let Some(panel) = canvas
                    .parent_panel(object.id)
                    .and_then(|id| canvas.panel(id))
                    && panel.visible
                    && panel.label.visible
                {
                    fonts.push(panel.label.font_size);
                }
                let typography = plot.figure().typography;
                if plot.figure().axis_frame != AxisFrame::Hidden {
                    fonts.extend([typography.tick_pt, typography.label_pt]);
                }
                if !plot.figure().range_annotations.is_empty() {
                    fonts.push(typography.tick_pt);
                }
                if !plot.figure().title.trim().is_empty() {
                    fonts.push(typography.title_pt);
                }
                if plotx_render::renders_legend(plot.figure()) {
                    fonts.push(typography.legend_pt);
                }
                for annotation in &plot.figure().annotations {
                    fonts.push(annotation.size);
                }
                for series in &plot.figure().series {
                    if !series.points.is_empty() {
                        lines.push(series.width);
                    }
                }
                for contour in &plot.figure().contours {
                    lines.push(contour.width);
                }
            }
        }
    }
    PageMetrics {
        width_mm: canvas.size_mm[0],
        min_font_pt: fonts.into_iter().reduce(f32::min),
        min_line_pt: lines.into_iter().reduce(f32::min),
    }
}

/// Pure compliance check: page metrics + target width + thresholds → a status
/// list. Each page's metric is scaled to the exported physical width (a wide
/// canvas downscaled to a column shrinks fonts and lines), then the worst page
/// is compared to the threshold.
pub fn precheck_report(
    metrics: &[PageMetrics],
    target_width_mm: Option<f32>,
    thresholds: &ComplianceThresholds,
    format: ExportFormat,
    dpi: u16,
) -> PrecheckReport {
    let font = worst_scaled(metrics, target_width_mm, |m| m.min_font_pt);
    let line = worst_scaled(metrics, target_width_mm, |m| m.min_line_pt);

    let mut items = vec![
        metric_item("Smallest text", font, thresholds.min_font_pt),
        metric_item("Thinnest line", line, thresholds.min_line_pt),
    ];
    items.push(if format.is_bitmap() {
        resolution_item(dpi)
    } else {
        ComplianceItem {
            status: ComplianceStatus::Pass,
            label: "Resolution".to_owned(),
            detail: "vector output — resolution independent".to_owned(),
        }
    });
    PrecheckReport { items }
}

pub fn image_precheck_items(
    canvases: &[&CanvasDocument],
    assets: &BTreeMap<AssetId, AssetRecord>,
    target_width_mm: Option<f32>,
) -> Vec<ComplianceItem> {
    let mut minimum_ppi: Option<f32> = None;
    let mut missing = 0usize;
    let mut converted = 0usize;
    let mut images = 0usize;
    for canvas in canvases {
        let output_scale = target_width_mm
            .map(|target| target / canvas.size_mm[0].max(f32::MIN_POSITIVE))
            .unwrap_or(1.0);
        for item in &canvas.objects {
            let CanvasObjectKind::RasterImage(image) = &item.kind else {
                continue;
            };
            if !item.visible
                || canvas
                    .parent_panel(item.id)
                    .and_then(|panel| canvas.panel(panel))
                    .is_some_and(|panel| !panel.visible)
            {
                continue;
            }
            images += 1;
            let Some(asset) = assets.get(&image.asset) else {
                missing += 1;
                continue;
            };
            let Ok(probe) = plotx_io::image::probe(&asset.bytes) else {
                missing += 1;
                continue;
            };
            if probe.high_precision || probe.has_icc {
                converted += 1;
            }
            let source = if image.page_index == 0 {
                asset.pixel_size
            } else {
                plotx_io::image::tiff_page_dimensions(&asset.bytes, image.page_index)
                    .unwrap_or(asset.pixel_size)
            };
            let Some(frame) = canvas.content_page_frame(item.id) else {
                continue;
            };
            let ppi = effective_ppi(source, image, frame, output_scale);
            if ppi.is_finite() {
                minimum_ppi = Some(minimum_ppi.map_or(ppi, |current| current.min(ppi)));
            }
        }
    }
    if images == 0 {
        return Vec::new();
    }
    let mut items = vec![if missing == 0 {
        ComplianceItem {
            status: ComplianceStatus::Pass,
            label: "Embedded images".to_owned(),
            detail: format!("{images} available"),
        }
    } else {
        ComplianceItem {
            status: ComplianceStatus::Fail,
            label: "Embedded images".to_owned(),
            detail: format!("{missing} missing or damaged; replace before publication export"),
        }
    }];
    if let Some(ppi) = minimum_ppi {
        items.push(ComplianceItem {
            status: if ppi >= 300.0 {
                ComplianceStatus::Pass
            } else if ppi >= 150.0 {
                ComplianceStatus::Warn
            } else {
                ComplianceStatus::Fail
            },
            label: "Lowest effective image PPI".to_owned(),
            detail: format!("{ppi:.0} PPI at exported size (recommended 300 PPI)"),
        });
    }
    items.push(ComplianceItem {
        status: if converted == 0 {
            ComplianceStatus::Pass
        } else {
            ComplianceStatus::Warn
        },
        label: "Image color".to_owned(),
        detail: if converted == 0 {
            "8-bit RGB sources".to_owned()
        } else {
            format!(
                "{converted} image(s) with an ICC profile or high-precision samples export as 8-bit RGBA without an embedded profile"
            )
        },
    });
    items
}

fn effective_ppi(
    source: [u32; 2],
    image: &crate::state::RasterImageContent,
    frame: crate::state::ObjectFrame,
    output_scale: f32,
) -> f32 {
    let mut pixels = [
        source[0] as f32 * (image.crop[2] - image.crop[0]),
        source[1] as f32 * (image.crop[3] - image.crop[1]),
    ];
    if matches!(
        image.rotation,
        QuarterTurn::Clockwise90 | QuarterTurn::Clockwise270
    ) {
        pixels.swap(0, 1);
    }
    let frame = [frame.width * output_scale, frame.height * output_scale];
    match image.fit {
        ImageFit::Stretch => (pixels[0] * 72.0 / frame[0].max(f32::MIN_POSITIVE))
            .min(pixels[1] * 72.0 / frame[1].max(f32::MIN_POSITIVE)),
        ImageFit::Contain => {
            let points_per_pixel = (frame[0] / pixels[0].max(f32::MIN_POSITIVE))
                .min(frame[1] / pixels[1].max(f32::MIN_POSITIVE));
            72.0 / points_per_pixel.max(f32::MIN_POSITIVE)
        }
        ImageFit::Cover => {
            let points_per_pixel = (frame[0] / pixels[0].max(f32::MIN_POSITIVE))
                .max(frame[1] / pixels[1].max(f32::MIN_POSITIVE));
            72.0 / points_per_pixel.max(f32::MIN_POSITIVE)
        }
    }
}

fn worst_scaled(
    metrics: &[PageMetrics],
    target_width_mm: Option<f32>,
    pick: impl Fn(&PageMetrics) -> Option<f32>,
) -> Option<f32> {
    metrics
        .iter()
        .filter_map(|m| {
            pick(m).map(|value| {
                let scale = target_width_mm.map_or(1.0, |t| t / m.width_mm.max(f32::MIN_POSITIVE));
                value * scale
            })
        })
        .reduce(f32::min)
}

fn metric_item(label: &str, value: Option<f32>, min: f32) -> ComplianceItem {
    match value {
        None => ComplianceItem {
            status: ComplianceStatus::Pass,
            label: label.to_owned(),
            detail: "none present".to_owned(),
        },
        Some(value) => ComplianceItem {
            status: threshold_status(value, min),
            label: label.to_owned(),
            detail: format!("{value:.1} pt rendered (min {min:.1} pt)"),
        },
    }
}

fn resolution_item(dpi: u16) -> ComplianceItem {
    let status = if dpi >= 300 {
        ComplianceStatus::Pass
    } else if dpi >= 150 {
        ComplianceStatus::Warn
    } else {
        ComplianceStatus::Fail
    };
    ComplianceItem {
        status,
        label: "Resolution".to_owned(),
        detail: format!("{dpi} dpi"),
    }
}

/// Below the threshold fails; within 15 % above it is a borderline warning.
fn threshold_status(value: f32, min: f32) -> ComplianceStatus {
    if value < min {
        ComplianceStatus::Fail
    } else if value < min * 1.15 {
        ComplianceStatus::Warn
    } else {
        ComplianceStatus::Pass
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{
        AssetId, AssetRecord, AxisOverrides, AxisProjections, CanvasObject, CanvasObjectKind,
        CanvasViewport, ChartSpec, DataBinding, ObjectFrame, ObjectId, PlotObject,
        RasterImageContent, StackSpec,
    };
    use image::{DynamicImage, ImageBuffer, ImageFormat, Luma};
    use plotx_figure::{Axis, Color, Figure, RangeAnnotation, Series};
    use sha2::{Digest, Sha256};
    use std::io::Cursor;

    fn thresholds() -> ComplianceThresholds {
        ComplianceThresholds {
            min_font_pt: 7.0,
            min_line_pt: 0.5,
        }
    }

    #[test]
    fn flags_sub_threshold_font_after_downscale() {
        // A comfortable 10 pt label on a 200 mm page falls to 4.45 pt once the
        // page is scaled down to an 89 mm column — a violation.
        let metrics = [PageMetrics {
            width_mm: 200.0,
            min_font_pt: Some(10.0),
            min_line_pt: Some(2.0),
        }];
        let report = precheck_report(&metrics, Some(89.0), &thresholds(), ExportFormat::Tiff, 600);
        let font = &report.items[0];
        assert_eq!(font.status, ComplianceStatus::Fail);
        assert_eq!(report.worst(), ComplianceStatus::Fail);
    }

    #[test]
    fn post_scale_line_width_decides_status() {
        // A 1.0 pt line is fine at full size but drops below 0.5 pt once a
        // 254 mm canvas is squeezed into an 89 mm column.
        let metrics = [PageMetrics {
            width_mm: 254.0,
            min_font_pt: Some(30.0),
            min_line_pt: Some(1.0),
        }];
        let scaled = precheck_report(&metrics, Some(89.0), &thresholds(), ExportFormat::Png, 300);
        assert_eq!(scaled.items[1].status, ComplianceStatus::Fail);

        let natural = precheck_report(&metrics, None, &thresholds(), ExportFormat::Png, 300);
        assert_eq!(natural.items[1].status, ComplianceStatus::Pass);
    }

    #[test]
    fn low_resolution_bitmap_warns_then_fails() {
        let metrics = [PageMetrics {
            width_mm: 89.0,
            min_font_pt: Some(9.0),
            min_line_pt: Some(1.0),
        }];
        let warn = precheck_report(&metrics, None, &thresholds(), ExportFormat::Png, 200);
        assert_eq!(warn.items[2].status, ComplianceStatus::Warn);
        let fail = precheck_report(&metrics, None, &thresholds(), ExportFormat::Png, 96);
        assert_eq!(fail.items[2].status, ComplianceStatus::Fail);
        let vector = precheck_report(&metrics, None, &thresholds(), ExportFormat::Svg, 96);
        assert_eq!(vector.items[2].status, ComplianceStatus::Pass);
    }

    #[test]
    fn effective_ppi_accounts_for_fit_crop_rotation_and_export_scale() {
        let mut image = RasterImageContent::new(AssetId::new());
        let frame = ObjectFrame::new(0.0, 0.0, 144.0, 72.0);
        image.fit = ImageFit::Stretch;
        assert_eq!(effective_ppi([600, 300], &image, frame, 1.0), 300.0);

        image.crop = [0.0, 0.0, 0.5, 1.0];
        assert_eq!(effective_ppi([600, 300], &image, frame, 1.0), 150.0);
        image.crop = [0.0, 0.0, 1.0, 1.0];
        image.rotation = QuarterTurn::Clockwise90;
        assert_eq!(effective_ppi([600, 300], &image, frame, 0.5), 300.0);
    }

    #[test]
    fn image_precheck_reports_availability_resolution_and_color_conversion() {
        let source = ImageBuffer::from_pixel(600, 300, Luma([1000_u16]));
        let mut encoded = Cursor::new(Vec::new());
        DynamicImage::ImageLuma16(source)
            .write_to(&mut encoded, ImageFormat::Png)
            .unwrap();
        let bytes = encoded.into_inner();
        let id = AssetId::new();
        let asset = AssetRecord {
            id,
            sha256: Sha256::digest(&bytes).into(),
            format: "png".to_owned(),
            pixel_size: [600, 300],
            bytes,
        };
        let mut canvas = CanvasDocument::new("precheck".to_owned(), [100.0, 100.0]);
        let mut image = RasterImageContent::new(id);
        image.fit = ImageFit::Stretch;
        canvas.objects.push(CanvasObject {
            id: ObjectId::new(1),
            name: "image".to_owned(),
            frame: ObjectFrame::new(0.0, 0.0, 144.0, 72.0),
            locked: false,
            visible: true,
            kind: CanvasObjectKind::RasterImage(image),
        });

        let items = image_precheck_items(&[&canvas], &BTreeMap::from([(id, asset)]), None);
        assert_eq!(items[0].status, ComplianceStatus::Pass);
        assert_eq!(items[1].status, ComplianceStatus::Pass);
        assert!(items[1].detail.contains("300 PPI"));
        assert_eq!(items[2].status, ComplianceStatus::Warn);
        assert!(items[2].detail.contains("without an embedded profile"));

        let missing = image_precheck_items(&[&canvas], &BTreeMap::new(), None);
        assert_eq!(missing[0].status, ComplianceStatus::Fail);
    }

    #[test]
    fn hidden_axes_do_not_contribute_unrendered_typography() {
        let mut figure = Figure::new("", Axis::new("x", 0.0, 1.0), Axis::new("y", 0.0, 1.0));
        figure.axis_frame = AxisFrame::Hidden;
        figure.typography.tick_pt = 3.0;
        figure.typography.label_pt = 4.0;
        let viewport = CanvasViewport::from_figure(&figure);
        let mut canvas = CanvasDocument::new("Hidden axes".to_owned(), [200.0, 100.0]);
        canvas.objects.push(CanvasObject {
            id: ObjectId::new(1),
            name: "Plot".to_owned(),
            frame: ObjectFrame::new(0.0, 0.0, 100.0, 100.0),
            locked: false,
            visible: true,
            kind: CanvasObjectKind::Plot(Box::new(PlotObject::new(
                None,
                crate::state::SeriesId::new(1),
                DataBinding { series: Vec::new() },
                ChartSpec::default(),
                StackSpec::default(),
                AxisProjections::default(),
                AxisOverrides::default(),
                figure,
                viewport,
            ))),
        });

        assert_eq!(page_metrics(&canvas).min_font_pt, None);
        canvas.objects[0]
            .plot_mut()
            .unwrap()
            .set_axis_frame(AxisFrame::Open);
        assert_eq!(page_metrics(&canvas).min_font_pt, Some(3.0));
    }

    #[test]
    fn visible_legend_contributes_its_authored_font_size() {
        let mut figure = Figure::new("", Axis::new("x", 0.0, 1.0), Axis::new("y", 0.0, 1.0));
        figure.axis_frame = AxisFrame::Hidden;
        figure.guide_visibility = plotx_figure::GuideVisibility::Show;
        figure.typography.legend_pt = 5.5;
        figure.series = vec![
            Series::line("A", vec![[0.0, 0.0]]),
            Series::line("B", vec![[1.0, 1.0]]).colored(Color::rgb(200, 0, 0)),
        ];
        let viewport = CanvasViewport::from_figure(&figure);
        let mut canvas = CanvasDocument::new("Legend".to_owned(), [200.0, 100.0]);
        canvas.objects.push(CanvasObject {
            id: ObjectId::new(1),
            name: "Plot".to_owned(),
            frame: ObjectFrame::new(0.0, 0.0, 100.0, 100.0),
            locked: false,
            visible: true,
            kind: CanvasObjectKind::Plot(Box::new(PlotObject::new(
                None,
                crate::state::SeriesId::new(1),
                DataBinding { series: Vec::new() },
                ChartSpec::default(),
                StackSpec::default(),
                AxisProjections::default(),
                AxisOverrides::default(),
                figure,
                viewport,
            ))),
        });

        assert_eq!(page_metrics(&canvas).min_font_pt, Some(5.5));
    }

    #[test]
    fn range_label_counts_even_when_the_axis_frame_is_hidden() {
        let mut figure = Figure::new("", Axis::new("x", 0.0, 1.0), Axis::new("y", 0.0, 1.0));
        figure.axis_frame = AxisFrame::Hidden;
        figure.typography.tick_pt = 4.5;
        figure.range_annotations.push(RangeAnnotation {
            source_id: 1,
            x0: 0.2,
            x1: 0.4,
            label: "window".to_owned(),
            label_position: None,
            color: Color::AXIS,
            fill_opacity: 0.1,
            width: 1.0,
        });
        let viewport = CanvasViewport::from_figure(&figure);
        let mut canvas = CanvasDocument::new("Ranges".to_owned(), [200.0, 100.0]);
        canvas.objects.push(CanvasObject {
            id: ObjectId::new(1),
            name: "Plot".to_owned(),
            frame: ObjectFrame::new(0.0, 0.0, 100.0, 100.0),
            locked: false,
            visible: true,
            kind: CanvasObjectKind::Plot(Box::new(PlotObject::new(
                None,
                crate::state::SeriesId::new(1),
                DataBinding { series: Vec::new() },
                ChartSpec::default(),
                StackSpec::default(),
                AxisProjections::default(),
                AxisOverrides::default(),
                figure,
                viewport,
            ))),
        });

        assert_eq!(page_metrics(&canvas).min_font_pt, Some(4.5));
    }
}
