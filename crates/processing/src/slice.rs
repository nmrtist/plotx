//! Pulling a 1D trace out of a 2D dataset: a single row/column cut through a
//! true-2D spectrum, one increment of a pseudo-2D stack, or a whole-axis
//! projection. Kept free of egui/figure types so the interactive slice tool and
//! a later axis-projection feature share this one extraction core.

use num_complex::Complex64;
use plotx_io::Domain;

use crate::Spectrum2D;

/// The orientation of a 1D cut through a true-2D spectrum. `Row`/`Column` name
/// the axis the resulting trace runs *along*.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SliceKind {
    /// A horizontal cut at a fixed F1 (indirect) index: intensity vs F2 (direct).
    Row,
    /// A vertical cut at a fixed F2 (direct) index: intensity vs F1 (indirect).
    Column,
}

/// How a whole-axis projection collapses the summed-over dimension.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectionMode {
    /// Sum of every trace (a total projection).
    Sum,
    /// Per-point value of greatest magnitude across traces (a skyline projection).
    Skyline,
}

/// A 1D trace lifted out of a 2D dataset, retaining the scientific domain of
/// both its surviving coordinate and (for a cut) its fixed-axis position.
#[derive(Debug, Clone)]
pub struct Slice1D {
    pub coordinates: Vec<f64>,
    pub domain: Domain,
    pub values: Vec<Complex64>,
    pub nucleus: String,
    pub observe_freq_mhz: Option<f64>,
    pub reference_freq_mhz: Option<f64>,
    pub unit: nmr::axis::AxisUnit,
    /// The fixed-axis coordinate the cut was taken at, for labelling.
    /// `None` for a projection, which spans the whole axis.
    pub position: Option<f64>,
    pub position_domain: Domain,
}

impl Spectrum2D {
    /// Nearest F2 (direct) grid index to a ppm coordinate.
    pub fn nearest_f2(&self, ppm: f64) -> usize {
        nearest(&self.f2_ppm, ppm)
    }

    /// Nearest F1 (indirect) grid index to a ppm coordinate.
    pub fn nearest_f1(&self, ppm: f64) -> usize {
        nearest(&self.f1_ppm, ppm)
    }
}

fn nearest(axis: &[f64], ppm: f64) -> usize {
    axis.iter()
        .enumerate()
        .min_by(|(_, a), (_, b)| (**a - ppm).abs().total_cmp(&(**b - ppm).abs()))
        .map(|(i, _)| i)
        .unwrap_or(0)
}

/// A native reduction removes the selected dimension and its explicitly chosen
/// real component. The surviving axis retains all of its Cartesian components.
#[derive(Clone, Copy, Debug)]
pub enum Reduction {
    Slice(usize),
    Projection(ProjectionMode),
}

pub fn extract(
    source: &plotx_io::nmr_view::NmrSource,
    kind: SliceKind,
    reduction: Reduction,
) -> Result<(plotx_io::nmr_view::NmrSource, Slice1D), String> {
    use crate::Processed1D;
    use nmr::processing::{
        ProcessingOperation, ProcessingOptions, ProcessingPlan, SpectrumOperation,
    };
    let axis = match kind {
        SliceKind::Row => 0,
        SliceKind::Column => 1,
    };
    let axes = source.axes();
    if axes.len() != 2 {
        return Err("Slice extraction requires two axes".into());
    }
    let operation = match reduction {
        Reduction::Slice(index) => SpectrumOperation::Slice {
            index,
            component: 0,
        },
        Reduction::Projection(ProjectionMode::Sum) => SpectrumOperation::Sum { component: 0 },
        Reduction::Projection(ProjectionMode::Skyline) => {
            SpectrumOperation::Skyline { component: 0 }
        }
    };
    let output = ProcessingPlan::new(vec![ProcessingOperation::Spectrum { axis, operation }])
        .and_then(|plan| {
            plan.apply_with_context(
                source.dataset(),
                ProcessingOptions::default(),
                &mut nmr::ExecutionContext::default(),
            )
        })
        .map_err(|error| error.to_string())?;
    let output = plotx_io::nmr_view::NmrSource::new(std::sync::Arc::new(output))
        .map_err(|error| error.to_string())?;
    let view = crate::nmr_execution::view_1d(&output).map_err(|error| error.to_string())?;
    let (coordinates, domain, values) = match view {
        Processed1D::Frequency(s) => (s.ppm, Domain::Frequency, s.values),
        Processed1D::Time(t) => (t.time_s, Domain::Time, t.values),
    };
    let surviving = &output.axes()[0];
    let position = match reduction {
        Reduction::Slice(index) => axes[axis]
            .coordinate_values()
            .map_err(|error| error.to_string())?
            .get(index)
            .copied(),
        Reduction::Projection(_) => None,
    };
    let slice = Slice1D {
        coordinates,
        domain,
        values,
        reference_freq_mhz: output.reference_frequency_mhz(0),
        nucleus: surviving.nucleus.clone().unwrap_or_default(),
        observe_freq_mhz: surviving.observe_frequency_mhz(),
        unit: surviving
            .unit
            .ok_or_else(|| "Slice has no spectral unit".to_owned())?,
        position,
        position_domain: if axes[axis].domain == nmr::axis::AxisDomain::Time {
            Domain::Time
        } else {
            Domain::Frequency
        },
    };
    Ok((output, slice))
}
