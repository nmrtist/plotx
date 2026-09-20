//! Two-dimensional execution keeps Cartesian lanes and sparse inputs in nmr.

use super::{ExecutionError, nmr_bridge};
use crate::nmr_bridge::{DelayPolicy, PhaseReport, RecipeError, RecipeRange};
use crate::{AxisMeta, Layout2D, Params2D, Processed2D, Spectrum2D, StackSpectrum};
use nmr::axis::{AxisDomain, AxisUnit};
use nmr::processing::{AutoNusSettings, NusSettings, ProcessingOptions};
use nmr::{Complex64, ExecutionContext};
use plotx_io::{Domain, IoError, nmr_view::NmrSource};
use std::sync::Arc;

#[derive(Clone, Debug)]
pub struct Output2D {
    pub source: NmrSource,
    pub view: Processed2D,
    pub phases: Vec<PhaseReport>,
}

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct NusRequest {
    /// Library iteration ceiling, 1..=2048; exhaustion is an error.
    pub max_iterations: usize,
    /// An independent noise estimate in the processed F2 spectrum amplitude
    /// units. Absence selects library automatic estimation, never zero noise.
    pub noise_standard_deviation: Option<f64>,
}

impl Default for NusRequest {
    fn default() -> Self {
        Self {
            max_iterations: 1000,
            noise_standard_deviation: None,
        }
    }
}

fn library(source: nmr::processing::ProcessingError) -> ExecutionError {
    RecipeError::Library { step: None, source }.into()
}

pub fn validate_2d_domains(
    source: &plotx_io::nmr_series::NmrSeriesSource,
    params: &Params2D,
) -> Result<(), String> {
    for (index, name, pipeline) in [(1, "F2", &params.f2), (0, "F1", &params.f1)] {
        if source.source_dataset().axes()[index].domain == AxisDomain::Parameter {
            if pipeline.steps.iter().any(|step| step.enabled) {
                return Err(format!(
                    "invalid {name} pipeline: parameter axes cannot accept spectral processing"
                ));
            }
        } else {
            pipeline
                .output_domain(
                    source
                        .input_domain(index)
                        .map_err(|error| error.to_string())?,
                )
                .map_err(|error| format!("invalid {name} pipeline: {error}"))?;
        }
    }
    Ok(())
}

pub fn execute_2d(
    source: &NmrSource,
    params: &Params2D,
    delay: DelayPolicy,
    range: RecipeRange,
    nus: Option<NusRequest>,
    context: &mut ExecutionContext<'_>,
) -> Result<Output2D, ExecutionError> {
    if source.axes().len() != 2 {
        return Err(RecipeError::Invalid("2D processing requires two axes".into()).into());
    }
    if matches!(range, RecipeRange::All) {
        let base = execute_2d(source, params, delay, RecipeRange::Base, nus, context)?;
        return execute_2d(
            &base.source,
            params,
            DelayPolicy::Disabled,
            RecipeRange::Frequency,
            None,
            context,
        );
    }
    let options = ProcessingOptions::default();
    let direct = nmr_bridge::compile(source.dataset().clone(), &params.f2, 1, delay, range)?;
    let sparse = source
        .dataset()
        .as_raw()
        .is_some_and(|raw| raw.data().is_sparse());
    let mut phases = Vec::new();
    let mut output =
        if sparse && params.f2.has_enabled_fft() && !matches!(range, RecipeRange::Frequency) {
            let request = nus.unwrap_or_default();
            let plan = direct.deterministic_plan()?;
            let prepared = if request.noise_standard_deviation.is_some() {
                NusSettings {
                    max_iterations: request.max_iterations,
                    noise_standard_deviation: request.noise_standard_deviation,
                }
                .prepare_with_context(source.dataset(), plan, options, context)
            } else {
                AutoNusSettings {
                    max_iterations: request.max_iterations,
                }
                .prepare_with_context(source.dataset(), plan, options, context)
            }
            .map_err(library)?;
            Arc::new(prepared.execute_with_context(context).map_err(library)?)
        } else if sparse && !params.f2.has_enabled_fft() {
            // Only an unchanged acquisition can be displayed before reconstruction.
            if params.f2.steps.iter().any(|step| step.enabled) {
                return Err(RecipeError::Invalid(
                    "Reconstruct NUS data before applying a time-domain recipe".into(),
                )
                .into());
            }
            source.dataset().clone()
        } else {
            let result = direct.execute_with_report(options, context)?;
            phases.extend(result.phases);
            result.dataset
        };
    if params.layout == Layout2D::Ft {
        let result =
            nmr_bridge::compile(output.clone(), &params.f1, 0, DelayPolicy::Disabled, range)?
                .execute_with_report(options, context)?;
        output = result.dataset;
        phases.extend(result.phases);
    }
    let source = NmrSource::new(output)?.with_display_label(source.source().to_owned());
    let view = view_2d(&source, params.layout, context)?;
    Ok(Output2D {
        source,
        view,
        phases,
    })
}

