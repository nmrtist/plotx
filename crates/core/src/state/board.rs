use crate::state::{BoardFrameId, CanvasDocument, Dataset, FrameRef, PlotxApp, TableDataset};
use plotx_render::Rect as PlotRect;

/// World-pt gap kept between board frames by auto-placement and Tidy Up — the
/// guaranteed margin so frames never touch. It is also the magnet offset a
/// dragged frame snaps to just clear of a neighbour.
pub const BOARD_GUTTER_PT: f32 = 96.0;

/// Frames per row in the board's auto-flow and Tidy Up grid.
pub const BOARD_COLS: usize = 3;

/// Every visible board frame in paint/hit order: all pages, then ordinary table
/// sheets. Live region tables remain first-class datasets but their generated
/// result page is their board representation.
pub fn board_frames(app: &PlotxApp) -> Vec<FrameRef> {
    let mut frames: Vec<FrameRef> = (0..app.doc.canvases.len()).map(FrameRef::Page).collect();
    frames.extend(
        app.doc
            .datasets
            .iter()
            .enumerate()
            .filter(|(_, d)| d.as_table().is_some_and(TableDataset::board_sheet_visible))
            .map(|(di, _)| FrameRef::Sheet(di)),
    );
    frames
}

/// Convert an immediate collection reference into the stable identity used by
/// queued reveal and fit work. Hidden calculated sheets are not board frames.
pub fn board_frame_id(app: &PlotxApp, frame: FrameRef) -> Option<BoardFrameId> {
    match frame {
        FrameRef::Page(ci) => app
            .doc
            .canvases
            .get(ci)
            .map(|canvas| BoardFrameId::Page(canvas.resource_id)),
        FrameRef::Sheet(di) => app
            .doc
            .datasets
            .get(di)
            .and_then(Dataset::as_table)
            .filter(|table| table.board_sheet_visible())
            .map(|table| BoardFrameId::Sheet(table.resource_id)),
    }
}

/// Resolve stable identity for one immediate board lookup. A removed target or
/// a table that is no longer a visible sheet resolves to `None`.
pub fn board_frame_ref(app: &PlotxApp, frame: BoardFrameId) -> Option<FrameRef> {
    match frame {
        BoardFrameId::Page(id) => app.doc.canvas_index(id).map(FrameRef::Page),
        BoardFrameId::Sheet(id) => app
            .doc
            .dataset_index(id)
            .filter(|&di| {
                app.doc.datasets[di]
                    .as_table()
                    .is_some_and(TableDataset::board_sheet_visible)
            })
            .map(FrameRef::Sheet),
    }
}

/// The board rect (pt) of any frame — a page or a table sheet. `None` if the
/// index is stale or a `Sheet` ref no longer points at a table.
pub fn frame_board_rect(app: &PlotxApp, frame: FrameRef) -> Option<PlotRect> {
    match frame {
        FrameRef::Page(ci) => app.doc.canvases.get(ci).map(CanvasDocument::board_rect_pt),
        FrameRef::Sheet(di) => app
            .doc
            .datasets
            .get(di)
            .and_then(Dataset::as_table)
            .filter(|table| table.board_sheet_visible())
            .map(TableDataset::board_rect_pt),
    }
}

fn rects_with_extra(app: &PlotxApp, extra: &[PlotRect]) -> Vec<PlotRect> {
    board_frames(app)
        .into_iter()
        .filter_map(|frame| frame_board_rect(app, frame))
        .chain(extra.iter().copied())
        .collect()
}

fn separated(a: PlotRect, b: PlotRect, gutter: f32) -> bool {
    a.right() + gutter <= b.left
        || b.right() + gutter <= a.left
        || a.bottom() + gutter <= b.top
        || b.bottom() + gutter <= a.top
}

/// Find a collision-free row-major position for one new visible frame. Rows are
/// tried from top to bottom and each row from left to right, with at most
/// [`BOARD_COLS`] occupied slots before wrapping. Existing frames never move.
pub fn next_board_frame_pos(app: &PlotxApp, size: [f32; 2]) -> [f32; 2] {
    next_board_frame_pos_with_extra(app, size, &[])
}

