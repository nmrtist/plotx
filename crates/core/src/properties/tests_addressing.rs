//! Property addressing and component-shape tests.

use super::*;

/// The addressing rule the whole design turns on. A contour setting belongs to
/// the series that draws it, so its address is the plot object plus a
/// `Series(SeriesId)` component — never the dataset, never the field child
/// resource the series happens to read, and never a bare object.
#[test]
fn a_contour_property_is_addressed_by_series_and_nothing_else() {
    let (app, target) = contour_app();
    assert!(matches!(target.component, Some(ComponentRef::Series(_))));
    assert_eq!(
        target.resource.kind.0,
        crate::automation::KIND_CANVAS_OBJECT
    );

    let resolved = app
        .resolve_property(&PropertyAddress::new(target.clone(), contour::COUNT))
        .expect("a series component resolves");
    assert!(matches!(
        resolved.value,
        AggregateValue::Uniform(PropertyValue::Int(_))
    ));

    // The same object without a component names no series at all.
    let bare = TargetRef::resource(target.resource.clone());
    let error = app
        .resolve_property(&PropertyAddress::new(bare, contour::COUNT))
        .expect_err("a bare object is not a contour target");
    assert!(matches!(error, PropertyError::ComponentKind { .. }));

    // The dataset that owns the values is not the owner of the setting.
    let dataset = TargetRef {
        resource: ResourceRef::from(app.doc.datasets[0].resource_id()),
        component: target.component,
    };
    let error = app
        .resolve_property(&PropertyAddress::new(dataset, contour::COUNT))
        .expect_err("a dataset is not a contour target");
    assert!(matches!(error, PropertyError::NotApplicable(_)));

    // Neither is the field child resource the series reads from: fields carry
    // their own stats and provenance properties, addressed with no component.
    let field = TargetRef {
        resource: ResourceRef {
            id: format!("{}/nmr.real", app.doc.datasets[0].resource_id()),
            kind: crate::automation::ResourceKindId::new(KIND_FIELD),
            parent_id: Some(app.doc.datasets[0].resource_id().to_string()),
            local_id: Some("nmr.real".to_owned()),
        },
        component: target.component,
    };
    let error = app
        .resolve_property(&PropertyAddress::new(field, contour::COUNT))
        .expect_err("a field child resource is not a contour target");
    assert!(matches!(error, PropertyError::NotApplicable(_)));
}

/// Applicability is decided from the definition before the target is looked up
/// at all, so a misaddressed property never reaches plot code. Pinned with a
/// resource that does not exist: the component-kind rejection must still win
/// over "no such target", which is only true if it happens first.
#[test]
fn the_component_shape_is_rejected_before_any_document_lookup() {
    let (app, target) = contour_app();
    let nowhere = TargetRef {
        resource: ResourceRef {
            id: "00000000-0000-0000-0000-000000000000/999".to_owned(),
            kind: crate::automation::ResourceKindId::new(crate::automation::KIND_CANVAS_OBJECT),
            parent_id: Some("00000000-0000-0000-0000-000000000000".to_owned()),
            local_id: Some("999".to_owned()),
        },
        component: Some(ComponentRef::ProcessingStep(plotx_processing::StepId::new(
            0,
        ))),
    };
    let error = app
        .resolve_property(&PropertyAddress::new(nowhere, contour::COUNT))
        .expect_err("a processing step does not own contour levels");
    match error {
        PropertyError::ComponentKind {
            property,
            expected,
            actual,
        } => {
            assert_eq!(property, contour::COUNT, "the real property is named");
            assert_eq!(expected, "series");
            assert_eq!(actual, "processing_step");
        }
        other => panic!("expected the definition's own component gate, got {other:?}"),
    }
    // The same address with the right component shape does get as far as the
    // document, proving the rejection above was the component gate and not an
    // accident of the bogus id.
    assert!(matches!(
        app.resolve_property(&PropertyAddress::new(
            TargetRef {
                resource: ResourceRef {
                    id: "00000000-0000-0000-0000-000000000000/999".to_owned(),
                    kind: crate::automation::ResourceKindId::new(
                        crate::automation::KIND_CANVAS_OBJECT
                    ),
                    parent_id: Some("00000000-0000-0000-0000-000000000000".to_owned()),
                    local_id: Some("999".to_owned()),
                },
                component: target.component,
            },
            contour::COUNT,
        )),
        Err(PropertyError::UnknownTarget(_))
    ));
}
