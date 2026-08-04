use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

macro_rules! typed_id {
    ($name:ident) => {
        #[derive(
            Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
        )]
        #[serde(transparent)]
        pub struct $name(u64);

        impl $name {
            pub const fn new(value: u64) -> Self {
                Self(value)
            }
            pub const fn get(self) -> u64 {
                self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(formatter, "{}", self.0)
            }
        }
    };
}

typed_id!(AcquisitionStreamId);
typed_id!(SpectrumId);

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
pub enum StreamRole {
    Primary,
    Reference,
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
pub enum SpectrumRepresentation {
    Profile,
    Centroid,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Precursor {
    pub selected_mz: f64,
    pub charge: Option<i32>,
    pub isolation_window_lower_offset: Option<f64>,
    pub isolation_window_upper_offset: Option<f64>,
    pub collision_energy: Option<f64>,
    pub activation_method: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MassSpectrum {
    pub id: SpectrumId,
    pub source_native_id: Option<String>,
    pub retention_time_min: f64,
    pub ms_level: u8,
    pub polarity: Polarity,
    pub representation: SpectrumRepresentation,
    pub mz: Vec<f64>,
    pub intensity: Vec<f64>,
    pub tic: f64,
    pub base_peak_mz: Option<f64>,
    pub base_peak_intensity: Option<f64>,
    pub precursor: Option<Precursor>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcquisitionStream {
    pub id: AcquisitionStreamId,
    pub source_native_id: Option<String>,
    pub source_label: Option<String>,
    pub role: StreamRole,
    pub acquisition_range: Option<[f64; 2]>,
    pub spectra: Vec<MassSpectrum>,
}

impl AcquisitionStream {
    /// The stream polarity when every spectrum agrees, otherwise unknown.
    pub fn polarity(&self) -> Polarity {
        let Some(first) = self.spectra.first().map(|spectrum| spectrum.polarity) else {
            return Polarity::Unknown;
        };
        if first == Polarity::Unknown
            || self
                .spectra
                .iter()
                .any(|spectrum| spectrum.polarity != first)
        {
            Polarity::Unknown
        } else {
            first
        }
    }
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
    pub source_stream: Option<AcquisitionStreamId>,
    pub coordinate: Option<f64>,
    pub description: String,
    pub unit: String,
    pub time_min: Vec<f64>,
    pub values: Vec<f64>,
}

/// One programmed composition in a liquid-chromatography gradient method.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LcGradientPoint {
    pub time_min: f64,
    pub flow_ml_min: f64,
    pub percent_b: f64,
}

/// The method information needed to relate a chromatographic retention time to
/// the programmed mobile-phase composition.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LiquidChromatographyMethod {
    pub name: Option<String>,
    pub run_time_min: f64,
    pub solvent_a: Option<String>,
    pub solvent_b: Option<String>,
    pub gradient: Vec<LcGradientPoint>,
    pub detector_wavelengths_nm: Vec<f64>,
    pub column: Option<String>,
}

impl LiquidChromatographyMethod {
    pub fn validate(&self) -> Result<(), String> {
        if !self.run_time_min.is_finite() || self.run_time_min <= 0.0 {
            return Err("LC method has an invalid run time".to_owned());
        }
        if self.gradient.len() < 2 {
            return Err("LC method needs at least two gradient points".to_owned());
        }
        let mut previous = f64::NEG_INFINITY;
        for point in &self.gradient {
            if !point.time_min.is_finite()
                || point.time_min < 0.0
                || point.time_min <= previous
                || !point.flow_ml_min.is_finite()
                || point.flow_ml_min <= 0.0
                || !point.percent_b.is_finite()
                || !(0.0..=100.0).contains(&point.percent_b)
            {
                return Err("LC method has an invalid gradient point".to_owned());
            }
            previous = point.time_min;
        }
        if self
            .gradient
            .last()
            .is_some_and(|point| point.time_min > self.run_time_min)
        {
            return Err("LC method gradient extends past its run time".to_owned());
        }
        if self
            .detector_wavelengths_nm
            .iter()
            .any(|value| !value.is_finite() || *value <= 0.0)
        {
            return Err("LC method has an invalid detector wavelength".to_owned());
        }
        Ok(())
    }