/// As [`next_board_frame_pos`], while also reserving frames that will be inserted
/// by the same compound operation (for example a result page plus a table sheet).
pub fn next_board_frame_pos_with_extra(
    app: &PlotxApp,
    size: [f32; 2],
    extra: &[PlotRect],
) -> [f32; 2] {
    let existing = rects_with_extra(app, extra);
    if existing.is_empty() {
        return [0.0, 0.0];
    }
    let mut rows = vec![0.0];
    rows.extend(existing.iter().map(|rect| rect.bottom() + BOARD_GUTTER_PT));
    rows.sort_by(f32::total_cmp);
    rows.dedup_by(|a, b| (*a - *b).abs() < f32::EPSILON);

    for top in rows {
        let mut blockers = existing
            .iter()
            .copied()
            .filter(|rect| {
                top < rect.bottom() + BOARD_GUTTER_PT && top + size[1] + BOARD_GUTTER_PT > rect.top
            })
            .collect::<Vec<_>>();
        blockers.sort_by(|a, b| a.left.total_cmp(&b.left));
        if blockers.len() >= BOARD_COLS {
            continue;
        }
        let mut left = 0.0;
        loop {
            let candidate = PlotRect::new(left, top, size[0], size[1]);
            let Some(blocker) = blockers
                .iter()
                .find(|other| !separated(candidate, **other, BOARD_GUTTER_PT))
            else {
                if existing
                    .iter()
                    .all(|other| separated(candidate, *other, BOARD_GUTTER_PT))
                {
                    return [left, top];
                }
                break;
            };
            left = blocker.right() + BOARD_GUTTER_PT;
        }
    }

    let top = existing
        .iter()
        .map(|rect| rect.bottom())
        .fold(0.0, f32::max)
        + BOARD_GUTTER_PT;
    [0.0, top]
}

/// Row-major top-lefts (pt) for frames of the given `sizes` packed into `cols`
/// columns with `gutter` spacing. Each column's x aligns to its widest frame and
/// each row's y to its tallest, so the result is a cleanly aligned matrix.
fn grid_positions(sizes: &[[f32; 2]], cols: usize, gutter: f32) -> Vec<[f32; 2]> {
    if sizes.is_empty() || cols == 0 {
        return Vec::new();
    }
    let rows = sizes.len().div_ceil(cols);
    let mut col_w = vec![0.0f32; cols];
    let mut row_h = vec![0.0f32; rows];
    for (i, s) in sizes.iter().enumerate() {
        col_w[i % cols] = col_w[i % cols].max(s[0]);
        row_h[i / cols] = row_h[i / cols].max(s[1]);
    }
    let mut col_x = vec![0.0f32; cols];
    for c in 1..cols {
        col_x[c] = col_x[c - 1] + col_w[c - 1] + gutter;
    }
    let mut row_y = vec![0.0f32; rows];
    for r in 1..rows {
        row_y[r] = row_y[r - 1] + row_h[r - 1] + gutter;
    }
    (0..sizes.len())
        .map(|i| [col_x[i % cols], row_y[i / cols]])
        .collect()
}

/// A perfectly aligned board layout for every frame: a `BOARD_COLS`-wide,
/// row-major matrix (pages then sheets, in `board_frames` order) with one gutter
/// between frames. Returns each frame paired with its new top-left (pt) — the
/// input for an undoable Tidy Up.
pub fn tidy_board_layout(app: &PlotxApp) -> Vec<(FrameRef, [f32; 2])> {
    let mut refs = Vec::new();
    let mut sizes = Vec::new();
    for f in board_frames(app) {
        if let Some(r) = frame_board_rect(app, f) {
            refs.push(f);
            sizes.push([r.right() - r.left, r.bottom() - r.top]);
        }
    }
    refs.into_iter()
        .zip(grid_positions(&sizes, BOARD_COLS, BOARD_GUTTER_PT))
        .collect()
}

/// The first page frame that plots dataset `di` (by its primary binding), used
/// for semantic jumps between an extracted table, its source spectrum, and its
/// fit chart.
pub fn page_frame_showing_dataset(app: &PlotxApp, di: usize) -> Option<FrameRef> {
    let dataset_id = app.doc.datasets.get(di)?.resource_id();
    app.doc
        .canvases
        .iter()
        .position(|c| c.objects.iter().any(|o| o.dataset() == Some(dataset_id)))
        .map(FrameRef::Page)
}

/// This frame's board position (pt), or `None` for a stale ref.
pub fn frame_board_pos(app: &PlotxApp, frame: FrameRef) -> Option<[f32; 2]> {
    match frame {
        FrameRef::Page(ci) => app.doc.canvases.get(ci).map(|c| c.board_pos),
        FrameRef::Sheet(di) => app
            .doc
            .datasets
            .get(di)
            .and_then(Dataset::as_table)
            .map(|t| t.board_pos),
    }
}

