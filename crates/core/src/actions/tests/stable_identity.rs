use super::{sample_app, synthetic_1d};
use crate::actions::Action;
use crate::state::{
    AxisProjection, DEFAULT_CANVAS_SIZE_MM, Dataset, DatasetId, DatasetLineage, DerivationKind,
    NmrDataset, ObjectFrame, ProjectionSource, SeriesBinding,
};

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
