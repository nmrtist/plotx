use super::{ObjectFrame, ObjectId};

/// A previewed auto-tiling drop, computed while a single plot hovers another page.
#[derive(Clone, Debug)]
pub struct TileDropPreview {
    pub cache_key: TileDropCacheKey,
    pub target: usize,
    pub newcomer: ObjectFrame,
    pub existing: Vec<(ObjectId, ObjectFrame)>,
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
}
