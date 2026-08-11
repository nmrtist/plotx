use super::{BoardFrameId, CanvasId, ContentId, DatasetId, ObjectId, PanelId};

/// A stable hierarchical selection address. `content` is only valid in the
/// loose page scope when `panel` is `None`, or as a direct child of `panel`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct SelectionPath {
    pub canvas: CanvasId,
    pub panel: Option<PanelId>,
    pub content: Option<ContentId>,
}

impl SelectionPath {
    pub fn panel(canvas: CanvasId, panel: PanelId) -> Self {
        Self {
            canvas,
            panel: Some(panel),
            content: None,
        }
    }

    pub fn content(canvas: CanvasId, panel: Option<PanelId>, content: ContentId) -> Self {
        Self {
            canvas,
            panel,
            content: Some(content),
        }
    }

    pub fn sibling_scope(self) -> (CanvasId, Option<PanelId>) {
        (self.canvas, self.content.and(self.panel))
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HierarchicalSelection {
    paths: Vec<SelectionPath>,
}

impl HierarchicalSelection {
    pub fn paths(&self) -> &[SelectionPath] {
        &self.paths
    }

    pub fn replace(&mut self, path: SelectionPath) {
        self.paths = vec![path];
    }

    pub fn clear(&mut self) {
        self.paths.clear();
    }

    pub fn lead(&self) -> Option<SelectionPath> {
        self.paths.first().copied()
    }

    pub fn contains(&self, path: SelectionPath) -> bool {
        self.paths.contains(&path)
    }

    pub fn editing_panel(&self) -> Option<(CanvasId, PanelId)> {
        self.lead().and_then(|path| {
            path.content
                .and(path.panel)
                .map(|panel| (path.canvas, panel))
        })
    }

    pub fn extend_sibling(&mut self, path: SelectionPath) -> Result<(), &'static str> {
        if self
            .paths
            .first()
            .is_some_and(|lead| lead.sibling_scope() != path.sibling_scope())
        {
            return Err("Select sibling items in the same page or panel scope.");
        }
        if !self.paths.contains(&path) {
            self.paths.push(path);
        }
        Ok(())
    }

    pub fn exit_scope(&mut self) {
        let Some(lead) = self.paths.first().copied() else {
            return;
        };
        self.paths = match (lead.panel, lead.content) {
            (Some(panel), Some(_)) => vec![SelectionPath::panel(lead.canvas, panel)],
            _ => Vec::new(),
        };
    }

    pub fn toggle_sibling(&mut self, path: SelectionPath) -> Result<(), &'static str> {
        if self
            .paths
            .first()
            .is_some_and(|lead| lead.sibling_scope() != path.sibling_scope())
        {
            return Err("Select sibling items in the same page or panel scope.");
        }
        if let Some(index) = self.paths.iter().position(|selected| *selected == path) {
            self.paths.remove(index);
        } else {
            self.paths.push(path);
        }
        Ok(())
    }
}

#[cfg(test)]
mod hierarchy_tests {
    use super::*;

    #[test]
    fn rejects_cross_scope_multi_selection_and_exits_to_panel() {
        let canvas = CanvasId::new();
        let panel = PanelId::new();
        let mut selection = HierarchicalSelection::default();
        selection.replace(SelectionPath::content(
            canvas,
            Some(panel),
            ContentId::new(1),
        ));
        assert!(
            selection
                .extend_sibling(SelectionPath::content(canvas, None, ContentId::new(2)))
                .is_err()
        );
        selection.exit_scope();
        assert_eq!(selection.paths(), &[SelectionPath::panel(canvas, panel)]);
    }

    #[test]
    fn panels_share_page_scope_but_contents_require_the_same_parent() {
        let canvas = CanvasId::new();
        let first = PanelId::new();
        let second = PanelId::new();
        let mut selection = HierarchicalSelection::default();
        selection.replace(SelectionPath::panel(canvas, first));
        assert!(
            selection
                .extend_sibling(SelectionPath::panel(canvas, second))
                .is_ok()
        );
        assert!(
            selection
                .extend_sibling(SelectionPath::content(canvas, None, ContentId::new(3)))
                .is_ok()
        );
        selection.replace(SelectionPath::content(
            canvas,
            Some(first),
            ContentId::new(1),
        ));
        assert!(
            selection
                .extend_sibling(SelectionPath::content(
                    canvas,
                    Some(second),
                    ContentId::new(2),
                ))
                .is_err()
        );
    }
}

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
