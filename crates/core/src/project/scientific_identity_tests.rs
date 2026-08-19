use super::*;

#[test]
fn v1_rejects_dataset_objects_without_scientific_identity() {
    let app = tests::sample_app();
    let mut objects = dataset_to_objects(&app.doc.datasets[0], "data-1", "recipe-1").unwrap();
    objects
        .data
        .extensions
        .as_object_mut()
        .expect("data extensions")
        .remove("plotx.scientific_identity");

    let error = read_scientific_identity(&objects.data).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("missing plotx.scientific_identity")
    );
}
