use super::{dataset_id, sample_app, synthetic_1d};
use crate::actions::Action;
use crate::state::{
    AxisProjection, DEFAULT_CANVAS_SIZE_MM, Dataset, DatasetId, DatasetLineage, DerivationKind,
    NmrDataset, ObjectFrame, ProjectionSource, SeriesBinding,
};
use plotx_processing::{ProcessingStep, StepKind, StepSource};

#[test]
fn dataset_delete_undo_restores_identity_and_persistent_references() {
    let mut app = sample_app();
    let mut inserted = Dataset::Nmr(Box::new(NmrDataset::load(synthetic_1d())));
    let inserted_id = DatasetId::new();
    inserted.set_resource_id(inserted_id);
    let action = Action::insert_dataset_with_default_canvas(
        &app,
        inserted,
        "referenced".to_owned(),
        DEFAULT_CANVAS_SIZE_MM,
    );

    let plot = app.doc.canvases[0].objects[0].plot_mut().unwrap();
    plot.binding.series.push(SeriesBinding::new(inserted_id));
    plot.projections.top = AxisProjection {
        source: ProjectionSource::Attached(inserted_id),
        visible: true,
    };
    app.doc.datasets[0].set_lineage(Some(DatasetLineage::new(
        DerivationKind::Projection,
        [inserted_id],
    )));

    app.execute_action(action);
    let canvas_id = app.doc.canvases[1].resource_id;
    let object_id = app.doc.canvases[1].objects[0].id;
    assert_eq!(app.doc.dataset_index(inserted_id), Some(1));

    app.undo();
    assert!(app.doc.dataset_index(inserted_id).is_none());
    assert!(app.doc.canvas_index(canvas_id).is_none());

    app.redo();
    assert_eq!(app.doc.dataset_index(inserted_id), Some(1));
    assert_eq!(app.doc.canvas_index(canvas_id), Some(1));
    assert!(app.doc.canvases[1].object(object_id).is_some());
    let plot = app.doc.canvases[0].objects[0].plot().unwrap();
    assert!(plot.binding.contains_dataset(inserted_id));
    assert_eq!(
        plot.projections.top.source,
        ProjectionSource::Attached(inserted_id)
    );
    assert_eq!(
        app.doc.datasets[0].lineage().unwrap().sources,
        vec![inserted_id]
    );
}

#[test]
fn canvas_dataset_ids_follow_first_appearance_and_page_indices_follow_document_order() {
    let mut app = sample_app();
    let ids: [DatasetId; 3] = [
        "ffffffff-ffff-4fff-8fff-ffffffffffff".parse().unwrap(),
        "00000000-0000-4000-8000-000000000001".parse().unwrap(),
        "77777777-7777-4777-8777-777777777777".parse().unwrap(),
    ];
    app.doc.datasets[0].set_resource_id(ids[0]);
    for id in &ids[1..] {
        let mut dataset = Dataset::Nmr(Box::new(NmrDataset::load(synthetic_1d())));
        dataset.set_resource_id(*id);
        app.doc.datasets.push(dataset);
    }

    let canvas = &mut app.doc.canvases[0];
    canvas.objects[0].plot_mut().unwrap().binding.series = vec![
        SeriesBinding::new(ids[2]),
        SeriesBinding::new(ids[0]),
        SeriesBinding::new(ids[2]),
    ];
    let object_id = canvas.allocate_object_id();
    let second_plot = app.build_plot_object(
        1,
        ObjectFrame::new(0.0, 0.0, 40.0, 30.0),
        object_id,
        "Plot 2".to_owned(),
    );
    app.doc.canvases[0].objects.push(second_plot);

    let expected_ids = vec![ids[2], ids[0], ids[1]];
    assert_eq!(app.doc.canvases[0].dataset_ids(), expected_ids);
    assert_eq!(app.doc.canvases[0].dataset_ids(), expected_ids);
    assert_eq!(app.doc.page_dataset_indices(0), vec![0, 1, 2]);
    assert_eq!(app.doc.page_dataset_indices(0), vec![0, 1, 2]);
}

#[test]
fn syncing_integral_curves_ignores_a_stale_dataset_index() {
    let mut app = sample_app();
    let stale_index = app.doc.datasets.len();

    app.sync_integral_curves_for(stale_index);
}

#[test]
fn series_reorder_preserves_ids_and_only_changes_order() {
    let mut app = sample_app();
    let second = Dataset::Nmr(Box::new(NmrDataset::load(synthetic_1d())));
    let second_id = second.resource_id();
    app.doc.datasets.push(second);
    let plot = app.doc.canvases[0].objects[0].plot_mut().unwrap();
    let id = plot.allocate_series_id();
    let mut series = SeriesBinding::new(second_id);
    series.id = id;
    plot.binding.series.push(series);
    let before: Vec<_> = plot.binding.series.iter().map(|series| series.id).collect();

    plot.binding.series.swap(0, 1);

    let after: Vec<_> = plot.binding.series.iter().map(|series| series.id).collect();
    assert_eq!(after, before.into_iter().rev().collect::<Vec<_>>());
}

