//! Content-fit helpers for pages: bounding boxes, overflow detection, uniform
//! content scaling for canvas-size changes, and the auto-height / preset-id
//! reconciliation that runs outside the undo stack.

use crate::state::{
    CanvasDocument, MM_TO_PT, ObjectFrame, ObjectId, PlotxApp, SizePreset, matching_preset,
    preset_by_id, size_presets,
};

/// Height floor for auto-height pages so an empty or near-empty page stays
/// grabbable. Matches the lower bound of the manual size drag range.
pub const AUTO_HEIGHT_MIN_MM: f32 = 20.0;
/// Height ceiling when no preset constrains the page; matches the manual drag
/// range upper bound.
const AUTO_HEIGHT_MAX_MM: f32 = 1000.0;
/// Slack (pt) before geometry counts as outside the page, absorbing float
/// wobble from unit round-trips.
const OVERFLOW_SLACK_PT: f32 = 0.5;

/// Union of the visible objects' frames in page pt as `[min_x, min_y, max_x,
/// max_y]`, or `None` for a page with no visible content.
pub fn content_bounds_pt(canvas: &CanvasDocument) -> Option<[f32; 4]> {
    let mut bounds: Option<[f32; 4]> = None;
    for object in canvas.objects.iter().filter(|o| o.visible) {
        let f = object.frame;
        let b = bounds.get_or_insert([f.x, f.y, f.x + f.width, f.y + f.height]);
        b[0] = b[0].min(f.x);
        b[1] = b[1].min(f.y);
        b[2] = b[2].max(f.x + f.width);
        b[3] = b[3].max(f.y + f.height);
    }
    bounds
}

/// True when visible content extends past any page edge.
pub fn content_overflows(canvas: &CanvasDocument) -> bool {
    let Some([min_x, min_y, max_x, max_y]) = content_bounds_pt(canvas) else {
        return false;
    };
    let page_w = canvas.size_mm[0] * MM_TO_PT;
    let page_h = canvas.size_mm[1] * MM_TO_PT;
    min_x < -OVERFLOW_SLACK_PT
        || min_y < -OVERFLOW_SLACK_PT
        || max_x > page_w + OVERFLOW_SLACK_PT
        || max_y > page_h + OVERFLOW_SLACK_PT
}

/// `(before, after)` frame lists in the shape `Action::set_object_frames` takes.
pub type FramePairs = (Vec<(ObjectId, ObjectFrame)>, Vec<(ObjectId, ObjectFrame)>);

/// Every object frame scaled uniformly by the width ratio of a size change,
/// anchored at the page origin, as `(before, after)` pairs for
/// `Action::set_object_frames`. Font sizes are deliberately untouched: they are
/// physical pt values that journal compliance is checked against.
pub fn scaled_frames(
    canvas: &CanvasDocument,
    before_mm: [f32; 2],
    after_mm: [f32; 2],
) -> Option<FramePairs> {
    let scale = content_scale_factor(before_mm, after_mm)?;
    let before: Vec<_> = canvas.objects.iter().map(|o| (o.id, o.frame)).collect();
    if before.is_empty() {
        return None;
    }
    let after = before
        .iter()
        .map(|&(id, f)| {
            (
                id,
                ObjectFrame::new(f.x * scale, f.y * scale, f.width * scale, f.height * scale),
            )
        })
        .collect();
    Some((before, after))
}

/// The uniform content scale implied by a width change, or `None` when the
/// change is degenerate or a no-op.
pub fn content_scale_factor(before_mm: [f32; 2], after_mm: [f32; 2]) -> Option<f32> {
    if before_mm[0] <= 0.0 || after_mm[0] <= 0.0 {
        return None;
    }
    let scale = after_mm[0] / before_mm[0];
    ((scale - 1.0).abs() > f32::EPSILON).then_some(scale)
}

/// The page height (mm) an auto-height page should have: content bottom plus
/// the layout's bottom margin, clamped to the floor and to the preset's
/// maximum figure depth when one is known. `None` when the page has no visible
/// content (the current height is kept rather than collapsing the page).
pub fn auto_height_mm(canvas: &CanvasDocument) -> Option<f32> {
    let [_, _, _, max_y] = content_bounds_pt(canvas)?;
    let target = max_y / MM_TO_PT + canvas.layout.margin_mm[2];
    let cap = matching_preset(canvas.size_mm, canvas.size_preset_id.as_deref())
        .and_then(|preset| preset.max_height_mm)
        .unwrap_or(AUTO_HEIGHT_MAX_MM);
    Some(target.clamp(AUTO_HEIGHT_MIN_MM, cap))
}

/// The wider preset worth suggesting for a page: set when the page currently
/// matches a journal single-column width but its layout grid asks for two or
/// more columns of panels. Returns the widest preset of the same journal
/// family. Purely advisory — callers surface it as a dismissible hint, never
/// apply it on their own.
pub fn wider_preset_suggestion(canvas: &CanvasDocument) -> Option<&'static SizePreset> {
    if canvas.layout.cols < 2 {
        return None;
    }
    let current = matching_preset(canvas.size_mm, canvas.size_preset_id.as_deref())?;
    let family = current.id.strip_suffix("-1col")?;
    size_presets()
        .iter()
        .filter(|p| p.id.starts_with(family) && p.width_mm > current.width_mm)
        .max_by(|a, b| a.width_mm.total_cmp(&b.width_mm))
}

