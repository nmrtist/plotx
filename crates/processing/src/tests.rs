//! Unit tests for the processing pipeline and 2D transforms.

use super::*;
use plotx_io::{Dim, Domain, NmrData2D, QuadMode};

fn data2d(exp: Option<&str>, f2_nuc: &str, f1_nuc: &str) -> NmrData2D {
    let dim = |nuc: &str| Dim {
        spectral_width_hz: 1000.0,
        observe_freq_mhz: 400.0,
        carrier_ppm: 0.0,
        nucleus: nuc.into(),
        group_delay: 0.0,
    };
    NmrData2D {
        data: Vec::new(),
        rows: 0,
        cols: 0,
        domain: Domain::Time,
        direct: dim(f2_nuc),
        indirect: dim(f1_nuc),
        quad: QuadMode::Complex,
        indirect_conjugate: false,
        experiment: exp.map(str::to_owned),
        pseudo_axis: None,
        diffusion: None,
        nus: None,
        source: String::new(),
    }
}

#[test]
fn preset_from_pulse_program_hint() {
    assert_eq!(
        recommend_preset(&data2d(Some("hsqcedetgpsisp2.4"), "1H", "13C")),
        Preset2D::Hsqc
    );
    assert_eq!(
        recommend_preset(&data2d(Some("hmbcgplpndqf"), "1H", "13C")),
        Preset2D::Hmbc
    );
    assert_eq!(
        recommend_preset(&data2d(Some("cosygpppqf"), "1H", "1H")),
        Preset2D::Cosy
    );
    assert_eq!(
        recommend_preset(&data2d(Some("ledbpgp2s"), "1H", "1H")),
        Preset2D::Dosy
    );
    assert_eq!(
        recommend_preset(&data2d(Some("t1ir1d"), "1H", "1H")),
        Preset2D::Relaxation
    );
}

#[test]
fn preset_falls_back_to_nuclei() {
    assert_eq!(recommend_preset(&data2d(None, "1H", "1H")), Preset2D::Cosy);
    assert_eq!(recommend_preset(&data2d(None, "1H", "13C")), Preset2D::Hsqc);
}

#[test]
fn preset_layout_maps_pseudo_2d_to_stack() {
    assert_eq!(Preset2D::Dosy.layout(), Layout2D::Stack);
    assert_eq!(Preset2D::Relaxation.layout(), Layout2D::Stack);
    assert_eq!(Preset2D::Hsqc.layout(), Layout2D::Ft);
    assert_eq!(Preset2D::Cosy.layout(), Layout2D::Ft);
}

use num_complex::Complex64;
use plotx_io::NmrData;
use std::f64::consts::TAU;

fn fid(shift_ppm: f64, group_delay: f64) -> NmrData {
    let (n, sw, obs) = (2048usize, 4000.0, 400.0);
    let dt = 1.0 / sw;
    let freq_hz = shift_ppm * obs;
    let points = (0..n)
        .map(|k| {
            let t = k as f64 * dt;
            Complex64::from_polar((-t / 0.5).exp(), TAU * freq_hz * t + 0.9)
        })
        .collect();
    NmrData {
        points,
        domain: Domain::Time,
        spectral_width_hz: sw,
        observe_freq_mhz: obs,
        carrier_ppm: 0.0,
        nucleus: "1H".into(),
        source: "test".into(),
        group_delay,
    }
}

fn step(kind: StepKind) -> ProcessingStep {
    ProcessingStep::new(kind, StepSource::User)
}

fn peak(spec: &Spectrum) -> Complex64 {
    *spec
        .values
        .iter()
        .max_by(|a, b| a.norm().total_cmp(&b.norm()))
        .unwrap()
}

