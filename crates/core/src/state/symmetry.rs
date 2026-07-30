//! Runtime state for homonuclear 2D symmetry review.

use std::sync::{Arc, mpsc};
use std::time::Instant;

use plotx_analysis::symmetry::{
    ArtifactLikelihood, ArtifactReason, CandidateKey, PartnerStatus, SymmetryAudit,
};
use plotx_processing::{DisplayMode, Spectrum2D};

use super::{DatasetId, Peak2DPoint};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SymmetryAuditFilter {
    #[default]
    All,
    Matched,
    Unpaired,
    Ambiguous,
    Suggestions,
}

impl SymmetryAuditFilter {
    pub const fn label(self) -> &'static str {
        match self {
            Self::All => "All",
            Self::Matched => "Paired",
            Self::Unpaired => "Unpaired",
            Self::Ambiguous => "Ambiguous",
            Self::Suggestions => "Suggestions",
        }
    }
}

pub struct SymmetryAuditState {
    pub dataset: DatasetId,
    pub spectrum: Arc<Spectrum2D>,
    pub mode: DisplayMode,
    pub result: SymmetryAudit,
}

/// Runtime-only background audit. Its result is accepted only while the exact
/// processed spectrum `Arc` is still current.
pub struct SymmetryAuditJob {
    pub dataset: DatasetId,
    pub spectrum: Arc<Spectrum2D>,
    pub mode: DisplayMode,
    pub started_at: Instant,
    pub(crate) rx: mpsc::Receiver<Result<SymmetryAudit, String>>,
}

#[derive(Clone, Debug)]
pub struct SymmetryCursorReading {
    pub dataset: DatasetId,
    pub current: Peak2DPoint,
    /// Exact cross-diagonal coordinate, used even when no local extremum exists.
    pub partner_target: [f64; 2],
    pub partner: Option<Peak2DPoint>,
    pub current_key: Option<CandidateKey>,
    pub partner_key: Option<CandidateKey>,
    pub alternatives: usize,
    pub status: Option<PartnerStatus>,
    pub likelihood: Option<ArtifactLikelihood>,
    pub reasons: Vec<ArtifactReason>,
    pub current_signal_to_noise: Option<f64>,
    pub partner_signal_to_noise: Option<f64>,
    pub on_diagonal: bool,
}
