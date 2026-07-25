use super::*;
use crate::actions::Action;
use crate::automation::{
    CAP_FIELD_NOISE_SCALE, CAP_FIELD_SIGNED, CapabilityId, ComponentRef, KIND_FIELD, ResourceRef,
    TargetRef,
};
use crate::state::{
    CONTOUR_BASE_FRACTION_OF_RANGE, CONTOUR_BASE_NOISE_FLOOR, CanvasDocument, Dataset,
    Nmr2DDataset, ObjectFrame, PlotxApp, SeriesBinding, SeriesId,
};

/// The default plane: values running -7..8, so its noise estimate is an
/// ordinary fraction of its peak and no contour floor is ever reached.
fn default_plane() -> Vec<f64> {
    (0..16).map(|value| f64::from(value) - 7.0).collect()
}

fn nmr2d_with(source: &str, values: &[f64]) -> plotx_io::NmrData2D {
    let dimension = |nucleus: &str| plotx_io::Dim {
        spectral_width_hz: 4_000.0,
        observe_freq_mhz: 400.0,
        carrier_ppm: 0.0,
        nucleus: nucleus.to_owned(),
        group_delay: 0.0,
    };
    plotx_io::NmrData2D {
        data: values
            .iter()
            .map(|value| num_complex::Complex64::new(*value, 0.5))
            .collect(),
        rows: 4,
        cols: 4,
        domain: plotx_io::Domain::Frequency,
        direct: dimension("1H"),
        indirect: dimension("13C"),
        quad: plotx_io::QuadMode::Complex,
        indirect_conjugate: false,
        experiment: None,
        pseudo_axis: None,
        diffusion: None,
        nus: None,
        source: source.to_owned(),
    }
}

/// One page holding one plot bound to a true-2D spectrum, i.e. the exact shape
/// the driving case has: a contour drawn from a signed scalar grid.
pub(crate) fn contour_app() -> (PlotxApp, TargetRef) {
    contour_app_with_plane(&default_plane())
}

/// The same page over a plane the caller chooses, so a test can put a field of
/// a given dynamic range in front of the catalog.
pub(crate) fn contour_app_with_plane(values: &[f64]) -> (PlotxApp, TargetRef) {
    let mut app = PlotxApp::new();
    app.doc
        .datasets
        .push(Dataset::Nmr2D(Box::new(Nmr2DDataset::load(nmr2d_with(
            "contour", values,
        )))));
    let mut canvas = CanvasDocument::new("page".to_owned(), [120.0, 80.0]);
    let id = canvas.allocate_object_id();
    let object = app.build_plot_object(
        0,
        ObjectFrame::new(0.0, 0.0, 100.0, 80.0),
        id,
        "Plot".into(),
    );
    canvas.objects.push(object);
    app.doc.canvases.push(canvas);
    app.session.active_canvas = Some(0);
    let series = app.doc.canvases[0]
        .object(id)
        .and_then(|object| object.plot())
        .and_then(|plot| plot.binding.series.first())
        .map(|series| series.id)
        .expect("the plot has a series");
    let target = app.series_target(0, id, series).expect("target resolves");
    (app, target)
}

pub(crate) fn contour_spec(app: &PlotxApp, target: &TargetRef) -> plotx_figure::ContourSpec {
    let Some(ComponentRef::Series(series)) = target.component else {
        panic!("the fixture addresses a series");
    };
    let binding = &app.doc.canvases[0]
        .object(
            target
                .resource
                .local_id
                .as_deref()
                .unwrap()
                .parse()
                .unwrap(),
        )
        .and_then(|object| object.plot())
        .expect("plot")
        .binding;
    match &binding
        .series
        .iter()
        .find(|candidate| candidate.id == series)
        .expect("series")
        .encoding
    {
        plotx_figure::SeriesEncoding::Contour(spec) => spec.clone(),
        other => panic!("expected a contour, got {other:?}"),
    }
}

