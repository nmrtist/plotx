//! Immutable descriptor views. The library Dataset remains the scientific input.

use crate::{IoError, NmrData};
use nmr::acquisition::{ComponentBasis, GroupDelayState};
use nmr::axis::{AxisCoordinates, AxisDomain, AxisRole, AxisUnit, FrequencyEvidence};
use nmr::dataset::DescriptorRef;
use nmr::{Complex64, Dataset};
use std::sync::Arc;

#[derive(Clone, Debug)]
pub struct NmrAxis {
    pub role: AxisRole,
    pub domain: AxisDomain,
    pub unit: Option<AxisUnit>,
    pub points: usize,
    pub coordinates: AxisCoordinates,
    pub nucleus: Option<String>,
    pub label: Option<String>,
    pub frequency: Option<FrequencyEvidence>,
    pub spectral_width_hz: Option<f64>,
}

impl NmrAxis {
    pub fn observe_frequency_mhz(&self) -> Option<f64> {
        self.frequency
            .and_then(|value| value.observe_frequency_mhz())
    }

    pub fn coordinate_values(&self) -> Result<Vec<f64>, IoError> {
        match &self.coordinates {
            AxisCoordinates::Explicit(values) => Ok(values.clone()),
            AxisCoordinates::Uniform { start, step } => Ok((0..self.points)
                .map(|index| step.mul_add(index as f64, *start))
                .collect()),
            _ => Err(IoError::NmrConversion(
                "axis coordinates are unknown".into(),
            )),
        }
    }
}

/// The descriptor summaries cannot be mutated independently of the owned input.
#[derive(Clone, Debug)]
pub struct NmrSource {
    dataset: Arc<Dataset>,
    axes: Vec<NmrAxis>,
    source_label: String,
}

impl NmrSource {
    pub fn new(dataset: Arc<Dataset>) -> Result<Self, IoError> {
        let axes = match dataset.descriptor() {
            DescriptorRef::Raw(descriptor) => descriptor
                .axes()
                .iter()
                .map(|axis| NmrAxis {
                    role: axis.role(),
                    domain: axis.domain(),
                    unit: axis.unit(),
                    points: axis.points(),
                    coordinates: axis.coordinates().clone(),
                    nucleus: axis.nucleus().map(str::to_owned),
                    label: axis.label().map(str::to_owned),
                    frequency: axis.frequency_evidence(),
                    spectral_width_hz: axis.spectral_width_hz(),
                })
                .collect(),
            DescriptorRef::Processed(descriptor) => descriptor
                .axes()
                .iter()
                .map(|axis| NmrAxis {
                    role: axis.role(),
                    domain: axis.domain(),
                    unit: axis.unit(),
                    points: axis.points(),
                    coordinates: axis.coordinates().clone(),
                    nucleus: axis.nucleus().map(str::to_owned),
                    label: axis.label().map(str::to_owned),
                    frequency: axis.frequency_evidence(),
                    spectral_width_hz: axis.spectral_width_hz(),
                })
                .collect(),
            _ => return Err(IoError::NmrConversion("unsupported NMR descriptor".into())),
        };
        let source_label = crate::nmr_bridge::identity(&dataset).source_label;
        Ok(Self {
            dataset,
            axes,
            source_label,
        })
    }

    pub fn dataset(&self) -> &Arc<Dataset> {
        &self.dataset
    }

    pub fn axes(&self) -> &[NmrAxis] {
        &self.axes
    }

    pub fn source(&self) -> &str {
        &self.source_label
    }

    pub fn identity(&self) -> crate::AcquisitionIdentity {
        let mut identity = crate::nmr_bridge::identity(&self.dataset);
        identity.source_label = self.source_label.clone();
        identity
    }

    /// A host display label does not alter acquisition facts or canonical digests.
    pub fn with_display_label(mut self, label: String) -> Self {
        self.source_label = label;
        self
    }

    /// MHz used for chemical-shift differences, distinct from observe frequency.
    /// Axes without reference evidence return `None`.
    pub fn reference_frequency_mhz(&self, axis: usize) -> Option<f64> {
        if let Some(raw) = self.dataset.as_raw() {
            return raw
                .descriptor()
                .axes()
                .get(axis)?
                .chemical_shift_reference()
                .map(|reference| reference.reference_frequency_mhz());
        }
        self.dataset
            .as_processed()?
            .axis_evidence(axis)?
            .reference_frequency_mhz()
    }

