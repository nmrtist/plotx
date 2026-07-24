use super::*;
use crate::state::{DatasetLineage, DerivationKind};
use crate::state::{MultipletPatternKind, PeakMark, PeakOrigin, StoredMultiplet};
use plotx_processing::Slice1D;

fn doublet_marked_app() -> PlotxApp {
    let ppm: Vec<f64> = (0..600).map(|i| 10.0 * i as f64 / 599.0).collect();
    let values = ppm.iter().map(|_| Complex64::new(0.0, 0.0)).collect();
    let slice = Slice1D {
        ppm,
        values,
        nucleus: "1H".to_owned(),
        observe_freq_mhz: 400.0,
        position_ppm: None,
    };
    let mut nmr = NmrDataset::from_slice(slice, "doublet".to_owned());
    for (id, x) in [(0u64, 2.0), (1u64, 2.0 + 7.0 / 400.0)] {
        nmr.peaks.marks.push(PeakMark {
            id,
            x,
            y: 10.0,
            origin: PeakOrigin::Manual,
            label: None,
        });
    }
    nmr.peaks.next_id = 2;
    let mut app = PlotxApp::new();
    app.doc.save_include_view_snapshots = false;
    app.doc.datasets.push(Dataset::Nmr(Box::new(nmr)));
    push_canvas(&mut app, 0, "spectrum", [120.0, 80.0]);
    app.focus_single(0);
    app.session.active_canvas = Some(0);
    app
}

#[test]
fn analyze_multiplets_classifies_marked_doublet() {
    let app = doublet_marked_app();
    let found = app.analyze_multiplets(0, 1.5, 2.5).expect("analysis runs");
    assert_eq!(found.len(), 1);
    let m = &found[0];
    assert_eq!(m.pattern, MultipletPatternKind::Doublet);
    assert!((m.j_values[0].hz - 7.0).abs() < 1e-9);
    assert!((m.center_ppm - (2.0 + 3.5 / 400.0)).abs() < 1e-9);
    assert_eq!(m.peak_ppm.len(), 2);
    assert!(m.peak_ppm[0] > m.peak_ppm[1]);
    assert_eq!(m.area, 0.0);
}

#[test]
fn analyze_multiplets_rejects_bad_targets() {
    let app = doublet_marked_app();
    assert!(app.analyze_multiplets(5, 0.0, 10.0).is_err());
    assert!(app.analyze_multiplets(0, 8.0, 9.0).is_err());
}

#[test]
fn apply_multiplet_analysis_stores_creates_table_and_undoes_as_one_step() {
    let mut app = doublet_marked_app();
    let canvases_before = app.doc.canvases.len();
    let found = app.analyze_multiplets(0, 1.5, 2.5).unwrap();

    app.apply_multiplet_analysis(0, found);

    let n = app.doc.datasets[0].as_nmr().unwrap();
    assert_eq!(n.multiplets.len(), 1);
    assert_eq!(n.next_multiplet_id, 1);
    assert_eq!(app.doc.datasets.len(), 2);
    assert_eq!(app.doc.canvases.len(), canvases_before + 1);
    let table = app.doc.datasets[1].as_table().unwrap();
    let rows = table.typed_rows(1, &[]).unwrap();
    assert_eq!(
        rows.columns[0].values,
        vec![plotx_data::ScalarValue::Float64(1.0)]
    );
    assert_eq!(rows.columns[0].schema.name, "multiplet");
    let names: Vec<&str> = table
        .series_bindings
        .iter()
        .map(|binding| {
            table
                .typed_state
                .envelope
                .revision
                .snapshot
                .schema
                .column(binding.value_column)
                .unwrap()
                .name
                .as_str()
        })
        .collect();
    assert_eq!(
        names,
        vec!["center (ppm)", "J1 (Hz)", "J2 (Hz)", "area", "peaks"]
    );
    assert_eq!(
        app.doc.datasets[1].lineage(),
        Some(&DatasetLineage::new(
            DerivationKind::MultipletTable,
            [app.doc.datasets[0].resource_id()]
        ))
    );

    app.undo();
    assert!(app.doc.datasets[0].multiplets().is_empty());
    assert_eq!(app.doc.datasets.len(), 1);
    assert_eq!(app.doc.canvases.len(), canvases_before);

    app.redo();
    assert_eq!(app.doc.datasets[0].multiplets().len(), 1);
    assert_eq!(app.doc.datasets.len(), 2);
}

#[test]
fn set_multiplets_applies_reverts_and_skips_noops() {
    let mut app = doublet_marked_app();
    let m = StoredMultiplet {
        id: 0,
        lo: 2.0,
        hi: 2.02,
        center_ppm: 2.01,
        pattern: MultipletPatternKind::Doublet,
        j_values: vec![],
        area: 1.0,
        peak_ppm: vec![2.02, 2.0],
    };
    app.execute_action(Action::set_multiplets(
        dataset_id(&app, 0),
        Vec::new(),
        vec![m.clone()],
    ));
    assert_eq!(app.doc.datasets[0].multiplets(), std::slice::from_ref(&m));

    app.undo();
    assert!(app.doc.datasets[0].multiplets().is_empty());
    app.redo();
    assert_eq!(app.doc.datasets[0].multiplets(), std::slice::from_ref(&m));

    assert!(
        Action::set_multiplets(dataset_id(&app, 0), vec![m.clone()], vec![m.clone()]).is_noop()
    );

    app.remove_multiplet(0, 0);
    assert!(app.doc.datasets[0].multiplets().is_empty());
    app.undo();
    assert_eq!(app.doc.datasets[0].multiplets().len(), 1);
}
