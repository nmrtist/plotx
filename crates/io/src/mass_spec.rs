use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;

/// Stable Waters acquisition-function identity, derived from the vendor number.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct FunctionId(u16);

impl FunctionId {
    pub const fn new(value: u16) -> Self {
        Self(value)
    }

    pub const fn get(self) -> u16 {
        self.0
    }
}

impl fmt::Display for FunctionId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.0)
    }
}

/// Stable native scan identity within one acquisition function.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ScanId(u32);

impl ScanId {
    pub const fn new(value: u32) -> Self {
        Self(value)
    }

    pub const fn get(self) -> u32 {
        self.0
    }
}

impl fmt::Display for ScanId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ChromatogramChannelId(pub String);

impl fmt::Display for ChromatogramChannelId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FunctionKind {
    MassSpectrum,
    OpticalDetector,
    ReferenceLockMass,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Polarity {
    Positive,
    Negative,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WatersDecoder {
    LowResolution6,
    Unsupported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScanEncoding {
    pub idx_stride: u16,
    pub pair_width: u8,
    pub decoder: WatersDecoder,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MassScan {
    pub id: ScanId,
    pub retention_time_min: f64,
    /// Calibrated m/z for MS functions; detector coordinate for other functions.
    pub mz: Vec<f64>,
    pub intensity: Vec<f64>,
    pub tic: f64,
    pub base_peak_mz: Option<f64>,
    pub base_peak_intensity: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcquisitionFunction {
    pub id: FunctionId,
    pub kind: FunctionKind,
    pub polarity: Polarity,
    pub acquisition_range: Option<[f64; 2]>,
    pub encoding: ScanEncoding,
    pub scans: Vec<MassScan>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChromatogramKind {
    Optical,
    Temperature,
    Pressure,
    Housekeeping,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChromatogramChannel {
    pub id: ChromatogramChannelId,
    pub kind: ChromatogramKind,
    pub source_function: Option<FunctionId>,
    pub coordinate: Option<f64>,
    pub description: String,
    pub unit: String,
    pub time_min: Vec<f64>,
    pub values: Vec<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MassSpecRun {
    pub source: String,
    pub metadata: BTreeMap<String, String>,
    pub instrument: Option<String>,
    pub functions: Vec<AcquisitionFunction>,
    pub chromatograms: Vec<ChromatogramChannel>,
    pub import_warnings: Vec<String>,
}

impl MassSpecRun {
    pub fn function(&self, id: FunctionId) -> Option<&AcquisitionFunction> {
        self.functions.iter().find(|function| function.id == id)
    }
}