/// Move this frame to board position `pos` (pt); no-op for a stale ref.
pub fn set_frame_board_pos(app: &mut PlotxApp, frame: FrameRef, pos: [f32; 2]) {
    match frame {
        FrameRef::Page(ci) => {
            if let Some(c) = app.doc.canvases.get_mut(ci) {
                c.board_pos = pos;
            }
        }
        FrameRef::Sheet(di) => {
            if let Some(t) = app.doc.datasets.get_mut(di).and_then(Dataset::as_table_mut) {
                t.board_pos = pos;
            }
        }
    }
}

impl PlotxApp {
    /// Activate, select, and enqueue a one-shot animated reveal of a visible
    /// board frame. Creation paths can call this without an egui dependency.
    pub fn reveal_board_frame(&mut self, frame: FrameRef) {
        let Some(frame_id) = board_frame_id(self, frame) else {
            return;
        };
        match frame {
            FrameRef::Page(ci) => self.activate_canvas(ci),
            FrameRef::Sheet(di) => self.focus_single(di),
        }
        self.session.view = crate::state::PrimaryView::Canvas;
        self.session.ui.frame_selection = vec![frame];
        self.session.board_reveal = Some(frame_id);
    }
}

/// Add or remove a frame from the multi-select set (Shift/Ctrl-click).
pub fn toggle_frame_selection(app: &mut PlotxApp, frame: FrameRef) {
    if let Some(pos) = app
        .session
        .ui
        .frame_selection
        .iter()
        .position(|&f| f == frame)
    {
        app.session.ui.frame_selection.remove(pos);
    } else {
        app.session.ui.frame_selection.push(frame);
    }
}

/// Toggle a frame in the multi-select and mirror the whole selection into the
/// Data list, so pages/sheets picked in the workspace can be stacked without
/// re-selecting their datasets. Used by the board and the sidebar canvas list;
/// the Data list drives its own selection, so it toggles directly.
pub fn toggle_frame_selection_synced(app: &mut PlotxApp, frame: FrameRef) {
    toggle_frame_selection(app, frame);
    sync_data_selection_from_frames(app);
}

