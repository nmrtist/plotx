use super::tests::contour_app;
use super::*;
use crate::automation::{ComponentRef, TargetRef};
use crate::state::PlotxApp;
use plotx_figure::{HeatmapSpec, SeriesEncoding};

fn heatmap_app() -> (PlotxApp, TargetRef) {
    let (mut app, target) = contour_app();
    let Some(ComponentRef::Series(series_id)) = target.component else {
        panic!("fixture target is a series");
    };
    let object = target
        .resource
        .local_id
        .as_deref()
        .unwrap()
        .parse()
        .unwrap();
    let series = app.doc.canvases[0]
        .object_mut(object)
        .and_then(|object| object.plot_mut())
        .unwrap()
        .binding
        .series
        .iter_mut()
        .find(|series| series.id == series_id)
        .unwrap();
    series.encoding = SeriesEncoding::Heatmap(HeatmapSpec::default());
    (app, target)
}

fn heatmap_spec(app: &PlotxApp, target: &TargetRef) -> HeatmapSpec {
    let Some(ComponentRef::Series(series_id)) = target.component else {
        panic!("target is a series");
    };
    let object = target
        .resource
        .local_id
        .as_deref()
        .unwrap()
        .parse()
        .unwrap();
    let series = app.doc.canvases[0]
        .object(object)
        .and_then(|object| object.plot())
        .unwrap()
        .binding
        .series
        .iter()
        .find(|series| series.id == series_id)
        .unwrap();
    let SeriesEncoding::Heatmap(spec) = &series.encoding else {
        panic!("series is a heatmap");
    };
    spec.clone()
}

#[test]
fn automatic_range_is_read_from_the_scalar_field_summary() {
    let (app, target) = heatmap_app();
    let span = app
        .resolve_property(&PropertyAddress::new(target.clone(), heatmap::RANGE_SPAN))
        .unwrap();
    let center = app
        .resolve_property(&PropertyAddress::new(target, heatmap::RANGE_CENTER))
        .unwrap();
    let Some(PropertyValue::Float(span_value)) = span.value.uniform() else {
        panic!("span resolves to a number");
    };
    let Some(PropertyValue::Float(center_value)) = center.value.uniform() else {
        panic!("centre resolves to a number");
    };
    assert!(*span_value > 0.0);
    assert!(center_value.is_finite());
    assert_eq!(span.default_value, Some(PropertyValue::Float(*span_value)));
    assert!(!span.is_modified());
}

#[test]
fn stepping_span_preserves_center_and_reset_restores_auto_range() {
    let (mut app, target) = heatmap_app();
    let before_span = app
        .resolve_property(&PropertyAddress::new(target.clone(), heatmap::RANGE_SPAN))
        .unwrap()
        .value
        .uniform()
        .and_then(PropertyValue::as_float)
        .unwrap();
    let before_center = app
        .resolve_property(&PropertyAddress::new(target.clone(), heatmap::RANGE_CENTER))
        .unwrap()
        .value
        .uniform()
        .and_then(PropertyValue::as_float)
        .unwrap();
    let commit = app
        .plan_property_step(
            heatmap::RANGE_SPAN,
            std::slice::from_ref(&target),
            PropertyStep::Lower,
        )
        .unwrap();
    assert_eq!(app.commit_property(commit), 1);
    let [lo, hi] = heatmap_spec(&app, &target).value_range.unwrap();
    assert!(((f64::from(lo) + f64::from(hi)) * 0.5 - before_center).abs() < 1.0e-5);
    assert!((f64::from(hi) - f64::from(lo) - before_span / 1.2).abs() < 1.0e-5);
    assert!(
        app.resolve_property(&PropertyAddress::new(target.clone(), heatmap::RANGE_SPAN))
            .unwrap()
            .is_modified()
    );

    let commit = app
        .plan_property_reset(heatmap::RANGE_SPAN, std::slice::from_ref(&target))
        .unwrap();
    app.commit_property(commit);
    assert_eq!(heatmap_spec(&app, &target).value_range, None);
}

#[test]
fn presentation_edits_preserve_the_spatial_viewport_and_name_undo() {
    let (mut app, target) = heatmap_app();
    let object = target
        .resource
        .local_id
        .as_deref()
        .unwrap()
        .parse()
        .unwrap();
    let plot = app.doc.canvases[0]
        .object_mut(object)
        .and_then(|object| object.plot_mut())
        .unwrap();
    let figure = plot.figure().clone();
    let anchor = (figure.x.min + figure.x.max) * 0.5;
    plot.viewport.zoom_x(&figure, anchor, 0.5);
    plot.apply_viewport();
    let viewport = plot.viewport.clone();

    let commit = app
        .plan_property_write(
            heatmap::RANGE_CENTER,
            std::slice::from_ref(&target),
            &PropertyValue::Float(1.0),
        )
        .unwrap();
    app.commit_property(commit);
    assert_eq!(
        app.doc.canvases[0]
            .object(object)
            .and_then(|object| object.plot())
            .unwrap()
            .viewport,
        viewport
    );

    app.undo();
    assert_eq!(app.session.status, "Undid display setting.");
}