#[test]
fn step_and_series_allocators_do_not_rollback_with_undo() {
    let mut app = sample_app();
    let second = Dataset::Nmr(Box::new(NmrDataset::load(synthetic_1d())));
    let second_id = second.resource_id();
    app.doc.datasets.push(second);

    let before_processing =
        crate::actions::DatasetProcessingState::from_dataset(&app.doc.datasets[0]);
    let step_id = app.doc.datasets[0].as_nmr_mut().unwrap().allocate_step_id();
    let step_high_water = app.doc.datasets[0].as_nmr().unwrap().next_step_id;
    let mut after_processing = before_processing.clone();
    let crate::actions::DatasetProcessingState::Nmr { pipeline, .. } = &mut after_processing else {
        unreachable!()
    };
    pipeline.steps.push(ProcessingStep {
        id: step_id,
        kind: StepKind::Invert,
        enabled: true,
        source: StepSource::User,
    });
    app.execute_action(Action::update_dataset_processing(
        app.doc.datasets[0].resource_id(),
        before_processing,
        after_processing,
    ));

    let (series_id, series_high_water, before_binding, after_binding, object_id) = {
        let object = &mut app.doc.canvases[0].objects[0];
        let object_id = object.id;
        let plot = object.plot_mut().unwrap();
        let before = plot.binding.clone();
        let series_id = plot.allocate_series_id();
        let high_water = plot.next_series_id;
        let mut after = before.clone();
        let mut series = SeriesBinding::new(second_id);
        series.id = series_id;
        after.series.push(series);
        (series_id, high_water, before, after, object_id)
    };
    app.execute_action(Action::set_data_binding(
        0,
        object_id,
        before_binding,
        after_binding,
    ));

    app.undo();
    assert_eq!(
        app.doc.canvases[0].objects[0]
            .plot()
            .unwrap()
            .next_series_id,
        series_high_water
    );
    assert!(
        app.doc.canvases[0].objects[0]
            .plot()
            .unwrap()
            .binding
            .series
            .iter()
            .all(|series| series.id != series_id)
    );
    app.undo();
    assert_eq!(
        app.doc.datasets[0].as_nmr().unwrap().next_step_id,
        step_high_water
    );
    assert!(
        app.doc.datasets[0]
            .axis_pipeline(crate::state::PhaseAxis::Direct)
            .unwrap()
            .steps
            .iter()
            .all(|step| step.id != step_id)
    );

    app.redo();
    assert!(
        app.doc.datasets[0]
            .axis_pipeline(crate::state::PhaseAxis::Direct)
            .unwrap()
            .steps
            .iter()
            .any(|step| step.id == step_id)
    );
    app.redo();
    assert!(
        app.doc.canvases[0].objects[0]
            .plot()
            .unwrap()
            .binding
            .series
            .iter()
            .any(|series| series.id == series_id)
    );
}

/// P0-2 regression. `StepId` is owner-local: every dataset numbers its steps
/// from zero, so two datasets genuinely hold equal `StepId` values. With the
/// expanded-row state stored as a bare `StepId`, switching the active dataset
/// made the same-numbered row on the new dataset read as expanded, and
/// `phase_editor_dataset` then reported phasing on a dataset the user never
/// touched — flipping the tool to ManualPhase and opening a processing session.
/// Reverting to `Option<StepId>` makes both assertions below fail.
#[test]
fn an_expanded_step_does_not_leak_onto_another_dataset_with_the_same_id() {
    let mut app = sample_app();
    app.doc
        .datasets
        .push(Dataset::Nmr(Box::new(NmrDataset::load(synthetic_1d()))));

    let phase_id = |app: &crate::state::PlotxApp, index: usize| {
        app.doc.datasets[index]
            .axis_pipeline(crate::state::PhaseAxis::Direct)
            .unwrap()
            .steps
            .iter()
            .find(|step| matches!(step.kind, StepKind::Phase(_)))
            .unwrap()
            .id
    };
    // The premise: the two datasets really do share the id value.
    assert_eq!(phase_id(&app, 0), phase_id(&app, 1));

    // Expand the Phase row on dataset 0 while dataset 0 is active.
    app.focus_single(0);
    app.session.ui.proc_expanded_step = Some((dataset_id(&app, 0), phase_id(&app, 0)));
    app.sync_phase_interaction();
    assert!(app.phase_editor_open());
    assert_eq!(app.session.tool, crate::state::Tool::ManualPhase);

    // Switching to the other dataset must not inherit that expansion.
    app.focus_single(1);
    app.sync_phase_interaction();
    assert!(
        !app.phase_editor_open(),
        "an equal StepId on another dataset must not read as an open Phase editor"
    );
    assert_ne!(
        app.session.tool,
        crate::state::Tool::ManualPhase,
        "no tool switch on a dataset the user never expanded a row on"
    );
}

/// P0-3 regression. These entry points take a positional index that a
/// concurrent delete can invalidate. Reading `self.doc.datasets[index]` up
/// front to fetch the stable id turned every stale call into a panic; each one
/// must guard and no-op instead. Reverting any guard aborts this test.
#[test]
fn stale_dataset_indices_are_inert_rather_than_fatal() {
    let mut app = sample_app();
    let stale = app.doc.datasets.len() + 5;
    let state = crate::actions::DatasetProcessingState::from_dataset(&app.doc.datasets[0]);

    app.commit_processing_edit(stale, state.clone(), state);
    app.begin_processing_session(stale);
    app.edit_peaks(stale, |_| unreachable!("no peak set behind a stale index"));
    app.edit_integrals(stale, |_, _| {
        unreachable!("no integrals behind a stale index")
    });
    app.edit_integrals_2d(stale, |_, _| {
        unreachable!("no 2D integrals behind a stale index")
    });
    app.edit_regions(stale, |_, _| {
        unreachable!("no regions behind a stale index")
    });
    app.remove_statistics(stale, 0);
    app.remove_line_fit(stale, 0);
    app.remove_multiplet(stale, 0);
    app.cancel_compute(stale, crate::state::ComputeKind::Dosy);

    assert!(
        !app.can_undo(),
        "a stale target records no history instead of crashing"
    );
}
