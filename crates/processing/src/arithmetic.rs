//! Spectrum arithmetic: dataset ± dataset (with a scale on the second operand)
//! and constant scale/offset, producing a new standalone frequency-domain trace.

use crate::Spectrum;
use num_complex::Complex64;
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpectrumBinaryOp {
    Add,
    Subtract,
}

impl SpectrumBinaryOp {
    pub const fn symbol(self) -> &'static str {
        match self {
            Self::Add => "+",
            Self::Subtract => "−",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArithmeticError {
    NucleusMismatch { a: String, b: String },
    EmptyOperand,
}

impl fmt::Display for ArithmeticError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NucleusMismatch { a, b } => {
                write!(
                    formatter,
                    "Nuclei differ ({a} vs {b}); pick two spectra of the same nucleus."
                )
            }
            Self::EmptyOperand => formatter.write_str("Both spectra need at least one point."),
        }
    }
}

impl std::error::Error for ArithmeticError {}

pub fn same_grid(a: &Spectrum, b: &Spectrum) -> bool {
    a.ppm.len() == b.ppm.len()
        && a.ppm
            .iter()
            .zip(&b.ppm)
            .all(|(x, y)| (x - y).abs() <= 1e-9 * x.abs().max(y.abs()).max(1.0))
}

/// `a op k·b` on `a`'s axis. `b` is linearly interpolated onto `a`'s grid;
/// points of `a` outside `b`'s range use `b = 0`.
pub fn combine_spectra(
    a: &Spectrum,
    b: &Spectrum,
    op: SpectrumBinaryOp,
    k: f64,
) -> Result<Spectrum, ArithmeticError> {
    if a.is_empty() || b.is_empty() {
        return Err(ArithmeticError::EmptyOperand);
    }
    if a.nucleus.trim() != b.nucleus.trim() {
        return Err(ArithmeticError::NucleusMismatch {
            a: a.nucleus.clone(),
            b: b.nucleus.clone(),
        });
    }
    let scale = match op {
        SpectrumBinaryOp::Add => k,
        SpectrumBinaryOp::Subtract => -k,
    };
    let b_on_a = if same_grid(a, b) {
        b.values.clone()
    } else {
        resample_linear(&b.ppm, &b.values, &a.ppm)
    };
    let mut out = a.clone();
    for (v, w) in out.values.iter_mut().zip(&b_on_a) {
        *v += scale * w;
    }
    Ok(out)
}

/// `scale·a + offset` (the offset raises the real channel only).
pub fn scale_offset_spectrum(a: &Spectrum, scale: f64, offset: f64) -> Spectrum {
    let mut out = a.clone();
    for v in &mut out.values {
        *v = scale * *v + Complex64::new(offset, 0.0);
    }
    out
}

