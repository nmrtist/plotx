use super::*;
use plotx_core::state::SliceCursor;
use plotx_processing::{DisplayMode, Processed2D, Slice1D, SliceKind};

const SLICE_COLOR: Color32 = Color32::from_rgb(0x1f, 0x9d, 0x74);
const SLICE_INSET_FRAC: f32 = 0.26;

/// A pseudo-2D stack picks its increment in the sidebar instead, so here we only
/// ensure a cursor exists for it.
pub(crate) fn handle_slice(
    app: &mut PlotxApp,
    ci: usize,
    object_id: ObjectId,
    di: usize,
    plot: PlotRect,
    ui: &Ui,
) {
    let is_stack = matches!(
        app.doc
            .datasets
            .get(di)
            .and_then(Dataset::as_nmr2d)
            .map(|n| &n.processed),
        Some(Processed2D::Stack(_))
    );
    if is_stack {
        if !matches!(app.session.ui.slice, Some(c) if c.dataset == di) {
            app.session.ui.slice = Some(SliceCursor {
                dataset: di,
                object: object_id,
                kind: SliceKind::Row,
                index: 0,
            });
        }
        return;
    }

    let Some(p) = ui
        .input(|i| i.pointer.hover_pos())
        .filter(|p| plot_contains(plot, *p))
    else {
        return;
    };
    let kind = app.session.ui.slice_kind;
    let index = {
        let Some(fig) = app.doc.canvases[ci]
            .object(object_id)
            .and_then(|o| o.plot())
            .map(|pl| &pl.figure)
        else {
            return;
        };
        let Some(Processed2D::Ft(s)) = app
            .doc
            .datasets
            .get(di)
            .and_then(Dataset::as_nmr2d)
            .map(|n| &n.processed)
        else {
            return;
        };
        match kind {
            SliceKind::Row => s.nearest_f1(screen_to_y(
                p.y,
                plot,
                fig.y.min,
                fig.y.span(),
                fig.y.reversed,
            )),
            SliceKind::Column => s.nearest_f2(screen_to_x(
                p.x,
                plot,
                fig.x.min,
                fig.x.span(),
                fig.x.reversed,
            )),
        }
    };
    app.session.ui.slice = Some(SliceCursor {
        dataset: di,
        object: object_id,
        kind,
        index,
    });
}

pub(crate) fn paint_slice(
    app: &PlotxApp,
    ci: usize,
    object_id: ObjectId,
    di: usize,
    plot: PlotRect,
    painter: &egui::Painter,
) {
    if app.session.tool != Tool::Slice {
        return;
    }
    let Some(cursor) = app.session.ui.slice.filter(|c| c.dataset == di) else {
        return;
    };
    let Some(fig) = app.doc.canvases[ci]
        .object(object_id)
        .and_then(|o| o.plot())
        .map(|pl| &pl.figure)
    else {
        return;
    };
    let Some(n) = app.doc.datasets.get(di).and_then(Dataset::as_nmr2d) else {
        return;
    };

    // A Row cut runs along F2 (the plot's x-axis); a Column cut along F1 (y).
    let (slice, along_x, mode) = match &n.processed {
        Processed2D::Ft(s) => (
            s.slice(cursor.kind, cursor.index),
            cursor.kind == SliceKind::Row,
            DisplayMode::Real,
        ),
        Processed2D::Stack(s) => (s.slice(cursor.index), true, DisplayMode::Real),
    };

    if let Some(ppm) = slice.position_ppm {
        if along_x {
            let py = y_to_screen(ppm, plot, fig.y.min, fig.y.span(), fig.y.reversed)
                .clamp(plot.top, plot.bottom());
            painter.line_segment(
                [Pos2::new(plot.left, py), Pos2::new(plot.right(), py)],
                Stroke::new(1.2_f32, SLICE_COLOR),
            );
        } else {
            let px = x_to_screen(ppm, plot, fig.x.min, fig.x.span(), fig.x.reversed)
                .clamp(plot.left, plot.right());
            painter.line_segment(
                [Pos2::new(px, plot.top), Pos2::new(px, plot.bottom())],
                Stroke::new(1.2_f32, SLICE_COLOR),
            );
        }
    }

    paint_inset(painter, plot, fig, &slice, along_x, mode);
}

fn paint_inset(
    painter: &egui::Painter,
    plot: PlotRect,
    fig: &plotx_figure::Figure,
    slice: &Slice1D,
    along_x: bool,
    mode: DisplayMode,
) {
    let disp: Vec<f64> = slice.values.iter().map(|c| mode.reduce(c)).collect();
    if disp.len() < 2 {
        return;
    }
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    for &v in &disp {
        lo = lo.min(v);
        hi = hi.max(v);
    }
    let span = (hi - lo).max(f64::MIN_POSITIVE);

    if along_x {
        let inset_h = plot.height * SLICE_INSET_FRAC;
        let base_y = plot.top + inset_h;
        painter.rect_filled(
            EguiRect::from_min_max(
                Pos2::new(plot.left, plot.top),
                Pos2::new(plot.right(), base_y),
            ),
            0.0,
            Color32::from_black_alpha(20),
        );
        let pts: Vec<Pos2> = slice
            .ppm
            .iter()
            .zip(&disp)
            .map(|(&ppm, &v)| {
                let x = x_to_screen(ppm, plot, fig.x.min, fig.x.span(), fig.x.reversed);
                let t = ((v - lo) / span) as f32;
                Pos2::new(x, base_y - inset_h * (0.04 + 0.92 * t))
            })
            .collect();
        painter.add(egui::Shape::line(pts, Stroke::new(1.2_f32, SLICE_COLOR)));
    } else {
        let inset_w = plot.width * SLICE_INSET_FRAC;
        painter.rect_filled(
            EguiRect::from_min_max(
                Pos2::new(plot.left, plot.top),
                Pos2::new(plot.left + inset_w, plot.bottom()),
            ),
            0.0,
            Color32::from_black_alpha(20),
        );
        let pts: Vec<Pos2> = slice
            .ppm
            .iter()
            .zip(&disp)
            .map(|(&ppm, &v)| {
                let y = y_to_screen(ppm, plot, fig.y.min, fig.y.span(), fig.y.reversed);
                let t = ((v - lo) / span) as f32;
                Pos2::new(plot.left + inset_w * (0.04 + 0.92 * t), y)
            })
            .collect();
        painter.add(egui::Shape::line(pts, Stroke::new(1.2_f32, SLICE_COLOR)));
    }
}