#[test]
fn the_fixture_draws_a_contour() {
    let (app, target) = contour_app();
    let address = PropertyAddress::new(target.clone(), contour::BASE_MAGNITUDE);
    let resolved = app.resolve_property(&address).expect("contour resolves");
    assert_eq!(resolved.availability, Availability::Editable);
}

/// Two definitions sharing an id would make every lookup, every search hit and
/// every reset ambiguous.
#[test]
fn stable_property_ids_are_unique() {
    let mut ids: Vec<&str> = catalog()
        .iter()
        .map(|definition| definition.id.as_str())
        .collect();
    let total = ids.len();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), total, "catalog ids must be unique");
}

/// Every entry must be reachable: a definition nothing can address is dead
/// weight that still costs a panel row and a search hit.
#[test]
fn every_definition_declares_an_addressable_shape() {
    for definition in catalog() {
        assert!(
            !definition.canonical_label.is_empty(),
            "{} has no canonical label",
            definition.id
        );
        if definition.access == PropertyAccess::ReadOnly {
            assert!(
                matches!(definition.default_policy, DefaultPolicy::None),
                "{} is read-only and cannot have a default to reset to",
                definition.id
            );
        }
    }
}

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

/// A panel edit must reach the document as the ordinary typed binding action,
/// so it undoes, redoes and rebuilds like every other binding change.
#[test]
fn an_edit_compiles_into_a_typed_binding_action() {
    let (mut app, target) = contour_app();
    let before = contour_spec(&app, &target);
    let commit = app
        .plan_property_write(
            contour::COUNT,
            std::slice::from_ref(&target),
            &PropertyValue::Int(7),
        )
        .expect("count is writable");
    assert_eq!(commit.applied.len(), 1);
    assert!(commit.skipped.is_empty());
    let Action::Composite(actions) = &commit.action else {
        panic!("a commit is always one atomic composite");
    };
    assert_eq!(actions.len(), 1);
    assert!(matches!(actions[0], Action::SetDataBinding { .. }));

    app.commit_property(commit);
    let after = contour_spec(&app, &target);
    assert_eq!(before.positive.count, 14);
    assert_eq!(after.positive.count, 7);
    assert_eq!(
        after.negative.as_ref().map(|half| half.count),
        Some(7),
        "the mirrored half follows the shared ladder"
    );
}

/// Two series of one object must fold into a single action. Two actions built
/// from the same pre-edit snapshot would make the second overwrite the first.
#[test]
fn several_series_of_one_object_fold_into_one_action() {
    let (mut app, first) = contour_app();
    let object: crate::state::ObjectId =
        first.resource.local_id.as_deref().unwrap().parse().unwrap();
    let second_id = {
        let plot = app.doc.canvases[0]
            .object_mut(object)
            .and_then(|object| object.plot_mut())
            .expect("plot");
        let id = plot.allocate_series_id();
        let mut extra = plot.binding.series[0].clone();
        extra.id = id;
        plot.binding.series.push(extra);
        id
    };
    let second = app.series_target(0, object, second_id).expect("target");

    let commit = app
        .plan_property_write(
            contour::COUNT,
            &[first.clone(), second.clone()],
            &PropertyValue::Int(9),
        )
        .expect("count is writable");
    assert_eq!(commit.applied.len(), 2);
    let Action::Composite(actions) = &commit.action else {
        panic!("expected a composite");
    };
    assert_eq!(
        actions.len(),
        1,
        "both series belong to one object and share one binding action"
    );

    app.commit_property(commit);
    for target in [first, second] {
        assert_eq!(contour_spec(&app, &target).positive.count, 9);
    }
}

