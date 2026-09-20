//! Diagnostic transforms over the CRAFT input view use the NMR library kernels.

use super::{CraftError, CraftReference};
use crate::Spectrum;
use nmr::axis::{AxisCoordinates, AxisDomain, AxisUnit, FrequencyEvidence};
use nmr::processing::{
    FourierTransform, FrequencyFrame, ProcessingOperation as Op, ProcessingPlan, ReferenceSource,
    SpectrumOperation, Window, ZeroFill,
};
use nmr::raw::{
    ChemicalShiftReference, DirectSamples, RawAxis, RawAxisKind, RawDatasetBuilder, RawMetadata,
};
use plotx_io::NmrData;

/// Transform the selected modeling interval with a matched exponential window.
/// The diagnostic has no digital-filter correction: only magnitudes are used.
pub fn preview_spectrum(
    data: &NmrData,
    reference: CraftReference,
    skip: usize,
) -> Result<Spectrum, CraftError> {
    let fail = |error: &dyn std::fmt::Display| CraftError::Preflight(error.to_string());
    if data.domain != plotx_io::Domain::Time {
        return Err(CraftError::InvalidInput);
    }
    reference.validate(data)?;
    let retained = data
        .points
        .len()
        .checked_sub(skip)
        .filter(|count| *count >= 3)
        .ok_or(CraftError::InvalidInput)?;
    let target = retained
        .checked_next_power_of_two()
        .ok_or(CraftError::InvalidInput)?;
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
    .map_err(|error| fail(&error))?
    .with_spectral_width_hz(Some(data.spectral_width_hz))
    .map_err(|error| fail(&error))?
    .with_frequency_evidence(Some(
        FrequencyEvidence::new(Some(data.observe_freq_mhz), None).map_err(|error| fail(&error))?,
    ))
    .map_err(|error| fail(&error))?
    .with_chemical_shift_reference(Some(
        ChemicalShiftReference::user_constructed(
            reference.effective_carrier_ppm(),
            reference.reference_frequency_mhz,
        )
        .map_err(|error| fail(&error))?,
    ))
    .map_err(|error| fail(&error))?;
    // This is a disposable analysis view, not a substitute for the acquisition
    // Dataset and its provenance in application storage.
    let input = RawDatasetBuilder::new(vec![axis], RawMetadata::default())
        .map_err(|error| fail(&error))?
        .dense(data.points.clone())
        .map_err(|error| fail(&error))?;
    let plan = ProcessingPlan::new(vec![
        Op::Spectrum {
            axis: 0,
            operation: SpectrumOperation::RetainRange {
                start: skip,
                end: data.points.len(),
            },
        },
        Op::Window {
            axis: 0,
            window: Window::exponential(data.spectral_width_hz / retained as f64)
                .map_err(|error| fail(&error))?,
        },
        Op::ZeroFill {
            axis: 0,
            zero_fill: ZeroFill::new(target).map_err(|error| fail(&error))?,
        },
        Op::FourierTransform {
            axis: 0,
            transform: FourierTransform::default(),
        },
        Op::ResolveFrequencyFrame {
            axis: 0,
            frame: FrequencyFrame::Ppm(ReferenceSource::AxisEvidence),
        },
        Op::Spectrum {
            axis: 0,
            operation: SpectrumOperation::Magnitude,
        },
    ])
    .map_err(|error| fail(&error))?;
    let output = plan.apply(&input.into()).map_err(|error| fail(&error))?;
    let source = plotx_io::nmr_view::NmrSource::new(std::sync::Arc::new(output))
        .map_err(|error| fail(&error))?;
    Ok(Spectrum {
        ppm: source.axes()[0]
            .coordinate_values()
            .map_err(|error| fail(&error))?,
        values: source.trace().map_err(|error| fail(&error))?,
        unit: nmr::axis::AxisUnit::Ppm,
        hz_per_point: Some(data.spectral_width_hz / target as f64),
        observe_freq_mhz: Some(data.observe_freq_mhz),
        nucleus: data.nucleus.clone(),
    })
}
