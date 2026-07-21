//! Deterministic, black-box quality contracts for automatic spectrum correction.
//!
//! These tests intentionally assert signal-level outcomes rather than recovered
//! parameters. A different optimizer or baseline solver is therefore free to
//! replace the current implementation as long as the user-visible quality is
//! preserved.

use num_complex::Complex64;
use plotx_processing::{AutoPhaseMethod, BaselineMethod, Spectrum, auto_phase, baseline, phase};

const PHASE_POINTS: usize = 512;
const BASELINE_POINTS: usize = 640;

fn spectrum(values: Vec<Complex64>) -> Spectrum {
    let n = values.len();
    Spectrum {
        ppm: (0..n).map(|i| i as f64).collect(),
        values,
        hz_per_point: 1.0,
        observe_freq_mhz: 400.0,
        nucleus: "1H".into(),
    }
}

fn deterministic_complex_noise(i: usize, amplitude: f64) -> Complex64 {
    let x = i as f64;
    Complex64::new(
        amplitude * ((0.731 * x).sin() + 0.37 * (0.193 * x).cos()),
        amplitude * (0.61 * (0.417 * x).cos() - 0.29 * (0.113 * x).sin()),
    )
}

/// A correctly phased spectrum containing a resolved line and a partially
/// overlapping pair. Each line has absorptive real and dispersive imaginary
/// components, matching the quadrature structure of a frequency-domain NMR
/// resonance rather than using an unrealistically real-only test signal.
fn ideal_phase_spectrum(scale: f64) -> Vec<Complex64> {
    let peaks = [
        (0.24, 0.010, 0.72),
        (0.455, 0.012, 1.00),
        (0.477, 0.017, 0.64),
        (0.78, 0.014, 0.48),
    ];
    (0..PHASE_POINTS)
        .map(|i| {
            let frac = i as f64 / (PHASE_POINTS - 1) as f64;
            let mut value = Complex64::new(0.0, 0.0);
            for &(center, width, height) in &peaks {
                let d = (frac - center) / width;
                value += Complex64::new(1.0 / (1.0 + d * d), -d / (1.0 + d * d)) * height;
            }
            (value + deterministic_complex_noise(i, 0.0015)) * scale
        })
        .collect()
}

fn inject_phase(values: &[Complex64], phase0: f64, phase1: f64) -> Vec<Complex64> {
    let denom = (values.len() - 1).max(1) as f64;
    values
        .iter()
        .enumerate()
        .map(|(i, value)| {
            let frac = i as f64 / denom;
            value * Complex64::from_polar(1.0, phase0 + phase1 * frac)
        })
        .collect()
}

#[derive(Clone, Copy, Debug)]
struct PhaseQuality {
    negative_energy_fraction: f64,
    complex_nrms_error: f64,
    imaginary_energy_fraction_error: f64,
    peak_height_ratio: f64,
    signed_area_ratio: f64,
}

fn energy(values: &[Complex64]) -> f64 {
    values.iter().map(Complex64::norm_sqr).sum()
}

fn imaginary_energy_fraction(values: &[Complex64]) -> f64 {
    let total = energy(values);
    values.iter().map(|value| value.im * value.im).sum::<f64>() / total
}

fn assess_phase_quality(corrected: &[Complex64], reference: &[Complex64]) -> PhaseQuality {
    let real_energy: f64 = corrected.iter().map(|value| value.re * value.re).sum();
    let negative_energy: f64 = corrected
        .iter()
        .filter(|value| value.re < 0.0)
        .map(|value| value.re * value.re)
        .sum();
    let error_energy: f64 = corrected
        .iter()
        .zip(reference)
        .map(|(actual, expected)| (*actual - *expected).norm_sqr())
        .sum();
    let corrected_peak = corrected
        .iter()
        .map(|value| value.re)
        .fold(f64::NEG_INFINITY, f64::max);
    let reference_peak = reference
        .iter()
        .map(|value| value.re)
        .fold(f64::NEG_INFINITY, f64::max);
    let corrected_area: f64 = corrected.iter().map(|value| value.re).sum();
    let reference_area: f64 = reference.iter().map(|value| value.re).sum();
    PhaseQuality {
        negative_energy_fraction: negative_energy / real_energy,
        complex_nrms_error: (error_energy / energy(reference)).sqrt(),
        imaginary_energy_fraction_error: (imaginary_energy_fraction(corrected)
            - imaginary_energy_fraction(reference))
        .abs(),
        peak_height_ratio: corrected_peak / reference_peak,
        signed_area_ratio: corrected_area / reference_area,
    }
}

