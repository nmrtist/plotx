use crate::{Apodization, AxisPipeline, Spectrum};
use num_complex::Complex64;
use plotx_io::{Domain, NmrData};
use rustfft::FftPlanner;

/// Transform an FID into an *unphased* frequency-domain [`Spectrum`]: apply the
/// pipeline's enabled apodization windows and zero-fill, run the forward FFT
/// (removing the digital-filter group delay unless `group_delay_correct` is
/// false), `fftshift`, and build a ppm axis. Phase and other frequency-domain
/// steps are a separate cheap stage ([`crate::reapply`]).
pub fn transform_base(data: &NmrData, pipe: &AxisPipeline, group_delay_correct: bool) -> Spectrum {
    let n_raw = data.len();
    if n_raw == 0 {
        return Spectrum {
            ppm: Vec::new(),
            values: Vec::new(),
            hz_per_point: 0.0,
            observe_freq_mhz: data.observe_freq_mhz,
            nucleus: data.nucleus.clone(),
        };
    }

    if data.domain == Domain::Frequency {
        let n = data.len();
        let sw = data.spectral_width_hz;
        let hz_per_point = sw / n as f64;
        let obs = data.observe_freq_mhz.max(f64::MIN_POSITIVE);
        let half = n as f64 / 2.0;
        return Spectrum {
            ppm: (0..n)
                .map(|i| data.carrier_ppm + (i as f64 - half) * hz_per_point / obs)
                .collect(),
            values: data.points.clone(),
            hz_per_point,
            observe_freq_mhz: data.observe_freq_mhz,
            nucleus: data.nucleus.clone(),
        };
    }

    let dt = if data.spectral_width_hz != 0.0 {
        1.0 / data.spectral_width_hz
    } else {
        0.0
    };
    let mut buf: Vec<Complex64> = data.points.clone();
    for apo in pipe.apodizations() {
        apply_apodization(&mut buf, apo, dt);
    }
    let n = pipe.zero_fill().target(n_raw);
    buf.resize(n, Complex64::new(0.0, 0.0));

    if data.domain == Domain::Time {
        let mut planner = FftPlanner::<f64>::new();
        let fft = planner.plan_fft_forward(n);
        fft.process(&mut buf);
        if group_delay_correct {
            remove_group_delay(&mut buf, data.group_delay);
        }
    }

    let shifted = fftshift(&buf);

    let sw = data.spectral_width_hz;
    let hz_per_point = if n > 0 { sw / n as f64 } else { 0.0 };
    let obs = data.observe_freq_mhz.max(f64::MIN_POSITIVE);
    let half = n as f64 / 2.0;
    let ppm: Vec<f64> = (0..n)
        .map(|i| {
            let offset_hz = (i as f64 - half) * hz_per_point;
            data.carrier_ppm + offset_hz / obs
        })
        .collect();

    Spectrum {
        ppm,
        values: shifted,
        hz_per_point,
        observe_freq_mhz: data.observe_freq_mhz,
        nucleus: data.nucleus.clone(),
    }
}

/// Apodize a FID in place over its populated samples. `t = i·dt` seconds, with
/// `dt` the sample interval; `dt` is unused by the point-index windows.
pub(crate) fn apply_apodization(buf: &mut [Complex64], apo: Apodization, dt: f64) {
    let n = buf.len();
    match apo {
        Apodization::None => {}
        Apodization::CosineBell => {
            if n <= 1 {
                return;
            }
            let denom = (n - 1) as f64;
            for (i, c) in buf.iter_mut().enumerate() {
                *c *= (std::f64::consts::FRAC_PI_2 * i as f64 / denom).cos();
            }
        }
        Apodization::Exponential { lb_hz } => {
            let k = std::f64::consts::PI * lb_hz;
            for (i, c) in buf.iter_mut().enumerate() {
                *c *= (-k * (i as f64 * dt)).exp();
            }
        }
        Apodization::Gaussian { lb_hz, gb_hz } => {
            let a = std::f64::consts::PI * lb_hz;
            // 4·ln2 maps the Gaussian's frequency FWHM onto its time-domain width.
            let g = (std::f64::consts::PI * gb_hz).powi(2) / (4.0 * std::f64::consts::LN_2);
            for (i, c) in buf.iter_mut().enumerate() {
                let t = i as f64 * dt;
                *c *= (a * t - g * t * t).exp();
            }
        }
    }
}

