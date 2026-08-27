use super::tests::synthetic_1d;
use super::*;
use crate::state::{
    CraftRunId, FieldPayload, NewAnalysisReport, ReportKindId, ReportSource, ReportStatus,
    StoredCraftRun,
};
use plotx_processing::craft::{
    CraftComponent, CraftComponentId, CraftDiagnostics, CraftModelingWindowDiagnostic,
    CraftParamOverrides, CraftParams, CraftReference, CraftRegionId, CraftRegionRatio,
    CraftRegionSummary, CraftReportDefinition, CraftResult, CraftRunStatus,
    CraftStabilityDiagnostics, CraftStabilityMetric, CraftStabilityRegion,
    resolve_craft_invocation,
};

fn sample_run(data: &NmrData) -> StoredCraftRun {
    StoredCraftRun::from_result(
        CraftRunId(4),
        data,
        resolve_craft_invocation(
            data,
            CraftReference::new(data.carrier_ppm, 0.25),
            &CraftParamOverrides::from_params(CraftParams::conventional()),
            None,
        ),
        Some(CraftRunId(2)),
        CraftResult {
            components: vec![
                CraftComponent {
                    id: CraftComponentId(0),
                    region: CraftRegionId(0),
                    frequency_hz: -1200.0,
                    chemical_shift_ppm: 2.0,
                    amplitude_t0: 1.0,
                    phase_rad: 0.2,
                    decay_rate_s_inv: 2.5,
                    linewidth_hz: 2.5 / std::f64::consts::PI,
                    amplitude_to_noise: 20.0,
                    amplitude_std: Some(0.01),
                    frequency_std_hz: Some(0.02),
                    linewidth_std_hz: Some(0.03),
                    phase_std_rad: Some(0.01),
                },
                CraftComponent {
                    id: CraftComponentId(1),
                    region: CraftRegionId(1),
                    frequency_hz: 1200.0,
                    chemical_shift_ppm: 6.0,
                    amplitude_t0: 2.0,
                    phase_rad: 0.2,
                    decay_rate_s_inv: 2.5,
                    linewidth_hz: 2.5 / std::f64::consts::PI,
                    amplitude_to_noise: 40.0,
                    amplitude_std: Some(0.01),
                    frequency_std_hz: Some(0.02),
                    linewidth_std_hz: Some(0.03),
                    phase_std_rad: Some(0.01),
                },
            ],
            region_summaries: vec![
                CraftRegionSummary {
                    region: CraftRegionId(0),
                    start_ppm: 1.8,
                    end_ppm: 1.9,
                    component_count: 1,
                    scalar_amplitude_sum_t0: 1.0,
                    coherent_amplitude_t0: 1.0,
                },
                CraftRegionSummary {
                    region: CraftRegionId(1),
                    start_ppm: 6.3,
                    end_ppm: 6.5,
                    component_count: 1,
                    scalar_amplitude_sum_t0: 2.0,
                    coherent_amplitude_t0: 2.0,
                },
            ],
            region_ratio: Some(CraftRegionRatio {
                numerator: CraftRegionId(0),
                denominator: CraftRegionId(1),
                value: 0.5,
            }),
            diagnostics: CraftDiagnostics {
                status: CraftRunStatus::Complete,
                noise_sigma: 0.05,
                residual_rss: 1.0,
                normalized_residual: 0.02,
                maximum_condition_number: Some(4.0),
                modeling_windows: vec![
                    CraftModelingWindowDiagnostic {
                        retention_band_hz: (-1300.0, -1100.0),
                        modeling_band_hz: (-1320.0, -1080.0),
                        decimation_factor: 4,
                        modeled_sample_count: 256,
                        evaluated_model_orders: 7,
                        selected_model_order: 1,
                        training_bic: Some(-25.0),
                        condition_number: Some(3.0),
                        modeled_duration_s: 1.0,
                        training_normalized_residual: 0.01,
                        validation_normalized_residual: 0.02,
                    },
                    CraftModelingWindowDiagnostic {
                        retention_band_hz: (1100.0, 1300.0),
                        modeling_band_hz: (1080.0, 1320.0),
                        decimation_factor: 4,
                        modeled_sample_count: 256,
                        evaluated_model_orders: 7,
                        selected_model_order: 1,
                        training_bic: Some(-20.0),
                        condition_number: Some(4.0),
                        modeled_duration_s: 1.0,
                        training_normalized_residual: 0.01,
                        validation_normalized_residual: 0.02,
                    },
                ],
                warnings: Vec::new(),
                stability: CraftStabilityDiagnostics {
                    delta_ppm: 0.016,
                    regions: vec![CraftStabilityRegion {
                        region: CraftRegionId(0),
                        metric: CraftStabilityMetric {
                            median: 1.0,
                            minimum: 0.999,
                            maximum: 1.001,
                            relative_dispersion: 0.002,
                        },
                        component_count_min: 1,
                        component_count_max: 1,
                        model_order_min: 1,
                        model_order_max: 1,
                    }],
                    ratio: None,
                    passed: true,
                    skipped: Vec::new(),
                },
            },
            synthetic_fid: Vec::new(),
            residual_fid: Vec::new(),
        },
    )
}

