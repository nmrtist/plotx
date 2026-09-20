//! Explicit programmatic samples for simulations and high-level analysis.
//! File imports use `nmr_bridge::read` and never pass through this constructor.

use crate::{Domain, IoError, NmrData, nmr_view::NmrSource};
use nmr::acquisition::{ComponentBasis, GroupDelayState, PendingGroupDelay};
use nmr::axis::{AxisCoordinates, AxisDomain, AxisRole, AxisUnit, FrequencyEvidence};
use nmr::processed::{ProcessedAxis, ProcessedDataset, ProcessedOrigin, ProcessedProvenance};
use nmr::raw::{
    ChemicalShiftReference, DirectSamples, RawAxis, RawAxisKind, RawDatasetBuilder, RawMetadata,
};
use std::sync::Arc;

impl TryFrom<NmrData> for NmrSource {
    type Error = IoError;

    fn try_from(data: NmrData) -> Result<Self, Self::Error> {
        let fail = |error: &dyn std::fmt::Display| IoError::NmrConversion(error.to_string());
        let frequency =
            FrequencyEvidence::new(Some(data.observe_freq_mhz), None).map_err(|e| fail(&e))?;
        let dataset = match data.domain {
            Domain::Time => {
                let axis = RawAxis::new(
                    RawAxisKind::Direct(DirectSamples::Complex),
                    AxisDomain::Time,
                    Some(AxisUnit::Second),
                    data.points.len(),
                    AxisCoordinates::Uniform {
                        start: 0.0,
                        step: 1.0 / data.spectral_width_hz,
                    },
                )
                .map_err(|e| fail(&e))?
                .with_spectral_width_hz(Some(data.spectral_width_hz))
                .map_err(|e| fail(&e))?
                .with_frequency_evidence(Some(frequency))
                .map_err(|e| fail(&e))?
                .with_nucleus(Some(data.nucleus))
                .map_err(|e| fail(&e))?
                .with_chemical_shift_reference(Some(
                    ChemicalShiftReference::user_constructed(
                        data.carrier_ppm,
                        data.observe_freq_mhz,
                    )
                    .map_err(|e| fail(&e))?,
                ))
                .map_err(|e| fail(&e))?
                .with_group_delay(GroupDelayState::Pending(
                    PendingGroupDelay::user_constructed(data.group_delay).map_err(|e| fail(&e))?,
                ))
                .map_err(|e| fail(&e))?;
                RawDatasetBuilder::new(vec![axis], RawMetadata::default())
                    .map_err(|e| fail(&e))?
                    .dense(data.points)
                    .map_err(|e| fail(&e))?
                    .into()
            }
            Domain::Frequency => {
                // This input type declares a uniform ppm grid by width, carrier
                // and reference frequency. Imported explicit grids bypass it.
                let step = data.spectral_width_hz / data.points.len() as f64;
                let axis = ProcessedAxis::new(
                    AxisRole::Signal,
                    AxisDomain::Frequency,
                    Some(AxisUnit::Hertz),
                    data.points.len(),
                    AxisCoordinates::Uniform {
                        start: -(data.points.len() as f64) / 2.0 * step,
                        step,
                    },
                    ComponentBasis::Cartesian,
                )
                .map_err(|e| fail(&e))?
                .with_frequency_evidence(Some(frequency))
                .map_err(|e| fail(&e))?
                .with_spectral_width_hz(Some(data.spectral_width_hz.abs()))
                .map_err(|e| fail(&e))?
                .with_nucleus(Some(data.nucleus))
                .map_err(|e| fail(&e))?;
                let spectrum = ProcessedDataset::from_complex_trace(
                    axis,
                    data.points,
                    ProcessedProvenance::new(ProcessedOrigin::Unknown, Vec::new())
                        .map_err(|e| fail(&e))?,
                )
                .map_err(|e| fail(&e))?;
                use nmr::processing::{
                    FrequencyFrame, ProcessingOperation, ProcessingPlan, ReferenceSource,
                };
                ProcessingPlan::new(vec![ProcessingOperation::ResolveFrequencyFrame {
                    axis: 0,
                    frame: FrequencyFrame::Ppm(ReferenceSource::Explicit(
                        ChemicalShiftReference::user_constructed(
                            data.carrier_ppm,
                            data.observe_freq_mhz,
                        )
                        .map_err(|e| fail(&e))?,
                    )),
                }])
                .map_err(|e| fail(&e))?
                .apply(&spectrum.into())
                .map_err(|e| fail(&e))?
            }
        };
        let mut source = Self::new(Arc::new(dataset))?;
        source.set_programmatic_label(data.source);
        Ok(source)
    }
}
