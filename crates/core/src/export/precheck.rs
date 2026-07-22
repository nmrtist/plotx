use super::ExportFormat;
use crate::state::{CanvasDocument, CanvasObjectKind};
use plotx_figure::AxisFrame;

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
/// Since figure typography became a document style, tick and axis-title sizes
/// count too; only truly fixed renderer chrome (the axis frame line) stays out.
pub fn page_metrics(canvas: &CanvasDocument) -> PageMetrics {
    let mut fonts: Vec<f32> = Vec::new();
    let mut lines: Vec<f32> = Vec::new();
    for object in &canvas.objects {
        if !object.visible {
            continue;
        }
        match &object.kind {
            CanvasObjectKind::Text(t) | CanvasObjectKind::PanelLabel(t) => {
                if !t.text.trim().is_empty() {
                    fonts.push(t.font_size);
                }
            }
            CanvasObjectKind::Shape(s) => lines.push(s.stroke_width),
            CanvasObjectKind::Plot(plot) => {
                if plot.panel.visible {
                    fonts.push(plot.panel.font_size);
                }
                let typography = plot.figure.typography;
                if plot.figure.axis_frame != AxisFrame::Hidden {
                    fonts.extend([typography.tick_pt, typography.label_pt]);
                }
                if !plot.figure.title.trim().is_empty() {
                    fonts.push(typography.title_pt);
                }
                for annotation in &plot.figure.annotations {
                    fonts.push(annotation.size);
                }
                for series in &plot.figure.series {
                    if !series.points.is_empty() {
                        lines.push(series.width);
                    }
                }
                for contour in &plot.figure.contours {
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
        AxisOverrides, AxisProjections, CanvasObject, CanvasObjectKind, CanvasViewport, ChartSpec,
        DataBinding, ObjectFrame, PanelMeta, PlotObject, StackSpec,
    };
    use plotx_figure::{Axis, Figure};

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
    fn hidden_axes_do_not_contribute_unrendered_typography() {
        let mut figure = Figure::new("", Axis::new("x", 0.0, 1.0), Axis::new("y", 0.0, 1.0));
        figure.axis_frame = AxisFrame::Hidden;
        figure.typography.tick_pt = 3.0;
        figure.typography.label_pt = 4.0;
        let viewport = CanvasViewport::from_figure(&figure);
        let mut panel = PanelMeta::new(String::new(), 100.0);
        panel.visible = false;
        let mut canvas = CanvasDocument::new("Hidden axes".to_owned(), [200.0, 100.0]);
        canvas.objects.push(CanvasObject {
            id: 1,
            name: "Plot".to_owned(),
            frame: ObjectFrame::new(0.0, 0.0, 100.0, 100.0),
            locked: false,
            visible: true,
            group: None,
            kind: CanvasObjectKind::Plot(Box::new(PlotObject {
                binding: DataBinding::single(0),
                chart: ChartSpec::default(),
                stack: StackSpec::default(),
                projections: AxisProjections::default(),
                axis_overrides: AxisOverrides::default(),
                figure,
                viewport,
                panel,
            })),
        });

        assert_eq!(page_metrics(&canvas).min_font_pt, None);
        canvas.objects[0].plot_mut().unwrap().figure.axis_frame = AxisFrame::Open;
        assert_eq!(page_metrics(&canvas).min_font_pt, Some(3.0));
    }
}
