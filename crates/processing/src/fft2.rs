use crate::fft::apply_apodization;
use crate::phase::apply_slice;
use crate::{AxisMeta, Params2D, Spectrum2D, StackSpectrum};
use num_complex::Complex64;
use plotx_io::{Domain, NmrData2D, QuadMode};
use rustfft::FftPlanner;

fn sample_interval(sw_hz: f64) -> f64 {
    if sw_hz != 0.0 { 1.0 / sw_hz } else { 0.0 }
}

/// Transform a 2D FID into an *unphased* frequency-domain [`Spectrum2D`]: FFT
/// along the direct (F2) axis of every row, quadrature recombination + FFT along
/// the indirect (F1) axis, `fftshift` on both, and ppm axes from the per-dimension
/// spectral widths and observe frequencies. Each axis's window and zero-fill are
/// applied before its FFT; phase is a separate cheap stage ([`reapply_phase_2d`]).
pub fn transform(data: &NmrData2D, params: &Params2D) -> Spectrum2D {
    transform_cancellable(data, params, &|| false).expect("non-cancelling transform")
}

/// Cooperative-cancellation variant used by the desktop compute service. The
/// callback is checked between FFT rows/columns, which bounds cancellation
/// latency without adding synchronization inside rustfft itself.
pub fn transform_cancellable(
    data: &NmrData2D,
    params: &Params2D,
    cancelled: &impl Fn() -> bool,
) -> Option<Spectrum2D> {
    let cols = data.cols;
    let rows = data.rows;
    if cols == 0 || rows == 0 {
        return Some(empty(data));
    }
    if data.domain == Domain::Frequency {
        return Some(Spectrum2D {
            f2_ppm: ppm_axis(
                cols,
                data.direct.spectral_width_hz,
                data.direct.observe_freq_mhz,
                data.direct.carrier_ppm,
            ),
            f1_ppm: ppm_axis(
                rows,
                data.indirect.spectral_width_hz,
                data.indirect.observe_freq_mhz,
                data.indirect.carrier_ppm,
            ),
            data: data.data.clone(),
            f2_size: cols,
            f1_size: rows,
            direct: AxisMeta::from(&data.direct),
            indirect: AxisMeta::from(&data.indirect),
            source: data.source.clone(),
        });
    }

    let mut planner = FftPlanner::<f64>::new();

    let f2_apo = params.f2.apodizations();
    let f2_dt = sample_interval(data.direct.spectral_width_hz);
    let f2_n = params.f2.zero_fill().target(cols);
    let f2_fft = planner.plan_fft_forward(f2_n);
    let mut rows_ft: Vec<Vec<Complex64>> = Vec::with_capacity(rows);
    for r in 0..rows {
        if cancelled() {
            return None;
        }
        let mut buf: Vec<Complex64> = data.row(r).to_vec();
        for apo in &f2_apo {
            apply_apodization(&mut buf, *apo, f2_dt);
        }
        buf.resize(f2_n, Complex64::new(0.0, 0.0));
        if data.domain == Domain::Time {
            f2_fft.process(&mut buf);
            remove_group_delay(&mut buf, data.direct.group_delay);
        }
        rows_ft.push(fftshift(&buf));
    }

    // For NUS the acquired increments are reconstructed onto the full grid
    // first; without a user-supplied schedule no (mirrored/aliased) spectrum is
    // produced — the app surfaces a prompt to enter the sampling list.
    let t1_rows = match build_t1_rows(data, &rows_ft, f2_n, &mut planner) {
        Some(rows) => rows,
        None => return Some(empty(data)),
    };
    if cancelled() {
        return None;
    }
    let f1_inc = t1_rows.len();
    let f1_apo = params.f1.apodizations();
    let f1_dt = sample_interval(data.indirect.spectral_width_hz);
    let f1_n = params.f1.zero_fill().target(f1_inc);
    let f1_fft = planner.plan_fft_forward(f1_n);

    let mut out = vec![Complex64::new(0.0, 0.0); f1_n * f2_n];
    let mut col: Vec<Complex64> = Vec::with_capacity(f1_n);
    for c in 0..f2_n {
        if cancelled() {
            return None;
        }
        col.clear();
        for row in t1_rows.iter() {
            col.push(row[c]);
        }
        for apo in &f1_apo {
            apply_apodization(&mut col, *apo, f1_dt);
        }
        col.resize(f1_n, Complex64::new(0.0, 0.0));
        f1_fft.process(&mut col);
        let shifted = fftshift(&col);
        for (k, v) in shifted.into_iter().enumerate() {
            out[k * f2_n + c] = v;
        }
    }

    let f2_ppm = ppm_axis(
        f2_n,
        data.direct.spectral_width_hz,
        data.direct.observe_freq_mhz,
        data.direct.carrier_ppm,
    );
    let f1_ppm = ppm_axis(
        f1_n,
        data.indirect.spectral_width_hz,
        data.indirect.observe_freq_mhz,
        data.indirect.carrier_ppm,
    );

    Some(Spectrum2D {
        f2_ppm,
        f1_ppm,
        data: out,
        f2_size: f2_n,
        f1_size: f1_n,
        direct: AxisMeta::from(&data.direct),
        indirect: AxisMeta::from(&data.indirect),
        source: data.source.clone(),
    })
}

