use super::Dataset;
use serde::{Deserialize, Serialize};

/// Why a dataset was materialized from one or more earlier datasets.
///
/// This is intentionally separate from `TableProvenance`: provenance owns the
/// recipe needed to refresh a live region table, while lineage only describes
/// relationships in the data browser and project archive.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DerivationKind {
    Slice,
    Projection,
    SpectrumArithmetic,
    LiveRegionTable,
    FrozenRegionTable,
    LineFitTable,
    MultipletTable,
    WindowStatisticsTable,
    IvTable,
    StatisticsTable,
    RelationalTransform,
}

impl DerivationKind {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Slice => "Slice",
            Self::Projection => "Projection",
            Self::SpectrumArithmetic => "Spectrum arithmetic",
            Self::LiveRegionTable => "Live regions table",
            Self::FrozenRegionTable => "Frozen regions table",
            Self::LineFitTable => "Peak fit table",
            Self::MultipletTable => "Multiplet table",
            Self::WindowStatisticsTable => "Window statistics table",
            Self::IvTable => "IV table",
            Self::StatisticsTable => "Statistics table",
            Self::RelationalTransform => "Table transform",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DatasetLineage {
    pub kind: DerivationKind,
    pub sources: Vec<usize>,
}

impl DatasetLineage {
    pub fn new(kind: DerivationKind, sources: impl IntoIterator<Item = usize>) -> Self {
        let mut unique = Vec::new();
        for source in sources {
            if !unique.contains(&source) {
                unique.push(source);
            }
        }
        Self {
            kind,
            sources: unique,
        }
    }
}

impl Dataset {
    pub fn lineage(&self) -> Option<&DatasetLineage> {
        match self {
            Dataset::Nmr(data) => data.lineage.as_ref(),
            Dataset::Nmr2D(data) => data.lineage.as_ref(),
            Dataset::Table(data) => data.lineage.as_ref(),
            Dataset::Electrophysiology(data) => data.lineage.as_ref(),
            Dataset::Afm(data) => data.lineage.as_ref(),
        }
    }

    pub fn set_lineage(&mut self, lineage: Option<DatasetLineage>) {
        match self {
            Dataset::Nmr(data) => data.lineage = lineage,
            Dataset::Nmr2D(data) => data.lineage = lineage,
            Dataset::Table(data) => data.lineage = lineage,
            Dataset::Electrophysiology(data) => data.lineage = lineage,
            Dataset::Afm(data) => data.lineage = lineage,
        }
    }
}
