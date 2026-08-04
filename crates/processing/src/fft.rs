use crate::{Apodization, AxisPipeline, Spectrum, StepKind, TimeTrace};
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

    let dt = data.dwell_s();
    let mut buf = apply_time_steps(data.points.clone(), pipe, dt);
    let n = buf.len();

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

/// Apply the enabled time-domain prefix without inventing an FFT. Callers use
/// this when the typed pipeline finishes in the time domain.
pub fn transform_time(data: &NmrData, pipe: &AxisPipeline) -> TimeTrace {
    let dt = data.dwell_s();
    let values = apply_time_steps(data.points.clone(), pipe, dt);
    TimeTrace {
        time_s: (0..values.len()).map(|index| index as f64 * dt).collect(),
        values,
        nucleus: data.nucleus.clone(),
        source: data.source.clone(),
    }
}

/// Apply the enabled time-domain prefix exactly in recipe order.
///
/// Keeping this one kernel for time output and FFT input means adding/removing
/// FFT changes only the domain transition: it cannot silently reorder windows
/// or collapse multiple zero-fill steps.
pub(crate) fn apply_time_steps(
    mut values: Vec<Complex64>,
    pipe: &AxisPipeline,
    dt: f64,
) -> Vec<Complex64> {
    for step in pipe.steps.iter().filter(|step| step.enabled) {
        match step.kind {
            StepKind::Apodize(window) => apply_apodization(&mut values, window, dt),
            StepKind::ZeroFill(fill) => {
                let target = fill.target(values.len());
                values.resize(target, Complex64::new(0.0, 0.0));
            }
            StepKind::Fft => break,
            StepKind::Phase(_)
            | StepKind::Baseline(_)
            | StepKind::Reference(_)
            | StepKind::Magnitude
            | StepKind::Smooth(_)
            | StepKind::Normalize(_)
            | StepKind::Bin(_)
            | StepKind::Reverse
            | StepKind::Invert => {}
        }
    }
    values
}

pub(crate) fn time_step_output_len(mut len: usize, pipe: &AxisPipeline) -> usize {
    for step in pipe.steps.iter().filter(|step| step.enabled) {
        match step.kind {
            StepKind::ZeroFill(fill) => len = fill.target(len),
            StepKind::Fft => break,
            _ => {}
        }
    }
    len
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
// by the shift theorem appears as a linear phase ramp. Use signed FFT-bin
// frequencies here: for a fractional delay, treating the upper half as positive
// frequencies puts the phase wrap at DC after `fftshift`, creating a visible
// discontinuity in the real spectrum. With signed bins the unavoidable wrap is
// at the Nyquist boundary instead.
fn remove_group_delay(spectrum: &mut [Complex64], delay: f64) {
    if delay == 0.0 || !delay.is_finite() {
        return;
    }
    let n = spectrum.len();
    if n == 0 {
        return;
    }
    let phase_per_bin = std::f64::consts::TAU * delay / n as f64;
    let negative_start = n.div_ceil(2);
    for (m, c) in spectrum.iter_mut().enumerate() {
        let signed_bin = if m < negative_start {
            m as f64
        } else {
            m as f64 - n as f64
        };
        *c *= Complex64::from_polar(1.0, phase_per_bin * signed_bin);
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
    use crate::{
        Apodization, AxisPipeline, ProcessingStep, StepId, StepKind, StepSource, ZeroFill,
    };
    use plotx_io::Domain;
    use std::f64::consts::TAU;

    // A detached test recipe: no dataset owns it, so numbering its own steps
    // 0..n is enough to keep them distinguishable.
    fn pipe(apo: Option<Apodization>, zf: ZeroFill) -> AxisPipeline {
        let kinds = apo
            .map(StepKind::Apodize)
            .into_iter()
            .chain([StepKind::ZeroFill(zf), StepKind::Fft]);
        AxisPipeline {
            steps: kinds
                .enumerate()
                .map(|(index, kind)| {
                    ProcessingStep::new(StepId::new(index as u64), kind, StepSource::User)
                })
                .collect(),
        }
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
    fn fractional_group_delay_uses_signed_fft_frequencies() {
        let n = 16usize;
        let delay = 3.25;
        let negative_start = n.div_ceil(2);
        let phase_per_bin = std::f64::consts::TAU * delay / n as f64;
        let mut delayed: Vec<Complex64> = (0..n)
            .map(|m| {
                let signed_bin = if m < negative_start {
                    m as f64
                } else {
                    m as f64 - n as f64
                };
                Complex64::from_polar(1.0, -phase_per_bin * signed_bin)
            })
            .collect();

        remove_group_delay(&mut delayed, delay);

        assert!(
            delayed
                .iter()
                .all(|value| (*value - Complex64::new(1.0, 0.0)).norm() < 1e-12),
            "fractional delay correction must not introduce a phase jump at DC"
        );
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
    fn time_steps_keep_recipe_order_with_or_without_fft() {
        let data = NmrData {
            points: vec![Complex64::new(1.0, 0.0); 3],
            domain: Domain::Time,
            spectral_width_hz: 1000.0,
            observe_freq_mhz: 400.0,
            carrier_ppm: 0.0,
            nucleus: "1H".into(),
            source: "ordered time steps".into(),
            group_delay: 0.0,
        };
        let kinds = [
            StepKind::ZeroFill(ZeroFill::Size(5)),
            StepKind::Apodize(Apodization::CosineBell),
            StepKind::ZeroFill(ZeroFill::Factor(1)),
            StepKind::Fft,
        ];
        let spectral = AxisPipeline {
            steps: kinds
                .into_iter()
                .enumerate()
                .map(|(index, kind)| {
                    ProcessingStep::new(StepId::new(index as u64), kind, StepSource::User)
                })
                .collect(),
        };
        let mut temporal = spectral.clone();
        temporal.steps.last_mut().unwrap().enabled = false;

        let trace = transform_time(&data, &temporal);
        let spectrum = transform_base(&data, &spectral, true);
        assert_eq!(trace.values.len(), 8);
        assert_eq!(spectrum.values.len(), trace.values.len());
        assert!((trace.values[2].re - std::f64::consts::FRAC_1_SQRT_2).abs() < 1e-12);
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
