use super::*;
use crate::state::{
    ACS_DOUBLE_COLUMN, AxisRange, CanvasDocument, CanvasSizeUnit, DEFAULT_CANVAS_SIZE_MM,
    NATURE_SINGLE_COLUMN, NmrDataset, PAPER_A4, PRESENTATION_16X9, matching_preset,
};

mod align;
mod arithmetic;
mod authoring;
mod board;
mod composite;
mod integral_curve;
mod interaction;
mod linefit;
mod more;
mod multiplet;
mod scheme_apply;
mod stack;
mod tiling;
use num_complex::Complex64;
use plotx_io::{Domain, NmrData};
use std::f64::consts::TAU;

fn synthetic_1d() -> NmrData {
    let npoints = 256;
    let sw = 4000.0;
    let obs = 400.0;
    let carrier = 5.0;
    let dt = 1.0 / sw;
    let points = (0..npoints)
        .map(|k| {
            let t = k as f64 * dt;
            let decay = (-t / 0.25).exp();
            let freq_hz = (2.0 - carrier) * obs;
            Complex64::from_polar(decay, TAU * freq_hz * t)
        })
        .collect();
    NmrData {
        points,
        domain: Domain::Time,
        spectral_width_hz: sw,
        observe_freq_mhz: obs,
        carrier_ppm: carrier,
        nucleus: "1H".to_owned(),
        source: "synthetic".to_owned(),
        group_delay: 0.0,
    }
}

pub(super) fn sample_app() -> PlotxApp {
    let mut app = PlotxApp::new();
    app.doc.save_include_view_snapshots = false;
    app.doc
        .datasets
        .push(Dataset::Nmr(Box::new(NmrDataset::load(synthetic_1d()))));
    push_canvas(&mut app, 0, "sample canvas", [120.0, 80.0]);
    app.focus_single(0);
    app.session.active_canvas = Some(0);
    app
}

pub(super) fn push_canvas(app: &mut PlotxApp, dataset: usize, name: &str, size_mm: [f32; 2]) {
    let mut canvas = CanvasDocument::new(name.to_owned(), size_mm);
    let [w, h] = canvas.size_pt();
    let id = canvas.allocate_object_id();
    let object = app.build_plot_object(
        dataset,
        crate::state::ObjectFrame::new(0.0, 0.0, w, h),
        id,
        "Plot 1".to_owned(),
    );
    canvas.selected_object = Some(id);
    canvas.objects.push(object);
    app.doc.canvases.push(canvas);
}

fn first_plot(app: &PlotxApp) -> &crate::state::PlotObject {
    app.doc.canvases[0].objects[0].plot().unwrap()
}

fn data_target(app: &PlotxApp, ci: usize) -> Option<crate::state::ObjectId> {
    if !app.session.tool.is_data_tool() || app.session.active_canvas != Some(ci) {
        return None;
    }
    app.doc.canvases.get(ci)?.selected_plot_object_id()
}

#[test]
fn data_tool_target_requires_data_verb_and_selected_plot() {
    use crate::state::Tool;
    let mut app = sample_app();
    let id = app.doc.canvases[0].objects[0].id;

    app.set_tool(Tool::Select);
    app.doc.canvases[0].selected_object = Some(id);
    assert_eq!(data_target(&app, 0), None);

    app.set_tool(Tool::BrowseZoom);
    app.doc.canvases[0].selected_object = None;
    assert_eq!(data_target(&app, 0), None);

    app.select_object(0, id);
    assert_eq!(data_target(&app, 0), Some(id));
}

#[test]
fn insert_dataset_new_canvas_does_not_select_object() {
    let mut app = PlotxApp::new();
    app.doc.save_include_view_snapshots = false;
    let dataset = Dataset::Nmr(Box::new(NmrDataset::load(synthetic_1d())));

    app.execute_action(Action::insert_dataset_with_default_canvas(
        &app,
        dataset,
        "new canvas".to_owned(),
        DEFAULT_CANVAS_SIZE_MM,
    ));

    assert_eq!(app.doc.canvases.len(), 1);
    assert_eq!(app.doc.canvases[0].objects.len(), 1);
    assert_eq!(app.doc.canvases[0].selected_object, None);
    assert_eq!(
        app.doc.canvases[0].active_plot_object_id(),
        Some(app.doc.canvases[0].objects[0].id)
    );
    assert_eq!(app.active_dataset(), Some(0));
    assert_eq!(app.doc.canvases[0].size_mm, DEFAULT_CANVAS_SIZE_MM);
}

