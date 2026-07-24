use super::*;

#[test]
fn typed_global_ids_round_trip_through_resource_dtos() {
    let dataset = DatasetId::new();
    let canvas = CanvasId::new();

    let dataset_ref = ResourceRef::from(dataset);
    let canvas_ref = ResourceRef::from(canvas);

    assert_eq!(DatasetId::try_from(&dataset_ref), Ok(dataset));
    assert_eq!(CanvasId::try_from(&canvas_ref), Ok(canvas));
}

#[test]
fn typed_resource_conversion_rejects_wrong_kind_children_and_invalid_uuids() {
    let dataset = DatasetId::new();
    let dataset_ref = ResourceRef::from(dataset);

    assert!(matches!(
        CanvasId::try_from(&dataset_ref),
        Err(ResourceIdError::WrongKind { .. })
    ));

    let mut child = dataset_ref.clone();
    child.parent_id = Some(dataset.to_string());
    child.local_id = Some("child".into());
    assert_eq!(
        DatasetId::try_from(&child),
        Err(ResourceIdError::ChildResource)
    );

    let invalid = ResourceRef {
        id: "not-a-uuid".into(),
        kind: ResourceKindId::new(crate::automation::KIND_DATASET),
        parent_id: None,
        local_id: None,
    };
    assert!(matches!(
        DatasetId::try_from(&invalid),
        Err(ResourceIdError::InvalidUuid(_))
    ));
}

#[test]
fn target_ref_serializes_resource_and_typed_components() {
    let resource = ResourceRef::from(DatasetId::new());
    let resource_only = TargetRef::resource(resource.clone());
    let json = serde_json::to_value(&resource_only).unwrap();
    assert!(json.get("component").is_none());

    let series_target = TargetRef {
        resource: resource.clone(),
        component: Some(ComponentRef::Series(SeriesId::new(42))),
    };
    assert_eq!(
        serde_json::to_value(&series_target).unwrap()["component"],
        serde_json::json!({"kind": "series", "id": 42})
    );

    let step_target = TargetRef {
        resource,
        component: Some(ComponentRef::ProcessingStep(StepId(9))),
    };
    let encoded = serde_json::to_string(&step_target).unwrap();
    assert_eq!(
        serde_json::from_str::<TargetRef>(&encoded).unwrap(),
        step_target
    );
}

#[test]
fn target_ref_rejects_unknown_component_kinds() {
    let json = serde_json::json!({
        "resource": ResourceRef::from(DatasetId::new()),
        "component": {"kind": "field", "id": 1}
    });
    assert!(serde_json::from_value::<TargetRef>(json).is_err());
}