fn resample_linear(src_ppm: &[f64], src: &[Complex64], dst_ppm: &[f64]) -> Vec<Complex64> {
    let ascending = src_ppm.first() <= src_ppm.last();
    let (axis, values): (Vec<f64>, Vec<Complex64>) = if ascending {
        (src_ppm.to_vec(), src.to_vec())
    } else {
        (
            src_ppm.iter().rev().copied().collect(),
            src.iter().rev().copied().collect(),
        )
    };
    dst_ppm
        .iter()
        .map(|&x| {
            let (lo, hi) = (axis[0], axis[axis.len() - 1]);
            if x < lo || x > hi {
                return Complex64::new(0.0, 0.0);
            }
            let j = axis.partition_point(|&p| p < x);
            if j == 0 {
                return values[0];
            }
            if j >= axis.len() {
                return values[values.len() - 1];
            }
            let (x0, x1) = (axis[j - 1], axis[j]);
            let span = x1 - x0;
            if span <= 0.0 {
                return values[j];
            }
            let t = (x - x0) / span;
            values[j - 1] * (1.0 - t) + values[j] * t
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(ppm: Vec<f64>, re: Vec<f64>, nucleus: &str) -> Spectrum {
        let values = re.into_iter().map(|r| Complex64::new(r, 0.0)).collect();
        Spectrum {
            ppm,
            values,
            hz_per_point: 1.0,
            observe_freq_mhz: 400.0,
            nucleus: nucleus.into(),
        }
    }

    #[test]
    fn add_and_subtract_on_the_same_grid() {
        let a = spec(vec![0.0, 1.0, 2.0], vec![10.0, 20.0, 30.0], "1H");
        let b = spec(vec![0.0, 1.0, 2.0], vec![1.0, 2.0, 3.0], "1H");
        let sum = combine_spectra(&a, &b, SpectrumBinaryOp::Add, 1.0).unwrap();
        assert_eq!(sum.real(), vec![11.0, 22.0, 33.0]);
        let diff = combine_spectra(&a, &b, SpectrumBinaryOp::Subtract, 1.0).unwrap();
        assert_eq!(diff.real(), vec![9.0, 18.0, 27.0]);
        assert_eq!(diff.ppm, a.ppm);
        assert_eq!(diff.nucleus, "1H");
    }

    #[test]
    fn scale_factor_applies_to_second_operand_only() {
        let a = spec(vec![0.0, 1.0], vec![10.0, 10.0], "1H");
        let b = spec(vec![0.0, 1.0], vec![4.0, 8.0], "1H");
        let out = combine_spectra(&a, &b, SpectrumBinaryOp::Subtract, 0.5).unwrap();
        assert_eq!(out.real(), vec![8.0, 6.0]);
    }

    #[test]
    fn different_grid_is_interpolated_onto_a() {
        let a = spec(vec![0.0, 0.5, 1.0], vec![0.0, 0.0, 0.0], "1H");
        let b = spec(vec![0.0, 1.0], vec![0.0, 10.0], "1H");
        let out = combine_spectra(&a, &b, SpectrumBinaryOp::Add, 1.0).unwrap();
        let re = out.real();
        assert!((re[0] - 0.0).abs() < 1e-12);
        assert!((re[1] - 5.0).abs() < 1e-12);
        assert!((re[2] - 10.0).abs() < 1e-12);
    }

    #[test]
    fn non_overlapping_region_treats_b_as_zero() {
        let a = spec(vec![0.0, 1.0, 2.0, 3.0], vec![1.0, 1.0, 1.0, 1.0], "1H");
        let b = spec(vec![1.0, 2.0], vec![5.0, 5.0], "1H");
        let out = combine_spectra(&a, &b, SpectrumBinaryOp::Subtract, 1.0).unwrap();
        assert_eq!(out.real(), vec![1.0, -4.0, -4.0, 1.0]);
    }

    #[test]
    fn descending_source_axis_still_interpolates() {
        let b = spec(vec![1.0, 0.0], vec![10.0, 0.0], "1H");
        let a = spec(vec![0.5], vec![0.0], "1H");
        let out = combine_spectra(&a, &b, SpectrumBinaryOp::Add, 1.0).unwrap();
        assert!((out.real()[0] - 5.0).abs() < 1e-12);
    }

    #[test]
    fn nucleus_mismatch_is_rejected() {
        let a = spec(vec![0.0, 1.0], vec![1.0, 1.0], "1H");
        let b = spec(vec![0.0, 1.0], vec![1.0, 1.0], "13C");
        let err = combine_spectra(&a, &b, SpectrumBinaryOp::Add, 1.0).unwrap_err();
        assert!(matches!(err, ArithmeticError::NucleusMismatch { .. }));
    }

    #[test]
    fn empty_operand_is_rejected() {
        let a = spec(vec![], vec![], "1H");
        let b = spec(vec![0.0], vec![1.0], "1H");
        assert!(matches!(
            combine_spectra(&a, &b, SpectrumBinaryOp::Add, 1.0),
            Err(ArithmeticError::EmptyOperand)
        ));
    }

    #[test]
    fn scale_offset_produces_expected_trace() {
        let a = spec(vec![0.0, 1.0], vec![2.0, -3.0], "1H");
        let out = scale_offset_spectrum(&a, 2.0, 1.0);
        assert_eq!(out.real(), vec![5.0, -5.0]);
        assert_eq!(out.ppm, a.ppm);
    }
}