/// A target the property does not apply to is reported with a reason. Silently
/// dropping it would leave the user believing the edit landed everywhere.
#[test]
fn an_inapplicable_target_is_reported_rather_than_ignored() {
    let (mut app, target) = contour_app();
    let object: crate::state::ObjectId = target
        .resource
        .local_id
        .as_deref()
        .unwrap()
        .parse()
        .unwrap();
    let line_id = {
        let plot = app.doc.canvases[0]
            .object_mut(object)
            .and_then(|object| object.plot_mut())
            .expect("plot");
        let id = plot.allocate_series_id();
        let mut extra = plot.binding.series[0].clone();
        extra.id = id;
        extra.encoding = plotx_figure::SeriesEncoding::Line(plotx_figure::LineEncoding::default());
        plot.binding.series.push(extra);
        id
    };
    let line = app.series_target(0, object, line_id).expect("target");

    let set = app.resolve_property_set(contour::COUNT, &[target.clone(), line.clone()]);
    assert_eq!(set.applicable_targets.len(), 1);
    assert_eq!(set.skipped_targets.len(), 1);
    assert!(
        set.skipped_targets[0].1.contains("contour"),
        "the reason names the mismatch: {}",
        set.skipped_targets[0].1
    );

    let commit = app
        .plan_property_write(contour::COUNT, &[target, line], &PropertyValue::Int(5))
        .expect("the compatible target still commits");
    assert_eq!(commit.applied.len(), 1);
    assert_eq!(commit.skipped.len(), 1);
}

/// A value one target rejects must abort the entire commit; a partially applied
/// multi-selection edit is exactly what the atomic composite exists to prevent.
#[test]
fn a_rejected_value_aborts_the_whole_commit() {
    let (app, target) = contour_app();
    let before = contour_spec(&app, &target);
    let error = app
        .plan_property_write(
            contour::COUNT,
            std::slice::from_ref(&target),
            &PropertyValue::Int(0),
        )
        .expect_err("zero levels is not a ladder");
    assert!(matches!(error, PropertyError::InvalidValue { .. }));
    assert_eq!(
        contour_spec(&app, &target).positive.count,
        before.positive.count,
        "nothing may change when planning failed"
    );
}

/// The base-policy gate is a capability gate, not a domain check: a signed field
/// is never offered a fraction of its value range, and a field with no noise
/// estimator is never offered a multiple of σ.
#[test]
fn base_policies_are_gated_by_field_capability() {
    let signed = crate::state::FieldCapabilities::new([
        CapabilityId::new(crate::automation::CAP_FIELD_SCALAR_GRID_2D_REGULAR),
        CapabilityId::new(CAP_FIELD_SIGNED),
        CapabilityId::new(CAP_FIELD_NOISE_SCALE),
    ]);
    let schema = definition(contour::BASE_POLICY)
        .expect("the policy property is registered")
        .value_schema;
    let offered: Vec<&str> = permitted_variants(&schema, &signed)
        .into_iter()
        .map(|variant| variant.id)
        .collect();
    assert!(offered.contains(&CONTOUR_BASE_NOISE_FLOOR));
    assert!(
        !offered.contains(&CONTOUR_BASE_FRACTION_OF_RANGE),
        "a fraction of a range that straddles zero is not a threshold"
    );

    let bounded = crate::state::FieldCapabilities::new([
        CapabilityId::new(crate::automation::CAP_FIELD_SCALAR_GRID_2D_REGULAR),
        CapabilityId::new(crate::automation::CAP_FIELD_BOUNDED),
    ]);
    let offered: Vec<&str> = permitted_variants(&schema, &bounded)
        .into_iter()
        .map(|variant| variant.id)
        .collect();
    assert!(offered.contains(&CONTOUR_BASE_FRACTION_OF_RANGE));
    assert!(!offered.contains(&CONTOUR_BASE_NOISE_FLOOR));
}

/// The gate must also hold on the write path, not only in the control that
/// offers the choices.
#[test]
fn an_ungated_base_policy_is_refused_on_write() {
    let (app, target) = contour_app();
    let error = app
        .plan_property_write(
            contour::BASE_POLICY,
            std::slice::from_ref(&target),
            &PropertyValue::Enum(CONTOUR_BASE_FRACTION_OF_RANGE),
        )
        .expect_err("the signed NMR plane must not accept a range fraction");
    assert!(matches!(error, PropertyError::InvalidValue { .. }));
}

