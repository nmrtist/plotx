//! Retained scientific phase gates from src/tests.rs, executed through nmr.
use nmr::axis::{AxisCoordinates, AxisDomain, AxisRole, AxisUnit};
use nmr::processed::{
    ComponentBasis, ProcessedAxis, ProcessedDataset, ProcessedOrigin, ProcessedProvenance,
};
use nmr::processing::{PhaseMethod as AutoPhaseMethod, ProcessingOptions};
use nmr::{Complex64, Dataset, ExecutionContext};
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

/// Ground-truth auto-phase harness: build a known clean (absorptive) spectrum,
/// scramble it by a known `(phase0, phase1)`, and score how well a method's
/// correction recovers the original real part. `residual` is normalized RMS
/// against the clean spectrum, so 0 is a perfect recovery. These guard against
/// selecting a phase method on a p1=0-only benchmark, where any zero-order-only
/// method scores perfectly for the wrong reason.
mod groundtruth {
    use super::*;

    pub fn clean(n: usize, peaks: &[(f64, f64, f64)]) -> Vec<Complex64> {
        (0..n)
            .map(|i| {
                let mut c = Complex64::new(0.0, 0.0);
                for &(frac_c, h, w) in peaks {
                    let d = (i as f64 - (frac_c * (n - 1) as f64).round()) / w;
                    c += Complex64::new(h / (1.0 + d * d), h * d / (1.0 + d * d));
                }
                c
            })
            .collect()
    }

    pub fn scramble(vals: &[Complex64], a0: f64, a1: f64, noise: f64) -> Vec<Complex64> {
        let denom = (vals.len() - 1) as f64;
        vals.iter()
            .enumerate()
            .map(|(i, c)| {
                let frac = i as f64 / denom;
                let mut v = c * Complex64::from_polar(1.0, a0 + a1 * frac);
                if noise > 0.0 {
                    let h = |k: f64| (((k * 12.9898).sin() * 43758.5453).fract() - 0.5) * 2.0;
                    v += Complex64::new(noise * h(i as f64), noise * h(i as f64 + 7.0));
                }
                v
            })
            .collect()
    }

    fn residual(recovered: &[Complex64], truth: &[Complex64]) -> f64 {
        let num: f64 = recovered
            .iter()
            .zip(truth)
            .map(|(r, t)| (r.re - t.re).powi(2))
            .sum();
        let den: f64 = truth.iter().map(|t| t.re * t.re).sum();
        (num / den).sqrt()
    }

    /// Measure recovery at the original sample resolution.
    pub fn recover_n(
        n: usize,
        peaks: &[(f64, f64, f64)],
        a0: f64,
        a1: f64,
        noise: f64,
        m: AutoPhaseMethod,
    ) -> Result<(f64, f64)> {
        let truth = clean(n, peaks);
        let axis = ProcessedAxis::new(
            AxisRole::Signal,
            AxisDomain::Frequency,
            Some(AxisUnit::Ppm),
            n,
            AxisCoordinates::Uniform {
                start: 0.0,
                step: 1.0,
            },
            ComponentBasis::Cartesian,
        )?;
        let input = Dataset::from_processed(ProcessedDataset::from_complex_trace(
            axis,
            scramble(&truth, a0, a1, noise),
            ProcessedProvenance::new(ProcessedOrigin::Unknown, vec![])?,
        )?);
        let mut context = ExecutionContext::default();
        let options = ProcessingOptions::new();
        let estimate = m
            .prepare(&input, 0, options)?
            .estimate_with_context(&mut context)?;
        let p1 = -estimate.correction().p1_degrees() * (n - 1) as f64 / n as f64;
        let output = estimate.apply_with_context(&input, options, &mut context)?;
        let data = output
            .as_dense_processed()
            .ok_or("missing processed result")?;
        let values = (0..n)
            .map(|i| Ok(Complex64::new(data.get(&[i], &[0])?, data.get(&[i], &[1])?)))
            .collect::<Result<Vec<_>>>()?;
        Ok((residual(&values, &truth), p1))
    }

    pub fn one() -> Vec<(f64, f64, f64)> {
        vec![(0.5, 1.0, 4.0)]
    }
    pub fn many() -> Vec<(f64, f64, f64)> {
        vec![
            (0.15, 1.0, 4.0),
            (0.4, 0.7, 4.0),
            (0.62, 0.9, 4.0),
            (0.86, 0.5, 4.0),
        ]
    }
}

#[test]
fn retained_phase_quality_gates() {
    use groundtruth::*;
    let cases = [
        (
            "AbsorptivePeak-zero",
            1024,
            many(),
            0.3,
            0.0,
            0.0,
            AutoPhaseMethod::AbsorptivePeak,
            0.05,
        ),
        (
            "Entropy-first-order",
            1024,
            many(),
            0.3,
            3.0,
            0.0,
            AutoPhaseMethod::Entropy,
            0.15,
        ),
        (
            "Entropy-negative-ramp",
            1024,
            many(),
            -0.5,
            -4.5,
            0.0,
            AutoPhaseMethod::Entropy,
            0.2,
        ),
        (
            "Entropy-single",
            1024,
            one(),
            0.9,
            0.0,
            0.0,
            AutoPhaseMethod::Entropy,
            0.1,
        ),
        (
            "Entropy-large-narrow",
            32768,
            vec![
                (0.15, 1.0, 2.0),
                (0.4, 0.7, 2.0),
                (0.62, 0.9, 2.0),
                (0.86, 0.5, 2.0),
            ],
            0.3,
            220f64.to_radians(),
            0.0,
            AutoPhaseMethod::Entropy,
            0.2,
        ),
        (
            "Entropy-90deg",
            1024,
            many(),
            2.0,
            90f64.to_radians(),
            0.01,
            AutoPhaseMethod::Entropy,
            0.35,
        ),
        (
            "Entropy-270deg",
            1024,
            many(),
            2.0,
            270f64.to_radians(),
            0.01,
            AutoPhaseMethod::Entropy,
            0.35,
        ),
        (
            "Entropy-500deg",
            1024,
            many(),
            2.0,
            500f64.to_radians(),
            0.01,
            AutoPhaseMethod::Entropy,
            0.35,
        ),
    ];
    for (label, n, peaks, p0, p1, noise, method, limit) in cases {
        let (residual, estimated_p1) = recover_n(n, &peaks, p0, p1, noise, method)
            .unwrap_or_else(|error| panic!("{label}: {error}"));
        assert!(
            residual < limit,
            "{label}: residual={residual}, required <{limit}; endpoint p1={estimated_p1}deg"
        );
    }
}
