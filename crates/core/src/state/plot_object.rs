use super::{CanvasViewport, DatasetId, PlotObject, SeriesId};
use plotx_figure::Figure;

impl PlotObject {
    pub fn allocate_series_id(&mut self) -> SeriesId {
        let id = self.next_series_id;
        self.next_series_id = id.checked_advance(1);
        id
    }

    /// Assign identities to a newly materialized binding in order. Callers that
    /// restore persisted bindings must preserve their ids and use
    /// `repair_series_allocator` instead.
    pub fn mint_series_ids(&mut self) {
        let start = self.next_series_id;
        for (offset, series) in self.binding.series.iter_mut().enumerate() {
            series.id = start.checked_advance(offset as u64);
        }
        self.next_series_id = start.checked_advance(self.binding.series.len() as u64);
    }

    /// Raise the allocator above every id the (possibly persisted) binding
    /// already carries, so the next `allocate_series_id` cannot alias one.
    ///
    /// Returns `None` when the binding's highest id is `u64::MAX` and no
    /// successor exists: the file is unusable rather than merely inconsistent,
    /// and the caller must reject it. Defaulting to zero here would skip the
    /// repair entirely and hand out a duplicate on the very next allocation.
    #[must_use]
    pub fn repair_series_allocator(&mut self) -> Option<()> {
        let Some(highest) = self.binding.series.iter().map(|series| series.id).max() else {
            return Some(());
        };
        let required = highest.try_advance(1)?;
        self.next_series_id = self.next_series_id.max(required);
        Some(())
    }

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
