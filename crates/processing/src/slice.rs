//! Pulling a 1D trace out of a 2D dataset: a single row/column cut through a
//! true-2D spectrum, one increment of a pseudo-2D stack, or a whole-axis
//! projection. Kept free of egui/figure types so the interactive slice tool and
//! a later axis-projection feature share this one extraction core.

use num_complex::Complex64;

use crate::{Spectrum2D, StackSpectrum};

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

/// A 1D trace lifted out of a 2D dataset: its ppm axis, complex intensities, and
/// the axis metadata needed to re-plot and analyze it as a standalone spectrum.
#[derive(Debug, Clone)]
pub struct Slice1D {
    pub ppm: Vec<f64>,
    pub values: Vec<Complex64>,
    pub nucleus: String,
    pub observe_freq_mhz: f64,
    /// The fixed-axis coordinate (ppm) the cut was taken at, for labelling.
    /// `None` for a projection, which spans the whole axis.
    pub position_ppm: Option<f64>,
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

    /// A single row/column cut at a grid index (clamped in range).
    pub fn slice(&self, kind: SliceKind, index: usize) -> Slice1D {
        match kind {
            SliceKind::Row => {
                let r = index.min(self.f1_size.saturating_sub(1));
                let start = r * self.f2_size;
                Slice1D {
                    ppm: self.f2_ppm.clone(),
                    values: self.data[start..start + self.f2_size].to_vec(),
                    nucleus: self.direct.nucleus.clone(),
                    observe_freq_mhz: self.direct.observe_freq_mhz,
                    position_ppm: self.f1_ppm.get(r).copied(),
                }
            }
            SliceKind::Column => {
                let c = index.min(self.f2_size.saturating_sub(1));
                Slice1D {
                    ppm: self.f1_ppm.clone(),
                    values: (0..self.f1_size).map(|r| self.at(r, c)).collect(),
                    nucleus: self.indirect.nucleus.clone(),
                    observe_freq_mhz: self.indirect.observe_freq_mhz,
                    position_ppm: self.f2_ppm.get(c).copied(),
                }
            }
        }
    }

    /// A whole-axis projection. `kind` names the surviving axis (as for
    /// [`Self::slice`]): a `Row` projection collapses F1 to give intensity vs F2,
    /// a `Column` projection collapses F2 to give intensity vs F1.
    pub fn project(&self, kind: SliceKind, mode: ProjectionMode) -> Slice1D {
        match kind {
            SliceKind::Row => Slice1D {
                ppm: self.f2_ppm.clone(),
                values: (0..self.f2_size)
                    .map(|c| reduce((0..self.f1_size).map(|r| self.at(r, c)), mode))
                    .collect(),
                nucleus: self.direct.nucleus.clone(),
                observe_freq_mhz: self.direct.observe_freq_mhz,
                position_ppm: None,
            },
            SliceKind::Column => Slice1D {
                ppm: self.f1_ppm.clone(),
                values: (0..self.f1_size)
                    .map(|r| reduce((0..self.f2_size).map(|c| self.at(r, c)), mode))
                    .collect(),
                nucleus: self.indirect.nucleus.clone(),
                observe_freq_mhz: self.indirect.observe_freq_mhz,
                position_ppm: None,
            },
        }
    }
}

impl StackSpectrum {
    /// One increment's direct-dimension 1D trace (clamped in range).
    pub fn slice(&self, increment: usize) -> Slice1D {
        let i = increment.min(self.increments().saturating_sub(1));
        Slice1D {
            ppm: self.ppm.clone(),
            values: self.traces.get(i).cloned().unwrap_or_default(),
            nucleus: self.direct.nucleus.clone(),
            observe_freq_mhz: self.direct.observe_freq_mhz,
            position_ppm: None,
        }
    }
}

fn nearest(axis: &[f64], ppm: f64) -> usize {
    axis.iter()
        .enumerate()
        .min_by(|(_, a), (_, b)| (**a - ppm).abs().total_cmp(&(**b - ppm).abs()))
        .map(|(i, _)| i)
        .unwrap_or(0)
}

fn reduce(values: impl Iterator<Item = Complex64>, mode: ProjectionMode) -> Complex64 {
    match mode {
        ProjectionMode::Sum => values.sum(),
        ProjectionMode::Skyline => values.fold(Complex64::new(0.0, 0.0), |best, c| {
            if c.norm() > best.norm() { c } else { best }
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AxisMeta;

    fn spectrum() -> Spectrum2D {
        // 2 rows (F1) × 3 cols (F2): row r, col c carries value (r*10 + c).
        let (f2_size, f1_size) = (3, 2);
        let data = (0..f1_size)
            .flat_map(|r| (0..f2_size).map(move |c| Complex64::new((r * 10 + c) as f64, 0.0)))
            .collect();
        Spectrum2D {
            f2_ppm: vec![1.0, 2.0, 3.0],
            f1_ppm: vec![10.0, 20.0],
            data,
            f2_size,
            f1_size,
            direct: AxisMeta {
                nucleus: "1H".into(),
                observe_freq_mhz: 400.0,
            },
            indirect: AxisMeta {
                nucleus: "13C".into(),
                observe_freq_mhz: 100.0,
            },
            source: "t".into(),
        }
    }

    #[test]
    fn row_slice_is_a_full_f2_trace_at_fixed_f1() {
        let s = spectrum();
        let row = s.slice(SliceKind::Row, 1);
        assert_eq!(row.ppm, vec![1.0, 2.0, 3.0]);
        assert_eq!(
            row.values.iter().map(|c| c.re).collect::<Vec<_>>(),
            vec![10.0, 11.0, 12.0]
        );
        assert_eq!(row.nucleus, "1H");
        assert_eq!(row.position_ppm, Some(20.0));
    }

    #[test]
    fn column_slice_is_a_full_f1_trace_at_fixed_f2() {
        let s = spectrum();
        let col = s.slice(SliceKind::Column, 2);
        assert_eq!(col.ppm, vec![10.0, 20.0]);
        assert_eq!(
            col.values.iter().map(|c| c.re).collect::<Vec<_>>(),
            vec![2.0, 12.0]
        );
        assert_eq!(col.nucleus, "13C");
        assert_eq!(col.position_ppm, Some(3.0));
    }

    #[test]
    fn sum_projection_collapses_the_other_axis() {
        let s = spectrum();
        let proj = s.project(SliceKind::Row, ProjectionMode::Sum);
        // Column c sums rows: (0+10), (1+11), (2+12).
        assert_eq!(
            proj.values.iter().map(|c| c.re).collect::<Vec<_>>(),
            vec![10.0, 12.0, 14.0]
        );
        assert_eq!(proj.position_ppm, None);
    }

    #[test]
    fn nearest_index_snaps_to_the_grid() {
        let s = spectrum();
        assert_eq!(s.nearest_f2(2.4), 1);
        assert_eq!(s.nearest_f1(18.0), 1);
    }
}
