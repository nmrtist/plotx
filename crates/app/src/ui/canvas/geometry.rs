use super::*;

#[derive(Clone, Copy)]
pub(crate) struct ObjectHit {
    pub(crate) object: ObjectId,
    pub(crate) kind: ObjectDragKind,
}

pub(crate) fn hit_object(canvas: &CanvasDocument, p: Pos2, zoom: f32) -> Option<ObjectHit> {
    let handle_radius = (HANDLE_SIZE_PX / zoom.max(0.01)).max(3.0);
    canvas.objects.iter().rev().find_map(|object| {
        if !object.visible {
            return None;
        }
        let r = egui::Rect::from_min_size(
            Pos2::new(object.frame.x, object.frame.y),
            egui::vec2(object.frame.width, object.frame.height),
        );
        let handles = [
            (r.left_top(), ResizeHandle::TopLeft),
            (r.right_top(), ResizeHandle::TopRight),
            (r.left_bottom(), ResizeHandle::BottomLeft),
            (r.right_bottom(), ResizeHandle::BottomRight),
        ];
        for (pos, handle) in handles {
            if pos.distance(p) <= handle_radius {
                return Some(ObjectHit {
                    object: object.id,
                    kind: ObjectDragKind::Resize(handle),
                });
            }
        }
        r.contains(p).then_some(ObjectHit {
            object: object.id,
            kind: ObjectDragKind::Move,
        })
    })
}

pub(crate) fn drag_frame(
    frame: ObjectFrame,
    kind: ObjectDragKind,
    dx: f32,
    dy: f32,
) -> ObjectFrame {
    match kind {
        ObjectDragKind::Move => {
            ObjectFrame::new(frame.x + dx, frame.y + dy, frame.width, frame.height)
        }
        ObjectDragKind::Resize(ResizeHandle::TopLeft) => resize_from_edges(
            frame.x + dx,
            frame.y + dy,
            frame.x + frame.width,
            frame.y + frame.height,
        ),
        ObjectDragKind::Resize(ResizeHandle::TopRight) => resize_from_edges(
            frame.x,
            frame.y + dy,
            frame.x + frame.width + dx,
            frame.y + frame.height,
        ),
        ObjectDragKind::Resize(ResizeHandle::BottomLeft) => resize_from_edges(
            frame.x + dx,
            frame.y,
            frame.x + frame.width,
            frame.y + frame.height + dy,
        ),
        ObjectDragKind::Resize(ResizeHandle::BottomRight) => resize_from_edges(
            frame.x,
            frame.y,
            frame.x + frame.width + dx,
            frame.y + frame.height + dy,
        ),
    }
}

pub(crate) fn resize_from_edges(left: f32, top: f32, right: f32, bottom: f32) -> ObjectFrame {
    let width = (right - left).max(MIN_OBJECT_SIZE_PT);
    let height = (bottom - top).max(MIN_OBJECT_SIZE_PT);
    ObjectFrame::new(left, top, width, height)
}

pub(crate) fn data_edit_target(app: &PlotxApp, ci: usize) -> Option<ObjectId> {
    if !app.session.tool.is_data_tool() || app.session.active_canvas != Some(ci) {
        return None;
    }
    app.doc.canvases.get(ci)?.selected_plot_object_id()
}

pub(crate) fn clear_canvas_interaction_state(
    app: &mut PlotxApp,
    ci: usize,
    scope: CanvasInteractionClearScope,
) {
    app.reset_interaction();
    app.session.ui.wheel_zoom = None;

    if matches!(
        scope,
        CanvasInteractionClearScope::Selection | CanvasInteractionClearScope::All
    ) {
        if let Some(canvas) = app.doc.canvases.get_mut(ci) {
            canvas.selected_object = None;
        }
        app.session.ui.selection = Selection::None;
        if matches!(app.session.ui.panel_label_selection, Some((canvas, _)) if canvas == ci) {
            app.session.ui.panel_label_selection = None;
        }
        if matches!(app.session.ui.panel_note_edit, Some(PanelNoteEditState { canvas, .. }) if canvas == ci)
        {
            app.session.ui.panel_note_edit = None;
        }
        if matches!(app.session.ui.panel_note_inline_edit, Some(PanelNoteEditState { canvas, .. }) if canvas == ci)
        {
            app.session.ui.panel_note_inline_edit = None;
        }
        if matches!(app.session.ui.text_edit, Some(TextEditState { canvas, .. }) if canvas == ci) {
            app.session.ui.text_edit = None;
        }
    }

    if matches!(scope, CanvasInteractionClearScope::All) {
        app.session.ui.analysis_selection = None;
    }
}