#[test]
fn insert_dataset_existing_canvas_does_not_select_inserted_object() {
    let mut app = sample_app();
    app.doc.canvases[0].selected_object = None;
    let inserted_id = app.doc.canvases[0].next_object_id;
    let dataset_index = app.doc.datasets.len();
    let dataset = Dataset::Nmr(Box::new(NmrDataset::load(synthetic_1d())));

    app.execute_action(Action::InsertDatasetWithCanvas {
        dataset_index,
        canvas_index: app.doc.canvases.len(),
        canvas_resource_id: uuid::Uuid::new_v4().to_string(),
        dataset: Box::new(dataset),
        canvas_name: "unused".to_owned(),
        size_mm: DEFAULT_CANVAS_SIZE_MM,
        active_canvas_before: app.session.active_canvas,
        active_dataset_before: app.active_dataset(),
        inserted_into_existing_canvas: Some(0),
        inserted_object_id: Some(inserted_id),
    });

    assert_eq!(app.doc.canvases[0].objects.len(), 2);
    assert_eq!(app.doc.canvases[0].selected_object, None);
    app.doc.canvases[0].selected_object = Some(inserted_id);

    app.undo();
    assert_eq!(app.doc.canvases[0].objects.len(), 1);
    assert_eq!(app.doc.canvases[0].selected_object, None);
}

#[test]
fn canvas_size_presets_and_pixel_conversion_are_stable() {
    assert_eq!(DEFAULT_CANVAS_SIZE_MM, NATURE_SINGLE_COLUMN.size_mm());
    assert_eq!(PRESENTATION_16X9.size_mm(), [254.0, 142.875]);
    assert_eq!(PAPER_A4.size_mm(), [210.0, 297.0]);
    assert_eq!(
        matching_preset([177.8, 120.0], None),
        Some(&ACS_DOUBLE_COLUMN)
    );
    assert!((CanvasSizeUnit::Pixel.from_mm(25.4) - 96.0).abs() < 0.001);
    assert!((CanvasSizeUnit::Pixel.to_mm(96.0) - 25.4).abs() < 0.001);
}

#[test]
fn processing_undo_redo_rebuilds_spectrum_and_canvas() {
    use plotx_processing::{PhaseParams, StepKind};
    let phase0_of = |app: &PlotxApp| {
        app.doc.datasets[0]
            .as_nmr()
            .unwrap()
            .pipeline
            .steps
            .iter()
            .find_map(|s| match &s.kind {
                StepKind::Phase(p) => Some((p.phase0, p.auto.is_some())),
                _ => None,
            })
            .unwrap()
    };

    let mut app = sample_app();
    let original_y = first_plot(&app).figure.series[0].points[3][1];
    let (_, original_auto) = phase0_of(&app);
    let before = DatasetProcessingState::from_dataset(&app.doc.datasets[0]);
    let mut after = before.clone();
    if let DatasetProcessingState::Nmr { pipeline, .. } = &mut after {
        for step in &mut pipeline.steps {
            if let StepKind::Phase(p) = &mut step.kind {
                *p = PhaseParams {
                    phase0: 0.5,
                    ..PhaseParams::MANUAL_ZERO
                };
            }
        }
    }

    app.execute_action(Action::update_dataset_processing(0, before, after));
    let edited_y = first_plot(&app).figure.series[0].points[3][1];
    assert_ne!(edited_y, original_y);
    assert_eq!(phase0_of(&app), (0.5, false));

    app.undo();
    assert_eq!(phase0_of(&app).1, original_auto);
    assert_eq!(first_plot(&app).figure.series[0].points[3][1], original_y);

    app.redo();
    assert_eq!(phase0_of(&app), (0.5, false));
    assert_eq!(first_plot(&app).figure.series[0].points[3][1], edited_y);
}