/// Rebuild the Data-list selection from the multi-selected frames (union of each
/// page's datasets plus any sheets). The active dataset is the set's lead, so it
/// can no longer point outside the multi-select the Stack command counts.
fn sync_data_selection_from_frames(app: &mut PlotxApp) {
    let frames = app.session.ui.frame_selection.clone();
    let mut datasets: Vec<usize> = Vec::new();
    for frame in frames {
        let indices = match frame {
            FrameRef::Page(ci) => app.doc.page_dataset_indices(ci),
            FrameRef::Sheet(di) => vec![di],
        };
        for di in indices {
            if !datasets.contains(&di) {
                datasets.push(di);
            }
        }
    }
    app.focus_datasets(&datasets, None);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::materialized_float_series_table;

    #[test]
    fn next_board_frame_pos_flows_with_gutter_and_wraps() {
        let mut app = PlotxApp::new();
        let size = CanvasDocument::new("p".to_owned(), [100.0, 80.0]).size_pt();
        assert_eq!(next_board_frame_pos(&app, size), [0.0, 0.0]);

        for _ in 0..BOARD_COLS {
            let mut c = CanvasDocument::new("p".to_owned(), [100.0, 80.0]);
            c.board_pos = next_board_frame_pos(&app, c.size_pt());
            app.doc.canvases.push(c);
        }
        let r0 = app.doc.canvases[0].board_rect_pt();
        assert_eq!(
            app.doc.canvases[1].board_pos,
            [r0.right() + BOARD_GUTTER_PT, r0.top]
        );
        // The row filled BOARD_COLS; the next page wraps below the lowest edge.
        let bottom = app
            .doc
            .canvases
            .iter()
            .map(|c| c.board_rect_pt().bottom())
            .fold(f32::MIN, f32::max);
        assert_eq!(
            next_board_frame_pos(&app, size),
            [0.0, bottom + BOARD_GUTTER_PT]
        );
    }

    #[test]
    fn placement_considers_visible_pages_and_sheets_together() {
        let mut app = PlotxApp::new();
        let mut p = CanvasDocument::new("p".to_owned(), [100.0, 80.0]);
        p.board_pos = [0.0, 0.0];
        app.doc.canvases.push(p);

        let mut sheet = materialized_float_series_table(
            ("x".into(), "".into(), vec![Some(0.0), Some(1.0)]),
            Vec::new(),
            "plotx.test.board-placement.v1",
        )
        .unwrap();
        let sheet_size = sheet.board_rect_pt();
        sheet.board_pos = next_board_frame_pos(&app, [sheet_size.width, sheet_size.height]);
        app.doc.datasets.push(Dataset::Table(Box::new(sheet)));

        let page_size = app.doc.canvases[0].size_pt();
        let position = next_board_frame_pos(&app, page_size);
        let candidate = PlotRect::new(position[0], position[1], page_size[0], page_size[1]);
        assert_eq!(
            position,
            [
                app.doc.datasets[0]
                    .as_table()
                    .unwrap()
                    .board_rect_pt()
                    .right()
                    + BOARD_GUTTER_PT,
                0.0
            ]
        );
        assert!(
            board_frames(&app)
                .into_iter()
                .filter_map(|frame| frame_board_rect(&app, frame))
                .all(|rect| separated(candidate, rect, 0.0))
        );
    }

    #[test]
    fn grid_positions_aligns_columns_and_rows() {
        // Row-major into 2 columns: col 0 widest = 100, col 1 = 60; rows tall 40 & 30.
        let sizes = [[100.0, 40.0], [60.0, 20.0], [50.0, 30.0]];
        let pos = grid_positions(&sizes, 2, 10.0);
        assert_eq!(pos[0], [0.0, 0.0]);
        assert_eq!(pos[1], [100.0 + 10.0, 0.0]);
        assert_eq!(pos[2], [0.0, 40.0 + 10.0]);
    }

    #[test]
    fn tidy_board_layout_orders_pages_then_sheets_from_origin() {
        let mut app = PlotxApp::new();
        let mut a = CanvasDocument::new("a".to_owned(), [100.0, 80.0]);
        a.board_pos = [500.0, 500.0];
        app.doc.canvases.push(a);
        let mut sheet = materialized_float_series_table(
            ("x".into(), "".into(), vec![Some(0.0), Some(1.0)]),
            Vec::new(),
            "plotx.test.board-sheet.v1",
        )
        .unwrap();
        sheet.board_pos = [0.0, 0.0];
        app.doc.datasets.push(Dataset::Table(Box::new(sheet)));

        let layout = tidy_board_layout(&app);
        assert_eq!(layout[0].0, FrameRef::Page(0));
        assert_eq!(layout[0].1, [0.0, 0.0]);
        assert_eq!(layout[1].0, FrameRef::Sheet(0));
        let page_w = {
            let r = app.doc.canvases[0].board_rect_pt();
            r.right() - r.left
        };
        assert_eq!(layout[1].1, [page_w + BOARD_GUTTER_PT, 0.0]);
    }

    #[test]
    fn reveal_request_activates_and_selects_a_visible_frame() {
        let mut app = PlotxApp::new();
        app.doc
            .canvases
            .push(CanvasDocument::new("source".to_owned(), [100.0, 80.0]));
        app.doc
            .canvases
            .push(CanvasDocument::new("result".to_owned(), [100.0, 80.0]));
        let result_id = app.doc.canvases[1].resource_id;
        app.session.active_canvas = Some(0);
        app.session.ui.selection = crate::state::Selection::single(crate::state::ObjectId::new(1));

        app.reveal_board_frame(FrameRef::Page(1));

        assert_eq!(app.session.active_canvas, Some(1));
        assert_eq!(app.session.ui.selection, crate::state::Selection::None);
        assert_eq!(app.session.ui.frame_selection, vec![FrameRef::Page(1)]);
        assert_eq!(
            app.session.board_reveal,
            Some(BoardFrameId::Page(result_id))
        );
    }

    #[test]
    fn stable_sheet_identity_survives_dataset_index_changes() {
        let mut app = PlotxApp::new();
        for _ in 0..2 {
            let table = materialized_float_series_table(
                ("x".into(), "".into(), vec![Some(0.0)]),
                Vec::new(),
                "plotx.test.stable-board-sheet.v1",
            )
            .unwrap();
            app.doc.datasets.push(Dataset::Table(Box::new(table)));
        }
        let target_id = app.doc.datasets[1].resource_id();
        let target = board_frame_id(&app, FrameRef::Sheet(1)).unwrap();
        assert_eq!(target, BoardFrameId::Sheet(target_id));

        app.doc.datasets.swap(0, 1);
        assert_eq!(board_frame_ref(&app, target), Some(FrameRef::Sheet(0)));

        app.doc.datasets.remove(0);
        assert_eq!(board_frame_ref(&app, target), None);
    }
}