/// Reset re-derives the value from the factory in the target's current context
/// rather than restoring a stored snapshot.
#[test]
fn reset_rederives_the_factory_default() {
    let (mut app, target) = contour_app();
    let commit = app
        .plan_property_write(
            contour::BASE_MAGNITUDE,
            std::slice::from_ref(&target),
            &PropertyValue::Float(11.0),
        )
        .expect("the multiplier is writable");
    app.commit_property(commit);
    let address = PropertyAddress::new(target.clone(), contour::BASE_MAGNITUDE);
    let resolved = app.resolve_property(&address).expect("resolves");
    assert!(resolved.is_modified());
    assert_eq!(resolved.default_value, Some(PropertyValue::Float(5.0)));

    let commit = app
        .plan_property_reset(contour::BASE_MAGNITUDE, std::slice::from_ref(&target))
        .expect("reset plans");
    app.commit_property(commit);
    let resolved = app.resolve_property(&address).expect("resolves");
    assert!(!resolved.is_modified());
    assert_eq!(
        resolved.value,
        AggregateValue::Uniform(PropertyValue::Float(5.0))
    );
}

/// Resetting a whole encoding goes back through the default factory, so it
/// yields a complete concrete encoding rather than a patched-up old one.
#[test]
fn resetting_an_encoding_calls_the_default_factory() {
    let (mut app, target) = contour_app();
    let commit = app
        .plan_property_write(
            contour::NEGATIVE_ENABLED,
            std::slice::from_ref(&target),
            &PropertyValue::Bool(false),
        )
        .expect("the negative half is writable on a signed field");
    app.commit_property(commit);
    assert!(contour_spec(&app, &target).negative.is_none());

    let commit = app
        .plan_encoding_reset(EncodingKind::Contour, std::slice::from_ref(&target))
        .expect("encoding reset plans");
    app.commit_property(commit);
    let spec = contour_spec(&app, &target);
    assert!(
        spec.negative.is_some(),
        "the factory restores the negative half a signed field gets by default"
    );
    assert_eq!(spec.positive.count, 14);
}

/// Reading across a heterogeneous selection reports `Mixed` instead of picking
/// one target's value and pretending it speaks for all of them.
#[test]
fn a_heterogeneous_selection_reads_as_mixed() {
    let (mut app, first) = contour_app();
    let object: crate::state::ObjectId =
        first.resource.local_id.as_deref().unwrap().parse().unwrap();
    let second_id = {
        let plot = app.doc.canvases[0]
            .object_mut(object)
            .and_then(|object| object.plot_mut())
            .expect("plot");
        let id = plot.allocate_series_id();
        let mut extra = plot.binding.series[0].clone();
        extra.id = id;
        if let plotx_figure::SeriesEncoding::Contour(spec) = &mut extra.encoding {
            spec.positive.count = 3;
        }
        plot.binding.series.push(extra);
        id
    };
    let second = app.series_target(0, object, second_id).expect("target");
    let set = app.resolve_property_set(contour::COUNT, &[first, second]);
    assert_eq!(set.value, AggregateValue::Mixed);

    let (app, only) = contour_app();
    let set = app.resolve_property_set(contour::COUNT, std::slice::from_ref(&only));
    assert_eq!(set.value, AggregateValue::Uniform(PropertyValue::Int(14)));
    let set = app.resolve_property_set(contour::COUNT, &[]);
    assert_eq!(set.value, AggregateValue::Unavailable);
}

