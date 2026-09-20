//! Immutable application summaries of a rank-two library Dataset.

use crate::nmr_view::{NmrAxis, NmrSource};
use crate::{AxisSource, DiffusionMeta, Domain, IoError, PseudoAxis, PseudoKind};
use nmr::axis::{AxisCoordinates, AxisDomain, AxisQuantity, AxisUnit};
use std::ops::Deref;

#[derive(Clone, Debug)]
pub struct NmrDimension {
    pub nucleus: String,
    pub observe_freq_mhz: Option<f64>,
    pub spectral_width_hz: Option<f64>,
    pub unit: Option<AxisUnit>,
    pub domain: AxisDomain,
}

impl From<&NmrAxis> for NmrDimension {
    fn from(axis: &NmrAxis) -> Self {
        Self {
            nucleus: axis.nucleus.clone().unwrap_or_default(),
            observe_freq_mhz: axis.observe_frequency_mhz(),
            spectral_width_hz: axis.spectral_width_hz,
            unit: axis.unit,
            domain: axis.domain,
        }
    }
}

#[derive(Clone, Debug)]
pub struct NusSummary {
    pub grid: usize,
    pub acquired: usize,
    pub schedule: Vec<usize>,
}

/// These fields are read-only through `NmrSeriesSource`. None are serialized as
/// another scientific payload or used to reconstruct library samples.
#[derive(Clone, Debug)]
pub struct SeriesSummary {
    pub rows: usize,
    pub cols: usize,
    pub direct: NmrDimension,
    pub indirect: NmrDimension,
    pub source: String,
    pub experiment: Option<String>,
    pub pseudo_axis: Option<PseudoAxis>,
    pub diffusion: Option<DiffusionMeta>,
    pub nus: Option<NusSummary>,
}

#[derive(Clone, Debug)]
pub struct NmrSeriesSource {
    source: NmrSource,
    summary: SeriesSummary,
}

impl Deref for NmrSeriesSource {
    type Target = SeriesSummary;
    fn deref(&self) -> &Self::Target {
        &self.summary
    }
}

impl NmrSeriesSource {
    pub fn new(source: NmrSource) -> Result<Self, IoError> {
        if source.axes().len() != 2 {
            return Err(IoError::NmrConversion(
                "select a rank-two NMR dataset".into(),
            ));
        }
        let axes = source.axes();
        let raw = source.dataset().as_raw();
        let quantity = raw
            .map(|raw| raw.descriptor().axes()[0].quantity())
            .or_else(|| {
                source
                    .dataset()
                    .as_processed()
                    .map(|data| data.descriptor().axes()[0].quantity())
            })
            .flatten();
        let pseudo_axis = if axes[0].domain == AxisDomain::Parameter
            && !matches!(axes[0].coordinates, AxisCoordinates::Unknown)
        {
            Some(PseudoAxis {
                name: axes[0].label.clone().unwrap_or_else(|| "Parameter".into()),
                kind: match quantity {
                    Some(AxisQuantity::MagneticFieldGradientStrength) => PseudoKind::Gradient,
                    Some(AxisQuantity::TimeDelay) => PseudoKind::Delay,
                    _ => PseudoKind::Generic,
                },
                values: axes[0].coordinate_values()?,
                unit: match axes[0].unit {
                    Some(AxisUnit::Second) => "s",
                    Some(AxisUnit::TeslaPerMeter) => "mT/m",
                    Some(AxisUnit::Tesla) => "T",
                    Some(AxisUnit::Hertz) => "Hz",
                    Some(AxisUnit::Ppm) => "ppm",
                    _ => "",
                }
                .into(),
                source: AxisSource::LibraryEvidence,
            })
        } else {
            None
        };
        let acquisition = raw.map(|raw| raw.descriptor().acquisition());
        let diffusion = acquisition
            .and_then(|metadata| metadata.diffusion())
            .and_then(|metadata| {
                let shape_factor = match metadata
                    .gradient_shape()?
                    .trim()
                    .to_ascii_uppercase()
                    .as_str()
                {
                    "SQUARE" => 1.0 / 3.0,
                    "SINE" => 0.3125,
                    "SQUARE_SINE" => 0.30167,
                    "TRAPEZOID" => 0.32545,
                    "S_RECTANGLE" => 0.32526,
                    _ => return None,
                };
                Some(DiffusionMeta {
                    gamma: crate::gyromagnetic_ratio(axes[1].nucleus.as_deref()?)?,
                    delta: metadata.gradient_pulse_duration_seconds(),
                    big_delta: metadata.diffusion_time_seconds(),
                    tau: metadata.recovery_delay_seconds()?,
                    shape_factor,
                })
            });
        let nus = raw
            .and_then(|raw| raw.sampling_schedule())
            .map(|schedule| NusSummary {
                grid: axes[0].points,
                acquired: schedule.coordinates().len(),
                schedule: schedule
                    .coordinates()
                    .iter()
                    .map(|coordinate| coordinate.as_slice()[0])
                    .collect(),
            });
        let summary = SeriesSummary {
            rows: axes[0].points,
            cols: axes[1].points,
            direct: (&axes[1]).into(),
            indirect: (&axes[0]).into(),
            source: source.source().to_owned(),
            experiment: acquisition
                .and_then(|metadata| metadata.pulse_program().map(str::to_owned)),
            pseudo_axis,
            diffusion,
            nus,
        };
        Ok(Self { source, summary })
    }

    pub fn source_dataset(&self) -> &NmrSource {
        &self.source
    }
    pub fn input_domain(&self, axis: usize) -> Result<Domain, IoError> {
        match self.source.axes().get(axis).map(|axis| axis.domain) {
            Some(AxisDomain::Time) => Ok(Domain::Time),
            Some(AxisDomain::Frequency) => Ok(Domain::Frequency),
            _ => Err(IoError::NmrConversion(
                "parameter axes do not accept spectral processing".into(),
            )),
        }
    }
}
