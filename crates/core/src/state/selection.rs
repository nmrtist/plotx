use super::{BoardFrameId, CanvasId, DatasetId, ObjectId};

/// The current desktop selection context. Global selection commands dispatch
/// through this scope instead of guessing from whichever collection is active.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SelectionScope {
    #[default]
    CanvasObjects,
    Board,
    CanvasList,
    DataList,
    Layers,
}

/// The whole-object selection on the active canvas. The first object is the
/// lead item used by inspectors and data-tool resolution.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub enum Selection {
    #[default]
    None,
    Objects(Vec<ObjectId>),
}

impl Selection {
    pub fn single(id: ObjectId) -> Self {
        Self::Objects(vec![id])
    }

    pub fn object(&self) -> Option<ObjectId> {
        self.objects().first().copied()
    }

    pub fn objects(&self) -> &[ObjectId] {
        match self {
            Self::None => &[],
            Self::Objects(ids) => ids,
        }
    }

    pub fn contains(&self, id: ObjectId) -> bool {
        self.objects().contains(&id)
    }
}

/// Stable lead/anchor identities for desktop extended selection.
#[derive(Clone, Copy, Debug, Default)]
pub struct SelectionAnchors {
    pub frame: Option<BoardFrameId>,
    pub canvas: Option<CanvasId>,
    pub dataset: Option<DatasetId>,
    pub layer: Option<ObjectId>,
    pub canvas_lead: Option<CanvasId>,
    pub dataset_lead: Option<DatasetId>,
    pub layer_lead: Option<ObjectId>,
}
