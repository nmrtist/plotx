use super::SpectrumRecord;
use crate::SpectrumSummaryProvenance;

pub(super) struct Summaries {
    pub(super) tic: f64,
    pub(super) tic_provenance: SpectrumSummaryProvenance,
    pub(super) base_peak_mz: Option<f64>,
    pub(super) base_peak_intensity: Option<f64>,
    pub(super) base_peak_provenance: SpectrumSummaryProvenance,
}

pub(super) fn resolve(record: &SpectrumRecord, intensity: &[f64]) -> Summaries {
    let (tic, tic_provenance) = record
        .total_ion_current
        .filter(|value| value.is_finite() && *value >= 0.0)
        .map_or_else(
            || {
                (
                    intensity.iter().copied().sum::<f64>().max(0.0),
                    SpectrumSummaryProvenance::Derived,
                )
            },
            |value| (value, SpectrumSummaryProvenance::Source),
        );
    let derived_base_peak = intensity
        .iter()
        .enumerate()
        .filter(|(_, value)| value.is_finite() && **value >= 0.0)
        .max_by(|(_, left), (_, right)| left.total_cmp(right));
    let (base_peak_mz, base_peak_intensity, base_peak_provenance) = record
        .base_peak_mz
        .zip(record.base_peak_intensity)
        .filter(|(mz, intensity)| {
            mz.is_finite() && *mz >= 0.0 && intensity.is_finite() && *intensity >= 0.0
        })
        .map_or_else(
            || {
                let (mz, intensity) = derived_base_peak.map_or((None, None), |(index, value)| {
                    (record.mz.get(index).copied(), Some(*value))
                });
                (mz, intensity, SpectrumSummaryProvenance::Derived)
            },
            |(mz, intensity)| (Some(mz), Some(intensity), SpectrumSummaryProvenance::Source),
        );
    Summaries {
        tic,
        tic_provenance,
        base_peak_mz,
        base_peak_intensity,
        base_peak_provenance,
    }
}
