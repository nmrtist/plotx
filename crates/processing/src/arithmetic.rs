//! Spectrum arithmetic executes in nmr and retains both parents for replay.
use crate::Spectrum;
use nmr::processing::{
    LinearCombination, ProcessingOperation, ProcessingOptions, ProcessingPlan, SpectrumOperation,
};
use plotx_io::nmr_view::NmrSource;
use std::sync::Arc;

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

#[derive(Debug, thiserror::Error)]
pub enum ArithmeticError {
    #[error(transparent)]
    Library(#[from] nmr::processing::ProcessingError),
    #[error(transparent)]
    View(#[from] plotx_io::IoError),
}

pub fn same_grid(a: &Spectrum, b: &Spectrum) -> bool {
    a.unit == b.unit
        && a.ppm.len() == b.ppm.len()
        && a.ppm
            .iter()
            .zip(&b.ppm)
            .all(|(x, y)| (x - y).abs() <= 1e-9 * x.abs().max(y.abs()).max(1.0))
}

pub fn validate_combination(a: &NmrSource, b: &NmrSource) -> Result<(), ArithmeticError> {
    LinearCombination::new(1.0)?.prepare(a.dataset(), b.dataset(), ProcessingOptions::default())?;
    Ok(())
}

pub fn combine_spectra(
    a: &NmrSource,
    b: &NmrSource,
    op: SpectrumBinaryOp,
    k: f64,
) -> Result<NmrSource, ArithmeticError> {
    let scale = match op {
        SpectrumBinaryOp::Add => k,
        SpectrumBinaryOp::Subtract => -k,
    };
    let output = LinearCombination::new(scale)?
        .prepare(a.dataset(), b.dataset(), ProcessingOptions::default())?
        .execute_with_context(&mut nmr::ExecutionContext::default())?;
    Ok(NmrSource::new(Arc::new(output))?)
}

pub fn scale_offset_spectrum(
    a: &NmrSource,
    scale: f64,
    offset: f64,
) -> Result<NmrSource, ArithmeticError> {
    let output = ProcessingPlan::new(vec![ProcessingOperation::Spectrum {
        axis: 0,
        operation: SpectrumOperation::Affine {
            scale,
            real_offset: offset,
        },
    }])?
    .apply(a.dataset())?;
    Ok(NmrSource::new(Arc::new(output))?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use num_complex::Complex64;

    fn input(spec: &Spectrum) -> Result<NmrSource, ArithmeticError> {
        use nmr::{axis::*, processed::*};
        let fail =
            |error: &dyn std::fmt::Display| plotx_io::IoError::NmrConversion(error.to_string());
        let axis = ProcessedAxis::new(
            AxisRole::Signal,
            AxisDomain::Frequency,
            Some(spec.unit),
            spec.len(),
            AxisCoordinates::Explicit(spec.ppm.clone()),
            ComponentBasis::Cartesian,
        )
        .map_err(|e| fail(&e))?
        .with_nucleus(Some(spec.nucleus.clone()))
        .map_err(|e| fail(&e))?;
        let data = ProcessedDataset::from_complex_trace(
            axis,
            spec.values.clone(),
            ProcessedProvenance::new(ProcessedOrigin::Unknown, vec![]).map_err(|e| fail(&e))?,
        )
        .map_err(|e| fail(&e))?;
        Ok(NmrSource::new(Arc::new(data.into()))?)
    }
    fn view(source: NmrSource) -> Spectrum {
        crate::nmr_execution::view_1d(&source)
            .unwrap()
            .as_frequency()
            .unwrap()
            .clone()
    }
    fn combine_spectra(
        a: &Spectrum,
        b: &Spectrum,
        op: SpectrumBinaryOp,
        k: f64,
    ) -> Result<Spectrum, ArithmeticError> {
        super::combine_spectra(&input(a)?, &input(b)?, op, k).map(view)
    }
    fn scale_offset_spectrum(a: &Spectrum, scale: f64, offset: f64) -> Spectrum {
        view(super::scale_offset_spectrum(&input(a).unwrap(), scale, offset).unwrap())
    }

    fn spec(ppm: Vec<f64>, re: Vec<f64>, nucleus: &str) -> Spectrum {
        let values = re.into_iter().map(|r| Complex64::new(r, 0.0)).collect();
        Spectrum {
            ppm,
            values,
            unit: nmr::axis::AxisUnit::Ppm,
            hz_per_point: Some(1.0),
            observe_freq_mhz: Some(400.0),
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
        assert!(err.to_string().contains("incompatible"));
    }

    #[test]
    fn empty_operand_is_rejected() {
        let a = spec(vec![], vec![], "1H");
        let b = spec(vec![0.0], vec![1.0], "1H");
        assert!(combine_spectra(&a, &b, SpectrumBinaryOp::Add, 1.0).is_err());
    }

    #[test]
    fn scale_offset_produces_expected_trace() {
        let a = spec(vec![0.0, 1.0], vec![2.0, -3.0], "1H");
        let out = scale_offset_spectrum(&a, 2.0, 1.0);
        assert_eq!(out.real(), vec![5.0, -5.0]);
        assert_eq!(out.ppm, a.ppm);
    }
}