/// The single page/object/screen coordinate transform for the board:
/// `world_pt = canvas.board_pos + page_pt` then `screen = origin + pan + world_pt * zoom`.
#[derive(Clone, Copy)]
pub(crate) struct BoardTransform {
    pub origin: Pos2,
    pub pan: egui::Vec2,
    pub zoom: f32,
}

impl BoardTransform {
    pub fn from_board(board: BoardViewport, screen: egui::Rect) -> Self {
        Self {
            origin: screen.min,
            pan: egui::vec2(board.pan[0], board.pan[1]),
            zoom: board.zoom,
        }
    }

    pub fn board_rect_screen(&self, r: PlotRect) -> egui::Rect {
        egui::Rect::from_min_size(
            self.origin + self.pan + egui::vec2(r.left, r.top) * self.zoom,
            egui::vec2(r.width * self.zoom, r.height * self.zoom),
        )
    }

    pub fn page_screen_rect(&self, canvas: &CanvasDocument) -> egui::Rect {
        self.board_rect_screen(canvas.board_rect_pt())
    }

    pub fn object_screen_rect(
        &self,
        canvas: &CanvasDocument,
        object_id: ObjectId,
    ) -> Option<PlotRect> {
        let page = self.page_screen_rect(canvas);
        let object = canvas.object(object_id)?;
        Some(PlotRect::new(
            page.left() + object.frame.x * self.zoom,
            page.top() + object.frame.y * self.zoom,
            object.frame.width * self.zoom,
            object.frame.height * self.zoom,
        ))
    }

    /// Screen px → board world (pt), before any per-page `board_pos` offset.
    pub fn screen_to_world(&self, p: Pos2) -> Pos2 {
        Pos2::new(
            (p.x - self.origin.x - self.pan.x) / self.zoom,
            (p.y - self.origin.y - self.pan.y) / self.zoom,
        )
    }

    pub fn screen_to_page(&self, canvas: &CanvasDocument, p: Pos2) -> Pos2 {
        let page = self.page_screen_rect(canvas);
        Pos2::new(
            (p.x - page.left()) / self.zoom,
            (p.y - page.top()) / self.zoom,
        )
    }
}

pub(crate) fn page_screen_rect(
    board: BoardViewport,
    canvas: &CanvasDocument,
    screen: egui::Rect,
) -> egui::Rect {
    BoardTransform::from_board(board, screen).page_screen_rect(canvas)
}

pub(crate) fn object_screen_rect(
    board: BoardViewport,
    canvas: &CanvasDocument,
    object_id: ObjectId,
    screen: egui::Rect,
) -> Option<PlotRect> {
    BoardTransform::from_board(board, screen).object_screen_rect(canvas, object_id)
}

pub(crate) fn plot_under_cursor(
    app: &PlotxApp,
    ci: usize,
    screen: egui::Rect,
    p: Pos2,
) -> Option<(ObjectId, EguiRect, PlotRect)> {
    let canvas = app.doc.canvases.get(ci)?;
    let zoom = app.session.board.zoom;
    for id in canvas.plot_object_ids().into_iter().rev() {
        let Some(outer) = object_screen_rect(app.session.board, canvas, id, screen) else {
            continue;
        };
        let outer_rect = plot_rect(outer);
        if !outer_rect.contains(p) {
            continue;
        }
        let Some(plot_object) = canvas.object(id).and_then(|object| object.plot()) else {
            continue;
        };
        let layout =
            plotx_render::axis_layout(&plot_object.figure, outer.width / zoom, outer.height / zoom);
        let plot =
            plotx_render::Projector::new(&plot_object.figure, outer, &layout.margins.scaled(zoom))
                .plot;
        return Some((id, outer_rect, plot));
    }
    None
}

