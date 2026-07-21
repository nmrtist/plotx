//! Pseudo-2D dataset tests (DOSY map extraction).

use super::*;
use num_complex::Complex64;
use plotx_io::{
    AxisSource, DiffusionMeta, Dim, Domain, NmrData2D, PseudoAxis, PseudoKind, QuadMode,
};

fn dim(nucleus: &str) -> Dim {
    Dim {
        spectral_width_hz: 4000.0,
        observe_freq_mhz: 400.0,
        carrier_ppm: 0.0,
        nucleus: nucleus.into(),
        group_delay: 0.0,
    }
}

// A synthetic DOSY array: one decaying resonance whose amplitude follows a
// Stejskal–Tanner decay with a known D across 16 linear gradient steps.
fn synthetic_dosy(d_true: f64) -> NmrData2D {
    let (cols, rows) = (256usize, 16usize);
    let meta = DiffusionMeta {
        gamma: 2.675_222e8,
        delta: 2e-3,
        big_delta: 0.1,
        tau: 0.0,
        shape_factor: 1.0 / 3.0,
    };
    let g: Vec<f64> = (0..rows)
        .map(|i| 0.02 + i as f64 * (0.28 - 0.02) / (rows as f64 - 1.0))
        .collect();
    let direct = dim("1H");
    let dt = 1.0 / direct.spectral_width_hz;
    let f_hz = 1.0 * direct.observe_freq_mhz; // 1 ppm
    let mut data = Vec::with_capacity(rows * cols);
    for &gr in &g {
        let att = (-d_true * meta.b_factor(gr)).exp();
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
        direct,
        indirect: dim("1H"),
        quad: QuadMode::Complex,
        indirect_conjugate: false,
        experiment: Some("bpp_ste_diffusion".into()),
        pseudo_axis: Some(PseudoAxis {
            name: "g".into(),
            kind: PseudoKind::Gradient,
            values: g,
            unit: "mT/m".into(),
            source: AxisSource::EmbeddedRamp,
        }),
        diffusion: Some(meta),
        nus: None,
        source: "synthetic DOSY".into(),
    }
}

#[test]
fn dataset_builds_dosy_map() {
    let d_true = 1.2e-9;
    let mut ds = Nmr2DDataset::load(synthetic_dosy(d_true));
    assert!(ds.is_pseudo());
    assert_eq!(ds.preset, Preset2D::Dosy);

    assert!(ds.build_dosy_map());
    assert_eq!(ds.display, PseudoDisplay::DosyMap);
    assert!(!ds.figure().contours.is_empty(), "DOSY map should contour");
}

#[test]
fn ordered_series_supports_region_analysis() {
    let series = Dataset::Nmr2D(Box::new(Nmr2DDataset::load(synthetic_dosy(1.2e-9))));
    assert!(series.supports_region_analysis());
    assert!(series.tool_groups().contains(&ToolGroup::RegionAnalysis));

    let mut without_ruler = synthetic_dosy(1.2e-9);
    without_ruler.pseudo_axis = None;
    let not_a_series = Dataset::Nmr2D(Box::new(Nmr2DDataset::load(without_ruler)));
    assert!(!not_a_series.supports_region_analysis());
    assert!(
        !not_a_series
            .tool_groups()
            .contains(&ToolGroup::RegionAnalysis)
    );
}