#[test]
fn phase_editor_session_is_one_undo_step() {
    use plotx_processing::StepKind;

    let mut app = sample_app();
    let phase_id = app.doc.datasets[0]
        .axis_pipeline(crate::state::PhaseAxis::Direct)
        .unwrap()
        .steps
        .iter()
        .find(|step| matches!(step.kind, StepKind::Phase(_)))
        .unwrap()
        .id;
    let before = DatasetProcessingState::from_dataset(&app.doc.datasets[0]);

    app.session.ui.proc_expanded_step = Some(phase_id);
    app.sync_phase_interaction();
    assert!(app.session.ui.processing_session.is_some());

    let params = app.doc.datasets[0]
        .phase_params_mut(crate::state::PhaseAxis::Direct)
        .unwrap();
    params.auto = None;
    params.phase0 = 0.25;
    app.apply_dataset_edit(0);

    // A sidebar-style commit joins the same session as the live canvas edit.
    let widget_before = DatasetProcessingState::from_dataset(&app.doc.datasets[0]);
    let mut widget_after = widget_before.clone();
    if let DatasetProcessingState::Nmr { pipeline, .. } = &mut widget_after {
        let StepKind::Phase(params) = &mut pipeline
            .steps
            .iter_mut()
            .find(|step| matches!(step.kind, StepKind::Phase(_)))
            .unwrap()
            .kind
        else {
            unreachable!();
        };
        params.phase0 = 0.75;
    }
    app.commit_processing_edit(0, widget_before, widget_after);
    assert!(
        app.session.undo_stack.is_empty(),
        "live previews stay out of history"
    );

    app.session.ui.proc_expanded_step = None;
    app.sync_phase_interaction();
    assert_eq!(app.session.undo_stack.len(), 1);
    assert_eq!(
        app.doc.datasets[0]
            .phase_params_mut(crate::state::PhaseAxis::Direct)
            .unwrap()
            .phase0,
        0.75
    );

    app.undo();
    assert_eq!(
        DatasetProcessingState::from_dataset(&app.doc.datasets[0]),
        before
    );
}

#[test]
fn viewport_undo_redo_keeps_figure_axes_in_sync() {
    let mut app = sample_app();
    let object_id = app.doc.canvases[0].objects[0].id;
    let before = first_plot(&app).viewport.clone();
    let mut after = before.clone();
    after.view_x = AxisRange::new(
        before.full_x.min,
        before.full_x.min + before.full_x.span() * 0.4,
    );
    after.auto_y = false;

    app.execute_action(Action::set_object_viewport(
        0,
        object_id,
        before.clone(),
        after.clone(),
    ));
    assert_eq!(first_plot(&app).viewport.view_x, after.view_x);
    assert_eq!(first_plot(&app).figure.x.min, after.view_x.min);

    app.undo();
    assert_eq!(first_plot(&app).viewport.view_x, before.view_x);
    assert_eq!(first_plot(&app).figure.x.min, before.view_x.min);

    app.redo();
    assert_eq!(first_plot(&app).viewport.view_x, after.view_x);
    assert_eq!(first_plot(&app).figure.x.max, after.view_x.max);
}

fn size_state(size_mm: [f32; 2], preset_id: Option<&str>) -> PageSizeState {
    PageSizeState {
        size_mm,
        preset_id: preset_id.map(str::to_owned),
    }
}

#[test]
fn canvas_size_undo_redo_rebuilds_figure_size() {
    let mut app = sample_app();
    let frame_before = app.doc.canvases[0].objects[0].frame;
    app.execute_action(Action::set_canvas_size(
        0,
        size_state([120.0, 80.0], None),
        size_state([180.0, 90.0], None),
    ));
    assert_eq!(app.doc.canvases[0].size_mm, [180.0, 90.0]);
    assert_eq!(app.doc.canvases[0].objects[0].frame, frame_before);

    app.undo();
    assert_eq!(app.doc.canvases[0].size_mm, [120.0, 80.0]);
    assert_eq!(app.doc.canvases[0].objects[0].frame, frame_before);
}

#[test]
fn canvas_size_preset_identity_survives_undo_and_redo() {
    let mut app = sample_app();
    let before = size_state(app.doc.canvases[0].size_mm, None);
    // 183 mm is both Science full width and Nature double column; only the
    // stored preset id keeps the user's choice.
    app.execute_action(Action::set_canvas_size(
        0,
        before.clone(),
        size_state([183.0, 100.0], Some("science-3col")),
    ));
    assert_eq!(
        app.doc.canvases[0].size_preset_id.as_deref(),
        Some("science-3col")
    );

    app.undo();
    assert_eq!(app.doc.canvases[0].size_mm, before.size_mm);
    assert_eq!(app.doc.canvases[0].size_preset_id, None);

    app.redo();
    assert_eq!(app.doc.canvases[0].size_mm, [183.0, 100.0]);
    assert_eq!(
        app.doc.canvases[0].size_preset_id.as_deref(),
        Some("science-3col")
    );
    assert_eq!(
        matching_preset(
            app.doc.canvases[0].size_mm,
            app.doc.canvases[0].size_preset_id.as_deref()
        )
        .map(|p| p.id),
        Some("science-3col")
    );
}

