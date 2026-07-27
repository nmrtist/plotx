//! Multi-target aggregation and domain-value resolution tests.

use super::*;

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