fn robust_phase_quality(phase0: f64, phase1: f64, scale: f64) -> PhaseQuality {
    let reference = ideal_phase_spectrum(scale);
    let mut observed = spectrum(inject_phase(&reference, phase0, phase1));
    let correction = auto_phase(&observed, AutoPhaseMethod::RobustConsensus);
    phase::apply_with_pivot(&mut observed, correction.0, correction.1, correction.2);
    assess_phase_quality(&observed.values, &reference)
}

fn assert_phase_quality(label: &str, quality: PhaseQuality) {
    assert!(
        quality.negative_energy_fraction < 0.05,
        "{label}: negative-energy fraction is {}",
        quality.negative_energy_fraction
    );
    assert!(
        quality.complex_nrms_error < 0.32,
        "{label}: complex normalized RMS error is {}",
        quality.complex_nrms_error
    );
    assert!(
        quality.imaginary_energy_fraction_error < 0.08,
        "{label}: imaginary-energy fraction differs by {}",
        quality.imaginary_energy_fraction_error
    );
    assert!(
        (0.80..=1.10).contains(&quality.peak_height_ratio),
        "{label}: peak-height retention is {}",
        quality.peak_height_ratio
    );
    assert!(
        (0.75..=1.20).contains(&quality.signed_area_ratio),
        "{label}: signed-area retention is {}",
        quality.signed_area_ratio
    );
}

#[test]
fn robust_consensus_corrects_phase0_phase1_overlap_and_noise() {
    let cases = [
        ("zero-order", 0.85, 0.0),
        ("positive-ramp", -0.65, 0.90),
        ("negative-ramp", 1.05, -1.15),
    ];
    for (label, phase0, phase1) in cases {
        assert_phase_quality(label, robust_phase_quality(phase0, phase1, 1.0));
    }
}

