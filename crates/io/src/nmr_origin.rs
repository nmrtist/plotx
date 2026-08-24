//! Retained provenance for imported NMR acquisitions.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NmrSourceFormat {
    BrukerRaw,
    JeolDelta,
}

impl NmrSourceFormat {
    pub const fn label(self) -> &'static str {
        match self {
            Self::BrukerRaw => "Bruker",
            Self::JeolDelta => "JEOL",
        }
    }
}

/// Lossless acquisition parameters retained without duplicating the signal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "vendor", rename_all = "snake_case")]
pub enum NmrSourceParameters {
    Bruker {
        acqus: String,
        title: Option<String>,
        pulse_program: Option<String>,
    },
    Jeol {
        /// Fixed header and parameter-list bytes before the signal section.
        metadata_base64: String,
    },
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct NmrPortableMetadata {
    pub solvent: Option<String>,
    pub temperature_k: Option<f64>,
    pub transients: Option<u64>,
    pub pulse_sequence: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NmrInstrumentOrigin {
    pub format: NmrSourceFormat,
    pub source_sha256: [u8; 32],
    pub portable: NmrPortableMetadata,
    pub parameters: NmrSourceParameters,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum NmrOrigin {
    Instrument { instrument: NmrInstrumentOrigin },
    Derived,
}

impl NmrOrigin {
    pub fn instrument(&self) -> Option<&NmrInstrumentOrigin> {
        match self {
            Self::Instrument { instrument } => Some(instrument),
            Self::Derived => None,
        }
    }
}