/// Apply the per-axis phase `(phase0, phase1, pivot_frac)` to an unphased
/// [`Spectrum2D`] from [`transform`], in place on a clone. F2 phase depends only
/// on the column (direct index), F1 phase only on the row (indirect index); both
/// are computed on the display (fftshifted) grid, exactly like the 1D kernel.
pub fn reapply_phase_2d(base: &Spectrum2D, f2: (f64, f64, f64), f1: (f64, f64, f64)) -> Spectrum2D {
    reapply_phase_2d_cancellable(base, f2, f1, &|| false).expect("non-cancelling phase pass")
}

pub fn reapply_phase_2d_cancellable(
    base: &Spectrum2D,
    f2: (f64, f64, f64),
    f1: (f64, f64, f64),
    cancelled: &impl Fn() -> bool,
) -> Option<Spectrum2D> {
    let mut out = base.clone();
    let nr = out.f1_size;
    let nc = out.f2_size;
    if nr == 0 || nc == 0 {
        return Some(out);
    }
    let d1 = (nr - 1).max(1) as f64;
    let d2 = (nc - 1).max(1) as f64;
    let (f2p0, f2p1, f2piv) = f2;
    let (f1p0, f1p1, f1piv) = f1;
    for r in 0..nr {
        if cancelled() {
            return None;
        }
        let phi1 = f1p0 + f1p1 * (r as f64 / d1 - f1piv);
        for c in 0..nc {
            let phi2 = f2p0 + f2p1 * (c as f64 / d2 - f2piv);
            out.data[r * nc + c] *= Complex64::from_polar(1.0, -(phi1 + phi2));
        }
    }
    Some(out)
}

/// Apply the direct-axis phase `(phase0, phase1, pivot_frac)` to every trace of
/// an unphased stack.
pub fn reapply_phase_stack(base: &StackSpectrum, f2: (f64, f64, f64)) -> StackSpectrum {
    reapply_phase_stack_cancellable(base, f2, &|| false).expect("non-cancelling phase pass")
}

pub fn reapply_phase_stack_cancellable(
    base: &StackSpectrum,
    f2: (f64, f64, f64),
    cancelled: &impl Fn() -> bool,
) -> Option<StackSpectrum> {
    let mut out = base.clone();
    let (p0, p1, piv) = f2;
    for t in out.traces.iter_mut() {
        if cancelled() {
            return None;
        }
        apply_slice(t, p0, p1, piv);
    }
    Some(out)
}

/// Pseudo-2D processing: Fourier transform only the direct dimension, keeping
/// each increment as its own 1D spectrum for a stacked display. No indirect FFT
/// or quadrature recombination is applied — the indirect axis is a parameter
/// array (gradient strength, relaxation delay, …), not a frequency.
pub fn stack(data: &NmrData2D, params: &Params2D) -> StackSpectrum {
    stack_cancellable(data, params, &|| false).expect("non-cancelling stack transform")
}

