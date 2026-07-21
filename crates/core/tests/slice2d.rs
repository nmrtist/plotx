//! End-to-end 2D slice: synthetic 2D FID → 2D FFT → contour / stack figure →
//! SVG export. No files needed.

use num_complex::Complex64;
use plotx_core::{build_figure_2d, build_stack_figure};
use plotx_io::{Dim, Domain, NmrData2D, QuadMode};
use plotx_processing::{Layout2D, Params2D, Preset2D, Processed2D, process_2d, recommend_preset};
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

/// A phase-modulated 2D FID with a single cross peak at `(f2_ppm, f1_ppm)`.
fn synthetic_hsqc(f2_ppm: f64, f1_ppm: f64, experiment: &str) -> NmrData2D {
    let (cols, rows) = (256usize, 128usize);
    let direct = dim(4000.0, 400.0, "1H");
    let indirect = dim(4000.0, 100.0, "13C");
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
        experiment: Some(experiment.to_owned()),
        pseudo_axis: None,
        diffusion: None,
        nus: None,
        source: "synthetic HSQC".into(),
    }
}

#[test]
fn contour_slice_places_peak_and_exports_svg() {
    // Shifts stay inside the ±SW/2 Nyquist range (F1: 10 ppm × 100 MHz = 1 kHz).
    let data = synthetic_hsqc(3.0, 10.0, "hsqcetgpsisp");
    let preset = recommend_preset(&data);
    assert_eq!(preset, Preset2D::Hsqc);
    assert_eq!(preset.layout(), Layout2D::Ft);

    let spec = match process_2d(&data, &Params2D::default()) {
        Processed2D::Ft(s) => s,
        Processed2D::Stack(_) => panic!("expected Ft"),
    };
    let mag = spec.magnitude();
    let (mut best, mut br, mut bc) = (f32::MIN, 0, 0);
    for r in 0..spec.f1_size {
        for c in 0..spec.f2_size {
            let v = mag[r * spec.f2_size + c];
            if v > best {
                best = v;
                br = r;
                bc = c;
            }
        }
    }
    assert!(
        (spec.f2_ppm[bc] - 3.0).abs() < 0.1,
        "F2 {}",
        spec.f2_ppm[bc]
    );
    assert!(
        (spec.f1_ppm[br] - 10.0).abs() < 0.5,
        "F1 {}",
        spec.f1_ppm[br]
    );

    let fig = build_figure_2d(&spec, preset);
    assert!(!fig.contours.is_empty());
    assert!(!fig.contours[0].segments.is_empty());

    let svg = plotx_render::svg::export(&fig);
    assert!(svg.starts_with("<svg"));
    assert!(svg.contains("<path"), "contour path present");
    assert!(svg.contains("chemical shift"));
}

#[test]
fn stack_slice_exports_waterfall() {
    let mut data = synthetic_hsqc(3.0, 40.0, "ledbpgp2s");
    // A DOSY-style hint should recommend the stacked (pseudo-2D) layout.
    data.experiment = Some("ledbpgp2s".into());
    assert_eq!(recommend_preset(&data).layout(), Layout2D::Stack);

    let stack = match process_2d(
        &data,
        &Params2D {
            layout: Layout2D::Stack,
            ..Params2D::default()
        },
    ) {
        Processed2D::Stack(s) => s,
        Processed2D::Ft(_) => panic!("expected Stack"),
    };
    assert_eq!(stack.increments(), data.rows);

    let fig = build_stack_figure(&stack);
    assert!(!fig.series.is_empty());
    let svg = plotx_render::svg::export(&fig);
    assert!(svg.contains("<polyline"), "stack traces present");
}