#[test]
fn craft_runs_survive_project_roundtrip_and_reseed_ids() {
    let data = synthetic_1d();
    let mut dataset = NmrDataset::load(data.clone());
    dataset.craft_runs.push(sample_run(&data));
    dataset.reconcile_craft_fields();
    dataset.next_craft_run_id = 5;
    let mut app = crate::state::PlotxApp::new();
    app.doc.datasets.push(Dataset::Nmr(Box::new(dataset)));

    let path = super::tests::temp_project("craft-run");
    let _ = std::fs::remove_file(&path);
    save_project(&app, &path, false).unwrap();
    let loaded = load_project(&path).unwrap();
    let _ = std::fs::remove_file(&path);

    let Dataset::Nmr(dataset) = &loaded.doc.datasets[0] else {
        panic!("NMR dataset survives round-trip");
    };
    assert_eq!(dataset.craft_runs, vec![sample_run(&data)]);
    assert_eq!(dataset.next_craft_run_id, 5);
}

#[test]
fn recipe_without_craft_runs_is_rejected() {
    let data = synthetic_1d();
    let mut dataset = NmrDataset::load(data.clone());
    dataset.craft_runs.push(sample_run(&data));
    dataset.next_craft_run_id = 5;
    let recipe = RecipeObject {
        id: "recipe_000000".to_owned(),
        role: "recipe".to_owned(),
        classification: Classification {
            domain: "spectroscopy".to_owned(),
            technique: Some("nmr".to_owned()),
            object: "recipe".to_owned(),
        },
        input: "data_000000".to_owned(),
        parameters: RecipeParameters::default(),
        extensions: serde_json::json!({
            "plotx.analysis": {
                "peaks": crate::state::PeakSet::default(),
                "integrals": [],
                "line_fits": [],
                "multiplets": []
            }
        }),
    };

    let error = apply_1d_recipe(&mut dataset, &recipe).unwrap_err();

    assert!(
        error
            .to_string()
            .contains("missing required field craft_runs")
    );
    assert_eq!(dataset.craft_runs, vec![sample_run(&data)]);
    assert_eq!(dataset.next_craft_run_id, 5);
}

#[test]
fn unavailable_craft_diagnostics_survive_project_roundtrip() {
    let data = synthetic_1d();
    let mut run = sample_run(&data);
    run.components[0].amplitude_std = None;
    run.components[0].frequency_std_hz = None;
    run.components[0].linewidth_std_hz = None;
    run.components[0].phase_std_rad = None;
    run.diagnostics.maximum_condition_number = None;
    run.diagnostics.modeling_windows[0].training_bic = None;
    let mut dataset = NmrDataset::load(data);
    dataset.craft_runs.push(run.clone());
    dataset.reconcile_craft_fields();
    let mut app = crate::state::PlotxApp::new();
    app.doc.datasets.push(Dataset::Nmr(Box::new(dataset)));
    let path = super::tests::temp_project("craft-unavailable-diagnostics");
    let _ = std::fs::remove_file(&path);

    save_project(&app, &path, false).unwrap();
    let loaded = load_project(&path).unwrap();
    let _ = std::fs::remove_file(&path);

    let Dataset::Nmr(dataset) = &loaded.doc.datasets[0] else {
        panic!("NMR dataset survives round-trip");
    };
    assert_eq!(dataset.craft_runs, vec![run]);
}

#[test]
fn only_stable_complete_runs_create_quantitative_reports() {
    let data = synthetic_1d();
    let definition = CraftReportDefinition::default();
    let stable = sample_run(&data);
    assert!(stable.amplitude_report(definition.clone()).is_ok());

    let mut needs_review = stable.clone();
    needs_review.diagnostics.status = CraftRunStatus::Partial;
    needs_review.diagnostics.stability.passed = false;
    assert!(
        needs_review
            .amplitude_report(definition)
            .unwrap_err()
            .contains("NeedsReview")
    );
    assert!(crate::state::craft_component_table(&needs_review).is_ok());
}

