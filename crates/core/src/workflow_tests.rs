use super::*;
use num_complex::Complex64;
use plotx_io::Domain;

fn acquisition() -> Acquisition {
    Acquisition::Nmr(
        plotx_io::NmrData {
            points: (0..8)
                .map(|i| Complex64::new(1.0 / (1.0 + (i as f64 - 3.0).powi(2)), 0.0))
                .collect(),
            domain: Domain::Frequency,
            spectral_width_hz: 4_000.0,
            observe_freq_mhz: 400.0,
            carrier_ppm: 4.7,
            nucleus: "1H".to_owned(),
            source: "sample.dx".to_owned(),
            group_delay: 0.0,
        }
        .try_into()
        .unwrap(),
    )
}

fn homonuclear_2d_acquisition() -> Acquisition {
    let dimension = plotx_io::Dim {
        spectral_width_hz: 4_000.0,
        observe_freq_mhz: 400.0,
        carrier_ppm: 4.7,
        nucleus: "1H".to_owned(),
        group_delay: 0.0,
    };
    let source = plotx_io::nmr_series::NmrSeriesSource::try_from(plotx_io::NmrData2D {
        data: (0..16)
            .map(|i| Complex64::new(1.0 / (1.0 + (i as f64 - 5.0).powi(2)), 0.0))
            .collect(),
        rows: 4,
        cols: 4,
        domain: Domain::Frequency,
        direct: dimension.clone(),
        indirect: dimension,
        quad: plotx_io::QuadMode::Complex,
        indirect_conjugate: false,
        experiment: Some("cosy".to_owned()),
        pseudo_axis: None,
        diffusion: None,
        nus: None,
        source: "cosy".to_owned(),
    })
    .unwrap();
    Acquisition::Nmr(source.source_dataset().clone())
}

#[test]
fn canonical_conversion_and_default_canvas_share_dataset_identity() {
    let (dataset, source) = dataset_from_acquisition(acquisition()).unwrap();
    assert_eq!(dataset.kind_label(), "NMR 1D");
    let canvas = build_default_canvas(&dataset, &source);
    assert_eq!(canvas.dataset_ids(), vec![dataset.resource_id()]);
    assert_eq!(canvas.objects.len(), 1);
    assert_eq!(canvas.panels.len(), 1);
    assert_eq!(canvas.panels[0].item_order, vec![canvas.objects[0].id]);
    assert_eq!(
        canvas.panel_letter(canvas.objects[0].id).as_deref(),
        Some("a")
    );
    assert!(canvas.panels[0].note.is_empty());
    assert!(canvas.panel_notes().is_empty());
    assert!(crate::state::document_items(&canvas).iter().any(|item| {
        matches!(
            item,
            plotx_render::DocumentItem::PanelLabel { visible: false, .. }
        )
    }));
}

#[test]
fn import_preference_seeds_one_persistent_plot_override() {
    for (preference, expected) in [(true, true), (false, false)] {
        let (dataset, source) = dataset_from_acquisition_with_equal_scale_preference(
            homonuclear_2d_acquisition(),
            preference,
        )
        .unwrap();
        let canvas = build_default_canvas(&dataset, &source);
        let plot = canvas.objects[0].plot().expect("default plot");
        assert_eq!(plot.axis_overrides.lock_aspect, Some(expected));
        assert_eq!(plot.figure().lock_aspect, expected);
    }
}

#[test]
fn inspection_contract_reports_canonical_shape_and_domain() {
    let report = inspection_report(
        DataFormat::Nmr(plotx_io::NmrFormat::JcampDx1D),
        &Provenance {
            selected_path: "sample.dx".into(),
            data_path: "sample.dx".into(),
            parameter_paths: Vec::new(),
            companion_paths: Vec::new(),
        },
        &[],
        &acquisition(),
    );
    assert_eq!(report.schema, INSPECTION_SCHEMA);
    assert_eq!(report.dimension.count, 1);
    assert_eq!(report.dimension.shape, vec![8]);
    assert_eq!(report.domain, "frequency");
}