pub(crate) fn plot_inner_rect(
    app: &PlotxApp,
    ci: usize,
    object_id: ObjectId,
    screen: egui::Rect,
) -> Option<PlotRect> {
    let canvas = app.doc.canvases.get(ci)?;
    let outer = object_screen_rect(app.session.board, canvas, object_id, screen)?;
    let plot_object = canvas.object(object_id).and_then(|object| object.plot())?;
    let zoom = app.session.board.zoom;
    let layout =
        plotx_render::axis_layout(&plot_object.figure, outer.width / zoom, outer.height / zoom);
    Some(
        plotx_render::Projector::new(&plot_object.figure, outer, &layout.margins.scaled(zoom)).plot,
    )
}

pub(crate) fn screen_to_page_unbounded(
    board: BoardViewport,
    canvas: &CanvasDocument,
    screen: egui::Rect,
    p: Pos2,
) -> Option<Pos2> {
    screen
        .contains(p)
        .then(|| BoardTransform::from_board(board, screen).screen_to_page(canvas, p))
}

pub(crate) fn bbox_of_rects(
    rects: impl IntoIterator<Item = PlotRect>,
) -> Option<(f32, f32, f32, f32)> {
    let mut iter = rects.into_iter();
    let first = iter.next()?;
    let mut bbox = (first.left, first.top, first.right(), first.bottom());
    for r in iter {
        bbox.0 = bbox.0.min(r.left);
        bbox.1 = bbox.1.min(r.top);
        bbox.2 = bbox.2.max(r.right());
        bbox.3 = bbox.3.max(r.bottom());
    }
    Some(bbox)
}

pub(crate) fn all_frames_bbox(app: &PlotxApp) -> Option<(f32, f32, f32, f32)> {
    bbox_of_rects(
        board_frames(app)
            .into_iter()
            .filter_map(|f| frame_board_rect(app, f)),
    )
}

/// Ceiling for the fit zoom. Fitting is an explicit "inspect this" intent, so
/// the page should fill the viewport on any screen; the cap only guards against
/// absurd magnification of degenerate (near-empty) bboxes.
const FIT_ZOOM_MAX: f32 = 6.0;

/// The board viewport that fits world-pt `bbox` centered in `screen`.
pub(crate) fn board_fit_bbox(
    (min_x, min_y, max_x, max_y): (f32, f32, f32, f32),
    screen: egui::Rect,
) -> BoardViewport {
    let w = (max_x - min_x).max(1.0);
    let h = (max_y - min_y).max(1.0);
    let zoom = ((screen.width() / w).min(screen.height() / h) * 0.9).clamp(0.1, FIT_ZOOM_MAX);
    BoardViewport {
        zoom,
        pan: [
            (screen.width() - w * zoom) * 0.5 - min_x * zoom,
            (screen.height() - h * zoom) * 0.5 - min_y * zoom,
        ],
        auto_fit: true,
    }
}

/// Like [`board_fit_bbox`], but fits the content into the band below
/// [`FIT_CHROME_PX`] of reserved headroom, so the frame header and the size
/// chip above the page stay on screen at fit zoom.
pub(crate) fn board_fit_bbox_with_chrome(
    bbox: (f32, f32, f32, f32),
    screen: egui::Rect,
) -> BoardViewport {
    let chrome = FIT_CHROME_PX.min(screen.height() * 0.5);
    let inset =
        egui::Rect::from_min_max(egui::pos2(screen.left(), screen.top() + chrome), screen.max);
    let mut vp = board_fit_bbox(bbox, inset);
    // `pan` positions content relative to the full viewport's origin, while the
    // inset centered it assuming its own origin; shift down into the band.
    vp.pan[1] += chrome;
    vp
}

pub(crate) fn x_to_screen(ppm: f64, plot: PlotRect, xmin: f64, xspan: f64, xrev: bool) -> f32 {
    let t = (ppm - xmin) / xspan;
    let n = if xrev { 1.0 - t } else { t };
    plot.left + (n as f32) * plot.width
}

pub(crate) fn screen_to_x(px: f32, plot: PlotRect, xmin: f64, xspan: f64, xrev: bool) -> f64 {
    let n = ((px - plot.left) / plot.width) as f64;
    let t = if xrev { 1.0 - n } else { n };
    xmin + t * xspan
}

pub(crate) fn screen_to_y(py: f32, plot: PlotRect, ymin: f64, yspan: f64, yrev: bool) -> f64 {
    let n = (1.0 - (py - plot.top) / plot.height) as f64;
    let t = if yrev { 1.0 - n } else { n };
    ymin + t * yspan
}

