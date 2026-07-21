use crate::actions::{
    Action, DatasetProcessingState, PendingCanvasSizeEdit, PendingInspectorEdit,
    PendingPageLayoutEdit, PendingProcessingEdit, PendingViewportEdit,
};
use crate::export::{ExportDialogState, ExportFormat, ExportSettings};
use crate::{
    DosyMethod, IltParams, Integral2D, IntegralResult, PseudoDisplay, apply_peak_labels,
    build_dosy_figure, build_figure, build_figure_2d_cancellable, build_ilt_figure,
    build_stack_figure, extract_region_series,
};
use plotx_analysis::diffusion::{DiffusionMap, diffusion_map};
use plotx_analysis::ilt::{IltResult, ilt_map, log_grid};
use plotx_figure::{Axis, Color, Figure};
use plotx_io::{
    Domain, ElectricalQuantity, ElectricalUnit, ElectrophysiologyData, NmrData, NmrData2D,
};
use plotx_processing::{
    AxisPipeline, DisplayMode, Params2D, PhaseParams, Preset2D, Processed2D, Spectrum, StepId,
    StepKind, fft, process_2d, reapply, reapply_2d, recommend_preset,
};

mod app_impl;
mod app_impl_align;
mod app_impl_analysis;
#[cfg(test)]
mod app_impl_analysis_tests;
mod app_impl_arithmetic;
mod app_impl_compute;
mod app_impl_io;
mod app_impl_linefit;
mod app_impl_multiplet;
mod app_impl_peaks;
mod app_impl_slice;
mod app_impl_statistics;
#[cfg(test)]
mod app_impl_statistics_tests;
mod app_state;
mod board;
mod charts;
mod compute;
mod dataset_identity;
mod dataset_trace;
mod datasets;
mod datasets_2d_figure;
mod datasets_2d_maps;
mod document;
mod electrophysiology;
mod fit_selection;
mod interaction;
mod lineage;
mod linefit;
mod multiplet;
mod nmr_integrals;
mod nmr_integrals_2d;
mod nus;
mod page_fit;
mod panel_label;
mod peaks;
mod region;
mod size_presets;
mod stack;
mod statistics;
mod statistics_prepare;
mod statistics_report;
mod table;
mod table_charts;
mod table_edit;
mod table_execution;
mod table_execution_job;
mod table_fit;
mod table_native;
mod table_numeric;
mod ui_state;
mod units;
mod workflow_tab;

pub use app_impl::*;
pub use app_impl_align::*;
pub use app_impl_linefit::LineFitJob;
pub use app_state::*;
pub use board::*;
pub use charts::*;
pub use compute::*;
pub use datasets::*;
pub(crate) use datasets_2d_figure::{build_processed_figure, build_processed_figure_cancellable};
pub use document::*;
pub use electrophysiology::*;
pub use interaction::*;
pub use lineage::*;
pub use linefit::*;
pub use multiplet::*;
pub use page_fit::*;
pub use panel_label::*;
pub use peaks::*;
pub use region::*;
pub use size_presets::*;
pub use statistics::*;
pub use statistics_report::{
    detail_lines, fmt_level, fmt_num, fmt_p, headline, outcome_table, report_text,
};
pub use table::*;
pub use table_edit::*;
pub use table_execution::*;
pub use table_execution_job::*;
pub use table_native::*;
pub use ui_state::*;
pub use units::*;
pub use workflow_tab::WorkflowTab;

/// Points per millimetre (72 pt/inch ÷ 25.4 mm/inch), for sizing print figures.
pub const MM_TO_PT: f32 = 72.0 / 25.4;
/// A fresh single-dataset canvas defaults to single-column width: one plot
/// spanning it carries the journal font-to-panel ratio by construction, and
/// figures are composited onto wider pages later at their natural size.
pub const DEFAULT_CANVAS_SIZE_MM: [f32; 2] = NATURE_SINGLE_COLUMN.size_mm();
const PX_PER_IN: f32 = 96.0;
const MM_PER_IN: f32 = 25.4;

pub type ObjectId = u64;
pub type GroupId = u64;

#[cfg(test)]
mod tests {
    use super::*;
    use plotx_figure::Series;

    #[test]
    fn selection_multi_reports_primary_and_membership() {
        let sel = Selection::Objects(vec![3, 7, 9]);
        assert_eq!(sel.object(), Some(3));
        assert_eq!(sel.objects(), &[3, 7, 9]);
        assert!(sel.contains(7));
        assert!(!sel.contains(4));
        assert_eq!(Selection::single(5).objects(), &[5]);
    }

    #[test]
    fn zoom_keeps_anchor_stable() {
        let full = AxisRange::new(0.0, 10.0);
        let zoomed = full.zoom_around(full, 2.0, 0.5);
        assert_eq!(zoomed, AxisRange::new(1.0, 6.0));
    }

    #[test]
    fn auto_y_uses_visible_x_window() {
        let fig =
            Figure::new("t", Axis::new("x", 0.0, 10.0), Axis::new("y", 0.0, 100.0)).with_series(
                Series::line("s", vec![[0.0, 1.0], [2.0, 10.0], [8.0, 80.0]]),
            );

        let y = visible_y_range(&fig, AxisRange::new(0.0, 3.0)).unwrap();
        assert!(y.max < 12.0);
    }
}