fn domain(axis: &plotx_io::nmr_view::NmrAxis) -> Result<Domain, IoError> {
    match (axis.domain, axis.unit) {
        (AxisDomain::Time, Some(AxisUnit::Second)) => Ok(Domain::Time),
        (AxisDomain::Frequency, Some(AxisUnit::Ppm | AxisUnit::Hertz)) => Ok(Domain::Frequency),
        _ => Err(IoError::NmrConversion(
            "Spectral display requires an axis in seconds, hertz or ppm".into(),
        )),
    }
}

pub fn view_2d(
    source: &NmrSource,
    layout: Layout2D,
    context: &mut ExecutionContext<'_>,
) -> Result<Processed2D, ExecutionError> {
    let axes = source.axes();
    if axes.len() != 2 {
        return Err(RecipeError::Invalid("2D view requires two axes".into()).into());
    }
    let direct_domain = domain(&axes[1])?;
    let cols = axes[1].points;
    let rows = source
        .dataset()
        .as_raw()
        .and_then(|raw| raw.data().sparse_traces())
        .map_or(axes[0].points, |traces| traces.len());
    super::check_view_size(rows, cols)?;
    let direct = AxisMeta {
        nucleus: axes[1].nucleus.clone().unwrap_or_default(),
        observe_freq_mhz: axes[1].observe_frequency_mhz(),
        unit: axes[1].unit,
    };
    let ppm = axes[1].coordinate_values()?;
    let mut traces = Vec::new();
    let mut magnitudes = Vec::new();
    if let Some(raw) = source.dataset().as_raw() {
        let lanes = raw.descriptor().axes()[0].component_lanes();
        let rows = raw
            .data()
            .sparse_traces()
            .map_or(axes[0].points, |traces| traces.len());
        if raw.data().is_sparse() && layout == Layout2D::Ft {
            return Err(RecipeError::Invalid(
                "Reconstruct the NUS grid before displaying contours".into(),
            )
            .into());
        }
        for row in 0..rows {
            context.check_cancelled().map_err(|e| library(e.into()))?;
            let trace = if raw.data().is_sparse() {
                raw.read_observation(nmr::raw::ObservationOrdinal::new(row))
            } else {
                raw.read_trace(&[row])
            }
            .map_err(|error| IoError::NmrConversion(error.to_string()))?;
            let samples = trace.samples();
            traces.push(samples[..cols].to_vec());
            for col in 0..cols {
                if col % 4096 == 0 {
                    context
                        .check_cancelled()
                        .map_err(|error| library(error.into()))?;
                }
                let mut magnitude = 0.0_f64;
                for lane in 0..lanes {
                    magnitude = magnitude.hypot(samples[lane * cols + col].norm());
                }
                magnitudes.push(magnitude);
            }
        }
    } else {
        let data = source
            .dataset()
            .as_processed()
            .ok_or_else(|| RecipeError::Invalid("unsupported 2D representation".into()))?;
        let descriptor = data.descriptor();
        let sample = |row, col, indirect, direct| {
            data.data()
                .get(&[row, col], &[indirect, direct])
                .map_err(|error| IoError::NmrConversion(error.to_string()))
        };
        for row in 0..axes[0].points {
            context.check_cancelled().map_err(|e| library(e.into()))?;
            let mut trace = Vec::with_capacity(cols);
            for col in 0..cols {
                if col % 4096 == 0 {
                    context
                        .check_cancelled()
                        .map_err(|error| library(error.into()))?;
                }
                trace.push(Complex64::new(
                    sample(row, col, 0, 0)?,
                    if descriptor.axes()[1].component_count() == 2 {
                        sample(row, col, 0, 1)?
                    } else {
                        0.0
                    },
                ));
                // A display reduction of every Cartesian field. The canonical
                // dataset and any requested magnitude operation stay in nmr.
                let mut magnitude = 0.0_f64;
                for indirect in 0..descriptor.axes()[0].component_count() {
                    for direct in 0..descriptor.axes()[1].component_count() {
                        magnitude = magnitude.hypot(sample(row, col, indirect, direct)?);
                    }
                }
                magnitudes.push(magnitude);
            }
            traces.push(trace);
        }
    }
    let source_label = source.source().to_owned();
    Ok(match layout {
        Layout2D::Stack => Processed2D::Stack(Arc::new(StackSpectrum {
            ppm,
            direct_domain,
            traces,
            direct,
            source: source_label,
        })),
        Layout2D::Ft => Processed2D::Ft(Arc::new(Spectrum2D {
            f2_ppm: ppm,
            f1_ppm: axes[0].coordinate_values()?,
            f2_domain: direct_domain,
            f1_domain: domain(&axes[0])?,
            data: traces.into_iter().flatten().collect(),
            magnitude_plane: Some(Arc::from(magnitudes)),
            f2_size: cols,
            f1_size: axes[0].points,
            direct,
            indirect: AxisMeta {
                nucleus: axes[0].nucleus.clone().unwrap_or_default(),
                observe_freq_mhz: axes[0].observe_frequency_mhz(),
                unit: axes[0].unit,
            },
            source: source_label,
        })),
    })
}
