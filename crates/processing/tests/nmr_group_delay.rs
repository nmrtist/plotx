//! Preserve the signal inputs and tolerances from the former fft.rs tests.
use nmr::{Complex64, ExecutionContext};
use plotx_io::{Domain, NmrData, nmr_view::NmrSource};
use plotx_processing::nmr_bridge::{DelayPolicy, RecipeRange};
use plotx_processing::{AxisPipeline, ProcessingStep, StepId, StepKind, StepSource};
use std::f64::consts::TAU;

fn fid() -> NmrData {
    NmrData {
        points: (0..1024)
            .map(|k| {
                let t = k as f64 / 4000.0;
                Complex64::from_polar((-t / 1.0).exp(), TAU * 800.0 * t)
            })
            .collect(),
        domain: Domain::Time,
        spectral_width_hz: 4000.0,
        observe_freq_mhz: 400.0,
        carrier_ppm: 0.0,
        nucleus: "1H".into(),
        source: "original group-delay regression".into(),
        group_delay: 0.0,
    }
}

fn corrected(data: NmrData) -> Vec<Complex64> {
    let source = NmrSource::try_from(data).unwrap();
    let pipe = AxisPipeline {
        steps: vec![ProcessingStep::new(
            StepId::new(0),
            StepKind::Fft,
            StepSource::User,
        )],
    };
    plotx_processing::nmr_execution::execute_1d(
        &source,
        &pipe,
        DelayPolicy::AxisEvidence,
        RecipeRange::Base,
        &mut ExecutionContext::default(),
    )
    .unwrap()
    .source
    .trace()
    .unwrap()
}

#[test]
fn group_delay_is_removed_with_the_original_seven_point_shift_and_tolerance() {
    let ideal = fid();
    let n = ideal.points.len();
    let d = 7usize;
    let mut delayed = ideal.clone();
    delayed.points = (0..n).map(|k| ideal.points[(k + n - d) % n]).collect();
    delayed.group_delay = d as f64;
    let a = corrected(ideal);
    let b = corrected(delayed);
    let max_err = a
        .iter()
        .zip(&b)
        .map(|(x, y)| (x.re - y.re).abs())
        .fold(0.0f64, f64::max);
    assert!(max_err < 1e-9, "group delay not removed: max_err={max_err}");
}

#[test]
fn fractional_group_delay_uses_the_original_signed_bins_and_tolerance() {
    let n = 16usize;
    let delay = 3.25;
    let negative_start = n.div_ceil(2);
    let phase_per_bin = TAU * delay / n as f64;
    let delayed: Vec<Complex64> = (0..n)
        .map(|m| {
            let signed_bin = if m < negative_start {
                m as f64
            } else {
                m as f64 - n as f64
            };
            Complex64::from_polar(1.0, -phase_per_bin * signed_bin)
        })
        .collect();
    // Independent inverse DFT feeds the original spectrum into the public FFT
    // bridge. The expected corrected spectrum remains exactly one at every bin.
    let mut input = fid();
    input.points = (0..n)
        .map(|k| {
            delayed
                .iter()
                .enumerate()
                .map(|(m, value)| {
                    value * Complex64::from_polar(1.0, TAU * (m * k) as f64 / n as f64)
                })
                .sum::<Complex64>()
                / n as f64
        })
        .collect();
    input.group_delay = delay;
    assert!(
        corrected(input)
            .iter()
            .all(|value| (*value - Complex64::new(1.0, 0.0)).norm() < 1e-12),
        "fractional delay correction must not introduce a phase jump at DC"
    );
}