    /// Linearly interpolate the programmed B composition between time points.
    pub fn percent_b_at(&self, time_min: f64) -> Option<f64> {
        if !time_min.is_finite() || time_min < 0.0 {
            return None;
        }
        let first = self.gradient.first()?;
        if time_min <= first.time_min {
            return Some(first.percent_b);
        }
        for pair in self.gradient.windows(2) {
            let [left, right] = pair else { continue };
            if time_min <= right.time_min {
                let fraction = (time_min - left.time_min) / (right.time_min - left.time_min);
                return Some(left.percent_b + fraction * (right.percent_b - left.percent_b));
            }
        }
        self.gradient.last().map(|point| point.percent_b)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MassSpecRun {
    pub source: String,
    pub metadata: BTreeMap<String, String>,
    pub instrument: Option<String>,
    pub streams: Vec<AcquisitionStream>,
    pub chromatograms: Vec<ChromatogramChannel>,
    pub import_warnings: Vec<String>,
}

impl MassSpecRun {
    pub fn stream(&self, id: AcquisitionStreamId) -> Option<&AcquisitionStream> {
        self.streams.iter().find(|stream| stream.id == id)
    }

    pub fn validate(&self) -> Result<(), String> {
        let mut stream_ids = BTreeSet::new();
        for stream in &self.streams {
            if !stream_ids.insert(stream.id) {
                return Err(format!("duplicate stream ID {}", stream.id));
            }
            if let Some([low, high]) = stream.acquisition_range
                && (!low.is_finite() || !high.is_finite() || high < low)
            {
                return Err(format!(
                    "stream {} has an invalid acquisition range",
                    stream.id
                ));
            }
            let mut spectrum_ids = BTreeSet::new();
            for spectrum in &stream.spectra {
                if !spectrum_ids.insert(spectrum.id) {
                    return Err(format!(
                        "stream {} has duplicate spectrum ID {}",
                        stream.id, spectrum.id
                    ));
                }
                validate_spectrum(stream.id, spectrum)?;
            }
        }
        if !self
            .streams
            .iter()
            .any(|stream| stream.role == StreamRole::Primary && !stream.spectra.is_empty())
        {
            return Err("run has no readable non-reference MS stream".to_owned());
        }
        let mut channel_ids = BTreeSet::new();
        for channel in &self.chromatograms {
            if !channel_ids.insert(channel.id.0.as_str()) {
                return Err(format!("duplicate chromatogram channel ID {}", channel.id));
            }
            if channel
                .source_stream
                .is_some_and(|id| !stream_ids.contains(&id))
            {
                return Err(format!(
                    "channel {} references a missing stream",
                    channel.id
                ));
            }
            if channel.time_min.len() != channel.values.len()
                || channel
                    .time_min
                    .iter()
                    .chain(&channel.values)
                    .any(|value| !value.is_finite())
            {
                return Err(format!("channel {} has invalid arrays", channel.id));
            }
            if channel.coordinate.is_some_and(|value| !value.is_finite()) {
                return Err(format!("channel {} has an invalid coordinate", channel.id));
            }
        }
        Ok(())
    }
}

fn validate_spectrum(stream: AcquisitionStreamId, spectrum: &MassSpectrum) -> Result<(), String> {
    if spectrum.ms_level == 0
        || !spectrum.retention_time_min.is_finite()
        || spectrum.mz.len() != spectrum.intensity.len()
        || spectrum
            .mz
            .iter()
            .any(|value| !value.is_finite() || *value < 0.0)
        || spectrum.intensity.iter().any(|value| !value.is_finite())
        || !spectrum.tic.is_finite()
        || spectrum.tic < 0.0
        || spectrum
            .base_peak_mz
            .is_some_and(|value| !value.is_finite() || value < 0.0)
        || spectrum
            .base_peak_intensity
            .is_some_and(|value| !value.is_finite() || value < 0.0)
        || spectrum.base_peak_mz.is_some() != spectrum.base_peak_intensity.is_some()
    {
        return Err(format!(
            "stream {stream} has invalid spectrum {}",
            spectrum.id
        ));
    }
    if let Some(precursor) = &spectrum.precursor
        && (!precursor.selected_mz.is_finite()
            || precursor.selected_mz <= 0.0
            || precursor.charge == Some(0)
            || precursor
                .isolation_window_lower_offset
                .is_some_and(|v| !v.is_finite() || v < 0.0)
            || precursor
                .isolation_window_upper_offset
                .is_some_and(|v| !v.is_finite() || v < 0.0)
            || precursor
                .collision_energy
                .is_some_and(|v| !v.is_finite() || v < 0.0))
    {
        return Err(format!(
            "stream {stream} spectrum {} has invalid precursor metadata",
            spectrum.id
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spectrum() -> MassSpectrum {
        MassSpectrum {
            id: SpectrumId::new(1),
            source_native_id: Some("scan=1".to_owned()),
            retention_time_min: 0.5,
            ms_level: 2,
            polarity: Polarity::Positive,
            representation: SpectrumRepresentation::Centroid,
            mz: vec![100.0],
            intensity: vec![5.0],
            tic: 5.0,
            base_peak_mz: Some(100.0),
            base_peak_intensity: Some(5.0),
            precursor: Some(Precursor {
                selected_mz: 445.2,
                charge: Some(2),
                isolation_window_lower_offset: Some(0.5),
                isolation_window_upper_offset: Some(0.5),
                collision_energy: Some(20.0),
                activation_method: Some("CID".to_owned()),
            }),
        }
    }

    fn run(spectra: Vec<MassSpectrum>) -> MassSpecRun {
        MassSpecRun {
            source: "test".to_owned(),
            metadata: BTreeMap::new(),
            instrument: None,
            streams: vec![AcquisitionStream {
                id: AcquisitionStreamId::new(4_294_967_297),
                source_native_id: Some("controllerType=0 controllerNumber=1".to_owned()),
                source_label: None,
                role: StreamRole::Primary,
                acquisition_range: None,
                spectra,
            }],
            chromatograms: Vec::new(),
            import_warnings: Vec::new(),
        }
    }

    #[test]
    fn validates_spectrum_metadata_and_wide_stable_ids() {
        assert!(run(vec![spectrum()]).validate().is_ok());
    }

    #[test]
    fn rejects_duplicate_spectra_and_invalid_precursors() {
        let item = spectrum();
        assert!(
            run(vec![item.clone(), item.clone()])
                .validate()
                .unwrap_err()
                .contains("duplicate spectrum")
        );
        let mut item = item;
        item.precursor.as_mut().unwrap().collision_energy = Some(f64::NAN);
        assert!(
            run(vec![item])
                .validate()
                .unwrap_err()
                .contains("precursor")
        );
    }

    #[test]
    fn stream_polarity_is_only_specific_when_all_spectra_agree() {
        let positive = spectrum();
        let mut negative = spectrum();
        negative.id = SpectrumId::new(2);
        negative.polarity = Polarity::Negative;
        assert_eq!(
            run(vec![positive.clone()]).streams[0].polarity(),
            Polarity::Positive
        );
        assert_eq!(
            run(vec![positive, negative]).streams[0].polarity(),
            Polarity::Unknown
        );
        assert_eq!(run(Vec::new()).streams[0].polarity(), Polarity::Unknown);
    }

    #[test]
    fn rejects_duplicate_chromatogram_channel_ids() {
        let mut run = run(vec![spectrum()]);
        let channel = ChromatogramChannel {
            id: ChromatogramChannelId("tic".to_owned()),
            kind: ChromatogramKind::Unknown,
            source_stream: None,
            coordinate: None,
            description: "TIC".to_owned(),
            unit: "count".to_owned(),
            time_min: vec![0.5],
            values: vec![5.0],
        };
        run.chromatograms = vec![channel.clone(), channel];
        assert!(
            run.validate()
                .unwrap_err()
                .contains("duplicate chromatogram")
        );
    }
}
