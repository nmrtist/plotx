use crate::state::{CanvasDocument, Dataset, FrameRef, PlotxApp, TableDataset};
use plotx_render::Rect as PlotRect;

/// World-pt gap kept between board frames by auto-placement and Tidy Up — the
/// guaranteed margin so frames never touch and never leave random empty grid
/// cells. Independent of the coarse drag-snap grid (`BOARD_GRID_PT`); also the
/// magnet offset a dragged frame snaps to just clear of a neighbour.
pub const BOARD_GUTTER_PT: f32 = 96.0;

/// Frames per row in the board's auto-flow and Tidy Up grid.
pub const BOARD_COLS: usize = 3;

/// Every board frame in paint/hit order: all pages, then all table sheets.
pub fn board_frames(app: &PlotxApp) -> Vec<FrameRef> {
    let mut frames: Vec<FrameRef> = (0..app.doc.canvases.len()).map(FrameRef::Page).collect();
    frames.extend(
        app.doc
            .datasets
            .iter()
            .enumerate()
            .filter(|(_, d)| matches!(d, Dataset::Table(_)))
            .map(|(di, _)| FrameRef::Sheet(di)),
    );
    frames
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
            .map(TableDataset::board_rect_pt),
    }
}

/// The board rects (pt) of every page in canvas order — the page auto-flow's
/// existing members.
fn page_rects(app: &PlotxApp) -> Vec<PlotRect> {
    app.doc
        .canvases
        .iter()
        .map(CanvasDocument::board_rect_pt)
        .collect()
}

fn sheet_rects(app: &PlotxApp) -> Vec<PlotRect> {
    app.doc
        .datasets
        .iter()
        .filter_map(Dataset::as_table)
        .map(TableDataset::board_rect_pt)
        .collect()
}

/// The next top-left (pt) for a frame appended to a flow of `existing` rects:
/// flush to the previous frame's right edge plus one gutter, wrapping to a fresh
/// row (below every existing frame) each `BOARD_COLS`. `origin` seeds an empty
/// flow. Reads where frames actually sit, so it stays sensible after manual
/// drags and is prefix-stable: earlier frames never need to move.
fn next_flow_pos(existing: &[PlotRect], origin: [f32; 2]) -> [f32; 2] {
    let n = existing.len();
    if n == 0 {
        return origin;
    }
    if n.is_multiple_of(BOARD_COLS) {
        let bottom = existing.iter().map(|r| r.bottom()).fold(f32::MIN, f32::max);
        [origin[0], bottom + BOARD_GUTTER_PT]
    } else {
        let pred = existing[n - 1];
        [pred.right() + BOARD_GUTTER_PT, pred.top]
    }
}

/// The resting board position (pt) for a newly created page: the next cell of the
/// page flow (a `BOARD_COLS`-wide grid from the origin), flush against the last
/// page plus one gutter.
pub fn next_page_board_pos(app: &PlotxApp) -> [f32; 2] {
    next_flow_pos(&page_rects(app), [0.0, 0.0])
}

/// The resting board position (pt) for a newly created data-table sheet: the next
/// cell of the sheet flow, which sits in its own band one gutter right of the page
/// block so a fresh sheet never lands on a figure while keeping the same gap as
/// between figures.
pub fn next_sheet_board_pos(app: &PlotxApp) -> [f32; 2] {
    let pages = page_rects(app);
    let origin = match pages.iter().map(|r| r.right()).reduce(f32::max) {
        Some(right) => [right + BOARD_GUTTER_PT, 0.0],
        None => [0.0, 0.0],
    };
    next_flow_pos(&sheet_rects(app), origin)
}

pub fn next_sheet_board_pos_after_page(app: &PlotxApp, page: PlotRect) -> [f32; 2] {
    let mut pages = page_rects(app);
    pages.push(page);
    let origin = match pages.iter().map(|r| r.right()).reduce(f32::max) {
        Some(right) => [right + BOARD_GUTTER_PT, 0.0],
        None => [0.0, 0.0],
    };
    next_flow_pos(&sheet_rects(app), origin)
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
    fn next_page_board_pos_flows_flush_with_gutter_and_wraps() {
        let mut app = PlotxApp::new();
        assert_eq!(next_page_board_pos(&app), [0.0, 0.0]);

        for _ in 0..BOARD_COLS {
            let mut c = CanvasDocument::new("p".to_owned(), [100.0, 80.0]);
            c.board_pos = next_page_board_pos(&app);
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
        assert_eq!(next_page_board_pos(&app), [0.0, bottom + BOARD_GUTTER_PT]);
    }

    #[test]
    fn next_sheet_board_pos_sits_in_a_band_right_of_pages() {
        let mut app = PlotxApp::new();
        assert_eq!(next_sheet_board_pos(&app), [0.0, 0.0]);

        let mut p = CanvasDocument::new("p".to_owned(), [100.0, 80.0]);
        p.board_pos = [0.0, 0.0];
        app.doc.canvases.push(p);
        let r = app.doc.canvases[0].board_rect_pt();
        assert_eq!(
            next_sheet_board_pos(&app),
            [r.right() + BOARD_GUTTER_PT, 0.0]
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
}
