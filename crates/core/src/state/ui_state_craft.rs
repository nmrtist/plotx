#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CraftTaskPage {
    #[default]
    Setup,
    Results,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CraftResultTab {
    #[default]
    Overview,
    Components,
    Diagnostics,
    Reports,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CraftComponentSort {
    #[default]
    ChemicalShift,
    AmplitudeToNoise,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CraftAnalysisIntent {
    #[default]
    ExploreBandwidth,
    SelectedSignals,
    CompareTwoSignals,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CraftSpectrumChannel {
    Real,
    #[default]
    Magnitude,
    Imaginary,
}

#[derive(Clone, Debug, PartialEq)]
pub enum CraftRunFeedback {
    Running(Box<plotx_processing::craft::CraftInvocation>),
    Cancelled,
    Failed { message: String },
    Completed(CraftRunId),
}

#[derive(Clone, Debug, PartialEq)]
pub struct CraftResolutionCache {
    pub dataset: super::DatasetId,
    pub dataset_epoch: u64,
    pub reference: plotx_processing::craft::CraftReference,
    pub overrides: plotx_processing::craft::CraftParamOverrides,
    pub parent_run: Option<CraftRunId>,
    pub invocation: plotx_processing::craft::CraftInvocation,
}
use super::CraftRunId;