pub fn stack_cancellable(
    data: &NmrData2D,
    params: &Params2D,
    cancelled: &impl Fn() -> bool,
) -> Option<StackSpectrum> {
    let cols = data.cols;
    let rows = data.rows;
    if cols == 0 || rows == 0 {
        return Some(StackSpectrum {
            ppm: Vec::new(),
            traces: Vec::new(),
            direct: AxisMeta::from(&data.direct),
            source: data.source.clone(),
        });
    }
    let f2_apo = params.f2.apodizations();
    let f2_dt = sample_interval(data.direct.spectral_width_hz);
    let f2_n = params.f2.zero_fill().target(cols);
    let mut planner = FftPlanner::<f64>::new();
    let fft = planner.plan_fft_forward(f2_n);

    // Unphased traces; the absorptive phase is derived by `reapply_phase_stack`.
    let mut traces = Vec::with_capacity(rows);
    for r in 0..rows {
        if cancelled() {
            return None;
        }
        let mut buf: Vec<Complex64> = data.row(r).to_vec();
        for apo in &f2_apo {
            apply_apodization(&mut buf, *apo, f2_dt);
        }
        buf.resize(f2_n, Complex64::new(0.0, 0.0));
        if data.domain == Domain::Time {
            fft.process(&mut buf);
            remove_group_delay(&mut buf, data.direct.group_delay);
        }
        traces.push(fftshift(&buf));
    }

    let ppm = ppm_axis(
        f2_n,
        data.direct.spectral_width_hz,
        data.direct.observe_freq_mhz,
        data.direct.carrier_ppm,
    );

    Some(StackSpectrum {
        ppm,
        traces,
        direct: AxisMeta::from(&data.direct),
        source: data.source.clone(),
    })
}

// Assemble the complex t1 interferogram rows from the F2-transformed stored
// rows, applying the indirect conjugation that fixes the F1 frequency sense.
// Returns `None` only for a NUS dataset with no user-supplied schedule, so the
// caller withholds the spectrum instead of showing a wrong reconstruction.
fn build_t1_rows(
    data: &NmrData2D,
    rows_ft: &[Vec<Complex64>],
    f2_n: usize,
    planner: &mut FftPlanner<f64>,
) -> Option<Vec<Vec<Complex64>>> {
    if let Some(nus) = &data.nus {
        let schedule = nus.schedule.as_ref()?;
        return Some(crate::nus::reconstruct_rows(
            rows_ft,
            nus.echo_antiecho,
            schedule,
            nus.grid,
            f2_n,
            data.indirect_conjugate,
            crate::nus::DEFAULT_IST_ITERS,
            planner,
        ));
    }
    let f1_inc = f1_increments(data.rows, data.quad);
    let rows: Vec<Vec<Complex64>> = (0..f1_inc)
        .map(|k| {
            (0..f2_n)
                .map(|c| {
                    let v = combine(rows_ft, k, c, data.quad);
                    if data.indirect_conjugate { v.conj() } else { v }
                })
                .collect()
        })
        .collect();
    Some(rows)
}

fn f1_increments(rows: usize, quad: QuadMode) -> usize {
    match quad {
        QuadMode::Complex => rows,
        QuadMode::States | QuadMode::StatesTppi | QuadMode::EchoAntiecho => rows / 2,
    }
}

// Build the complex t1 sample at increment `k`, F2 point `c`, from the
// F2-transformed rows, per the indirect-dimension quadrature scheme. Each stored
// row is already a complex F2 spectrum, so the cosine/sine channels combine
// directly without needing the F2 phase.
fn combine(rows_ft: &[Vec<Complex64>], k: usize, c: usize, quad: QuadMode) -> Complex64 {
    match quad {
        QuadMode::Complex => rows_ft[k][c],
        QuadMode::States => rows_ft[2 * k][c] + Complex64::i() * rows_ft[2 * k + 1][c],
        QuadMode::StatesTppi => {
            let sign = if (k & 1) == 0 { 1.0 } else { -1.0 };
            (rows_ft[2 * k][c] + Complex64::i() * rows_ft[2 * k + 1][c]) * sign
        }
        // Echo/anti-echo each select a single coherence pathway, so one row of
        // the pair is already a clean phase-modulated t1 series; magnitude mode
        // needs no further recombination.
        QuadMode::EchoAntiecho => rows_ft[2 * k + 1][c],
    }
}

