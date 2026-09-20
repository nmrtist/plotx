//! Programmatic tensor construction; vendor readers supply their own library Dataset.

use crate::nmr_series::NmrSeriesSource;
use crate::nmr_view::NmrSource;
use crate::{Domain, IoError, NmrData2D, PseudoKind, QuadMode};
use nmr::axis::{AxisCoordinates, AxisDomain, AxisQuantity, AxisRole, AxisUnit, FrequencyEvidence};
use nmr::processed::{
    ComponentBasis, ProcessedAxis, ProcessedDataset, ProcessedDescriptor, ProcessedOrigin,
    ProcessedProvenance,
};
use nmr::raw::*;
use std::sync::Arc;

fn fail(error: impl std::fmt::Display) -> IoError {
    IoError::NmrConversion(error.to_string())
}

impl TryFrom<NmrData2D> for NmrSeriesSource {
    type Error = IoError;
    fn try_from(input: NmrData2D) -> Result<Self, IoError> {
        if input.indirect_conjugate
            || matches!(input.quad, QuadMode::StatesTppi | QuadMode::EchoAntiecho)
        {
            return Err(fail(
                "Construct an explicit nmr component encoding for this programmatic input",
            ));
        }
        let parameter = input.pseudo_axis.as_ref();
        let lanes = if input.domain == Domain::Time
            && input.quad == QuadMode::States
            && parameter.is_none()
        {
            2
        } else {
            1
        };
        if !input.rows.is_multiple_of(lanes)
            || input.data.len()
                != input
                    .rows
                    .checked_mul(input.cols)
                    .ok_or_else(|| fail("tensor size overflow"))?
        {
            return Err(fail("programmatic tensor shape does not match samples"));
        }
        let rows = input
            .nus
            .as_ref()
            .map_or(input.rows / lanes, |nus| nus.grid);
        let axis_fields = |index: usize| {
            let dim = if index == 0 {
                &input.indirect
            } else {
                &input.direct
            };
            let points = if index == 0 { rows } else { input.cols };
            if index == 0
                && let Some(parameter) = parameter
            {
                let (unit, quantity) = match parameter.kind {
                    PseudoKind::Gradient => (
                        Some(AxisUnit::TeslaPerMeter),
                        Some(AxisQuantity::MagneticFieldGradientStrength),
                    ),
                    PseudoKind::Delay => (Some(AxisUnit::Second), Some(AxisQuantity::TimeDelay)),
                    PseudoKind::Generic => (None, None),
                };
                return (
                    dim,
                    points,
                    AxisDomain::Parameter,
                    unit,
                    AxisCoordinates::Explicit(parameter.values.clone()),
                    quantity,
                );
            }
            match input.domain {
                Domain::Time => (
                    dim,
                    points,
                    AxisDomain::Time,
                    Some(AxisUnit::Second),
                    AxisCoordinates::Uniform {
                        start: 0.0,
                        step: 1.0 / dim.spectral_width_hz,
                    },
                    None,
                ),
                Domain::Frequency => {
                    let step = dim.spectral_width_hz / points as f64 / dim.observe_freq_mhz;
                    (
                        dim,
                        points,
                        AxisDomain::Frequency,
                        Some(AxisUnit::Ppm),
                        AxisCoordinates::Uniform {
                            start: dim.carrier_ppm - points as f64 / 2.0 * step,
                            step,
                        },
                        None,
                    )
                }
            }
        };
        let dataset: nmr::Dataset = if input.domain == Domain::Time {
            let mut axes = Vec::new();
            for index in 0..2 {
                let (dim, points, domain, unit, coordinates, quantity) = axis_fields(index);
                let kind = if index == 1 {
                    RawAxisKind::Direct(DirectSamples::Complex)
                } else if parameter.is_some() {
                    RawAxisKind::Parameter
                } else if lanes == 2 {
                    RawAxisKind::Indirect(IndirectComponents::Cartesian(
                        ComponentEvidence::user_constructed(),
                    ))
                } else {
                    RawAxisKind::Indirect(IndirectComponents::Scalar)
                };
                let mut axis =
                    RawAxis::new(kind, domain, unit, points, coordinates).map_err(fail)?;
                if domain == AxisDomain::Parameter {
                    axis = axis
                        .with_quantity(quantity)
                        .map_err(fail)?
                        .with_label(parameter.map(|parameter| parameter.name.clone()));
                } else {
                    axis = axis
                        .with_nucleus((!dim.nucleus.is_empty()).then(|| dim.nucleus.clone()))
                        .map_err(fail)?
                        .with_spectral_width_hz(Some(dim.spectral_width_hz))
                        .map_err(fail)?
                        .with_frequency_evidence(Some(
                            FrequencyEvidence::new(Some(dim.observe_freq_mhz), None)
                                .map_err(fail)?,
                        ))
                        .map_err(fail)?
                        .with_chemical_shift_reference(Some(
                            ChemicalShiftReference::user_constructed(
                                dim.carrier_ppm,
                                dim.observe_freq_mhz,
                            )
                            .map_err(fail)?,
                        ))
                        .map_err(fail)?
                        .with_group_delay(if index == 1 {
                            GroupDelayState::Pending(
                                PendingGroupDelay::user_constructed(dim.group_delay)
                                    .map_err(fail)?,
                            )
                        } else {
                            GroupDelayState::NotApplicable
                        })
                        .map_err(fail)?;
                }
                axes.push(axis);
            }
            let mut metadata =
                RawMetadata::new(None, None, None, None, input.experiment.clone()).map_err(fail)?;
            if let Some(meta) = input.diffusion {
                let shape = if (meta.shape_factor - 1.0 / 3.0).abs() < 1e-12 {
                    "SQUARE"
                } else {
                    return Err(fail(
                        "Declare diffusion analysis settings separately for a custom gradient shape",
                    ));
                };
                metadata = metadata
                    .with_diffusion(Some(
                        DiffusionAcquisition::new(
                            0,
                            "programmatic_gradient".into(),
                            meta.delta,
                            "programmatic_delta".into(),
                            meta.big_delta,
                            "programmatic_big_delta".into(),
                            Some((meta.tau, "programmatic_tau".into())),
                            Some((shape.into(), "programmatic_shape".into())),
                        )
                        .map_err(fail)?,
                    ))
                    .map_err(fail)?;
            }
            let builder = RawDatasetBuilder::new(axes, metadata).map_err(fail)?;
            if let Some(nus) = &input.nus {
                let indices = nus
                    .schedule
                    .as_ref()
                    .ok_or_else(|| fail("Supply a complete NUS sampling schedule"))?;
                if indices.len() != input.rows / lanes || indices.len() != nus.acquired {
                    return Err(fail("NUS observations do not match the schedule"));
                }
                let coordinates: Vec<_> = indices
                    .iter()
                    .map(|index| SamplingCoordinate::new(vec![*index]))
                    .collect();
                let traces = input
                    .data
                    .chunks_exact(lanes * input.cols)
                    .enumerate()
                    .map(|(index, samples)| {
                        SparseTrace::new(
                            ObservationOrdinal::new(index),
                            coordinates[index].clone(),
                            samples.to_vec(),
                        )
                    })
                    .collect();
                builder
                    .sparse(
                        traces,
                        SamplingSchedule::new(vec![rows], coordinates).map_err(fail)?,
                    )
                    .map_err(fail)?
                    .into()
            } else {
                builder.dense(input.data).map_err(fail)?.into()
            }
        } else {
            let mut axes = Vec::new();
            for index in 0..2 {
                let (dim, points, domain, unit, coordinates, quantity) = axis_fields(index);
                let axis = ProcessedAxis::new(
                    if domain == AxisDomain::Parameter {
                        AxisRole::ArrayParameter
                    } else {
                        AxisRole::Signal
                    },
                    domain,
                    unit,
                    points,
                    coordinates,
                    if index == 1 {
                        ComponentBasis::Cartesian
                    } else {
                        ComponentBasis::Scalar
                    },
                )
                .map_err(fail)?;
                axes.push(if domain == AxisDomain::Parameter {
                    axis.with_quantity(quantity).map_err(fail)?
                } else {
                    axis.with_nucleus((!dim.nucleus.is_empty()).then(|| dim.nucleus.clone()))
                        .map_err(fail)?
                        .with_frequency_evidence(Some(
                            FrequencyEvidence::new(Some(dim.observe_freq_mhz), None)
                                .map_err(fail)?,
                        ))
                        .map_err(fail)?
                        .with_spectral_width_hz(Some(dim.spectral_width_hz.abs()))
                        .map_err(fail)?
                });
            }
            let samples = input
                .data
                .iter()
                .flat_map(|value| [value.re, value.im])
                .collect();
            ProcessedDataset::from_dense_samples(
                ProcessedDescriptor::new(axes).map_err(fail)?,
                samples,
                ProcessedProvenance::new(ProcessedOrigin::Unknown, vec![]).map_err(fail)?,
            )
            .map_err(fail)?
            .into()
        };
        let mut source = NmrSource::new(Arc::new(dataset))?;
        source.set_programmatic_label(input.source);
        Self::new(source)
    }
}

impl TryFrom<NmrSource> for NmrSeriesSource {
    type Error = IoError;
    fn try_from(source: NmrSource) -> Result<Self, IoError> {
        Self::new(source)
    }
}
