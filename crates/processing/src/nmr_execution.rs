//! Application execution retains the full library output beside disposable views.

use crate::nmr_bridge::{self, DelayPolicy, PhaseReport, RecipeError, RecipeRange};
use crate::{AxisPipeline, Processed1D, Spectrum, TimeTrace};
use nmr::axis::{AxisDomain, AxisUnit};
use nmr::{ExecutionContext, processing::ProcessingOptions};
use plotx_io::nmr_view::NmrSource;

#[path = "nmr_execution_2d.rs"]
mod twod;
pub use twod::{NusRequest, Output2D, execute_2d, validate_2d_domains, view_2d};

/// One bounded ledger for the complete F2/NUS/F1 task and its frequency suffix.
/// A 512 x 1024 NUS acquisition needs over 40 billion preflight work units;
/// the library default is too small even though its memory use is modest.
pub fn processing_2d_work_ledger() -> nmr::resource::WorkLedger {
    nmr::resource::WorkLedger::new(100_000_000_000)
}

#[derive(Debug, thiserror::Error)]
pub enum ExecutionError {
    #[error(transparent)]
    Recipe(#[from] RecipeError),
    #[error(transparent)]
    View(#[from] plotx_io::IoError),
}

impl ExecutionError {
    pub fn is_cancelled(&self) -> bool {
        matches!(self, Self::Recipe(error) if error.is_cancelled())
    }
}

#[derive(Clone, Debug)]
pub struct Output1D {
    pub source: NmrSource,
    pub view: Processed1D,
    pub phases: Vec<PhaseReport>,
}

pub fn execute_1d(
    source: &NmrSource,
    pipeline: &AxisPipeline,
    delay: DelayPolicy,
    range: RecipeRange,
    context: &mut ExecutionContext<'_>,
) -> Result<Output1D, ExecutionError> {
    let result = nmr_bridge::compile(source.dataset().clone(), pipeline, 0, delay, range)?
        .execute_with_report(ProcessingOptions::default(), context)?;
    let source = NmrSource::new(result.dataset)?.with_display_label(source.source().to_owned());
    let view = view_1d(&source)?;
    Ok(Output1D {
        source,
        view,
        phases: result.phases,
    })
}

pub fn view_1d(source: &NmrSource) -> Result<Processed1D, plotx_io::IoError> {
    let axis = source.direct_axis()?;
    check_view_size(1, axis.points)?;
    let coordinates = axis.coordinate_values()?;
    let values = source.trace()?;
    let nucleus = axis.nucleus.clone().unwrap_or_default();
    match (axis.domain, axis.unit) {
        (AxisDomain::Time, Some(AxisUnit::Second)) => Ok(Processed1D::Time(TimeTrace {
            time_s: coordinates,
            values,
            nucleus,
            source: source.source().to_owned(),
        })),
        (AxisDomain::Frequency, Some(unit @ (AxisUnit::Ppm | AxisUnit::Hertz))) => {
            Ok(Processed1D::Frequency(Spectrum {
                ppm: coordinates,
                values,
                unit,
                hz_per_point: match &axis.coordinates {
                    nmr::axis::AxisCoordinates::Uniform { step, .. } => {
                        if unit == AxisUnit::Hertz {
                            Some(step.abs())
                        } else {
                            source
                                .reference_frequency_mhz(0)
                                .map(|frequency| frequency * step.abs())
                        }
                    }
                    _ => None,
                },
                observe_freq_mhz: axis.observe_frequency_mhz(),
                nucleus,
            }))
        }
        _ => Err(plotx_io::IoError::NmrConversion(
            "NMR display requires coordinates in seconds, hertz or ppm".into(),
        )),
    }
}

/// Bound disposable host views separately from the library's scientific output.
/// This includes row vectors, axes, magnitude and the temporary flattening copy.
pub(super) fn check_view_size(rows: usize, cols: usize) -> Result<(), plotx_io::IoError> {
    let bytes = rows
        .checked_mul(cols)
        .and_then(|points| points.checked_mul(40))
        .and_then(|bytes| rows.checked_mul(32)?.checked_add(bytes))
        .and_then(|bytes| cols.checked_mul(8)?.checked_add(bytes));
    if bytes.is_none_or(|bytes| bytes > 512 * 1024 * 1024) {
        return Err(plotx_io::IoError::NmrConversion(
            "NMR display exceeds the 512 MiB view allocation limit".into(),
        ));
    }
    Ok(())
}