/// A series binding whose id no longer exists must not resolve to a neighbour.
#[test]
fn an_unknown_series_does_not_resolve_to_another_one() {
    let (app, target) = contour_app();
    let stale = TargetRef {
        resource: target.resource.clone(),
        component: Some(ComponentRef::Series(SeriesId::new(4_242))),
    };
    let error = app
        .resolve_property(&PropertyAddress::new(stale, contour::COUNT))
        .expect_err("a stale series id is not a target");
    assert!(matches!(error, PropertyError::UnknownTarget(_)));
}

/// The catalog never grows a parallel value store: a definition describes, and
/// the value stays in the encoding. This pins the property that makes that true
/// — the resolved value always equals what the domain model holds.
#[test]
fn resolved_values_come_from_the_domain_model() {
    let (mut app, target) = contour_app();
    let object: crate::state::ObjectId = target
        .resource
        .local_id
        .as_deref()
        .unwrap()
        .parse()
        .unwrap();
    if let Some(plotx_figure::SeriesEncoding::Contour(spec)) = app.doc.canvases[0]
        .object_mut(object)
        .and_then(|object| object.plot_mut())
        .and_then(|plot| plot.binding.series.first_mut())
        .map(|series| &mut series.encoding)
    {
        // Both halves, so this pins where the value comes from rather than
        // re-testing what an asymmetric ladder reads as.
        spec.positive.count = 21;
        if let Some(negative) = spec.negative.as_mut() {
            negative.count = 21;
        }
    }
    let resolved = app
        .resolve_property(&PropertyAddress::new(target, contour::COUNT))
        .expect("resolves");
    assert_eq!(
        resolved.value,
        AggregateValue::Uniform(PropertyValue::Int(21))
    );
}

/// `SeriesSource.field` says where the values come from; it is not the
/// component of a contour address. Two series of one object reading the very
/// same field must therefore still be told apart, and each keep its own levels.
#[test]
fn the_source_field_is_not_the_component() {
    let (mut app, first) = contour_app();
    let object: crate::state::ObjectId =
        first.resource.local_id.as_deref().unwrap().parse().unwrap();
    let second_id = {
        let plot = app.doc.canvases[0]
            .object_mut(object)
            .and_then(|object| object.plot_mut())
            .expect("plot");
        let id = plot.allocate_series_id();
        let mut extra = plot.binding.series[0].clone();
        extra.id = id;
        plot.binding.series.push(extra);
        id
    };
    let second = app.series_target(0, object, second_id).expect("target");
    let sources: Vec<crate::state::FieldId> = app.doc.canvases[0]
        .object(object)
        .and_then(|object| object.plot())
        .map(|plot| {
            plot.binding
                .series
                .iter()
                .map(|series: &SeriesBinding| series.source.field)
                .collect()
        })
        .expect("plot");
    assert_eq!(
        sources[0], sources[1],
        "both series must read one field for this to prove anything"
    );

    let commit = app
        .plan_property_write(
            contour::COUNT,
            std::slice::from_ref(&second),
            &PropertyValue::Int(3),
        )
        .expect("count is writable");
    app.commit_property(commit);
    assert_eq!(contour_spec(&app, &first).positive.count, 14);
    assert_eq!(contour_spec(&app, &second).positive.count, 3);
}

/// The typed entry point's own out-of-range refusal names both the value that
/// was rejected and the rule that rejected it.
///
/// The automation adapter checks the declared bound before it ever builds a
/// typed value, so this bound is reached only through the panel's path — and
/// a panel user who typed a number and saw "must be greater than 1 and at most
/// 10" still cannot tell which end their number fell off.
#[test]
fn the_typed_planner_names_the_rejected_value_and_the_bound() {
    let (app, target) = contour_app();
    let error = app
        .plan_property_write(
            contour::RATIO,
            std::slice::from_ref(&target),
            &PropertyValue::Float(42.0),
        )
        .expect_err("42 is above the declared ratio bound");
    let message = error.to_string();
    assert!(
        message.contains("42"),
        "the rejected value is named: {message}"
    );
    assert!(
        message.contains("greater than 1") && message.contains("at most 10"),
        "the bound is named: {message}"
    );
}