#[test]
fn preset_only_change_at_equal_size_is_undoable() {
    let mut app = sample_app();
    app.execute_action(Action::set_canvas_size(
        0,
        size_state(app.doc.canvases[0].size_mm, None),
        size_state([183.0, 120.0], Some("nature-2col")),
    ));
    // Same physical size, different journal: still one undoable step.
    app.execute_action(Action::set_canvas_size(
        0,
        size_state([183.0, 120.0], Some("nature-2col")),
        size_state([183.0, 120.0], Some("science-3col")),
    ));
    assert_eq!(
        app.doc.canvases[0].size_preset_id.as_deref(),
        Some("science-3col")
    );
    app.undo();
    assert_eq!(
        app.doc.canvases[0].size_preset_id.as_deref(),
        Some("nature-2col")
    );
}

#[test]
fn plot_title_undo_redo_restores_text_position_and_visibility() {
    let mut app = sample_app();
    let object_id = app.doc.canvases[0].objects[0].id;
    let before = first_plot(&app).panel.clone();
    let mut after = before.clone();
    after.user_note = "edited title".to_owned();
    after.position = [42.0, 12.0];
    after.visible = false;

    app.execute_action(Action::set_panel_meta(
        0,
        object_id,
        before.clone(),
        after.clone(),
    ));
    assert_eq!(first_plot(&app).panel, after);

    app.undo();
    assert_eq!(first_plot(&app).panel, before);

    app.redo();
    assert_eq!(first_plot(&app).panel, after);
}

#[test]
fn page_view_zoom_pan_does_not_change_svg_or_object_geometry() {
    let mut app = sample_app();
    let before_frame = app.doc.canvases[0].objects[0].frame;
    let before_viewport = first_plot(&app).viewport.clone();
    let before_svg = crate::state::render_document_svg(&app.doc.canvases[0]);

    app.session.board.zoom = 2.0;
    app.session.board.pan = [120.0, -40.0];

    assert_eq!(app.doc.canvases[0].objects[0].frame, before_frame);
    assert_eq!(first_plot(&app).viewport, before_viewport);
    assert_eq!(
        crate::state::render_document_svg(&app.doc.canvases[0]),
        before_svg
    );
}

#[test]
fn arrange_grid_positions_objects_and_undo_restores_frames() {
    let mut app = sample_app();
    for _ in 0..2 {
        let id = app.doc.canvases[0].allocate_object_id();
        let object = app.build_plot_object(
            0,
            crate::state::ObjectFrame::new(5.0, 5.0, 40.0, 30.0),
            id,
            "extra".to_owned(),
        );
        app.doc.canvases[0].objects.push(object);
    }
    let before: Vec<_> = app.doc.canvases[0]
        .objects
        .iter()
        .map(|o| o.frame)
        .collect();

    app.arrange_active_canvas_grid(2, 2);

    assert_eq!(app.doc.canvases[0].layout.rows, 2);
    assert_eq!(app.doc.canvases[0].layout.cols, 2);
    let after: Vec<_> = app.doc.canvases[0]
        .objects
        .iter()
        .map(|o| o.frame)
        .collect();
    assert_ne!(before, after);
    assert_ne!(after[0], after[1]);

    app.undo();
    let restored: Vec<_> = app.doc.canvases[0]
        .objects
        .iter()
        .map(|o| o.frame)
        .collect();
    assert_eq!(before, restored);
    assert_eq!(app.doc.canvases[0].layout.rows, 1);
}

#[test]
fn set_page_layout_undo_redo_restores_margins() {
    let mut app = sample_app();
    let before = app.doc.canvases[0].layout;
    let mut after = before;
    after.margin_mm = [12.0, 3.0, 12.0, 3.0];
    after.gutter_mm = 9.0;

    app.commit_page_layout(0, before, after);
    assert_eq!(app.doc.canvases[0].layout, after);

    app.undo();
    assert_eq!(app.doc.canvases[0].layout, before);
    app.redo();
    assert_eq!(app.doc.canvases[0].layout, after);
}