#[test]
fn report_status_tracks_stability_and_source_availability() {
    let data = synthetic_1d();
    let mut dataset = NmrDataset::load(data.clone());
    let mut run = sample_run(&data);
    run.provenance.invocation.reference = dataset.craft_reference();
    let definition = CraftReportDefinition::default();
    let snapshot = run.amplitude_report(definition.clone()).unwrap();
    let source = ReportSource {
        dataset: dataset.resource_id,
        craft_run: run.id,
    };
    dataset.craft_runs.push(run);
    dataset.reconcile_craft_fields();
    let mut app = crate::state::PlotxApp::new();
    app.doc.datasets.push(Dataset::Nmr(Box::new(dataset)));
    let report = app.doc.create_report(NewAnalysisReport {
        name: "CRAFT stability status".to_owned(),
        kind: ReportKindId::new("craft_amplitude"),
        source,
        definition: serde_json::to_value(definition).unwrap(),
        snapshot: serde_json::to_value(snapshot).unwrap(),
        source_fingerprint: crate::state::craft_input_sha256(&data),
        schema_version: 1,
    });

    assert_eq!(
        app.doc.report(report).unwrap().status(&app.doc),
        ReportStatus::Available
    );
    app.doc.datasets[0].as_nmr_mut().unwrap().craft_runs[0]
        .diagnostics
        .stability
        .passed = false;
    assert_eq!(
        app.doc.report(report).unwrap().status(&app.doc),
        ReportStatus::NeedsReview
    );
    app.doc.datasets[0].as_nmr_mut().unwrap().craft_runs.clear();
    assert_eq!(
        app.doc.report(report).unwrap().status(&app.doc),
        ReportStatus::Unavailable
    );
}

#[test]
fn stability_snapshot_survives_project_roundtrip() {
    let data = synthetic_1d();
    let mut run = sample_run(&data);
    run.diagnostics.stability.skipped = vec!["contract: overlapping regions".to_owned()];
    run.diagnostics.stability.ratio = Some(CraftStabilityMetric {
        median: 0.5,
        minimum: 0.498,
        maximum: 0.502,
        relative_dispersion: 0.008,
    });
    let mut dataset = NmrDataset::load(data);
    dataset.craft_runs.push(run.clone());
    dataset.reconcile_craft_fields();
    let mut app = crate::state::PlotxApp::new();
    app.doc.datasets.push(Dataset::Nmr(Box::new(dataset)));
    let path = super::tests::temp_project("craft-stability");
    let _ = std::fs::remove_file(&path);

    save_project(&app, &path, false).unwrap();
    let loaded = load_project(&path).unwrap();
    let _ = std::fs::remove_file(&path);

    assert_eq!(
        loaded.doc.datasets[0].as_nmr().unwrap().craft_runs[0]
            .diagnostics
            .stability,
        run.diagnostics.stability
    );
}

#[test]
fn craft_component_table_link_and_board_visibility_survive_roundtrip() {
    let data = synthetic_1d();
    let mut dataset = NmrDataset::load(data.clone());
    dataset.craft_runs.push(sample_run(&data));
    dataset.reconcile_craft_fields();
    let mut app = crate::state::PlotxApp::new();
    app.doc.datasets.push(Dataset::Nmr(Box::new(dataset)));
    let table = app
        .materialize_craft_component_table(0, CraftRunId(4))
        .unwrap();
    let table_id = app.doc.datasets[table].resource_id();
    let path = super::tests::temp_project("craft-component-table-link");
    let _ = std::fs::remove_file(&path);

    save_project(&app, &path, false).unwrap();
    let loaded = load_project(&path).unwrap();
    let _ = std::fs::remove_file(&path);

    assert_eq!(
        loaded.doc.datasets[0].as_nmr().unwrap().craft_runs[0].component_table,
        Some(table_id)
    );
    assert!(
        !loaded.doc.datasets[1]
            .as_table()
            .unwrap()
            .board_sheet_visible()
    );
}