/// `supports_region_analysis` gates the Regions and Series Table commands, so it
/// must agree with the predicate `build_region_table` enforces. A dataset that
/// says yes and then yields no table would strand the user with regions drawn
/// and a button that silently does nothing.
#[test]
fn region_support_matches_what_the_table_builder_accepts() {
    let mut app = crate::state::PlotxApp::new_with_settings(crate::settings::Settings::default());
    let mut series = Nmr2DDataset::load(synthetic_dosy(1.2e-9));
    series.regions = vec![Region {
        id: 0,
        lo: 0.9,
        hi: 1.1,
        name: "peak".to_owned(),
        color: [200, 80, 80],
        metric: None,
    }];
    series.next_region_id = 1;
    app.doc.datasets.push(Dataset::Nmr2D(Box::new(series)));
    assert!(app.doc.datasets[0].supports_region_analysis());
    app.create_region_table(0);
    assert!(
        app.region_table_index(0).is_some(),
        "a supported series with regions must actually yield a table"
    );

    // Regions survive in saved projects, so a dataset can carry them without
    // being a series. The support predicate must reject it, which is what keeps
    // the Series Table command from offering a table that cannot be built.
    let mut ruler_less = synthetic_dosy(1.2e-9);
    ruler_less.pseudo_axis = None;
    let mut stale = Nmr2DDataset::load(ruler_less);
    stale.regions = vec![Region {
        id: 0,
        lo: 0.9,
        hi: 1.1,
        name: "peak".to_owned(),
        color: [200, 80, 80],
        metric: None,
    }];
    let mut app = crate::state::PlotxApp::new_with_settings(crate::settings::Settings::default());
    app.doc.datasets.push(Dataset::Nmr2D(Box::new(stale)));
    assert!(!app.doc.datasets[0].supports_region_analysis());
    app.create_region_table(0);
    assert!(
        app.region_table_index(0).is_none(),
        "an unsupported dataset must not produce a series table"
    );
}

#[test]
fn dataset_builds_ilt_dosy_map() {
    let mut ds = Nmr2DDataset::load(synthetic_dosy(1.2e-9));
    let params = IltParams {
        lambda: 1e-2,
        d_min: 1e-10,
        d_max: 1e-8,
        n_grid: 64,
    };
    assert!(ds.build_ilt_map(params), "ILT map should populate");
    assert!(matches!(ds.dosy_method, DosyMethod::Ilt(_)));
    assert_eq!(ds.display, PseudoDisplay::DosyMap);
    assert!(!ds.figure().contours.is_empty(), "ILT map should contour");
    // The per-column mono-exp path must still coexist.
    assert!(ds.build_dosy_map());
    assert!(matches!(ds.dosy_method, DosyMethod::MonoExp));
    assert!(ds.ilt_map.is_some(), "ILT result should remain cached");
}

/// Both maps can be cached at once, so the figure cache must be keyed by method.
/// A single shared slot would serve whichever figure was built last for whichever
/// method the display happens to select — an ILT contour labelled per-column DOSY.
#[test]
fn switching_dosy_method_serves_that_methods_figure() {
    let params = IltParams {
        lambda: 1e-2,
        d_min: 1e-10,
        d_max: 1e-8,
        n_grid: 64,
    };
    let mut ds = Nmr2DDataset::load(synthetic_dosy(1.2e-9));
    assert!(ds.build_dosy_map(), "per-column map should populate");
    assert!(ds.build_ilt_map(params), "ILT map should populate");
    assert!(ds.figure().title.starts_with("DOSY (ILT)"));

    // Switching back is what the DOSY method buttons do: flip the method and
    // rebuild. Both maps are still cached, so the per-column figure must come
    // back rather than the ILT figure that happened to be built last.
    ds.dosy_method = DosyMethod::MonoExp;
    let title = ds.figure().title;
    assert!(
        title.starts_with("DOSY —"),
        "per-column display served the wrong method's figure: {title}"
    );

    ds.dosy_method = DosyMethod::Ilt(params);
    assert!(ds.figure().title.starts_with("DOSY (ILT)"));
}

/// A NUS schedule mutates `data` while leaving the recipe untouched, so nothing in
/// `params` records that the cached base is void. Without the explicit flag, a
/// frequency-only edit arriving before the reconstruction lands would schedule a
/// re-apply from the pre-NUS base and strand the reconstruction forever.
#[test]
fn entering_a_nus_schedule_forces_a_retransform_until_a_base_lands() {
    let mut data = synthetic_dosy(1.2e-9);
    data.nus = Some(plotx_io::NusMeta {
        grid: data.rows * 2,
        acquired: data.rows,
        idx_base: 0,
        mode: String::new(),
        echo_antiecho: false,
        schedule: None,
    });
    let mut ds = Nmr2DDataset::load(data);
    assert!(!ds.base_stale);

    let rows = ds.data.rows;
    ds.set_nus_schedule(&(0..rows).collect::<Vec<_>>(), 0)
        .expect("a full in-grid schedule is valid");
    assert!(ds.base_stale, "the cached base no longer derives from data");

    ds.retransform();
    assert!(!ds.base_stale, "a fresh base clears the flag");
}
