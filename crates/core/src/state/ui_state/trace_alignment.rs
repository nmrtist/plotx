use super::{CanvasId, ObjectId, SeriesId, TraceAlignmentMethod, TraceAlignmentPlan};

#[derive(Clone, Debug)]
pub struct TraceAlignmentDialogState {
    pub canvas: CanvasId,
    pub object: ObjectId,
    pub reference: SeriesId,
    pub method: TraceAlignmentMethod,
    pub peak_window: (f64, f64),
    pub peak_polarity: plotx_analysis::alignment::PeakPolarity,
    pub plan: Option<Result<TraceAlignmentPlan, String>>,
    pub history_mark: (usize, usize, u64),
}