#[test]
fn multiple_apodizations_compose() {
    let data = fid(2.0, 0.0);
    let single = AxisPipeline {
        steps: vec![
            step(StepKind::Apodize(Apodization::Exponential { lb_hz: 10.0 })),
            step(StepKind::Fft),
        ],
    };
    let twice = AxisPipeline {
        steps: vec![
            step(StepKind::Apodize(Apodization::Exponential { lb_hz: 5.0 })),
            step(StepKind::Apodize(Apodization::Exponential { lb_hz: 5.0 })),
            step(StepKind::Fft),
        ],
    };
    let a = transform_base(&data, &single, true);
    let b = transform_base(&data, &twice, true);
    // Two exp windows of lb=5 multiply to one of lb=10.
    for (x, y) in a.values.iter().zip(&b.values) {
        assert!((x - y).norm() < 1e-9);
    }
}

#[test]
fn disabled_step_is_skipped() {
    let data = fid(2.0, 0.0);
    let mut apo = step(StepKind::Apodize(Apodization::Exponential { lb_hz: 30.0 }));
    let with = AxisPipeline {
        steps: vec![apo.clone(), step(StepKind::Fft)],
    };
    apo.enabled = false;
    let without = AxisPipeline {
        steps: vec![apo, step(StepKind::Fft)],
    };
    let raw = AxisPipeline {
        steps: vec![step(StepKind::Fft)],
    };
    let disabled = transform_base(&data, &without, true);
    let bare = transform_base(&data, &raw, true);
    for (x, y) in disabled.values.iter().zip(&bare.values) {
        assert!((x - y).norm() < 1e-12);
    }
    // Sanity: enabling it actually changes the transform.
    let enabled = transform_base(&data, &with, true);
    let diff: f64 = enabled
        .values
        .iter()
        .zip(&bare.values)
        .map(|(x, y)| (x - y).norm())
        .sum();
    assert!(diff > 1.0);
}

#[test]
fn needs_retransform_only_for_time_side() {
    let base = AxisPipeline::default_1d();
    let mut freq_edit = base.clone();
    for s in &mut freq_edit.steps {
        if let StepKind::Phase(p) = &mut s.kind {
            *p = PhaseParams {
                phase0: 1.2,
                ..PhaseParams::MANUAL_ZERO
            };
        }
    }
    assert!(!needs_retransform(&base, &freq_edit, true, true));
    assert!(needs_retransform(&base, &freq_edit, true, false));

    let mut time_edit = base.clone();
    for s in &mut time_edit.steps {
        if let StepKind::Apodize(a) = &mut s.kind {
            *a = Apodization::CosineBell;
            s.enabled = true;
        }
    }
    assert!(needs_retransform(&base, &time_edit, true, true));
}

