use super::{ObjectFrame, ObjectId};

/// A previewed auto-tiling drop, computed while a single plot hovers another page.
#[derive(Clone, Debug)]
pub struct TileDropPreview {
    pub cache_key: TileDropCacheKey,
    pub target: usize,
    pub newcomer: ObjectFrame,
    pub existing: Vec<(ObjectId, ObjectFrame)>,
    /// Current cursor in screen pixels and its clamped relative grab point in
    /// the source's pre-drag frame. These are independent of the target layout.
    pub pointer_screen: [f32; 2],
    pub anchor: [f32; 2],
}

impl TileDropPreview {
    pub fn ghost_frame(&self, before: ObjectFrame, zoom: f32) -> ObjectFrame {
        let zoom = if zoom.is_finite() { zoom.max(0.0) } else { 0.0 };
        let width = before.width.max(0.0) * zoom;
        let height = before.height.max(0.0) * zoom;
        ObjectFrame::new(
            self.pointer_screen[0] - self.anchor[0].clamp(0.0, 1.0) * width,
            self.pointer_screen[1] - self.anchor[1].clamp(0.0, 1.0) * height,
            width,
            height,
        )
    }
}

/// Every input that can change an auto-tiling preview without moving the pointer
/// inside its current drop region.
#[derive(Clone, Debug, PartialEq)]
pub struct TileDropCacheKey {
    pub source_canvas: usize,
    pub source_object: ObjectId,
    pub target_canvas: usize,
    pub target_page_pt: [f32; 2],
    pub target_layout: crate::layout::PageLayout,
    pub target_existing_ids: Vec<ObjectId>,
    pub region: crate::layout::TilingDropRegion,
    pub pointer_cell: Option<usize>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ghost_keeps_clamped_grab_anchor_and_before_size() {
        let preview = TileDropPreview {
            cache_key: TileDropCacheKey {
                source_canvas: 0,
                source_object: ObjectId::new(1),
                target_canvas: 1,
                target_page_pt: [100.0; 2],
                target_layout: crate::layout::PageLayout::default(),
                target_existing_ids: vec![],
                region: crate::layout::TilingDropRegion::Left,
                pointer_cell: None,
            },
            target: 1,
            newcomer: ObjectFrame::new(0.0, 0.0, 5.0, 5.0),
            existing: vec![],
            pointer_screen: [90.0, 70.0],
            anchor: [0.25, 2.0],
        };
        let ghost = preview.ghost_frame(ObjectFrame::new(2.0, 3.0, 40.0, 20.0), 2.0);
        assert_eq!(ghost, ObjectFrame::new(70.0, 30.0, 80.0, 40.0));
    }
}
