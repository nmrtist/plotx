//! Cross-scope provider slices that would otherwise make the contour fixture
//! module too large.

use super::*;

/// The document scope is not a disguised canvas-object scope. Typography can
/// be addressed before a page or plot exists, and its provider compiles the
/// edit to the document action that already owns undo/rebuild semantics.
#[test]
fn document_typography_is_addressable_without_a_canvas_object() {
    let mut app = PlotxApp::new();
    let target = app.document_target();
    assert!(target.component.is_none());
    assert_eq!(target.resource.kind.0, "plotx.document");

    let address = PropertyAddress::new(target.clone(), typography::TICK_PT);
    let before = app
        .resolve_property(&address)
        .expect("the document resolves");
    assert_eq!(
        before.value,
        AggregateValue::Uniform(PropertyValue::Float(7.0))
    );
    assert_eq!(before.default_value, Some(PropertyValue::Float(7.0)));

    let commit = app
        .plan_property_write(
            typography::TICK_PT,
            std::slice::from_ref(&target),
            &PropertyValue::Float(9.5),
        )
        .expect("the document property plans");
    let Some(Action::Composite(actions)) = &commit.document_action else {
        panic!("a catalog commit is composite");
    };
    assert!(matches!(
        actions.as_slice(),
        [Action::SetFigureTypography { .. }]
    ));
    assert_eq!(app.commit_property(commit), 1);
    assert_eq!(app.doc.style_library.figure_typography.tick_pt, 9.5);
}

#[test]
fn every_document_typography_property_resets_to_its_declared_default() {
    let mut app = PlotxApp::new();
    let target = app.document_target();
    for (property, changed) in [
        (typography::TICK_PT, 12.0),
        (typography::LABEL_PT, 13.0),
        (typography::TITLE_PT, 14.0),
        (typography::LEGEND_PT, 6.0),
    ] {
        let commit = app
            .plan_property_write(
                property,
                std::slice::from_ref(&target),
                &PropertyValue::Float(changed),
            )
            .expect("typography write plans");
        app.commit_property(commit);
        let reset = app
            .plan_property_reset(property, std::slice::from_ref(&target))
            .expect("typography reset plans");
        assert_eq!(reset.applied.len(), 1, "{property}");
        app.commit_property(reset);
        let resolved = app
            .resolve_property(&PropertyAddress::new(target.clone(), property))
            .expect("typography resolves after reset");
        assert_eq!(resolved.value.uniform(), resolved.default_value.as_ref());
    }
}

#[test]
fn all_typography_sizes_share_the_declared_point_schema() {
    for property in [
        typography::TICK_PT,
        typography::LABEL_PT,
        typography::TITLE_PT,
        typography::LEGEND_PT,
    ] {
        let definition = definition(property).expect("typography is registered");
        assert_eq!(
            definition.value_schema,
            ValueSchema::Float {
                bounds: FloatBounds::inclusive(1.0, 72.0),
                display: FloatDisplay::Linear("pt"),
                drag_step: Some(0.25),
            }
        );
        assert_eq!(definition.tier, Tier::Essential);
    }
}

#[test]
fn legend_text_color_uses_the_document_typography_action() {
    let mut app = PlotxApp::new();
    let target = app.document_target();
    let color = plotx_figure::Color::rgb(12, 34, 56);
    let commit = app
        .plan_property_write(
            typography::LEGEND_COLOR,
            std::slice::from_ref(&target),
            &PropertyValue::Color(color),
        )
        .expect("legend color plans");
    app.commit_property(commit);
    assert_eq!(app.doc.style_library.figure_typography.legend_color, color);
    app.undo();
    assert_eq!(
        app.doc.style_library.figure_typography.legend_color,
        plotx_figure::Color::AXIS
    );
}

#[test]
fn data_line_widths_share_the_fine_point_schema() {
    for property in [line::STROKE_WIDTH, contour::LINE_WIDTH] {
        let definition = definition(property).expect("line width is registered");
        assert_eq!(
            definition.value_schema,
            ValueSchema::Float {
                bounds: FloatBounds::inclusive(0.05, 10.0),
                display: FloatDisplay::Linear("pt"),
                drag_step: Some(0.05),
            }
        );
        assert_eq!(definition.tier, Tier::Essential);
    }
}

/// A write of the value already in typed storage is not an applied edit. The
/// caller gets an explicit skip, and the empty composite cannot create a fake
/// undo/revision entry.
#[test]
fn a_same_value_write_is_reported_without_an_empty_commit() {
    let mut app = PlotxApp::new();
    let target = app.document_target();
    let commit = app
        .plan_property_write(
            typography::TICK_PT,
            std::slice::from_ref(&target),
            &PropertyValue::Float(7.0),
        )
        .expect("the existing value is a valid request");
    assert!(
        commit.applied.is_empty(),
        "a no-op is not reported as applied"
    );
    assert_eq!(commit.skipped.len(), 1);
    assert!(commit.skipped[0].message.contains("already has that value"));
    assert!(
        commit.document_action.is_none(),
        "the transaction contains no document payload"
    );
    let revision = app.doc.automation_revision;
    assert_eq!(app.commit_property(commit), 0);
    assert_eq!(
        app.doc.automation_revision, revision,
        "a no-op does not create an automation revision"
    );
}

