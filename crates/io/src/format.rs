//! Scientific data families and the vendor/container formats that carry them.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataFormat {
    Nmr(NmrFormat),
    Electrophysiology(ElectrophysiologyFormat),
    Afm(AfmFormat),
    MassSpectrometry(MassSpectrometryFormat),
    Xrd(XrdFormat),
    Xps(XpsFormat),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NmrFormat {
    JeolDelta,
    BrukerRaw,
    VarianAgilentRaw,
    BrukerProcessed1D,
    BrukerProcessed2D,
    JcampDx1D,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ElectrophysiologyFormat {
    Abf2,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AfmFormat {
    BrukerNanoScopeSpm,
    BrukerPeakForceCapture,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MassSpectrometryFormat {
    WatersMassLynxRaw,
    MzMl,
    SciexWiff,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum XrdFormat {
    RigakuRasx,
    RigakuRaw,
    RigakuProfile,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum XpsFormat {
    VamasXps,
    CasaXpsText,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScientificDataType {
    Nmr,
    Electrophysiology,
    Afm,
    MassSpectrometry,
    Xrd,
    Xps,
}

impl DataFormat {
    pub const fn scientific_type(self) -> ScientificDataType {
        match self {
            Self::Nmr(_) => ScientificDataType::Nmr,
            Self::Electrophysiology(_) => ScientificDataType::Electrophysiology,
            Self::Afm(_) => ScientificDataType::Afm,
            Self::MassSpectrometry(_) => ScientificDataType::MassSpectrometry,
            Self::Xrd(_) => ScientificDataType::Xrd,
            Self::Xps(_) => ScientificDataType::Xps,
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Electrophysiology(ElectrophysiologyFormat::Abf2) => "abf2",
            Self::Nmr(NmrFormat::JeolDelta) => "jeol-delta",
            Self::Nmr(NmrFormat::BrukerRaw) => "bruker-raw",
            Self::Nmr(NmrFormat::VarianAgilentRaw) => "varian-agilent-raw",
            Self::Nmr(NmrFormat::BrukerProcessed1D) => "bruker-processed-1d",
            Self::Nmr(NmrFormat::BrukerProcessed2D) => "bruker-processed-2d",
            Self::Nmr(NmrFormat::JcampDx1D) => "jcamp-dx-1d",
            Self::Afm(AfmFormat::BrukerNanoScopeSpm) => "bruker-nanoscope-spm",
            Self::Afm(AfmFormat::BrukerPeakForceCapture) => "bruker-peakforce-capture",
            Self::MassSpectrometry(MassSpectrometryFormat::WatersMassLynxRaw) => {
                "waters-masslynx-raw"
            }
            Self::MassSpectrometry(MassSpectrometryFormat::MzMl) => "mzml",
            Self::MassSpectrometry(MassSpectrometryFormat::SciexWiff) => "sciex-wiff",
            Self::Xrd(XrdFormat::RigakuRasx) => "rigaku-rasx",
            Self::Xrd(XrdFormat::RigakuRaw) => "rigaku-raw-fi",
            Self::Xrd(XrdFormat::RigakuProfile) => "rigaku-profile",
            Self::Xps(XpsFormat::VamasXps) => "vamas-xps",
            Self::Xps(XpsFormat::CasaXpsText) => "casaxps-text",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_are_grouped_by_scientific_type() {
        assert_eq!(
            DataFormat::Nmr(NmrFormat::BrukerRaw).scientific_type(),
            ScientificDataType::Nmr
        );
        assert_eq!(
            DataFormat::Xrd(XrdFormat::RigakuRaw).scientific_type(),
            ScientificDataType::Xrd
        );
        assert_eq!(
            DataFormat::MassSpectrometry(MassSpectrometryFormat::MzMl).as_str(),
            "mzml"
        );
    }
}
