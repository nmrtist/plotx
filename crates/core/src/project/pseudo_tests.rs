use super::*;
use crate::PseudoDisplay;
use crate::state::{Dataset, Nmr2DDataset, PlotxApp};
use num_complex::Complex64;
use plotx_io::{
    AxisSource, DiffusionMeta, Dim, Domain, NmrData2D, PseudoAxis, PseudoKind, QuadMode,
};
use std::path::{Path, PathBuf};

fn temp_project(name: &str) -> PathBuf {
    let base = std::env::var_os("CARGO_TARGET_TMPDIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    base.join(format!("plotx-{name}-{}.plotx", std::process::id()))
}

fn synthetic_dosy_2d() -> NmrData2D {
    let (cols, rows) = (64usize, 8usize);
    let meta = DiffusionMeta {
        gamma: 2.675_222e8,
        delta: 2e-3,
        big_delta: 0.1,
        tau: 0.0,
        shape_factor: 1.0 / 3.0,
    };
    let direct = Dim {
        spectral_width_hz: 5000.0,
        observe_freq_mhz: 400.0,
        carrier_ppm: 5.0,
        nucleus: "1H".to_owned(),
        group_delay: 0.0,
    };
    let g: Vec<f64> = (0..rows)
        .map(|i| 0.02 + i as f64 * (0.28 - 0.02) / (rows as f64 - 1.0))
        .collect();
    let dt = 1.0 / direct.spectral_width_hz;
    let f_hz = direct.observe_freq_mhz;
    let mut data = Vec::with_capacity(rows * cols);
    for &gr in &g {
        let att = (-1.2e-9 * meta.b_factor(gr)).exp();
        for j in 0..cols {
            let t = j as f64 * dt;
            let decay = (-t / 0.2).exp();
            data.push(Complex64::from_polar(
                att * decay,
                std::f64::consts::TAU * f_hz * t,
            ));
        }
    }
    NmrData2D {
        data,
        rows,
        cols,
        domain: Domain::Time,
        direct: direct.clone(),
        indirect: direct,
        quad: QuadMode::Complex,
        indirect_conjugate: false,
        experiment: Some("bpp_ste_diffusion".to_owned()),
        pseudo_axis: Some(PseudoAxis {
            name: "g".to_owned(),
            kind: PseudoKind::Gradient,
            values: g,
            unit: "mT/m".to_owned(),
            source: AxisSource::EmbeddedRamp,
        }),
        diffusion: Some(meta),
        nus: None,
        source: "synthetic DOSY".to_owned(),
    }
}

fn inject_fit_curve_pseudo_extension(path: &Path) {
    let file = std::fs::File::open(path).unwrap();
    let mut zip = zip::ZipArchive::new(file).unwrap();
    let mut entries = Vec::new();
    for i in 0..zip.len() {
        let mut file = zip.by_index(i).unwrap();
        if file.is_dir() {
            continue;
        }
        let name = file.name().to_owned();
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes).unwrap();
        if name == "objects/recipe_000000/object.json" {
            let mut value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            value["extensions"]["plotx.pseudo"] = serde_json::json!({
                "display": "FitCurve",
                "fit": {
                    "region_ppm": [0.8, 1.2],
                    "kind": "Diffusion",
                    "value": 1.2e-9,
                    "sigma": 1.0e-11,
                    "r2": 0.999,
                    "points": [[20.0, 1.0], [280.0, 0.4]],
                    "ruler_unit": "mT/m"
                }
            });
            bytes = serde_json::to_vec_pretty(&value).unwrap();
        }
        entries.push((name, bytes));
    }
    drop(zip);

    let tmp = temporary_path(path);
    let file = std::fs::File::create(&tmp).unwrap();
    let mut out = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    for (name, bytes) in entries {
        out.start_file(name, options).unwrap();
        out.write_all(&bytes).unwrap();
    }
    out.finish().unwrap();
    std::fs::remove_file(path).unwrap();
    std::fs::rename(tmp, path).unwrap();
}

#[test]
fn project_load_ignores_stored_pseudo_fit_curve() {
    let mut app = PlotxApp::new();
    let ds = Nmr2DDataset::load(synthetic_dosy_2d());
    app.doc.datasets.push(Dataset::Nmr2D(Box::new(ds)));

    let path = temp_project("pseudo-fit-curve");
    let _ = std::fs::remove_file(&path);
    save_project(&app, &path, false).unwrap();
    inject_fit_curve_pseudo_extension(&path);
    let loaded = load_project(&path).unwrap();
    let _ = std::fs::remove_file(&path);

    let Dataset::Nmr2D(n) = &loaded.doc.datasets[0] else {
        panic!("expected a 2D NMR dataset");
    };
    assert_eq!(n.display, PseudoDisplay::Stack);
    assert!(n.is_pseudo());
    assert_eq!(n.data.rows, 8);
    assert_eq!(n.data.cols, 64);
    assert!(n.data.diffusion.is_some());
}