#[test]
fn cleanup_steps_are_frequency_domain_and_reapply_cheaply() {
    let kinds = [
        StepKind::Smooth(SmoothMethod::DEFAULT),
        StepKind::Normalize(NormalizeMethod::MaxPeak),
        StepKind::Bin(BinParams::DEFAULT),
        StepKind::Reverse,
        StepKind::Invert,
    ];
    let base = AxisPipeline::default_1d();
    for kind in kinds {
        assert_eq!(kind.domain(), StepDomain::Freq);
        let mut edited = base.clone();
        edited
            .steps
            .push(ProcessingStep::new(kind, StepSource::User));
        assert!(!needs_retransform(&base, &edited, true, true));
    }
}

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

    pub fn spec(values: Vec<Complex64>) -> Spectrum {
        let n = values.len();
        Spectrum {
            ppm: (0..n).map(|i| i as f64).collect(),
            values,
            hz_per_point: 1.0,
            observe_freq_mhz: 400.0,
            nucleus: "1H".into(),
        }
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

    /// Recover `(a0, a1)` on a 1024-point spectrum with `peaks`, return residual.
    pub fn recover(
        peaks: &[(f64, f64, f64)],
        a0: f64,
        a1: f64,
        noise: f64,
        m: AutoPhaseMethod,
    ) -> f64 {
        recover_n(1024, peaks, a0, a1, noise, m)
    }

    /// As [`recover`], but at an explicit length `n` — exercises the downsampling
    /// path that only engages for spectra larger than the optimizer's work budget.
    pub fn recover_n(
        n: usize,
        peaks: &[(f64, f64, f64)],
        a0: f64,
        a1: f64,
        noise: f64,
        m: AutoPhaseMethod,
    ) -> f64 {
        let truth = clean(n, peaks);
        let mut s = spec(scramble(&truth, a0, a1, noise));
        let (p0, p1, piv) = auto_phase(&s, m);
        phase::apply_with_pivot(&mut s, p0, p1, piv);
        residual(&s.values, &truth)
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
fn phasing_survives_large_spectrum_downsampling() {
    use groundtruth::*;
    use std::f64::consts::PI;
    // Regression for a real 160k-point 13C spectrum where plain stride decimation
    // stepped over the narrow peaks and phased noise (entropy gave a negative peak
    // 16x the tallest positive). Narrow peaks (width 2) in a 32k spectrum reproduce
    // the regime; max-magnitude pooling must keep them so the ramp is recovered.
    let narrow = vec![
        (0.15, 1.0, 2.0),
        (0.4, 0.7, 2.0),
        (0.62, 0.9, 2.0),
        (0.86, 0.5, 2.0),
    ];
    let r = recover_n(
        32768,
        &narrow,
        0.3,
        220.0 * PI / 180.0,
        0.0,
        AutoPhaseMethod::Entropy,
    );
    assert!(
        r < 0.2,
        "entropy residual on large narrow-peak spectrum: {r}"
    );
}

#[test]
fn absorptive_peak_does_zero_order_only() {
    use groundtruth::*;
    // Exact on a pure zero-order error across several peaks, ...
    assert!(recover(&many(), 0.3, 0.0, 0.0, AutoPhaseMethod::AbsorptivePeak) < 0.05);
    // ... but structurally cannot undo a genuine first-order ramp.
    assert!(recover(&many(), 0.3, 3.0, 0.0, AutoPhaseMethod::AbsorptivePeak) > 0.5);
}

#[test]
fn entropy_recovers_first_order_ramp() {
    use groundtruth::*;
    // A crowded spectrum that genuinely needs phase1: entropy recovers it, where
    // the zero-order rule leaves distant peaks dispersive.
    assert!(recover(&many(), 0.3, 3.0, 0.0, AutoPhaseMethod::Entropy) < 0.15);
    assert!(recover(&many(), -0.5, -4.5, 0.0, AutoPhaseMethod::Entropy) < 0.2);
}

#[test]
fn entropy_is_safe_on_a_single_peak() {
    use groundtruth::*;
    // Guards the regression where a too-narrow synthetic line made entropy invent
    // a ramp: at a realistic linewidth it leaves an already-absorptive peak alone.
    assert!(recover(&one(), 0.9, 0.0, 0.0, AutoPhaseMethod::Entropy) < 0.1);
}

#[test]
fn entropy_recovers_realistic_large_first_order() {
    use groundtruth::*;
    use std::f64::consts::PI;
    // Real spectra carry first-order phase of tens to a few hundred degrees. The
    // grid-seeded optimizer recovers ramps up to ~1.5 turns; beyond that (>~720°)
    // it exceeds the +-360deg coarse-grid span and is expected to fail.
    for p1_deg in [90.0, 270.0, 500.0] {
        let r = recover(
            &many(),
            2.0,
            p1_deg * PI / 180.0,
            0.01,
            AutoPhaseMethod::Entropy,
        );
        assert!(r < 0.35, "phase1={p1_deg}deg residual={r}");
    }
}

#[test]
fn auto_phase_makes_tallest_peak_real() {
    let data = fid(2.0, 0.0);
    // The exact tallest-bin-to-real contract belongs to the zero-order rule; the
    // full-optimizer methods trade that for first-order capability. Pin the method
    // so the test checks that contract rather than whichever is the current default.
    let absorptive = PhaseParams {
        auto: Some(AutoPhaseMethod::AbsorptivePeak),
        ..PhaseParams::MANUAL_ZERO
    };
    let pipe = AxisPipeline {
        steps: vec![step(StepKind::Fft), step(StepKind::Phase(absorptive))],
    };
    let spec = process(&data, &pipe, true);
    let p = peak(&spec);
    assert!(p.re > 0.0);
    assert!(p.im.abs() / p.norm() < 1e-6, "peak not absorptive: {p:?}");
}

#[test]
fn magnitude_step_nulls_imaginary() {
    let data = fid(2.0, 0.0);
    let pipe = AxisPipeline {
        steps: vec![step(StepKind::Fft), step(StepKind::Magnitude)],
    };
    let spec = process(&data, &pipe, true);
    assert!(spec.values.iter().all(|c| c.im == 0.0));
    assert!(spec.values.iter().all(|c| c.re >= 0.0));
}

#[test]
fn reference_step_shifts_ppm_axis() {
    let data = fid(2.0, 0.0);
    let base_pipe = AxisPipeline {
        steps: vec![step(StepKind::Fft)],
    };
    let referenced = AxisPipeline {
        steps: vec![
            step(StepKind::Fft),
            step(StepKind::Reference(ReferenceParams {
                at_ppm: 0.0,
                target_ppm: 1.5,
            })),
        ],
    };
    let a = process(&data, &base_pipe, true);
    let b = process(&data, &referenced, true);
    for (x, y) in a.ppm.iter().zip(&b.ppm) {
        assert!((y - x - 1.5).abs() < 1e-9);
    }
}

#[test]
fn group_delay_correct_false_leaves_the_ramp() {
    let ideal = fid(2.0, 0.0);
    let n = ideal.len();
    let d = 9usize;
    let mut delayed = ideal.clone();
    delayed.points = (0..n).map(|k| ideal.points[(k + n - d) % n]).collect();
    delayed.group_delay = d as f64;
    let raw = AxisPipeline {
        steps: vec![step(StepKind::Fft)],
    };
    let corrected = transform_base(&delayed, &raw, true);
    let uncorrected = transform_base(&delayed, &raw, false);
    let reference = transform_base(&ideal, &raw, true);
    let err = |s: &Spectrum| {
        s.values
            .iter()
            .zip(&reference.values)
            .map(|(x, y)| (x.re - y.re).abs())
            .fold(0.0f64, f64::max)
    };
    assert!(err(&corrected) < 1e-9);
    assert!(err(&uncorrected) > 1.0);
}

#[test]
fn process_up_to_returns_time_then_freq() {
    let data = fid(2.0, 0.0);
    let apo = step(StepKind::Apodize(Apodization::CosineBell));
    let fft = step(StepKind::Fft);
    let (apo_id, fft_id) = (apo.id, fft.id);
    let pipe = AxisPipeline {
        steps: vec![apo, fft],
    };
    match process_up_to(&data, &pipe, true, apo_id) {
        Preview::Time { fid, dt } => {
            assert_eq!(fid.len(), data.len());
            assert!((dt - 1.0 / data.spectral_width_hz).abs() < 1e-12);
        }
        _ => panic!("expected time-domain preview"),
    }
    assert!(matches!(
        process_up_to(&data, &pipe, true, fft_id),
        Preview::Freq(_)
    ));
}

#[test]
fn default_2d_matches_apodized_transform() {
    let params = Params2D::default_for(Preset2D::Hsqc);
    assert_eq!(params.layout, Layout2D::Ft);
    assert_eq!(params.f2.apodizations(), vec![Apodization::CosineBell]);
    assert_eq!(params.f1.apodizations(), vec![Apodization::CosineBell]);
    // F2 auto-phases, F1 does not.
    let f2_auto = params
        .f2
        .steps
        .iter()
        .any(|s| matches!(&s.kind, StepKind::Phase(p) if p.auto.is_some()));
    let f1_auto = params
        .f1
        .steps
        .iter()
        .any(|s| matches!(&s.kind, StepKind::Phase(p) if p.auto.is_some()));
    assert!(f2_auto && !f1_auto);
}