// A group delay is a circular shift of the FID origin by `delay` samples, which
// by the shift theorem appears as a linear phase ramp `X[m]·e^(+2πi·m·delay/N)`;
// divide it out. No-op when `delay` is zero.
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
    let mid = n.div_ceil(2); // pivot for both even and odd N
    let mut out = Vec::with_capacity(n);
    out.extend_from_slice(&v[mid..]);
    out.extend_from_slice(&v[..mid]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Apodization, AxisPipeline, ProcessingStep, StepKind, StepSource, ZeroFill};
    use plotx_io::Domain;
    use std::f64::consts::TAU;

    fn pipe(apo: Option<Apodization>, zf: ZeroFill) -> AxisPipeline {
        let mut steps = Vec::new();
        if let Some(a) = apo {
            steps.push(ProcessingStep::new(StepKind::Apodize(a), StepSource::User));
        }
        steps.push(ProcessingStep::new(
            StepKind::ZeroFill(zf),
            StepSource::User,
        ));
        steps.push(ProcessingStep::new(StepKind::Fft, StepSource::User));
        AxisPipeline { steps }
    }

    fn decaying_sinusoid(
        npoints: usize,
        spectral_width_hz: f64,
        observe_freq_mhz: f64,
        carrier_ppm: f64,
        shift_ppm: f64,
        group_delay: f64,
    ) -> NmrData {
        let dt = 1.0 / spectral_width_hz;
        let freq_hz = (shift_ppm - carrier_ppm) * observe_freq_mhz;
        let points = (0..npoints)
            .map(|k| {
                let t = k as f64 * dt;
                let decay = (-t / 1.0).exp();
                Complex64::from_polar(decay, TAU * freq_hz * t)
            })
            .collect();
        NmrData {
            points,
            domain: Domain::Time,
            spectral_width_hz,
            observe_freq_mhz,
            carrier_ppm,
            nucleus: "1H".into(),
            source: "test".into(),
            group_delay,
        }
    }

    #[test]
    fn single_peak_lands_at_expected_ppm() {
        let data = decaying_sinusoid(4096, 4000.0, 400.0, 0.0, 2.0, 0.0);
        let s = transform_base(&data, &pipe(None, ZeroFill::None), true);

        let (idx, _) = s
            .real()
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())
            .unwrap();
        let peak_ppm = s.ppm[idx];
        assert!(
            (peak_ppm - 2.0).abs() < 0.05,
            "peak found at {peak_ppm} ppm, expected ~2.0"
        );
    }

    #[test]
    fn group_delay_is_removed() {
        let ideal = decaying_sinusoid(1024, 4000.0, 400.0, 0.0, 2.0, 0.0);
        let n = ideal.len();
        let d = 7usize;
        // Right-shift the FID by `d` points (a leading group delay), tag it.
        let mut delayed = ideal.clone();
        delayed.points = (0..n).map(|k| ideal.points[(k + n - d) % n]).collect();
        delayed.group_delay = d as f64;

        let raw = pipe(None, ZeroFill::None);
        let a = transform_base(&ideal, &raw, true).real();
        let b = transform_base(&delayed, &raw, true).real();
        let max_err = a
            .iter()
            .zip(&b)
            .map(|(x, y)| (x - y).abs())
            .fold(0.0f64, f64::max);
        assert!(max_err < 1e-9, "group delay not removed: max_err={max_err}");
    }

    #[test]
    fn fftshift_moves_dc_to_center() {
        let v: Vec<Complex64> = (0..8).map(|i| Complex64::new(i as f64, 0.0)).collect();
        let s = fftshift(&v);
        assert_eq!(s[4], Complex64::new(0.0, 0.0));
    }

    #[test]
    fn zero_fill_target_never_shrinks() {
        assert_eq!(ZeroFill::None.target(3000), 3000);
        assert_eq!(ZeroFill::Factor(1).target(3000), 4096);
        assert_eq!(ZeroFill::Factor(2).target(3000), 8192);
        assert_eq!(ZeroFill::Size(1000).target(3000), 3000);
        assert_eq!(ZeroFill::Size(9000).target(3000), 9000);
    }

    #[test]
    fn zero_fill_interpolates_without_moving_the_peak() {
        let data = decaying_sinusoid(4096, 4000.0, 400.0, 0.0, 2.0, 0.0);
        let raw = transform_base(&data, &pipe(None, ZeroFill::None), true);
        let filled = transform_base(&data, &pipe(None, ZeroFill::Factor(2)), true);
        assert_eq!(filled.len(), 8192);
        assert!(filled.len() > raw.len());

        let peak_ppm = |s: &Spectrum| {
            let (i, _) = s
                .real()
                .iter()
                .enumerate()
                .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())
                .unwrap();
            s.ppm[i]
        };
        assert!((peak_ppm(&raw) - 2.0).abs() < 0.05);
        assert!((peak_ppm(&filled) - 2.0).abs() < 0.05);
    }

    #[test]
    fn exponential_window_broadens_the_line() {
        let data = decaying_sinusoid(4096, 4000.0, 400.0, 0.0, 2.0, 0.0);
        let sharp = transform_base(&data, &pipe(None, ZeroFill::None), true);
        let broad = transform_base(
            &data,
            &pipe(
                Some(Apodization::Exponential { lb_hz: 20.0 }),
                ZeroFill::None,
            ),
            true,
        );
        let fwhm = |s: &Spectrum| {
            let re = s.real();
            let (peak_i, &peak) = re
                .iter()
                .enumerate()
                .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())
                .unwrap();
            let half = peak / 2.0;
            let count = re.iter().filter(|&&v| v >= half).count();
            let _ = peak_i;
            count
        };
        assert!(
            fwhm(&broad) > fwhm(&sharp),
            "exponential window should broaden: sharp={} broad={}",
            fwhm(&sharp),
            fwhm(&broad)
        );
    }
}
