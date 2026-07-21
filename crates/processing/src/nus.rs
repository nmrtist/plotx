//! Non-uniform-sampling (NUS) indirect-axis reconstruction.
//!
//! JEOL echo/anti-echo HSQC (and other P/N experiments) select a single
//! coherence pathway per stored channel, so the two F1 channels are the echo
//! (P) and anti-echo (N) interferograms rather than States cosine/sine. Feeding
//! P/N straight into a `cos + i·sin` assembly places each peak at both ±Ω (the
//! F1 mirror); [`pn_to_shr`] first remaps them to States-Haberkorn-Ruben
//! channels so the assembly resolves a single frequency.
//!
//! Only a subset (M of N nominal) increments are acquired; [`ist`] fills the
//! gaps by iterative soft thresholding: transform, shrink all but the strongest
//! F1 components, inverse-transform, and re-impose the measured samples, so the
//! reconstruction stays consistent with the acquired data while favouring a
//! sparse (peak-like) spectrum.

use num_complex::Complex64;
use rustfft::FftPlanner;

/// Convert an echo/anti-echo channel pair `(P, N)` at one F1 increment and F2
/// point into the complex States t1 sample. `cos = (P + N)/2`, `sin = (P − N)/2i`,
/// and the hypercomplex assembly `cos + i·sin` collapses to a single-frequency
/// sample (no ±Ω mirror). Sign conventions vary between spectrometers; the
/// caller applies the indirect conjugation that fixes the F1 sense.
#[inline]
pub fn pn_to_shr(p: Complex64, n: Complex64) -> Complex64 {
    let cos = (p + n) * 0.5;
    // sin = (P − N) / (2i) = −i·(P − N)/2.
    let sin = (p - n) * Complex64::new(0.0, -0.5);
    cos + Complex64::i() * sin
}

/// Number of IST iterations when the recipe does not specify one.
pub const DEFAULT_IST_ITERS: usize = 100;

/// Reconstruct one dense length-`grid` complex t1 interferogram from the sparse
/// measured samples by iterative soft thresholding.
///
/// `measured[j]` holds the acquired sample for grid index `positions[j]`; all
/// other grid points start at zero and are filled by the iteration. `iters`
/// passes shrink the F1 spectrum toward the largest components (threshold
/// decays geometrically), inverse-transform, then restore the measured points.
pub fn ist(
    positions: &[usize],
    measured: &[Complex64],
    grid: usize,
    iters: usize,
    planner: &mut FftPlanner<f64>,
) -> Vec<Complex64> {
    let mut x = vec![Complex64::new(0.0, 0.0); grid];
    for (&pos, &m) in positions.iter().zip(measured) {
        if pos < grid {
            x[pos] = m;
        }
    }
    if grid == 0 || iters == 0 {
        return x;
    }
    let fwd = planner.plan_fft_forward(grid);
    let inv = planner.plan_fft_inverse(grid);
    let norm = 1.0 / grid as f64;
    // Threshold starts just below the strongest component and decays
    // geometrically so weaker peaks are admitted progressively, recovering
    // essentially the full peak list by the last pass.
    let decay = 0.98_f64;
    let mut spec = vec![Complex64::new(0.0, 0.0); grid];
    for i in 0..iters {
        spec.copy_from_slice(&x);
        fwd.process(&mut spec);
        let max_amp = spec.iter().map(|c| c.norm()).fold(0.0, f64::max);
        if max_amp <= f64::MIN_POSITIVE {
            break;
        }
        let threshold = max_amp * decay.powi(i as i32);
        for c in spec.iter_mut() {
            let amp = c.norm();
            if amp <= threshold {
                *c = Complex64::new(0.0, 0.0);
            } else {
                // Soft threshold: shrink the surviving magnitude by `threshold`.
                *c *= (amp - threshold) / amp;
            }
        }
        inv.process(&mut spec);
        for (dst, src) in x.iter_mut().zip(spec.iter()) {
            *dst = src * norm;
        }
        // Data consistency: re-impose the measured samples exactly.
        for (&pos, &m) in positions.iter().zip(measured) {
            if pos < grid {
                x[pos] = m;
            }
        }
    }
    x
}