fn ppm_axis(n: usize, sw_hz: f64, obs_mhz: f64, carrier_ppm: f64) -> Vec<f64> {
    let hz_per_point = if n > 0 { sw_hz / n as f64 } else { 0.0 };
    let obs = obs_mhz.max(f64::MIN_POSITIVE);
    let half = n as f64 / 2.0;
    (0..n)
        .map(|i| carrier_ppm + (i as f64 - half) * hz_per_point / obs)
        .collect()
}

/// Estimate a zero-order `(phase0, phase1)` that makes the highest-energy
/// trace's tallest peak purely absorptive-positive: `phase0 = arg(peak)`,
/// `phase1 = 0`. Applied uniformly, this phases the dominant resonance — the one
/// the user reads for a relaxation/diffusion fit — exactly, with its real part
/// carrying the signal and its imaginary (dispersive) part nulled. First-order
/// correction is deliberately skipped: with truncation ringing or spinning
/// sidebands a single ramp cannot phase every peak and tends to spoil the main
/// one. `None` for an empty stack. Seeded into `f2.phase0` at load.
pub fn absorptive_phase(traces: &[Vec<Complex64>]) -> Option<(f64, f64)> {
    let energy = |t: &[Complex64]| t.iter().map(|c| c.norm_sqr()).sum::<f64>();
    let reference = traces.iter().filter(|t| !t.is_empty()).max_by(|a, b| {
        energy(a)
            .partial_cmp(&energy(b))
            .unwrap_or(std::cmp::Ordering::Equal)
    })?;
    let peak = reference.iter().max_by(|a, b| {
        a.norm()
            .partial_cmp(&b.norm())
            .unwrap_or(std::cmp::Ordering::Equal)
    })?;
    if peak.norm() <= f64::MIN_POSITIVE {
        return None;
    }
    // arg(peak) rotates the peak onto the positive real axis (apply_phase rotates
    // by e^{-iφ}), so its absorptive lobe points up.
    Some((peak.arg(), 0.0))
}

fn remove_group_delay(spectrum: &mut [Complex64], delay: f64) {
    if delay == 0.0 || !delay.is_finite() {
        return;
    }
    let n = spectrum.len();
    if n == 0 {
        return;
    }
    let k = std::f64::consts::TAU * delay / n as f64;
    for (m, c) in spectrum.iter_mut().enumerate() {
        *c *= Complex64::from_polar(1.0, k * m as f64);
    }
}

fn fftshift(v: &[Complex64]) -> Vec<Complex64> {
    let n = v.len();
    let mid = n.div_ceil(2);
    let mut out = Vec::with_capacity(n);
    out.extend_from_slice(&v[mid..]);
    out.extend_from_slice(&v[..mid]);
    out
}

