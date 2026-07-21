//! End-to-end slice: synthetic FID → FFT/phase/baseline → figure → SVG export.

use num_complex::Complex64;
use plotx_analysis::peaks::{DetectParams, detect_peaks, estimate_noise};
use plotx_core::build_figure;
use plotx_io::{Domain, NmrData};
use plotx_processing::{AxisPipeline, process};
use std::f64::consts::TAU;

/// An ethanol-like ¹H FID (three singlets at 3:2:1), so the test needs no file.
fn ethanol_fid() -> NmrData {
    let npoints = 16_384;
    let sw = 4000.0;
    let obs = 400.13;
    let carrier = 5.0;
    let peaks = [(1.22, 3.0), (2.61, 1.0), (3.70, 2.0)];
    let dt = 1.0 / sw;
    let points = (0..npoints)
        .map(|k| {
            let t = k as f64 * dt;
            let decay = (-t / 0.5).exp();
            peaks
                .iter()
                .fold(Complex64::new(0.0, 0.0), |acc, &(ppm, amp)| {
                    let freq_hz = (ppm - carrier) * obs;
                    acc + Complex64::from_polar(amp * decay, TAU * freq_hz * t)
                })
        })
        .collect();
    NmrData {
        points,
        domain: Domain::Time,
        spectral_width_hz: sw,
        observe_freq_mhz: obs,
        carrier_ppm: carrier,
        nucleus: "1H".into(),
        source: "synthetic ethanol ¹H @ 400 MHz".into(),
        group_delay: 0.0,
    }
}

#[test]
fn full_slice_load_process_figure_export() {
    let data = ethanol_fid();
    assert_eq!(data.len(), 16_384);

    let spec = process(&data, &AxisPipeline::default_1d(), true);
    assert_eq!(spec.len(), data.len());

    let ys = spec.real();
    let sigma = estimate_noise(&ys);
    let peaks = detect_peaks(
        &spec.ppm,
        &ys,
        &DetectParams {
            min_height: Some(4.0 * sigma),
            min_prominence: sigma,
            min_spacing: None,
            max_count: None,
        },
    );
    let has = |target: f64| peaks.iter().any(|p| (p.x - target).abs() < 0.1);
    assert!(has(1.22), "missing CH3 peak; got {peaks:?}");
    assert!(has(2.61), "missing OH peak; got {peaks:?}");
    assert!(has(3.70), "missing CH2 peak; got {peaks:?}");

    let fig = build_figure(&data, &spec, &[]);
    let svg = plotx_render::svg::export(&fig);
    assert!(svg.starts_with("<svg"));
    assert!(svg.contains("<polyline"));
    assert!(svg.contains("chemical shift"));
}
