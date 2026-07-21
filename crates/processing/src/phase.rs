use crate::Spectrum;
use num_complex::Complex64;

pub fn apply(spec: &mut Spectrum, phase0: f64, phase1: f64) {
    apply_with_pivot(spec, phase0, phase1, 0.0);
}

/// Fractional index (`0..=1`) of the largest-magnitude point — a sensible default
/// first-order phase pivot, so the ramp rotates about the tallest peak. Returns
/// `0.0` for an empty or single-point buffer.
pub fn peak_pivot_frac(values: &[Complex64]) -> f64 {
    let n = values.len();
    if n < 2 {
        return 0.0;
    }
    let peak = values
        .iter()
        .enumerate()
        .max_by(|(_, a), (_, b)| a.norm().total_cmp(&b.norm()))
        .map_or(0, |(i, _)| i);
    peak as f64 / (n - 1) as f64
}

/// Zeroth- and first-order phase correction in place. The first-order ramp
/// rotates around `pivot_frac` (a `0..=1` fractional index): the phase there is
/// exactly `phase0`. `pivot_frac = 0.0` ramps from the first point.
pub fn apply_with_pivot(spec: &mut Spectrum, phase0: f64, phase1: f64, pivot_frac: f64) {
    apply_slice(&mut spec.values, phase0, phase1, pivot_frac);
}

/// The phase-correction kernel over a raw complex buffer, shared by the 1D
/// [`Spectrum`] path and each dimension of a 2D spectrum. Rotates point `i` by
/// `e^{-iφ}`, `φ = phase0 + phase1·(i/(n−1) − pivot_frac)`.
pub fn apply_slice(buf: &mut [Complex64], phase0: f64, phase1: f64, pivot_frac: f64) {
    let n = buf.len();
    if n == 0 {
        return;
    }
    let denom = (n - 1).max(1) as f64;
    for (i, c) in buf.iter_mut().enumerate() {
        let frac = i as f64 / denom;
        let phi = phase0 + phase1 * (frac - pivot_frac);
        *c *= Complex64::from_polar(1.0, -phi);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spectrum_from(values: Vec<Complex64>) -> Spectrum {
        let n = values.len();
        Spectrum {
            ppm: (0..n).map(|i| i as f64).collect(),
            values,
            hz_per_point: 1.0,
            observe_freq_mhz: 400.0,
            nucleus: "1H".into(),
        }
    }

    #[test]
    fn zero_phase_is_identity() {
        let mut s = spectrum_from(vec![Complex64::new(1.0, 2.0), Complex64::new(-3.0, 0.5)]);
        let before = s.values.clone();
        apply(&mut s, 0.0, 0.0);
        for (a, b) in before.iter().zip(&s.values) {
            assert!((a - b).norm() < 1e-12);
        }
    }

    #[test]
    fn peak_pivot_frac_lands_on_tallest_point() {
        let vals = vec![
            Complex64::new(0.1, 0.0),
            Complex64::new(0.2, 0.0),
            Complex64::new(5.0, 0.0),
            Complex64::new(0.3, 0.0),
            Complex64::new(0.1, 0.0),
        ];
        assert!((peak_pivot_frac(&vals) - 0.5).abs() < 1e-12);
        assert_eq!(peak_pivot_frac(&[]), 0.0);
        assert_eq!(peak_pivot_frac(&[Complex64::new(9.0, 0.0)]), 0.0);
    }

    #[test]
    fn phase0_rotates_imag_into_real() {
        // (0 + i)·e^(-iπ/2) = 1.
        let mut s = spectrum_from(vec![Complex64::new(0.0, 1.0)]);
        apply(&mut s, std::f64::consts::FRAC_PI_2, 0.0);
        assert!((s.values[0].re - 1.0).abs() < 1e-12);
        assert!(s.values[0].im.abs() < 1e-12);
    }

    #[test]
    fn repivot_preserves_the_phase_curve() {
        let mut params = crate::PhaseParams {
            phase0: 0.7,
            phase1: -1.4,
            pivot_frac: 0.2,
            auto: None,
        };
        let before = [0.0, 0.25, 0.8, 1.0]
            .map(|frac| params.phase0 + params.phase1 * (frac - params.pivot_frac));

        params.repivot(0.75);

        let after = [0.0, 0.25, 0.8, 1.0]
            .map(|frac| params.phase0 + params.phase1 * (frac - params.pivot_frac));
        for (before, after) in before.into_iter().zip(after) {
            assert!((before - after).abs() < 1e-12);
        }
    }
}