/// Reconstruct the full `grid × f2_n` complex t1 interferogram grid from the
/// acquired (F2-transformed) rows. For echo/anti-echo the stored rows are P/N
/// pairs remapped by [`pn_to_shr`]; otherwise each stored row is one measured
/// increment. Each F2 column is reconstructed independently along F1 by [`ist`].
/// `positions` holds the 0-based grid index of each acquired increment.
#[allow(clippy::too_many_arguments)]
pub fn reconstruct_rows(
    rows_ft: &[Vec<Complex64>],
    echo_antiecho: bool,
    positions: &[usize],
    grid: usize,
    f2_n: usize,
    indirect_conjugate: bool,
    iters: usize,
    planner: &mut FftPlanner<f64>,
) -> Vec<Vec<Complex64>> {
    let acquired = positions.len();
    let mut out = vec![vec![Complex64::new(0.0, 0.0); f2_n]; grid];
    let mut measured = vec![Complex64::new(0.0, 0.0); acquired];
    for c in 0..f2_n {
        for (k, m) in measured.iter_mut().enumerate() {
            let s = if echo_antiecho {
                pn_to_shr(rows_ft[2 * k][c], rows_ft[2 * k + 1][c])
            } else {
                rows_ft[k][c]
            };
            // The P/N→SHR remap already fixes the F1 sense, so echo/anti-echo takes
            // the opposite conjugation to the plain States/phase-modulated path.
            *m = if indirect_conjugate ^ echo_antiecho {
                s.conj()
            } else {
                s
            };
        }
        let col = ist(positions, &measured, grid, iters, planner);
        for (g, v) in col.into_iter().enumerate() {
            out[g][c] = v;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::TAU;

    #[test]
    fn pn_to_shr_resolves_single_frequency() {
        // P = e^{+iΩ}, N = e^{-iΩ}: the States assembly of the raw pair peaks at
        // both ±Ω, but pn_to_shr yields e^{+iΩ} (a single frequency).
        let omega = 0.7;
        let p = Complex64::from_polar(1.0, omega);
        let n = Complex64::from_polar(1.0, -omega);
        let t1 = pn_to_shr(p, n);
        assert!((t1 - Complex64::from_polar(1.0, omega)).norm() < 1e-12);
        // The naive States mix keeps a mirror term of comparable size.
        let naive = p + Complex64::i() * n;
        assert!(naive.norm() > 0.5);
    }

    #[test]
    fn ist_recovers_a_sparse_spectrum() {
        // One F1 tone sampled at a NUS subset of a 32-point grid must reconstruct
        // to a single peak at the right frequency with the gaps filled.
        let grid = 32usize;
        let k0 = 5usize; // target frequency bin
        let full: Vec<Complex64> = (0..grid)
            .map(|t| Complex64::from_polar(1.0, TAU * k0 as f64 * t as f64 / grid as f64))
            .collect();
        let positions = [0usize, 1, 2, 4, 7, 9, 13, 18, 21, 25, 29, 31];
        let measured: Vec<Complex64> = positions.iter().map(|&p| full[p]).collect();
        let mut planner = FftPlanner::<f64>::new();
        let recon = ist(&positions, &measured, grid, 200, &mut planner);

        let mut spec = recon.clone();
        planner.plan_fft_forward(grid).process(&mut spec);
        let peak = spec
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.norm().partial_cmp(&b.1.norm()).unwrap())
            .unwrap()
            .0;
        assert_eq!(
            peak, k0,
            "reconstructed peak lands at the sampled frequency"
        );
        for (&p, &m) in positions.iter().zip(measured.iter()) {
            assert!((recon[p] - m).norm() < 1e-6);
        }
    }
}
