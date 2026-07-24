use super::{CanvasId, DatasetId, Document};

impl Document {
    pub fn dataset_index(&self, id: DatasetId) -> Option<usize> {
        self.datasets
            .iter()
            .position(|dataset| dataset.resource_id() == id)
    }

    /// The dataset owning `id`, or `None` if it no longer exists (e.g. a binding
    /// left dangling by an undo). The one accessor every lookup should route
    /// through instead of `expect`/`usize::MAX` improvisation.
    pub fn dataset_by_id(&self, id: DatasetId) -> Option<&crate::state::Dataset> {
        self.datasets
            .iter()
            .find(|dataset| dataset.resource_id() == id)
    }

    pub fn canvas_index(&self, id: CanvasId) -> Option<usize> {
        self.canvases
            .iter()
            .position(|canvas| canvas.resource_id == id)
    }

    /// The datasets plotted on canvas `ci`, as document-ordered indices — the
    /// order the Data list mirror and stack-primary selection depend on.
    /// Deterministic regardless of DatasetId ordering; empty if `ci` is stale.
    pub fn page_dataset_indices(&self, ci: usize) -> Vec<usize> {
        let Some(canvas) = self.canvases.get(ci) else {
            return Vec::new();
        };
        let mut indices: Vec<usize> = canvas
            .dataset_ids()
            .into_iter()
            .filter_map(|id| self.dataset_index(id))
            .collect();
        indices.sort_unstable();
        indices.dedup();
        indices
    }
}
