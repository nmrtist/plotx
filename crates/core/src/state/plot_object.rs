use super::{CanvasViewport, DatasetId, PlotObject};
use plotx_figure::Figure;

impl PlotObject {
    pub fn primary_dataset(&self) -> Option<DatasetId> {
        self.binding.primary_dataset()
    }

    /// Rebuild → overrides → viewport sync/apply. Effective range overrides
    /// replace the full data bounds; zoom and pan remain constrained within them.
    pub(crate) fn preserve_viewport_on_rebuild(&mut self, mut figure: Figure) {
        self.axis_overrides.apply_to(&mut figure);
        if self.has_manual_y_range(&figure) {
            self.viewport.auto_y = false;
        }
        self.viewport.sync_full_from(&figure);
        self.viewport.apply_to(&mut figure);
        self.figure = figure;
    }

    /// Rebuild a plot whose chart semantics changed, starting its viewport at
    /// the effective overridden ranges rather than retaining an incompatible view.
    pub(crate) fn reset_viewport_on_rebuild(&mut self, mut figure: Figure) {
        self.axis_overrides.apply_to(&mut figure);
        self.viewport = CanvasViewport::from_figure(&figure);
        if self.has_manual_y_range(&figure) {
            self.viewport.auto_y = false;
        }
        self.viewport.apply_to(&mut figure);
        self.figure = figure;
    }

    pub(crate) fn normalize_viewport(&self, viewport: &mut CanvasViewport) {
        if self.has_manual_y_range(&self.figure) && viewport.auto_y {
            viewport.view_y = viewport.full_y;
            viewport.auto_y = false;
        }
    }

    fn has_manual_y_range(&self, figure: &Figure) -> bool {
        self.axis_overrides.y_range.is_some() && figure.y.categories.is_none()
    }
}