fn empty(data: &NmrData2D) -> Spectrum2D {
    Spectrum2D {
        f2_ppm: Vec::new(),
        f1_ppm: Vec::new(),
        data: Vec::new(),
        f2_size: 0,
        f1_size: 0,
        direct: AxisMeta::from(&data.direct),
        indirect: AxisMeta::from(&data.indirect),
        source: data.source.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Params2D;
    use plotx_io::Dim;
    use std::f64::consts::TAU;

    fn dim(sw: f64, obs: f64, nucleus: &str) -> Dim {
        Dim {
            spectral_width_hz: sw,
            observe_freq_mhz: obs,
            carrier_ppm: 0.0,
            nucleus: nucleus.into(),
            group_delay: 0.0,
        }
    }

    // A single 2D frequency: phase-modulated e^{iΩ2 t2}·e^{iΩ1 t1} with decay, so
    // a forward FFT in each dimension lands one magnitude peak at (f1, f2).
    fn single_peak_2d(f2_ppm: f64, f1_ppm: f64) -> NmrData2D {
        let (cols, rows) = (256usize, 128usize);
        let direct = dim(4000.0, 400.0, "1H");
        let indirect = dim(2000.0, 100.0, "13C");
        let dt2 = 1.0 / direct.spectral_width_hz;
        let dt1 = 1.0 / indirect.spectral_width_hz;
        let f2_hz = f2_ppm * direct.observe_freq_mhz;
        let f1_hz = f1_ppm * indirect.observe_freq_mhz;
        let mut data = Vec::with_capacity(rows * cols);
        for k in 0..rows {
            let t1 = k as f64 * dt1;
            for j in 0..cols {
                let t2 = j as f64 * dt2;
                let decay = (-t2 / 0.3 - t1 / 0.3).exp();
                data.push(Complex64::from_polar(
                    decay,
                    TAU * (f2_hz * t2 + f1_hz * t1),
                ));
            }
        }
        NmrData2D {
            data,
            rows,
            cols,
            domain: Domain::Time,
            direct,
            indirect,
            quad: QuadMode::Complex,
            indirect_conjugate: false,
            experiment: None,
            pseudo_axis: None,
            diffusion: None,
            nus: None,
            source: "synthetic 2D".into(),
        }
    }

    #[test]
    fn ft_places_peak_at_expected_shifts() {
        let data = single_peak_2d(2.0, 1.0);
        let s = transform(&data, &Params2D::default());
        assert_eq!((s.f1_size, s.f2_size), (128, 256));

        let mag = s.magnitude();
        let (mut best, mut br, mut bc) = (f32::MIN, 0, 0);
        for r in 0..s.f1_size {
            for c in 0..s.f2_size {
                let v = mag[r * s.f2_size + c];
                if v > best {
                    best = v;
                    br = r;
                    bc = c;
                }
            }
        }
        // Tolerances are one frequency bin: ~0.04 ppm (F2), ~0.16 ppm (F1).
        assert!((s.f2_ppm[bc] - 2.0).abs() < 0.05, "F2 at {}", s.f2_ppm[bc]);
        assert!((s.f1_ppm[br] - 1.0).abs() < 0.2, "F1 at {}", s.f1_ppm[br]);
    }

    #[test]
    fn reapply_phase_makes_peak_absorptive() {
        let data = single_peak_2d(2.0, 1.0);
        let base = transform(&data, &Params2D::default());

        let mag = base.magnitude();
        let (mut best, mut idx) = (f32::MIN, 0usize);
        for (i, &v) in mag.iter().enumerate() {
            if v > best {
                best = v;
                idx = i;
            }
        }
        let arg = base.data[idx].arg();

        // Rotating F2 by the peak's argument lands it on the positive real axis:
        // real part carries the magnitude, imaginary part nulls.
        let phased = reapply_phase_2d(&base, (arg, 0.0, 0.0), (0.0, 0.0, 0.0));
        let peak = phased.data[idx];
        assert!((peak.re - base.data[idx].norm()).abs() < 1e-6);
        assert!(peak.im.abs() < 1e-6);
    }

    #[test]
    fn stack_keeps_one_spectrum_per_increment() {
        let data = single_peak_2d(2.0, 1.0);
        let s = stack(&data, &Params2D::default());
        assert_eq!(s.increments(), data.rows);
        assert_eq!(s.ppm.len(), data.cols);
        let peak_ppm = |trace: &[Complex64]| {
            let (mut best, mut bi) = (f64::MIN, 0);
            for (i, c) in trace.iter().enumerate() {
                if c.norm() > best {
                    best = c.norm();
                    bi = i;
                }
            }
            s.ppm[bi]
        };
        assert!((peak_ppm(&s.traces[0]) - 2.0).abs() < 0.05);
        assert!((peak_ppm(&s.traces[10]) - 2.0).abs() < 0.05);
    }
}