/// A heterogeneous plot selection must retain both facts: the two line series
/// disagree (`Mixed`), and a contour in the same selection was not silently
/// treated as a line. Compatible targets still compile to one atomic composite.
#[test]
fn line_stroke_width_reports_mixed_values_and_skips_other_encodings() {
    let (mut app, contour) = contour_app();
    app.doc
        .datasets
        .push(Dataset::Nmr(Box::new(NmrDataset::load(nmr1d_with(
            "lines",
        )))));
    let mut line_targets = Vec::new();
    for name in ["Line A", "Line B"] {
        let id = app.doc.canvases[0].allocate_object_id();
        let object = app.build_plot_object(
            1,
            ObjectFrame::new(0.0, 0.0, 100.0, 40.0),
            id,
            name.to_owned(),
        );
        app.doc.canvases[0].objects.push(object);
        line_targets.push(
            app.series_targets(0, id)
                .into_iter()
                .next()
                .expect("one line series"),
        );
    }
    let second = line_targets[1].clone();
    let object: crate::state::ObjectId = second
        .resource
        .local_id
        .as_deref()
        .expect("object id")
        .parse()
        .expect("object id parses");
    let Some(ComponentRef::Series(series)) = second.component else {
        panic!("line target addresses its series");
    };
    let line = app.doc.canvases[0]
        .object_mut(object)
        .and_then(|object| object.plot_mut())
        .and_then(|plot| {
            plot.binding
                .series
                .iter_mut()
                .find(|candidate| candidate.id == series)
        })
        .expect("second line series");
    let plotx_figure::SeriesEncoding::Line(line) = &mut line.encoding else {
        panic!("the 1D field materializes a line encoding");
    };
    line.width = plotx_figure::PositiveFiniteF32::new(2.0).expect("literal width");

    let targets = vec![
        line_targets[0].clone(),
        line_targets[1].clone(),
        contour.clone(),
    ];
    let set = app.resolve_property_set(line::STROKE_WIDTH, &targets);
    assert_eq!(set.value, AggregateValue::Mixed);
    assert_eq!(set.applicable_targets.len(), 2);
    assert_eq!(set.skipped_targets.len(), 1);
    assert!(
        set.skipped_targets[0].message.contains("line")
            && set.skipped_targets[0].message.contains("contour"),
        "the incompatible target is named rather than discarded: {}",
        set.skipped_targets[0].message
    );

    let commit = app
        .plan_property_write(line::STROKE_WIDTH, &targets, &PropertyValue::Float(3.0))
        .expect("the compatible line series plan together");
    assert_eq!(commit.applied.len(), 2);
    assert_eq!(commit.skipped.len(), 1);
    let Some(Action::Composite(actions)) = &commit.document_action else {
        panic!("a multi-object edit is composite");
    };
    assert_eq!(
        actions.len(),
        2,
        "each line object keeps its typed binding action"
    );
    assert!(
        actions
            .iter()
            .all(|action| matches!(action, Action::SetSeriesPresentation { .. }))
    );
    app.commit_property(commit);
    for target in line_targets {
        let resolved = app
            .resolve_property(&PropertyAddress::new(target, line::STROKE_WIDTH))
            .expect("line width still resolves");
        assert_eq!(
            resolved.value,
            AggregateValue::Uniform(PropertyValue::Float(3.0))
        );
    }
    assert!(
        matches!(
            app.resolve_property(&PropertyAddress::new(contour, line::STROKE_WIDTH)),
            Err(PropertyError::NotApplicable(_))
        ),
        "the contour did not receive a line-only write"
    );
}

/// A second steppable encoding reads through the property-address dispatch, not
/// through the old contour-only target lookup. Its ordinary scalar payload is
/// deliberately distinct from the contour provider's richer semantic payload.
#[test]
fn a_line_readout_dispatches_by_property_address() {
    let (mut app, _) = contour_app();
    app.doc
        .datasets
        .push(Dataset::Nmr(Box::new(NmrDataset::load(nmr1d_with(
            "line readout",
        )))));
    let id = app.doc.canvases[0].allocate_object_id();
    let object = app.build_plot_object(
        1,
        ObjectFrame::new(0.0, 0.0, 100.0, 40.0),
        id,
        "Line".to_owned(),
    );
    app.doc.canvases[0].objects.push(object);
    let target = app
        .series_targets(0, id)
        .into_iter()
        .next()
        .expect("one line series");

    assert_eq!(
        app.property_readout(&PropertyAddress::new(target, line::STROKE_WIDTH))
            .expect("the line readout resolves"),
        PropertyReadout::Value(PropertyValue::Float(f64::from(
            plotx_figure::DEFAULT_DATA_LINE_WIDTH_PT,
        )))
    );
}