#[test]
fn robust_consensus_quality_is_stable_across_intensity_scales() {
    let low = robust_phase_quality(-0.72, 1.05, 1.0e-5);
    let nominal = robust_phase_quality(-0.72, 1.05, 1.0);
    let high = robust_phase_quality(-0.72, 1.05, 1.0e5);
    for (label, quality) in [("low", low), ("nominal", nominal), ("high", high)] {
        assert_phase_quality(label, quality);
    }

    let normalized_metrics = |quality: PhaseQuality| {
        [
            quality.negative_energy_fraction,
            quality.complex_nrms_error,
            quality.imaginary_energy_fraction_error,
            quality.peak_height_ratio,
            quality.signed_area_ratio,
        ]
    };
    let nominal = normalized_metrics(nominal);
    for (label, scaled) in [("low", low), ("high", high)] {
        for (index, (actual, expected)) in normalized_metrics(scaled)
            .into_iter()
            .zip(nominal)
            .enumerate()
        {
            assert!(
                (actual - expected).abs() < 0.01,
                "{label}: normalized metric {index} changed from {expected} to {actual}"
            );
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum BaselineShape {
    Linear,
    Quadratic,
    SlowlyVarying,
}

fn baseline_value(shape: BaselineShape, x: f64) -> f64 {
    match shape {
        BaselineShape::Linear => 0.16 + 0.075 * x,
        BaselineShape::Quadratic => 0.13 + 0.035 * x + 0.085 * x * x,
        BaselineShape::SlowlyVarying => {
            0.17 + 0.045 * (1.35 * std::f64::consts::PI * (x + 0.17)).sin()
        }
    }
}

fn gaussian(i: usize, center: usize, width: f64, height: f64) -> f64 {
    let distance = (i as f64 - center as f64) / width;
    height * (-0.5 * distance * distance).exp()
}

fn supported_peak_signal(i: usize) -> f64 {
    gaussian(i, 175, 5.0, 1.0) + gaussian(i, 405, 22.0, 0.62)
}

#[derive(Clone, Copy, Debug)]
struct BaselineQuality {
    normalized_baseline_rmse: f64,
    narrow_height_ratio: f64,
    broad_height_ratio: f64,
    narrow_area_ratio: f64,
    broad_area_ratio: f64,
}

fn area(values: &[f64], center: usize, radius: usize) -> f64 {
    let start = center.saturating_sub(radius);
    let end = (center + radius + 1).min(values.len());
    values[start..end].iter().sum()
}

fn asls_quality(shape: BaselineShape, scale: f64) -> BaselineQuality {
    let mut known_baseline = Vec::with_capacity(BASELINE_POINTS);
    let mut known_signal = Vec::with_capacity(BASELINE_POINTS);
    let mut values = Vec::with_capacity(BASELINE_POINTS);
    for i in 0..BASELINE_POINTS {
        let x = 2.0 * i as f64 / (BASELINE_POINTS - 1) as f64 - 1.0;
        let base = baseline_value(shape, x) * scale;
        let signal = supported_peak_signal(i) * scale;
        let noise = deterministic_complex_noise(i, 0.0015 * scale);
        known_baseline.push(base);
        known_signal.push(signal);
        values.push(Complex64::new(base + signal + noise.re, noise.im));
    }
    let observed = values.clone();
    let mut corrected = spectrum(values);
    baseline::apply(&mut corrected, BaselineMethod::AUTO);

    // Baseline correction is a real-channel operation. Treating the imaginary
    // channel as immutable is part of its public signal-preservation contract.
    for (before, after) in observed.iter().zip(&corrected.values) {
        assert_eq!(before.im, after.im);
    }

    let estimated_baseline: Vec<f64> = observed
        .iter()
        .zip(&corrected.values)
        .map(|(before, after)| before.re - after.re)
        .collect();
    let baseline_error_rms = (estimated_baseline
        .iter()
        .zip(&known_baseline)
        .map(|(actual, expected)| (actual - expected).powi(2))
        .sum::<f64>()
        / BASELINE_POINTS as f64)
        .sqrt();
    let baseline_rms = (known_baseline
        .iter()
        .map(|value| value * value)
        .sum::<f64>()
        / BASELINE_POINTS as f64)
        .sqrt();
    let corrected_real: Vec<f64> = corrected.values.iter().map(|value| value.re).collect();

    BaselineQuality {
        normalized_baseline_rmse: baseline_error_rms / baseline_rms,
        narrow_height_ratio: corrected_real[175] / known_signal[175],
        broad_height_ratio: corrected_real[405] / known_signal[405],
        narrow_area_ratio: area(&corrected_real, 175, 20) / area(&known_signal, 175, 20),
        broad_area_ratio: area(&corrected_real, 405, 88) / area(&known_signal, 405, 88),
    }
}

fn assert_baseline_quality(label: &str, quality: BaselineQuality) {
    assert!(
        quality.normalized_baseline_rmse < 0.12,
        "{label}: normalized baseline RMSE is {}",
        quality.normalized_baseline_rmse
    );
    assert!(
        (0.88..=1.08).contains(&quality.narrow_height_ratio),
        "{label}: narrow-peak height retention is {}",
        quality.narrow_height_ratio
    );
    assert!(
        (0.78..=1.10).contains(&quality.broad_height_ratio),
        "{label}: broad-peak height retention is {}",
        quality.broad_height_ratio
    );
    assert!(
        (0.86..=1.12).contains(&quality.narrow_area_ratio),
        "{label}: narrow-peak area retention is {}",
        quality.narrow_area_ratio
    );
    assert!(
        (0.65..=1.15).contains(&quality.broad_area_ratio),
        "{label}: broad-peak area retention is {}",
        quality.broad_area_ratio
    );
}

#[test]
fn asls_recovers_supported_baselines_and_preserves_narrow_and_broad_peaks() {
    for shape in [
        BaselineShape::Linear,
        BaselineShape::Quadratic,
        BaselineShape::SlowlyVarying,
    ] {
        assert_baseline_quality(&format!("{shape:?}"), asls_quality(shape, 1.0));
    }
}

#[test]
fn asls_quality_is_stable_across_intensity_scales() {
    let low = asls_quality(BaselineShape::Quadratic, 1.0e-4);
    let nominal = asls_quality(BaselineShape::Quadratic, 1.0);
    let high = asls_quality(BaselineShape::Quadratic, 1.0e4);
    for (label, quality) in [("low", low), ("nominal", nominal), ("high", high)] {
        assert_baseline_quality(label, quality);
    }

    let normalized_metrics = |quality: BaselineQuality| {
        [
            quality.normalized_baseline_rmse,
            quality.narrow_height_ratio,
            quality.broad_height_ratio,
            quality.narrow_area_ratio,
            quality.broad_area_ratio,
        ]
    };
    let nominal = normalized_metrics(nominal);
    for (label, scaled) in [("low", low), ("high", high)] {
        for (index, (actual, expected)) in normalized_metrics(scaled)
            .into_iter()
            .zip(nominal)
            .enumerate()
        {
            assert!(
                (actual - expected).abs() < 0.01,
                "{label}: normalized metric {index} changed from {expected} to {actual}"
            );
        }
    }
}

#[test]
fn asls_non_target_peak_shapes_remain_numerically_safe() {
    // AsLS with a small upper-envelope weight assumes peaks are positive and
    // appreciably narrower than the baseline. Negative peaks can anchor the fit,
    // while a peak spanning a substantial fraction of the spectrum is
    // indistinguishable from baseline curvature. Fidelity for either case is an
    // explicit non-goal of the automatic preset until polarity-aware or
    // peak-masked estimation is introduced. The boundary contract here is only
    // numerical safety and preservation of the untouched imaginary channel.
    for signal in [
        (0..BASELINE_POINTS)
            .map(|i| -gaussian(i, 300, 11.0, 0.8))
            .collect::<Vec<_>>(),
        (0..BASELINE_POINTS)
            .map(|i| gaussian(i, 320, BASELINE_POINTS as f64 / 5.0, 0.8))
            .collect::<Vec<_>>(),
    ] {
        let values: Vec<Complex64> = signal
            .iter()
            .enumerate()
            .map(|(i, peak)| {
                let x = 2.0 * i as f64 / (BASELINE_POINTS - 1) as f64 - 1.0;
                Complex64::new(
                    baseline_value(BaselineShape::Quadratic, x) + peak,
                    0.01 * (0.31 * i as f64).sin(),
                )
            })
            .collect();
        let imaginary_before: Vec<f64> = values.iter().map(|value| value.im).collect();
        let input_abs_max = values
            .iter()
            .map(|value| value.re.abs())
            .fold(0.0_f64, f64::max);
        let mut corrected = spectrum(values);
        baseline::apply(&mut corrected, BaselineMethod::AUTO);

        assert!(corrected.values.iter().all(|value| value.re.is_finite()));
        for (value, expected_imaginary) in corrected.values.iter().zip(&imaginary_before) {
            assert_eq!(value.im, *expected_imaginary);
        }
        let output_abs_max = corrected
            .values
            .iter()
            .map(|value| value.re.abs())
            .fold(0.0_f64, f64::max);
        assert!(
            output_abs_max / input_abs_max < 4.0,
            "unsupported peak shape was numerically amplified by {}x",
            output_abs_max / input_abs_max
        );
    }
}
