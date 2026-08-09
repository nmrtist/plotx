use super::{
    AxisOverrides, AxisProjections, CanvasViewport, ChartSpec, DataBinding, DatasetId, DerivedAxes,
    PanelMeta, SeriesId, StackSpec,
};
use plotx_figure::{Figure, FigureTypography};

#[derive(Clone)]
pub struct PlotObject {
    /// Dataset whose current display/channel choice this default view follows.
    /// Independent plots carry `None` and keep their item-addressed sources.
    pub display_owner: Option<DatasetId>,
    /// Persistent high-water mark for owner-local series identities. This is
    /// deliberately outside `binding`, which actions may replace wholesale.
    pub next_series_id: SeriesId,
    pub binding: DataBinding,
    /// The selected chart type (registry id) + its context, driving figure
    /// rebuilds through `state::charts`. Defaults to the dataset domain's default.
    pub chart: ChartSpec,
    /// The multi-series stacking layout. Default = superimposed overlay.
    pub stack: StackSpec,
    /// Marginal 1D axis projections for a 2D contour (empty for other plots).
    pub projections: AxisProjections,
    pub axis_overrides: AxisOverrides,
    /// Axis presentation emitted by the latest figure build, before author
    /// overrides are applied. Derived property defaults read this artifact.
    derived_axes: DerivedAxes,
    figure: Figure,
    pub viewport: CanvasViewport,
}

impl PlotObject {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        display_owner: Option<DatasetId>,
        next_series_id: SeriesId,
        binding: DataBinding,
        chart: ChartSpec,
        stack: StackSpec,
        projections: AxisProjections,
        axis_overrides: AxisOverrides,
        figure: Figure,
        viewport: CanvasViewport,
        _panel: PanelMeta,
    ) -> Self {
        let derived_axes = DerivedAxes::from_figure(&figure);
        Self {
            display_owner,
            next_series_id,
            binding,
            chart,
            stack,
            projections,
            axis_overrides,
            derived_axes,
            figure,
            viewport,
        }
    }

    /// Restore a materialized figure that already contains author overrides and
    /// viewport state while retaining the separately rebuilt automatic axes.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn from_materialized_figure(
        display_owner: Option<DatasetId>,
        next_series_id: SeriesId,
        binding: DataBinding,
        chart: ChartSpec,
        stack: StackSpec,
        projections: AxisProjections,
        axis_overrides: AxisOverrides,
        derived_axes: DerivedAxes,
        figure: Figure,
        viewport: CanvasViewport,
        _panel: PanelMeta,
    ) -> Self {
        Self {
            display_owner,
            next_series_id,
            binding,
            chart,
            stack,
            projections,
            axis_overrides,
            derived_axes,
            figure,
            viewport,
        }
    }

    pub fn figure(&self) -> &Figure {
        &self.figure
    }

    pub fn derived_axes(&self) -> &DerivedAxes {
        &self.derived_axes
    }

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

    fn commit_rebuilt_figure(
        &mut self,
        mut figure: Figure,
        prepare: impl FnOnce(&mut Self, &mut Figure),
    ) {
        self.derived_axes = DerivedAxes::from_figure(&figure);
        prepare(self, &mut figure);
        self.figure = figure;
    }

    /// Adopt a rebuilt figure whose chart semantics may have changed.
    pub(crate) fn adopt_rebuilt_figure(&mut self, figure: Figure) {
        self.commit_rebuilt_figure(figure, |plot, figure| {
            plot.axis_overrides.apply_to(figure);
            plot.viewport = CanvasViewport::from_figure(figure);
            if plot.has_manual_y_range(figure) {
                plot.viewport.auto_y = false;
            }
            plot.viewport.apply_to(figure);
        });
    }

    /// Rebuild → overrides → viewport sync/apply. Effective range overrides
    /// replace the full data bounds; zoom and pan remain constrained within them.
    pub(crate) fn preserve_viewport_on_rebuild(&mut self, figure: Figure) {
        self.commit_rebuilt_figure(figure, |plot, figure| {
            plot.axis_overrides.apply_to(figure);
            if plot.has_manual_y_range(figure) {
                plot.viewport.auto_y = false;
            }
            plot.viewport.sync_full_from(figure);
            plot.viewport.apply_to(figure);
        });
    }

    /// Rebuild a plot whose chart semantics changed, starting its viewport at
    /// the effective overridden ranges rather than retaining an incompatible view.
    pub(crate) fn reset_viewport_on_rebuild(&mut self, figure: Figure) {
        self.adopt_rebuilt_figure(figure);
    }

    pub(crate) fn rebuild_for_axis_overrides(
        &mut self,
        figure: Figure,
        x_range_changed: bool,
        y_range_changed: bool,
    ) {
        self.commit_rebuilt_figure(figure, |plot, figure| {
            plot.axis_overrides.apply_to(figure);
            let effective_y_range = plot.has_manual_y_range(figure);
            if y_range_changed {
                plot.viewport.auto_y = !effective_y_range;
            } else if effective_y_range {
                plot.viewport.auto_y = false;
            }
            plot.viewport.sync_full_from(figure);
            if x_range_changed {
                plot.viewport.reset_x(figure);
            }
            if y_range_changed {
                if effective_y_range {
                    plot.viewport.view_y = plot.viewport.full_y;
                    plot.viewport.auto_y = false;
                } else {
                    plot.viewport.reset_y(figure);
                }
            }
            plot.viewport.apply_to(figure);
        });
    }

    pub fn apply_viewport(&mut self) {
        self.viewport.apply_to(&mut self.figure);
    }

    pub(crate) fn apply_axis_overrides(&mut self) {
        self.axis_overrides.apply_to(&mut self.figure);
    }

    pub(crate) fn set_figure_typography(&mut self, typography: FigureTypography) {
        self.figure.typography = typography;
    }

    pub(crate) fn set_integral_curves(
        &mut self,
        curves: &[plotx_figure::IntegralCurve],
        visible: bool,
    ) {
        if visible {
            self.figure.integral_curves.clear();
            self.figure.integral_curves.extend_from_slice(curves);
        } else {
            self.figure.integral_curves.clear();
        }
    }

    #[cfg(test)]
    pub(crate) fn set_axis_frame(&mut self, axis_frame: plotx_figure::AxisFrame) {
        self.figure.axis_frame = axis_frame;
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
