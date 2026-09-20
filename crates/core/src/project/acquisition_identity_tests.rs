use super::*;

#[test]
fn v1_rejects_dataset_objects_without_acquisition_identity() {
    let app = tests::sample_app();
    let mut objects = dataset_to_objects(&app.doc.datasets[0], "data-1", "recipe-1").unwrap();
    objects
        .data
        .extensions
        .as_object_mut()
        .expect("data extensions")
        .remove("plotx.acquisition_identity");

    let error = read_acquisition_identity(&objects.data).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("missing plotx.acquisition_identity")
    );
}

#[test]
fn v1_embeds_one_nmr_snapshot_without_duplicate_vendor_metadata() {
    let app = tests::sample_app();
    let source = app.doc.datasets[0].as_nmr().unwrap().data.dataset();
    let objects = dataset_to_objects(&app.doc.datasets[0], "data-1", "recipe-1").unwrap();
    assert_eq!(objects.data.payload.storage, super::nmr_snapshot::STORAGE);
    assert!(objects.data.dimensions.is_empty());
    assert!(objects.data.extensions.get("plotx.nmr").is_none());
    let path = tests::temp_project("nmr-snapshot");
    save_project(&app, &path, false).unwrap();
    let restored = load_project(&path).unwrap();
    let restored = restored.doc.datasets[0].as_nmr().unwrap();
    assert_eq!(
        restored.data.dataset().canonical_digests(),
        source.canonical_digests()
    );
    std::fs::remove_file(path).unwrap();
}