impl PlotxApp {
    /// Reconciles derived page-size state for every page. Runs once per UI
    /// frame, outside the undo stack: an auto-height page's height and a stale
    /// preset id are functions of the page content, so they re-derive
    /// themselves after any action, undo, or redo instead of polluting the
    /// history with follow-up entries.
    pub fn reconcile_page_fit(&mut self) {
        for ci in 0..self.doc.canvases.len() {
            let canvas = &mut self.doc.canvases[ci];
            if let Some(id) = canvas.size_preset_id.as_deref()
                && !preset_by_id(id).is_some_and(|preset| preset.matches(canvas.size_mm))
            {
                canvas.size_preset_id = None;
                self.doc.dirty = true;
            }
            let canvas = &self.doc.canvases[ci];
            if !canvas.auto_height {
                continue;
            }
            let Some(target) = auto_height_mm(canvas) else {
                continue;
            };
            if (target - canvas.size_mm[1]).abs() > 0.05 {
                self.doc.canvases[ci].size_mm[1] = target;
                self.rebuild_canvas(ci);
                self.doc.dirty = true;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{CanvasObject, CanvasObjectKind, NATURE_SINGLE_COLUMN, TextBox};

    fn page_with_text(size_mm: [f32; 2], frame: ObjectFrame) -> CanvasDocument {
        let mut canvas = CanvasDocument::new("page".into(), size_mm);
        canvas.objects.push(CanvasObject {
            id: 1,
            name: "t".to_owned(),
            frame,
            locked: false,
            visible: true,
            group: None,
            kind: CanvasObjectKind::Text(TextBox::label("x".to_owned())),
        });
        canvas.next_object_id = 2;
        canvas
    }

    #[test]
    fn empty_page_has_no_bounds_and_never_overflows() {
        let canvas = CanvasDocument::new("page".into(), [89.0, 60.0]);
        assert_eq!(content_bounds_pt(&canvas), None);
        assert!(!content_overflows(&canvas));
        assert_eq!(auto_height_mm(&canvas), None);
    }

    #[test]
    fn overflow_detects_content_past_the_page_edge() {
        let inside = page_with_text([89.0, 60.0], ObjectFrame::new(10.0, 10.0, 50.0, 50.0));
        assert!(!content_overflows(&inside));
        let page_w_pt = 89.0 * MM_TO_PT;
        let outside = page_with_text(
            [89.0, 60.0],
            ObjectFrame::new(page_w_pt - 10.0, 10.0, 50.0, 50.0),
        );
        assert!(content_overflows(&outside));
    }

    #[test]
    fn scaled_frames_scale_uniformly_by_width_ratio() {
        let canvas = page_with_text([183.0, 120.0], ObjectFrame::new(20.0, 30.0, 100.0, 60.0));
        let (before, after) = scaled_frames(&canvas, [183.0, 120.0], [91.5, 120.0]).unwrap();
        assert_eq!(before[0].1, ObjectFrame::new(20.0, 30.0, 100.0, 60.0));
        assert_eq!(after[0].1, ObjectFrame::new(10.0, 15.0, 50.0, 30.0));
    }

    #[test]
    fn scale_factor_is_none_for_height_only_changes() {
        assert_eq!(content_scale_factor([89.0, 60.0], [89.0, 120.0]), None);
    }

    #[test]
    fn auto_height_tracks_content_and_respects_the_preset_cap() {
        let frame_bottom_mm = 100.0;
        let mut canvas = page_with_text(
            [89.0, 60.0],
            ObjectFrame::new(0.0, 0.0, 50.0, frame_bottom_mm * MM_TO_PT),
        );
        canvas.layout.margin_mm[2] = 5.0;
        canvas.size_preset_id = Some(NATURE_SINGLE_COLUMN.id.to_string());
        let h = auto_height_mm(&canvas).unwrap();
        assert!((h - 105.0).abs() < 0.01, "expected 105, got {h}");

        // Content deeper than Nature's 247 mm page depth clamps to the cap.
        canvas.objects[0].frame = ObjectFrame::new(0.0, 0.0, 50.0, 300.0 * MM_TO_PT);
        assert_eq!(auto_height_mm(&canvas), Some(247.0));
    }

    #[test]
    fn wider_preset_suggested_only_for_multi_column_grids() {
        let mut canvas = page_with_text([89.0, 60.0], ObjectFrame::new(0.0, 0.0, 50.0, 50.0));
        assert_eq!(wider_preset_suggestion(&canvas), None);
        canvas.layout.cols = 2;
        assert_eq!(
            wider_preset_suggestion(&canvas).map(|p| p.id),
            Some("nature-2col")
        );
        // A custom width has no family to suggest from.
        canvas.size_mm = [100.0, 60.0];
        assert_eq!(wider_preset_suggestion(&canvas), None);
    }
}