pub(crate) fn y_to_screen(v: f64, plot: PlotRect, ymin: f64, yspan: f64, yrev: bool) -> f32 {
    let t = (v - ymin) / yspan;
    let n = if yrev { 1.0 - t } else { t };
    plot.top + ((1.0 - n) as f32) * plot.height
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum HitZone {
    Plot,
    XAxis,
    YAxis,
    None,
}

pub(crate) fn hit_zone(p: Pos2, rect: egui::Rect, plot: PlotRect) -> HitZone {
    if plot_contains(plot, p) {
        HitZone::Plot
    } else if p.x >= plot.left && p.x <= plot.right() && p.y > plot.bottom() && p.y <= rect.bottom()
    {
        HitZone::XAxis
    } else if p.x >= rect.left() && p.x < plot.left && p.y >= plot.top && p.y <= plot.bottom() {
        HitZone::YAxis
    } else {
        HitZone::None
    }
}

pub(crate) fn plot_contains(plot: PlotRect, p: Pos2) -> bool {
    p.x >= plot.left && p.x <= plot.right() && p.y >= plot.top && p.y <= plot.bottom()
}

pub(crate) fn clamp_to_plot(plot: PlotRect, p: Pos2) -> Pos2 {
    Pos2::new(
        p.x.clamp(plot.left, plot.right()),
        p.y.clamp(plot.top, plot.bottom()),
    )
}

pub(crate) fn plot_rect(plot: PlotRect) -> EguiRect {
    EguiRect::from_min_max(
        Pos2::new(plot.left, plot.top),
        Pos2::new(plot.right(), plot.bottom()),
    )
}

pub(crate) fn pos(p: [f32; 2]) -> Pos2 {
    Pos2::new(p[0], p[1])
}

#[cfg(test)]
mod tests {
    use super::*;
    use plotx_core::state::TextBox;

    fn text_object(id: ObjectId, frame: ObjectFrame) -> CanvasObject {
        CanvasObject {
            id,
            name: format!("o{id}"),
            frame,
            locked: false,
            visible: true,
            group: None,
            kind: CanvasObjectKind::Text(TextBox::label("x".to_owned())),
        }
    }

    fn board_canvas() -> CanvasDocument {
        let mut canvas = CanvasDocument::new("t".to_owned(), [100.0, 200.0]);
        canvas.board_pos = [30.0, 40.0];
        canvas
    }

    fn page_bbox(canvas: &CanvasDocument) -> (f32, f32, f32, f32) {
        let r = canvas.board_rect_pt();
        (r.left, r.top, r.right(), r.bottom())
    }

    #[test]
    fn board_transform_page_rect_matches_origin_pan_boardpos() {
        let canvas = board_canvas();
        let bt = BoardTransform {
            origin: Pos2::new(100.0, 50.0),
            pan: egui::vec2(10.0, -20.0),
            zoom: 2.0,
        };
        let rect = bt.page_screen_rect(&canvas);
        let [w, h] = canvas.size_pt();
        assert!((rect.min.x - (100.0 + 10.0 + 30.0 * 2.0)).abs() < 1e-3);
        assert!((rect.min.y - (50.0 - 20.0 + 40.0 * 2.0)).abs() < 1e-3);
        assert!((rect.width() - w * 2.0).abs() < 1e-3);
        assert!((rect.height() - h * 2.0).abs() < 1e-3);
    }

    #[test]
    fn board_transform_screen_to_page_roundtrip() {
        let canvas = board_canvas();
        let bt = BoardTransform {
            origin: Pos2::new(100.0, 50.0),
            pan: egui::vec2(10.0, -20.0),
            zoom: 2.0,
        };
        let page = bt.page_screen_rect(&canvas);
        let origin_local = bt.screen_to_page(&canvas, page.min);
        assert!(origin_local.x.abs() < 1e-3 && origin_local.y.abs() < 1e-3);
        let [w, h] = canvas.size_pt();
        let center = bt.screen_to_page(&canvas, page.center());
        assert!((center.x - w / 2.0).abs() < 1e-3 && (center.y - h / 2.0).abs() < 1e-3);
    }

    #[test]
    fn board_transform_object_rect_offsets_by_frame() {
        let mut canvas = board_canvas();
        canvas
            .objects
            .push(text_object(5, ObjectFrame::new(12.0, 8.0, 40.0, 20.0)));
        let bt = BoardTransform {
            origin: Pos2::new(100.0, 50.0),
            pan: egui::vec2(10.0, -20.0),
            zoom: 2.0,
        };
        let page = bt.page_screen_rect(&canvas);
        let r = bt.object_screen_rect(&canvas, 5).unwrap();
        assert!((r.left - (page.left() + 12.0 * 2.0)).abs() < 1e-3);
        assert!((r.top - (page.top() + 8.0 * 2.0)).abs() < 1e-3);
        assert!((r.width - 40.0 * 2.0).abs() < 1e-3);
        assert!((r.height - 20.0 * 2.0).abs() < 1e-3);
    }

    #[test]
    fn board_fit_centers_page_and_ignores_board_pos() {
        let screen = egui::Rect::from_min_size(Pos2::new(7.0, 3.0), egui::vec2(1000.0, 800.0));
        let mut canvas = CanvasDocument::new(
            "slide".to_owned(),
            plotx_core::state::PRESENTATION_16X9.size_mm(),
        );

        canvas.board_pos = [0.0, 0.0];
        let vp0 = board_fit_bbox(page_bbox(&canvas), screen);
        let rect0 = BoardTransform::from_board(vp0, screen).page_screen_rect(&canvas);

        assert!((vp0.zoom - 1.25).abs() < 0.001);
        assert!((rect0.min.x - (screen.min.x + 50.0)).abs() < 0.01);
        assert!((rect0.min.y - (screen.min.y + 146.875)).abs() < 0.01);

        canvas.board_pos = [30.0, 40.0];
        let vp1 = board_fit_bbox(page_bbox(&canvas), screen);
        let rect1 = BoardTransform::from_board(vp1, screen).page_screen_rect(&canvas);
        assert!((rect1.min.x - rect0.min.x).abs() < 1e-3);
        assert!((rect1.min.y - rect0.min.y).abs() < 1e-3);
    }

    #[test]
    fn board_fit_fills_large_viewports_beyond_former_cap() {
        // A Nature double-column page on a 2.5K-class canvas area must fill the
        // limiting dimension (90% margin), not stall at the old 1.4 zoom cap.
        let screen = egui::Rect::from_min_size(Pos2::ZERO, egui::vec2(2400.0, 1300.0));
        let canvas = CanvasDocument::new(
            "page".to_owned(),
            plotx_core::state::NATURE_DOUBLE_COLUMN.size_mm(),
        );
        let vp = board_fit_bbox(page_bbox(&canvas), screen);
        let page = BoardTransform::from_board(vp, screen).page_screen_rect(&canvas);
        let fill = (page.width() / screen.width()).max(page.height() / screen.height());
        assert!(vp.zoom > 1.4, "zoom {} should exceed the old cap", vp.zoom);
        assert!(
            (fill - 0.9).abs() < 0.01,
            "limiting fill {fill} should be ~0.9"
        );
    }

    #[test]
    fn board_fit_caps_degenerate_bboxes() {
        let screen = egui::Rect::from_min_size(Pos2::ZERO, egui::vec2(2400.0, 1300.0));
        let vp = board_fit_bbox((0.0, 0.0, 2.0, 2.0), screen);
        assert!((vp.zoom - FIT_ZOOM_MAX).abs() < 1e-6);
    }

    #[test]
    fn bbox_single_canvas_is_its_own_rect() {
        let mut canvas = CanvasDocument::new("page".to_owned(), [120.0, 90.0]);
        canvas.board_pos = [45.0, 60.0];
        let bbox = bbox_of_rects([canvas.board_rect_pt()]).unwrap();
        assert_eq!(bbox, page_bbox(&canvas));
    }

    #[test]
    fn board_fit_bbox_spans_and_centers_all_frames() {
        let screen = egui::Rect::from_min_size(Pos2::new(0.0, 0.0), egui::vec2(1000.0, 800.0));
        let mut a = CanvasDocument::new("a".to_owned(), [100.0, 100.0]);
        a.board_pos = [0.0, 0.0];
        let mut b = CanvasDocument::new("b".to_owned(), [100.0, 100.0]);
        b.board_pos = [400.0, 150.0];
        let canvases = [a, b];

        let bbox = bbox_of_rects(canvases.iter().map(CanvasDocument::board_rect_pt)).unwrap();
        let ra = canvases[0].board_rect_pt();
        let rb = canvases[1].board_rect_pt();
        assert!((bbox.0 - ra.left).abs() < 1e-3);
        assert!((bbox.1 - ra.top).abs() < 1e-3);
        assert!((bbox.2 - rb.right()).abs() < 1e-3);
        assert!((bbox.3 - rb.bottom()).abs() < 1e-3);

        let vp = board_fit_bbox(bbox, screen);
        let bt = BoardTransform::from_board(vp, screen);
        let cx = (bbox.0 + bbox.2) * 0.5;
        let cy = (bbox.1 + bbox.3) * 0.5;
        let sx = screen.min.x + vp.pan[0] + cx * vp.zoom;
        let sy = screen.min.y + vp.pan[1] + cy * vp.zoom;
        assert!((sx - screen.center().x).abs() < 1e-2);
        assert!((sy - screen.center().y).abs() < 1e-2);

        let pa = bt.page_screen_rect(&canvases[0]);
        let pb = bt.page_screen_rect(&canvases[1]);
        assert!((pa.min.x - (screen.min.x + vp.pan[0])).abs() < 1e-2);
        assert!((pb.min.x - (screen.min.x + vp.pan[0] + rb.left * vp.zoom)).abs() < 1e-2);
    }

    #[test]
    fn single_frame_fit_centers_that_frame_at_page_zoom() {
        let screen = egui::Rect::from_min_size(Pos2::new(5.0, 9.0), egui::vec2(1200.0, 700.0));
        let mut canvas = CanvasDocument::new("p".to_owned(), [120.0, 90.0]);
        canvas.board_pos = [640.0, 320.0];
        let r = canvas.board_rect_pt();
        let vp = board_fit_bbox((r.left, r.top, r.right(), r.bottom()), screen);
        let page = BoardTransform::from_board(vp, screen).page_screen_rect(&canvas);
        assert!((page.center().x - screen.center().x).abs() < 1e-2);
        assert!((page.center().y - screen.center().y).abs() < 1e-2);

        let mut origin = CanvasDocument::new("p".to_owned(), [120.0, 90.0]);
        origin.board_pos = [0.0, 0.0];
        let ro = origin.board_rect_pt();
        let vp0 = board_fit_bbox((ro.left, ro.top, ro.right(), ro.bottom()), screen);
        assert!((vp.zoom - vp0.zoom).abs() < 1e-4);
    }

    #[test]
    fn all_frames_bbox_spans_pages_and_sheets() {
        let mut app = PlotxApp::new();
        let mut page = CanvasDocument::new("p".to_owned(), [100.0, 100.0]);
        page.board_pos = [0.0, 0.0];
        app.doc.canvases.push(page);

        let mut sheet = plotx_core::state::materialized_float_series_table(
            ("x".into(), "".into(), vec![Some(0.0), Some(1.0)]),
            Vec::new(),
            "plotx.test.sheet.v1",
        )
        .unwrap();
        sheet.board_pos = [800.0, 200.0];
        app.doc.datasets.push(Dataset::Table(Box::new(sheet)));

        assert_eq!(
            board_frames(&app),
            vec![FrameRef::Page(0), FrameRef::Sheet(0)]
        );

        let page_rect = app.doc.canvases[0].board_rect_pt();
        let sheet_rect = app.doc.datasets[0].as_table().unwrap().board_rect_pt();
        let bbox = all_frames_bbox(&app).unwrap();
        assert!((bbox.0 - page_rect.left).abs() < 1e-3);
        assert!((bbox.1 - page_rect.top).abs() < 1e-3);
        assert!((bbox.2 - sheet_rect.right()).abs() < 1e-3);
        assert!((bbox.3 - page_rect.bottom().max(sheet_rect.bottom())).abs() < 1e-3);
    }

    #[test]
    fn hit_object_returns_frontmost_of_overlap() {
        let mut canvas = CanvasDocument::new("t".to_owned(), [100.0, 100.0]);
        canvas
            .objects
            .push(text_object(1, ObjectFrame::new(10.0, 10.0, 50.0, 50.0)));
        canvas
            .objects
            .push(text_object(2, ObjectFrame::new(20.0, 20.0, 50.0, 50.0)));
        let hit = hit_object(&canvas, Pos2::new(35.0, 35.0), 1.0).unwrap();
        assert_eq!(hit.object, 2);
    }
}