#[test]
fn craft_result_canvas_round_trips_binding_fields_and_linked_x_axis() {
    let data = synthetic_1d();
    let mut dataset = NmrDataset::load(data.clone());
    let dataset_id = dataset.resource_id;
    dataset.store_craft_run(sample_run(&data));
    let mut app = crate::state::PlotxApp::new();
    app.doc.datasets.push(Dataset::Nmr(Box::new(dataset)));

    let canvas_id = app
        .open_craft_result_canvas(dataset_id, CraftRunId(4))
        .unwrap();
    assert_eq!(app.doc.canvases.len(), 1);
    assert_eq!(app.doc.canvases[0].objects.len(), 3);
    assert_eq!(app.doc.canvases[0].x_viewport_links[0].members.len(), 3);
    assert!(
        app.doc.canvases[0]
            .panels
            .iter()
            .all(|panel| (panel.frame.x - 9.0).abs() < f32::EPSILON)
    );
    assert!(app.doc.canvases[0].panels.windows(2).all(|panels| {
        (panels[1].frame.y - panels[0].frame.y - panels[0].frame.height - 5.0).abs() < 1e-4
    }));
    assert!(
        app.doc.canvases[0]
            .panels
            .iter()
            .all(|panel| panel.label.position == [2.0, 2.0])
    );
    assert_eq!(
        app.open_craft_result_canvas(dataset_id, CraftRunId(4))
            .unwrap(),
        canvas_id
    );
    assert_eq!(app.doc.canvases.len(), 1);

    let members = app.doc.canvases[0].x_viewport_links[0].members.clone();
    let source = app.doc.canvases[0]
        .object(members[0])
        .unwrap()
        .plot()
        .unwrap()
        .viewport
        .clone();
    let mut zoomed = source.clone();
    let span = source.full_x.span();
    zoomed.view_x = crate::state::AxisRange::new(
        source.full_x.min + span * 0.25,
        source.full_x.max - span * 0.25,
    );
    app.commit_object_viewport(0, members[0], source.clone(), zoomed.clone());
    assert!(members.iter().all(|member| {
        app.doc.canvases[0]
            .object(*member)
            .unwrap()
            .plot()
            .unwrap()
            .viewport
            .view_x
            == zoomed.view_x
    }));
    app.undo();
    assert!(members.iter().all(|member| {
        app.doc.canvases[0]
            .object(*member)
            .unwrap()
            .plot()
            .unwrap()
            .viewport
            .view_x
            == source.view_x
    }));

    let path = super::tests::temp_project("craft-result-canvas");
    let _ = std::fs::remove_file(&path);
    save_project(&app, &path, false).unwrap();
    let mut loaded = load_project(&path).unwrap();
    let _ = std::fs::remove_file(&path);

    let canvas = &loaded.doc.canvases[0];
    assert_eq!(
        canvas.analysis_binding,
        Some(crate::state::CanvasAnalysisBinding::Craft {
            dataset: dataset_id,
            run: CraftRunId(4),
        })
    );
    assert_eq!(canvas.x_viewport_links.len(), 1);
    assert_eq!(canvas.x_viewport_links[0].members.len(), 3);
    assert!(canvas.objects.iter().all(|object| object.plot().is_some()));

    let deleted = canvas.x_viewport_links[0].members[0];
    let action = crate::actions::Action::delete_object(&loaded, 0, deleted).unwrap();
    loaded.execute_action(action);
    assert_eq!(loaded.doc.canvases[0].x_viewport_links[0].members.len(), 2);
    loaded.undo();
    assert_eq!(loaded.doc.canvases[0].x_viewport_links[0].members.len(), 3);
}

#[test]
fn craft_group_field_uses_requested_reconstruction_duration() {
    let data = synthetic_1d();
    let mut run = sample_run(&data);
    let requested_points = data.points.len() * 2;
    let mut params = CraftParams::ssfp();
    params.reconstruction_duration_s = Some(requested_points as f64 / data.spectral_width_hz);
    run.provenance.invocation = resolve_craft_invocation(
        &data,
        CraftReference::acquisition(&data),
        &CraftParamOverrides::from_params(params),
        None,
    );
    assert_eq!(
        run.provenance.invocation.derived_plan.reconstruction_points,
        requested_points
    );
    let mut dataset = NmrDataset::load(data);
    dataset.store_craft_run(run);
    let dataset = Dataset::Nmr(Box::new(dataset));
    let group_field = dataset
        .field_descriptors()
        .into_iter()
        .find(|field| field.local_id.contains(".groups.magnitude"))
        .unwrap()
        .id;
    let FieldPayload::Curve1D(curve) = dataset.field_payload(group_field).unwrap() else {
        panic!("CRAFT group field is a curve");
    };

    assert_eq!(curve.x.len(), requested_points);
    assert_eq!(curve.values.len(), requested_points);
}
