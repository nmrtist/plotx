use super::*;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpacingMode {
    Frame,
    #[default]
    Visual,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GutterPreset {
    Tight,
    Normal,
    Spacious,
}

impl GutterPreset {
    pub const ALL: [Self; 3] = [Self::Tight, Self::Normal, Self::Spacious];

    pub const fn millimetres(self) -> f32 {
        match self {
            Self::Tight => 2.0,
            Self::Normal => 5.0,
            Self::Spacious => 10.0,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Tight => "Tight",
            Self::Normal => "Normal",
            Self::Spacious => "Spacious",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LayoutItem {
    pub id: ObjectId,
    /// Axis furniture insets in pt, ordered top, right, bottom, left.
    pub insets: [f32; 4],
}

pub fn layout_item(id: ObjectId, figure: &plotx_figure::Figure, frame: ObjectFrame) -> LayoutItem {
    let margins = plotx_render::axis_layout(figure, frame.width, frame.height).margins;
    LayoutItem {
        id,
        insets: [margins.top, margins.right, margins.bottom, margins.left],
    }
}

/// Tolerance for the small coordinate drift produced by PlotX auto-layout and
/// drag-tiling floating-point calculations.
const GRID_ALIGNMENT_TOLERANCE_PT: f32 = 1.0;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OccupiedGrid {
    pub rows: u32,
    pub cols: u32,
    pub ids: Vec<ObjectId>,
}

/// Infer a validated row-major occupied grid from aligned object frames. This
/// is used only by standalone commands, where persisted layout divisions may
/// not describe frames produced by drag-tiling.
pub fn infer_occupied_grid(frames: &[(ObjectId, ObjectFrame)]) -> Option<OccupiedGrid> {
    if frames.is_empty() {
        return None;
    }
    let (rows, row_for) = coordinate_clusters(frames, |frame| frame.y);
    let (cols, col_for) = coordinate_clusters(frames, |frame| frame.x);
    let mut cells = vec![None; rows.checked_mul(cols)?];
    for (index, &(id, _)) in frames.iter().enumerate() {
        let cell = row_for[index]
            .checked_mul(cols)?
            .checked_add(col_for[index])?;
        if cells[cell].replace(id).is_some() {
            return None;
        }
    }
    let ids: Option<Vec<_>> = cells.into_iter().take(frames.len()).collect();
    let ids = ids?;
    Some(OccupiedGrid {
        rows: u32::try_from(rows).ok()?,
        cols: u32::try_from(cols).ok()?,
        ids,
    })
}

fn coordinate_clusters(
    frames: &[(ObjectId, ObjectFrame)],
    coordinate: impl Fn(ObjectFrame) -> f32,
) -> (usize, Vec<usize>) {
    let mut ordered: Vec<_> = frames
        .iter()
        .enumerate()
        .map(|(index, (_, frame))| (index, coordinate(*frame)))
        .collect();
    ordered.sort_by(|a, b| a.1.total_cmp(&b.1));
    let mut assignments = vec![0; frames.len()];
    let mut cluster = 0;
    let mut anchor = ordered[0].1;
    for (position, &(index, value)) in ordered.iter().enumerate() {
        if position > 0 && (value - anchor).abs() > GRID_ALIGNMENT_TOLERANCE_PT {
            cluster += 1;
            anchor = value;
        }
        assignments[index] = cluster;
    }
    (cluster + 1, assignments)
}

/// Arrange occupied cells while interpreting `gutter_mm` as either frame gap
/// or minimum adjacent data-area clearance. Empty cells do not create phantom
/// inset requirements.
pub fn arrange_grid(
    page_pt: [f32; 2],
    layout: &PageLayout,
    items: &[LayoutItem],
) -> Vec<(ObjectId, ObjectFrame)> {
    if layout.spacing_mode == SpacingMode::Frame {
        let ids: Vec<ObjectId> = items.iter().map(|item| item.id).collect();
        return assign_grid(page_pt, layout, &ids);
    }
    let rows = layout.rows.max(1) as usize;
    let cols = layout.cols.max(1) as usize;
    let occupied = items.len().min(rows * cols);
    let gutter = layout.gutter_pt();
    let mut col_gaps = vec![0.0_f32; cols.saturating_sub(1)];
    let mut row_gaps = vec![0.0_f32; rows.saturating_sub(1)];
    for index in 0..occupied {
        let row = index / cols;
        let col = index % cols;
        if col + 1 < cols && index + 1 < occupied {
            col_gaps[col] = col_gaps[col]
                .max((gutter - items[index].insets[1] - items[index + 1].insets[3]).max(0.0));
        }
        let below = index + cols;
        if row + 1 < rows && below < occupied {
            row_gaps[row] = row_gaps[row]
                .max((gutter - items[index].insets[2] - items[below].insets[0]).max(0.0));
        }
    }
    let [mt, mr, mb, ml] = layout.margins_pt();
    let left = ml.clamp(0.0, page_pt[0].max(0.0));
    let top = mt.clamp(0.0, page_pt[1].max(0.0));
    let available_w = (page_pt[0] - left - mr.max(0.0)).max(0.0);
    let available_h = (page_pt[1] - top - mb.max(0.0)).max(0.0);
    fit_gaps(&mut col_gaps, (available_w - cols as f32).max(0.0));
    fit_gaps(&mut row_gaps, (available_h - rows as f32).max(0.0));
    let width = available_w - col_gaps.iter().sum::<f32>();
    let height = available_h - row_gaps.iter().sum::<f32>();
    let cell_w = width / cols as f32;
    let cell_h = height / rows as f32;
    let mut x = vec![left; cols];
    let mut y = vec![top; rows];
    for col in 1..cols {
        x[col] = x[col - 1] + cell_w + col_gaps[col - 1];
    }
    for row in 1..rows {
        y[row] = y[row - 1] + cell_h + row_gaps[row - 1];
    }
    items
        .iter()
        .take(occupied)
        .enumerate()
        .map(|(index, item)| {
            let row = index / cols;
            let col = index % cols;
            (item.id, ObjectFrame::new(x[col], y[row], cell_w, cell_h))
        })
        .collect()
}

fn fit_gaps(gaps: &mut [f32], available: f32) {
    let total = gaps.iter().sum::<f32>();
    if total > available && total > 0.0 {
        let scale = available / total;
        for gap in gaps {
            *gap *= scale;
        }
    }
}

/// Row-major cells that retain axis text when inner axes are simplified.
pub fn outer_axis_cells(item_count: usize, rows: u32, cols: u32) -> Vec<(bool, bool)> {
    let capacity = rows.max(1) as usize * cols.max(1) as usize;
    let count = item_count.min(capacity);
    let cols = cols.max(1) as usize;
    (0..count)
        .map(|index| {
            let row_start = index / cols * cols;
            (index + cols >= count, index == row_start)
        })
        .collect()
}

pub fn compute_tiling_plan_for_items(
    page_pt: [f32; 2],
    layout: &PageLayout,
    existing_items: &[LayoutItem],
    newcomer: LayoutItem,
    pointer_page: [f32; 2],
) -> TilingPlan {
    if layout.spacing_mode == SpacingMode::Frame {
        let ids: Vec<ObjectId> = existing_items.iter().map(|item| item.id).collect();
        return compute_tiling_plan(page_pt, layout, &ids, pointer_page);
    }
    match existing_items.len() {
        0 => TilingPlan {
            newcomer: arrange_grid(page_pt, layout, &[newcomer])
                .first()
                .map(|(_, frame)| *frame)
                .unwrap_or_else(|| ObjectFrame::new(0.0, 0.0, page_pt[0], page_pt[1])),
            existing: Vec::new(),
        },
        1 => split_plan(page_pt, layout, existing_items[0], newcomer, pointer_page),
        _ => {
            let (rows, cols) = even_grid_dims(existing_items.len() + 1);
            let grid_layout = PageLayout {
                rows,
                cols,
                ..*layout
            };
            let mut items = existing_items.to_vec();
            items.push(newcomer);
            let mut frames = arrange_grid(page_pt, &grid_layout, &items);
            let newcomer = frames
                .pop()
                .map(|(_, frame)| frame)
                .unwrap_or_else(|| ObjectFrame::new(0.0, 0.0, page_pt[0], page_pt[1]));
            TilingPlan {
                newcomer,
                existing: frames,
            }
        }
    }
}

fn split_plan(
    page_pt: [f32; 2],
    layout: &PageLayout,
    existing: LayoutItem,
    newcomer: LayoutItem,
    pointer: [f32; 2],
) -> TilingPlan {
    let [w, h] = page_pt;
    let nx = if w > 0.0 { pointer[0] / w } else { 0.5 };
    let ny = if h > 0.0 { pointer[1] / h } else { 0.5 };
    let horizontal = (nx - 0.5).abs() >= (ny - 0.5).abs();
    let newcomer_last = if horizontal { nx >= 0.5 } else { ny >= 0.5 };
    let split_layout = PageLayout {
        rows: if horizontal { 1 } else { 2 },
        cols: if horizontal { 2 } else { 1 },
        ..*layout
    };
    let ordered = if newcomer_last {
        [existing, newcomer]
    } else {
        [newcomer, existing]
    };
    let frames = arrange_grid(page_pt, &split_layout, &ordered);
    let newcomer_index = usize::from(newcomer_last);
    let newcomer = frames
        .get(newcomer_index)
        .map(|(_, frame)| *frame)
        .unwrap_or_else(|| ObjectFrame::new(0.0, 0.0, page_pt[0], page_pt[1]));
    let existing = frames
        .get(1 - newcomer_index)
        .copied()
        .into_iter()
        .collect();
    TilingPlan { newcomer, existing }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(id: ObjectId, inset: f32) -> LayoutItem {
        LayoutItem {
            id,
            insets: [inset; 4],
        }
    }

    #[test]
    fn split_retile_and_apply_grid_share_visual_geometry() {
        let page = [400.0, 300.0];
        let layout = PageLayout {
            rows: 1,
            cols: 2,
            ..PageLayout::default()
        };
        let existing = item(1, 8.0);
        let newcomer = item(2, 4.0);
        let split =
            compute_tiling_plan_for_items(page, &layout, &[existing], newcomer, [390.0, 150.0]);
        let apply = arrange_grid(page, &layout, &[existing, newcomer]);
        assert_eq!(split.existing[0].1, apply[0].1);
        assert_eq!(split.newcomer, apply[1].1);

        let third = item(3, 6.0);
        let retile = compute_tiling_plan_for_items(
            page,
            &layout,
            &[existing, newcomer],
            third,
            [10.0, 10.0],
        );
        let grid = PageLayout {
            rows: 2,
            cols: 2,
            ..layout
        };
        let apply = arrange_grid(page, &grid, &[existing, newcomer, third]);
        assert_eq!(retile.existing, apply[..2]);
        assert_eq!(retile.newcomer, apply[2].1);
    }

    #[test]
    fn impossible_requested_gap_still_keeps_frames_inside_page() {
        let layout = PageLayout {
            rows: 1,
            cols: 3,
            gutter_mm: 100.0,
            ..PageLayout::default()
        };
        let frames = arrange_grid(
            [100.0, 50.0],
            &layout,
            &[item(1, 0.0), item(2, 0.0), item(3, 0.0)],
        );
        assert!(
            frames
                .windows(2)
                .all(|pair| pair[0].1.x + pair[0].1.width <= pair[1].1.x)
        );
        assert!(
            frames
                .iter()
                .all(|(_, frame)| frame.x >= 0.0 && frame.x + frame.width <= 100.001)
        );
    }

    fn frame(id: ObjectId, col: u32, row: u32) -> (ObjectId, ObjectFrame) {
        (
            id,
            ObjectFrame::new(col as f32 * 20.0, row as f32 * 30.0, 10.0, 10.0),
        )
    }

    #[test]
    fn occupied_grid_accepts_complete_two_by_two() {
        let grid = infer_occupied_grid(&[
            frame(4, 1, 1),
            frame(2, 1, 0),
            frame(3, 0, 1),
            frame(1, 0, 0),
        ])
        .unwrap();
        assert_eq!((grid.rows, grid.cols), (2, 2));
        assert_eq!(grid.ids, vec![1, 2, 3, 4]);
    }

    #[test]
    fn occupied_grid_accepts_partial_last_row_in_row_major_order() {
        let grid = infer_occupied_grid(&[
            frame(5, 1, 1),
            frame(3, 2, 0),
            frame(1, 0, 0),
            frame(4, 0, 1),
            frame(2, 1, 0),
        ])
        .unwrap();
        assert_eq!((grid.rows, grid.cols), (2, 3));
        assert_eq!(grid.ids, vec![1, 2, 3, 4, 5]);
    }

    #[test]
    fn occupied_grid_accepts_single_row_and_single_column() {
        let row = infer_occupied_grid(&[frame(2, 1, 0), frame(1, 0, 0), frame(3, 2, 0)]).unwrap();
        assert_eq!((row.rows, row.cols, row.ids), (1, 3, vec![1, 2, 3]));
        let column =
            infer_occupied_grid(&[frame(3, 0, 2), frame(1, 0, 0), frame(2, 0, 1)]).unwrap();
        assert_eq!(
            (column.rows, column.cols, column.ids),
            (3, 1, vec![1, 2, 3])
        );
    }

    #[test]
    fn occupied_grid_rejects_diagonal_scatter() {
        assert!(infer_occupied_grid(&[frame(1, 0, 0), frame(2, 1, 1)]).is_none());
    }

    #[test]
    fn occupied_grid_rejects_a_hole_before_a_later_cell() {
        assert!(infer_occupied_grid(&[frame(1, 0, 0), frame(2, 2, 0), frame(3, 1, 1)]).is_none());
    }

    #[test]
    fn occupied_grid_rejects_two_objects_in_one_tolerance_cell() {
        let frames = [
            (1, ObjectFrame::new(0.0, 0.0, 10.0, 10.0)),
            (2, ObjectFrame::new(0.5, 0.5, 10.0, 10.0)),
        ];
        assert!(infer_occupied_grid(&frames).is_none());
    }
}
