use super::mass_spec::{MassSpecDataset, readable_ms_stream, stream_display_label_for_id};
use plotx_io::{AcquisitionStreamId, MassSpecRun};
use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct IonChromatogramId(u64);

impl IonChromatogramId {
    pub const fn new(value: u64) -> Self {
        Self(value)
    }
    pub const fn get(self) -> u64 {
        self.0
    }
    pub(crate) fn checked_advance(self) -> Option<Self> {
        self.0.checked_add(1).map(Self)
    }
}

impl fmt::Display for IonChromatogramId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.0)
    }
}

/// A fixed, user-requested extracted-ion chromatogram. The arrays are stored
/// so this result does not change with transient preview or stream state.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ExtractedIonChromatogram {
    pub id: IonChromatogramId,
    pub stream: AcquisitionStreamId,
    pub mz_min: f64,
    pub mz_max: f64,
    pub time_min: Vec<f64>,
    pub intensity: Vec<f64>,
}

impl MassSpecDataset {
    pub(crate) fn plan_ion_chromatogram(
        &self,
        stream: AcquisitionStreamId,
        first_mz: f64,
        second_mz: f64,
    ) -> Result<ExtractedIonChromatogram, String> {
        if !first_mz.is_finite() || !second_mz.is_finite() || first_mz < 0.0 || second_mz < 0.0 {
            return Err("The m/z interval must contain finite, non-negative bounds.".to_owned());
        }
        let (mz_min, mz_max) = if first_mz <= second_mz {
            (first_mz, second_mz)
        } else {
            (second_mz, first_mz)
        };
        if mz_min >= mz_max {
            return Err("The m/z interval must have distinct ordered bounds.".to_owned());
        }
        let scans = self
            .run
            .stream(stream)
            .filter(|stream| readable_ms_stream(stream))
            .ok_or_else(|| {
                format!(
                    "{} has no readable MS scans.",
                    stream_display_label_for_id(&self.run, stream)
                )
            })?;
        let intensity =
            plotx_analysis::mass_spec::extract_ion_chromatogram(&scans.spectra, mz_min, mz_max)?;
        Ok(ExtractedIonChromatogram {
            id: self.next_ion_chromatogram_id,
            stream,
            mz_min,
            mz_max,
            time_min: scans
                .spectra
                .iter()
                .map(|scan| scan.retention_time_min)
                .collect(),
            intensity,
        })
    }

    pub(crate) fn replace_ion_chromatograms(
        &mut self,
        chromatograms: Vec<ExtractedIonChromatogram>,
        next_id: IonChromatogramId,
    ) -> Result<(), String> {
        self.extracted_ion_chromatograms = chromatograms;
        self.next_ion_chromatogram_id = next_id;
        Self::validate_ion_chromatogram_state(
            &self.run,
            &mut self.extracted_ion_chromatograms,
            &mut self.next_ion_chromatogram_id,
        )?;
        self.rebuild_field_catalog();
        Ok(())
    }

    pub(crate) fn validate_ion_chromatograms(&mut self) -> Result<(), String> {
        Self::validate_ion_chromatogram_state(
            &self.run,
            &mut self.extracted_ion_chromatograms,
            &mut self.next_ion_chromatogram_id,
        )
    }

    pub(crate) fn validate_ion_chromatogram_state(
        run: &MassSpecRun,
        chromatograms: &mut [ExtractedIonChromatogram],
        next_id: &mut IonChromatogramId,
    ) -> Result<(), String> {
        chromatograms.sort_by_key(|xic| xic.id);
        let mut previous = None;
        for xic in chromatograms.iter() {
            if xic.id.get() == 0 {
                return Err("XIC has invalid id 0".to_owned());
            }
            if previous == Some(xic.id) {
                return Err(format!(
                    "LC–MS project contains duplicate XIC id {}",
                    xic.id
                ));
            }
            previous = Some(xic.id);
            if !xic.mz_min.is_finite()
                || !xic.mz_max.is_finite()
                || xic.mz_min < 0.0
                || xic.mz_min >= xic.mz_max
            {
                return Err(format!("XIC {} has an invalid m/z interval", xic.id));
            }
            let Some(stream) = run
                .stream(xic.stream)
                .filter(|stream| readable_ms_stream(stream))
            else {
                return Err(format!(
                    "XIC {} references missing stream {}",
                    xic.id, xic.stream
                ));
            };
            if xic.time_min.len() != xic.intensity.len()
                || xic.time_min.len() != stream.spectra.len()
            {
                return Err(format!(
                    "XIC {} has arrays inconsistent with its source stream",
                    xic.id
                ));
            }
            for ((&time, &intensity), scan) in
                xic.time_min.iter().zip(&xic.intensity).zip(&stream.spectra)
            {
                if !time.is_finite() || !intensity.is_finite() || time != scan.retention_time_min {
                    return Err(format!("XIC {} has invalid stored values", xic.id));
                }
            }
        }
        let minimum_next = chromatograms
            .last()
            .map_or(Ok(IonChromatogramId::new(1)), |xic| {
                xic.id
                    .checked_advance()
                    .ok_or_else(|| "LC–MS XIC identity overflow".to_owned())
            })?;
        if *next_id < minimum_next {
            return Err("LC–MS XIC identity allocator would reuse an existing identity".to_owned());
        }
        Ok(())
    }
}

pub fn xic_key(id: IonChromatogramId) -> String {
    format!("mass_spec.xic.{id}.chromatogram")
}

pub fn xic_title(run: &MassSpecRun, xic: &ExtractedIonChromatogram) -> String {
    format!(
        "Extracted ion chromatogram — m/z {:.4}–{:.4} — {}",
        xic.mz_min,
        xic.mz_max,
        stream_display_label_for_id(run, xic.stream)
    )
}