#[test]
fn move_resize_object_only_changes_frame() {
    let mut app = sample_app();
    let object_id = app.doc.canvases[0].objects[0].id;
    let before_frame = app.doc.canvases[0].objects[0].frame;
    let before_viewport = first_plot(&app).viewport.clone();
    let after_frame = crate::state::ObjectFrame::new(12.0, 16.0, 180.0, 120.0);

    app.execute_action(Action::move_resize_object(
        0,
        object_id,
        before_frame,
        after_frame,
    ));

    assert_eq!(app.doc.canvases[0].objects[0].frame, after_frame);
    assert_eq!(first_plot(&app).viewport.view_x, before_viewport.view_x);
    assert_eq!(first_plot(&app).viewport.view_y, before_viewport.view_y);

    app.undo();
    assert_eq!(app.doc.canvases[0].objects[0].frame, before_frame);
    assert_eq!(first_plot(&app).viewport.view_x, before_viewport.view_x);
}

#[test]
fn document_svg_exports_independent_plot_groups_and_clips() {
    let mut app = sample_app();
    let id = app.doc.canvases[0].allocate_object_id();
    let second = app.build_plot_object(
        0,
        crate::state::ObjectFrame::new(20.0, 20.0, 220.0, 140.0),
        id,
        "Plot 2".to_owned(),
    );
    app.doc.canvases[0].objects.push(second);

    let svg = crate::state::render_document_svg(&app.doc.canvases[0]);
    assert!(svg.contains(r#"<g id="object_1" transform="translate(0.00,0.00)">"#));
    assert!(svg.contains(r#"<g id="object_2" transform="translate(20.00,20.00)">"#));
    assert!(svg.contains(r#"id="object_1_clip""#));
    assert!(svg.contains(r#"id="object_2_clip""#));
}

#[test]
fn delete_canvas_undo_restores_order_and_active_canvas() {
    let mut app = sample_app();
    push_canvas(&mut app, 0, "second canvas", [90.0, 60.0]);
    app.session.active_canvas = Some(0);

    app.execute_action(Action::delete_canvas(&app, 0).unwrap());
    assert_eq!(app.doc.canvases[0].name, "second canvas");
    assert_eq!(app.session.active_canvas, Some(0));

    app.undo();
    assert_eq!(app.doc.canvases[0].name, "sample canvas");
    assert_eq!(app.doc.canvases[1].name, "second canvas");
    assert_eq!(app.session.active_canvas, Some(0));
}

#[test]
fn rename_and_redo_stack_behave() {
    let mut app = sample_app();
    app.execute_action(Action::rename_canvas(
        0,
        "sample canvas".to_owned(),
        "renamed".to_owned(),
    ));
    app.undo();
    assert!(app.can_redo());

    app.execute_action(Action::rename_dataset(
        0,
        None,
        Some("data name".to_owned()),
    ));
    assert!(!app.can_redo());
    assert_eq!(app.doc.canvases[0].name, "sample canvas");
    assert_eq!(app.doc.datasets[0].display_name(), "data name");
}

pub(super) fn push_text_object(
    app: &mut PlotxApp,
    ci: usize,
    text: &str,
) -> crate::state::ObjectId {
    use crate::state::{CanvasObject, CanvasObjectKind, ObjectFrame, TextBox};
    let id = app.doc.canvases[ci].allocate_object_id();
    app.doc.canvases[ci].objects.push(CanvasObject {
        id,
        name: "Text".to_owned(),
        frame: ObjectFrame::new(0.0, 0.0, 40.0, 20.0),
        locked: false,
        visible: true,
        group: None,
        kind: CanvasObjectKind::Text(TextBox::label(text.to_owned())),
    });
    id
}

#[test]
fn set_object_style_applies_and_undoes_across_selection() {
    use crate::state::ObjectStyle;
    let mut app = sample_app();
    let a = push_text_object(&mut app, 0, "a");
    let b = push_text_object(&mut app, 0, "b");

    let before: Vec<_> = [a, b]
        .iter()
        .map(|&id| (id, app.doc.canvases[0].object(id).unwrap().style().unwrap()))
        .collect();
    let after: Vec<_> = before
        .iter()
        .map(|(id, style)| {
            let ObjectStyle::Text(t) = style else {
                unreachable!()
            };
            let mut t = t.clone();
            t.font_size = 33.0;
            t.bold = true;
            (*id, ObjectStyle::Text(t))
        })
        .collect();

    app.execute_action(Action::set_object_style(0, before, after));
    for id in [a, b] {
        let t = app.doc.canvases[0].object(id).unwrap().text().unwrap();
        assert_eq!(t.font_size, 33.0);
        assert!(t.bold);
    }

    app.undo();
    for id in [a, b] {
        let t = app.doc.canvases[0].object(id).unwrap().text().unwrap();
        assert_eq!(t.font_size, 14.0);
        assert!(!t.bold);
    }

    app.redo();
    assert_eq!(
        app.doc.canvases[0]
            .object(a)
            .unwrap()
            .text()
            .unwrap()
            .font_size,
        33.0
    );
}

#[test]
fn apply_style_to_kind_copies_style_but_keeps_text() {
    let mut app = sample_app();
    let src = push_text_object(&mut app, 0, "source");
    let dst = push_text_object(&mut app, 0, "target");
    app.doc.canvases[0]
        .object_mut(src)
        .unwrap()
        .text_mut()
        .unwrap()
        .font_size = 42.0;

    app.apply_style_to_kind(0, src);

    let t = app.doc.canvases[0].object(dst).unwrap().text().unwrap();
    assert_eq!(t.font_size, 42.0);
    assert_eq!(t.text, "target");

    app.undo();
    assert_eq!(
        app.doc.canvases[0]
            .object(dst)
            .unwrap()
            .text()
            .unwrap()
            .font_size,
        14.0
    );
}

#[test]
fn style_default_feeds_new_authored_text() {
    let mut app = sample_app();
    let src = push_text_object(&mut app, 0, "styled");
    app.doc.canvases[0]
        .object_mut(src)
        .unwrap()
        .text_mut()
        .unwrap()
        .font_size = 27.0;

    app.set_style_default(0, src);
    assert_eq!(app.doc.style_library.text.font_size, 27.0);

    let new = app.doc.style_library.text.clone();
    assert_eq!(new.font_size, 27.0);
}

pub(super) fn table_app() -> (PlotxApp, crate::state::ObjectId) {
    use crate::state::{
        CanvasDocument, Dataset, FloatSeries, ObjectFrame, materialized_float_series_table,
    };
    let mut app = PlotxApp::new();
    let table = materialized_float_series_table(
        (
            "Gradient".into(),
            "mT/m".into(),
            vec![Some(0.0), Some(1.0), Some(2.0)],
        ),
        vec![FloatSeries {
            name: "a".to_owned(),
            unit: String::new(),
            values: vec![Some(3.0), Some(2.0), Some(1.0)],
            uncertainty: None,
            fit: None,
        }],
        "plotx.test.action-table.v1",
    )
    .unwrap();
    app.doc.datasets.push(Dataset::Table(Box::new(table)));
    let mut canvas = CanvasDocument::new("table".to_owned(), [120.0, 80.0]);
    let [w, h] = canvas.size_pt();
    let id = canvas.allocate_object_id();
    let object =
        app.build_plot_object(0, ObjectFrame::new(0.0, 0.0, w, h), id, "Plot 1".to_owned());
    canvas.objects.push(object);
    app.doc.canvases.push(canvas);
    app.focus_single(0);
    app.session.active_canvas = Some(0);
    (app, id)
}

pub(super) fn table_app_with_sigma(sigma: Vec<f64>) -> (PlotxApp, crate::state::ObjectId) {
    use crate::state::{Dataset, FloatSeries, materialized_float_series_table};
    let (mut app, id) = table_app();
    let table = materialized_float_series_table(
        (
            "Gradient".into(),
            "mT/m".into(),
            vec![Some(0.0), Some(1.0), Some(2.0)],
        ),
        vec![FloatSeries {
            name: "a".to_owned(),
            unit: String::new(),
            values: vec![Some(3.0), Some(2.0), Some(1.0)],
            uncertainty: Some(sigma.into_iter().map(Some).collect()),
            fit: None,
        }],
        "plotx.test.action-table.v1",
    )
    .unwrap();
    app.doc.datasets[0] = Dataset::Table(Box::new(table));
    app.rebuild_canvases_for(0);
    (app, id)
}

pub(super) fn synthetic_2d() -> plotx_io::NmrData2D {
    use plotx_io::{Dim, Domain, NmrData2D, QuadMode};
    let (cols, rows) = (32usize, 4usize);
    let dim = |nucleus: &str| Dim {
        spectral_width_hz: 4000.0,
        observe_freq_mhz: 400.0,
        carrier_ppm: 0.0,
        nucleus: nucleus.to_owned(),
        group_delay: 0.0,
    };
    NmrData2D {
        data: vec![Complex64::new(0.0, 0.0); rows * cols],
        rows,
        cols,
        domain: Domain::Time,
        direct: dim("1H"),
        indirect: dim("13C"),
        quad: QuadMode::Complex,
        indirect_conjugate: false,
        experiment: Some("hsqc".to_owned()),
        pseudo_axis: None,
        diffusion: None,
        nus: None,
        source: "synthetic 2d".to_owned(),
    }
}