    pub fn len(&self) -> usize {
        self.axes.first().map_or(0, |axis| axis.points)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn nucleus(&self) -> &str {
        self.axes
            .first()
            .and_then(|axis| axis.nucleus.as_deref())
            .unwrap_or("")
    }

    pub(crate) fn set_programmatic_label(&mut self, label: String) {
        self.source_label = label;
    }

    pub fn direct_axis(&self) -> Result<&NmrAxis, IoError> {
        self.axes
            .last()
            .ok_or_else(|| IoError::NmrConversion("NMR input has no axis".into()))
    }

    pub fn domain(&self) -> Result<crate::Domain, IoError> {
        match self.direct_axis()?.domain {
            AxisDomain::Time => Ok(crate::Domain::Time),
            AxisDomain::Frequency => Ok(crate::Domain::Frequency),
            _ => Err(IoError::NmrConversion(
                "NMR signal axis has no time or frequency domain".into(),
            )),
        }
    }

    pub fn has_imaginary(&self, axis: usize) -> bool {
        if let Some(raw) = self.dataset.as_raw() {
            return raw.descriptor().axes().get(axis).is_some_and(|axis| {
                matches!(
                    axis.kind(),
                    nmr::raw::RawAxisKind::Direct(nmr::raw::DirectSamples::Complex)
                ) || matches!(
                    axis.kind(),
                    nmr::raw::RawAxisKind::Indirect(
                        nmr::raw::IndirectComponents::Cartesian(_)
                            | nmr::raw::IndirectComponents::SharedComplex { .. }
                    )
                )
            });
        }
        self.dataset.as_processed().is_some_and(|processed| {
            processed.descriptor().axes().get(axis).is_some_and(|axis| {
                matches!(
                    axis.component_basis(),
                    ComponentBasis::Cartesian | ComponentBasis::SharedComplex { .. }
                )
            })
        })
    }

    /// Full complex samples of a rank-one input. A scalar display has zero
    /// imaginary values, while `has_imaginary` continues to report its true basis.
    pub fn trace(&self) -> Result<Vec<Complex64>, IoError> {
        if self.axes.len() != 1 {
            return Err(IoError::NmrConversion(
                "select a one-dimensional NMR trace".into(),
            ));
        }
        if let Some(raw) = self.dataset.as_raw() {
            return raw
                .read_trace(&[])
                .map(|trace| trace.samples().to_vec())
                .map_err(|error| IoError::Nmr(Box::new(error)));
        }
        let processed = self
            .dataset
            .as_processed()
            .ok_or_else(|| IoError::NmrConversion("unsupported NMR trace representation".into()))?;
        let axis = &processed.descriptor().axes()[0];
        if !matches!(
            axis.component_basis(),
            ComponentBasis::Scalar | ComponentBasis::Cartesian
        ) {
            return Err(IoError::NmrConversion(
                "decode NMR components before displaying a spectrum".into(),
            ));
        }
        (0..axis.points())
            .map(|point| {
                let sample = |component| {
                    processed
                        .data()
                        .get(&[point], &[component])
                        .map_err(|error| IoError::NmrConversion(error.to_string()))
                };
                Ok(Complex64::new(
                    sample(0)?,
                    if axis.component_count() == 2 {
                        sample(1)?
                    } else {
                        0.0
                    },
                ))
            })
            .collect()
    }

    /// CRAFT requires a calibrated complex FID. Missing facts prevent analysis;
    /// they do not prevent importing, saving or displaying the library Dataset.
    pub fn craft_fid(&self) -> Result<NmrData, IoError> {
        let missing = |message: &str| IoError::NmrConversion(message.to_owned());
        let raw = self
            .dataset
            .as_raw()
            .ok_or_else(|| missing("CRAFT requires a raw FID"))?;
        if self.axes.len() != 1 || !self.has_imaginary(0) {
            return Err(missing(
                "CRAFT requires one complex direct acquisition axis",
            ));
        }
        let axis = &raw.descriptor().axes()[0];
        let reference = axis
            .chemical_shift_reference()
            .ok_or_else(|| missing("CRAFT requires chemical-shift reference evidence"))?;
        let spectral_width_hz = axis
            .spectral_width_hz()
            .ok_or_else(|| missing("CRAFT requires spectral width"))?;
        let observe_freq_mhz = axis
            .frequency_evidence()
            .and_then(|value| value.observe_frequency_mhz())
            .ok_or_else(|| missing("CRAFT requires observe frequency"))?;
        let group_delay = match axis.group_delay() {
            GroupDelayState::Pending(delay) => delay.delay_points(),
            GroupDelayState::NotApplicable => 0.0,
            _ => {
                return Err(missing(
                    "CRAFT requires known digital-filter delay evidence",
                ));
            }
        };
        if axis.domain() != AxisDomain::Time
            || axis.unit() != Some(AxisUnit::Second)
            || !matches!(axis.coordinates(), AxisCoordinates::Uniform { start, step }
                if *start == 0.0 && (*step * spectral_width_hz - 1.0).abs() < 1e-10)
        {
            return Err(missing(
                "CRAFT requires a uniform FID starting at the acquisition time origin",
            ));
        }
        Ok(NmrData {
            points: self.trace()?,
            domain: crate::Domain::Time,
            spectral_width_hz,
            observe_freq_mhz,
            carrier_ppm: reference.carrier_ppm(),
            nucleus: axis.nucleus().unwrap_or("").to_owned(),
            source: crate::nmr_bridge::identity(&self.dataset).source_label,
            group_delay,
        })
    }
}
