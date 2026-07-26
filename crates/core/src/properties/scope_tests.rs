//! What a catalog write is *allowed to reach*, and what context it resolves in.
//!
//! Two failures live here, and both are silent by nature. A reset that rebuilds
//! more than the surface offering it names changes settings the user never
//! looked at, and reports success. A default resolved against a context the
//! target has since left hands the writer a value from a different frame of
//! reference, and either lands a wrong number or refuses forever. Neither is
//! visible without a test that states the boundary.

use super::tests::{contour_app, contour_spec};
use super::*;
use crate::automation::{ComponentRef, TargetRef};
use crate::state::{
    CONTOUR_BASE_ABSOLUTE, CONTOUR_BASE_BACKGROUND_SCALE, CONTOUR_BASE_FRACTION_OF_RANGE,
    CONTOUR_BASE_NOISE_FLOOR, CanvasDocument, Dataset, ObjectFrame, PlotxApp, contour_base_kind,
    default_contour_spec, field_peak_magnitude,
};
use plotx_figure::{HeatmapSpec, SeriesEncoding};

fn object_of(target: &TargetRef) -> crate::state::ObjectId {
    target
        .resource
        .local_id
        .as_deref()
        .expect("a canvas object target carries a local id")
        .parse()
        .expect("the local id is an object id")
}

fn encoding_of(app: &PlotxApp, target: &TargetRef) -> SeriesEncoding {
    let Some(ComponentRef::Series(series)) = target.component else {
        panic!("the fixture addresses a series");
    };
    app.doc.canvases[0]
        .object(object_of(target))
        .and_then(|object| object.plot())
        .expect("plot")
        .binding
        .series
        .iter()
        .find(|candidate| candidate.id == series)
        .expect("series")
        .encoding
        .clone()
}

/// One plot drawing a contour *and* a heatmap from the same field — the case a
/// section-scoped action has to survive. The heatmap is deliberately set away
/// from its factory value, so a reset that reaches it is visible.
fn stacked_app() -> (PlotxApp, TargetRef, TargetRef) {
    let (mut app, contour) = contour_app();
    let object = object_of(&contour);
    let heatmap_id = {
        let plot = app.doc.canvases[0]
            .object_mut(object)
            .and_then(|object| object.plot_mut())
            .expect("plot");
        let id = plot.allocate_series_id();
        let mut extra = plot.binding.series[0].clone();
        extra.id = id;
        extra.encoding = SeriesEncoding::Heatmap(HeatmapSpec {
            value_range: Some([1.0, 9.0]),
            ..HeatmapSpec::default()
        });
        plot.binding.series.push(extra);
        id
    };
    let heatmap = app
        .series_target(0, object, heatmap_id)
        .expect("the heatmap series is addressable");
    (app, contour, heatmap)
}

/// §4.2: resetting *one encoding* is scoped to that encoding.
///
/// The contour section offers "Reset contour" over every series of the objects
/// it is showing, because a series is where the setting lives. Without a scope,
/// that reset walked into the heatmap stacked under the contour, replaced it
/// from the factory, and counted it as a contour in the status line — a
/// destructive edit to a setting whose control was never on screen, reported as
/// a success. The scope belongs to the request rather than to the caller's
/// target list: a property write already gets it from the definition it belongs
/// to, and an encoding reset has no property to get it from.
#[test]
fn resetting_one_encoding_leaves_a_stacked_encoding_untouched() {
    let (mut app, contour, heatmap) = stacked_app();
    let before = encoding_of(&app, &heatmap);
    // Move the contour off its factory encoding first, so the reset has
    // something to do. A reset that finds a series already at its default now
    // reports a skip like every other control in the section, and the scope this
    // test is about would be invisible behind that.
    let levels = contour_spec(&app, &contour).positive.count;
    let commit = app
        .plan_property_write(
            contour::COUNT,
            std::slice::from_ref(&contour),
            &PropertyValue::Int(i64::from(levels) + 1),
        )
        .expect("the level count is writable");
    app.commit_property(commit);

    let commit = app
        .plan_encoding_reset(EncodingKind::Contour, &[contour.clone(), heatmap.clone()])
        .expect("the reset plans");
    assert_eq!(
        commit.applied.len(),
        1,
        "only the contour is in this reset's scope: {:?}",
        commit.applied
    );
    assert_eq!(
        commit.skipped.len(),
        1,
        "the series outside the scope is reported, never silently included"
    );
    assert!(
        commit.skipped[0].message.contains("heatmap"),
        "the reason names what the series actually draws: {}",
        commit.skipped[0].message
    );
    app.commit_property(commit);

    assert_eq!(
        encoding_of(&app, &heatmap),
        before,
        "a reset named for the contour must not rebuild the heatmap beneath it"
    );
    assert!(matches!(
        encoding_of(&app, &contour),
        SeriesEncoding::Contour(_)
    ));
}

/// The complement: the scope is not a way to skip work the caller did ask for.
#[test]
fn an_encoding_reset_still_rebuilds_every_target_in_its_scope() {
    let (mut app, contour, _) = stacked_app();
    let object = object_of(&contour);
    if let Some(SeriesEncoding::Contour(spec)) = app.doc.canvases[0]
        .object_mut(object)
        .and_then(|object| object.plot_mut())
        .and_then(|plot| plot.binding.series.first_mut())
        .map(|series| &mut series.encoding)
    {
        spec.positive.count = 2;
    }
    let commit = app
        .plan_encoding_reset(EncodingKind::Contour, std::slice::from_ref(&contour))
        .expect("the reset plans");
    app.commit_property(commit);
    assert_eq!(contour_spec(&app, &contour).positive.count, 14);
}

/// A bounded, unsigned 2D field: the AFM height map the design names as the
/// case whose background is not centred on zero. Its capabilities admit
/// `BackgroundScale` (the factory's choice) *and* `FractionOfRange`, which is
/// what makes it the field where re-anchoring and resetting can disagree.
fn afm_contour_app() -> (PlotxApp, TargetRef) {
    let channel = plotx_io::AfmImageChannel {
        name: "Height".to_owned(),
        width: 4,
        height: 4,
        scan_size_x: 3.0,
        scan_size_y: 3.0,
        lateral_unit: "nm".to_owned(),
        scale: plotx_io::AfmScale {
            multiplier: 1.0,
            offset: 0.0,
            unit: "nm".to_owned(),
        },
        raw: std::sync::Arc::from((0..16).collect::<Vec<i32>>()),
        frame_direction: plotx_io::AfmFrameDirection::Trace,
    };
    let mut app = PlotxApp::new();
    app.doc
        .datasets
        .push(Dataset::Afm(Box::new(crate::state::AfmDataset::load(
            plotx_io::AfmData {
                images: vec![channel],
                forces: None,
                source: "anchor scope".to_owned(),
                import_warnings: Vec::new(),
            },
        ))));
    let mut canvas = CanvasDocument::new("page".to_owned(), [120.0, 80.0]);
    let id = canvas.allocate_object_id();
    let object =
        app.build_plot_object(0, ObjectFrame::new(0.0, 0.0, 100.0, 80.0), id, "Map".into());
    canvas.objects.push(object);
    app.doc.canvases.push(canvas);
    app.session.active_canvas = Some(0);

    // A height map defaults to a heatmap; the encoding a user picks from the
    // chart gallery is what puts contour properties on it. §4.2: any field with
    // a regular scalar grid can carry either.
    let field = app.doc.canvases[0]
        .object(id)
        .and_then(|object| object.plot())
        .and_then(|plot| plot.binding.series.first())
        .map(|series| series.source.field)
        .expect("one series");
    let capabilities = app.doc.datasets[0]
        .field_descriptor(field)
        .map(|descriptor| descriptor.capabilities)
        .expect("the height map is a field");
    let encoding = SeriesEncoding::Contour(default_contour_spec(&capabilities, &|| {
        field_peak_magnitude(&app.doc.datasets[0], field)
    }));
    let series = {
        let series = app.doc.canvases[0]
            .object_mut(id)
            .and_then(|object| object.plot_mut())
            .and_then(|plot| plot.binding.series.first_mut())
            .expect("one series");
        series.encoding = encoding;
        series.id
    };
    let target = app
        .series_target(0, id, series)
        .expect("the series is addressable");
    (app, target)
}

#[test]
fn the_afm_fixture_anchors_on_the_background_and_admits_a_fraction() {
    let (app, target) = afm_contour_app();
    assert_eq!(
        contour_base_kind(&contour_spec(&app, &target).positive.base),
        CONTOUR_BASE_BACKGROUND_SCALE
    );
    let resolved = app
        .resolve_property(&PropertyAddress::new(target.clone(), contour::BASE_POLICY))
        .expect("the anchor resolves");
    let ResolvedSchema::Enum { variants } = resolved.schema else {
        panic!("the anchor is a choice");
    };
    assert!(
        variants
            .iter()
            .any(|variant| variant.id == CONTOUR_BASE_FRACTION_OF_RANGE),
        "an unsigned bounded field admits a fraction of its range"
    );
}

/// §8.1: the default is *derived in the target's current context*, and the
/// anchor is part of that context.
///
/// A magnitude means nothing on its own — "5" is five times a measured spread
/// under one anchor and five times the whole value range under another. Reading
/// the default off the factory's spec therefore answered with a number from a
/// frame of reference the target had left. On this field the factory anchors on
/// the background with a multiplier of 5, so a user who switches to a fraction
/// of the range and resets got 5 handed to a policy whose ceiling is 1: the
/// reset failed, every time, with no way to make it succeed.
#[test]
fn resetting_the_lowest_level_re_derives_it_under_the_anchor_in_force() {
    let (mut app, target) = afm_contour_app();
    let targets = std::slice::from_ref(&target);
    let commit = app
        .plan_property_write(
            contour::BASE_POLICY,
            targets,
            &PropertyValue::Enum(CONTOUR_BASE_FRACTION_OF_RANGE),
        )
        .expect("a bounded unsigned field admits a fraction anchor");
    app.commit_property(commit);

    let address = PropertyAddress::new(target.clone(), contour::BASE_MAGNITUDE);
    let resolved = app.resolve_property(&address).expect("the level resolves");
    assert_eq!(
        resolved.default_value,
        Some(PropertyValue::Float(0.04)),
        "the default is the magnitude a fresh base of the anchor in force carries, \
         not the multiplier the factory's own anchor happened to use"
    );

    let commit = app
        .plan_property_reset(contour::BASE_MAGNITUDE, targets)
        .expect("a reset under the anchor in force is a value that anchor accepts");
    app.commit_property(commit);
    let spec = contour_spec(&app, &target);
    assert_eq!(
        contour_base_kind(&spec.positive.base),
        CONTOUR_BASE_FRACTION_OF_RANGE,
        "resetting one property re-derives that property, and leaves the anchor alone"
    );
    assert_eq!(
        app.resolve_property(&address).expect("resolves").value,
        AggregateValue::Uniform(PropertyValue::Float(0.04))
    );
}

/// The same rule on a signed field, where the two anchors differ by orders of
/// magnitude rather than by a ceiling: an absolute level reset to the factory's
/// `5` would be five raw intensity units, which on this spectrum is a threshold
/// nobody chose.
#[test]
fn an_absolute_anchor_resets_to_a_level_and_not_to_a_multiplier() {
    let (mut app, target) = contour_app();
    let targets = std::slice::from_ref(&target);
    assert_eq!(
        contour_base_kind(&contour_spec(&app, &target).positive.base),
        CONTOUR_BASE_NOISE_FLOOR
    );
    let commit = app
        .plan_property_write(
            contour::BASE_POLICY,
            targets,
            &PropertyValue::Enum(CONTOUR_BASE_ABSOLUTE),
        )
        .expect("an absolute level needs no capability");
    app.commit_property(commit);

    let anchored = super::contour::base_magnitude(&contour_spec(&app, &target).positive.base);
    let commit = app
        .plan_property_reset(contour::BASE_MAGNITUDE, targets)
        .expect("the reset plans");
    app.commit_property(commit);
    let after = super::contour::base_magnitude(&contour_spec(&app, &target).positive.base);
    assert!(
        (after - anchored).abs() < 1.0e-12,
        "an absolute base freshly anchored to this field is what the reset must \
         produce; got {after} where re-anchoring produces {anchored}"
    );
    assert_ne!(
        after, 5.0,
        "the noise multiplier belongs to the anchor the target no longer holds"
    );
}

/// §4.3 declares `ratio > 1.0`. A schema that admits exactly one while the
/// writer refuses it is one rule written twice, and the control was built from
/// the half that was wrong: the value could be entered and never stored.
#[test]
fn the_level_ratio_is_open_at_one_in_the_schema_the_control_is_built_from() {
    let bounds = definition(contour::RATIO)
        .expect("the ratio is registered")
        .value_schema
        .float_bounds()
        .expect("the ratio is a float");
    assert!(bounds.exclusive_min, "a ratio of one draws one level twice");
    assert!(!bounds.admits(1.0));
    assert!(bounds.lowest() > 1.0);

    let (mut app, target) = contour_app();
    let targets = std::slice::from_ref(&target);
    let refused = app
        .plan_property_write(contour::RATIO, targets, &PropertyValue::Float(1.0))
        .expect_err("the writer refuses the bound itself");
    assert!(matches!(refused, PropertyError::InvalidValue { .. }));

    // The floor a control offers has to be a value the writer takes. This is
    // the pairing that drifted: the two ends now read the same declaration.
    let commit = app
        .plan_property_write(
            contour::RATIO,
            targets,
            &PropertyValue::Float(bounds.lowest()),
        )
        .expect("the smallest value the schema admits is storable");
    app.commit_property(commit);
    assert!(contour_spec(&app, &target).positive.ratio.get() > 1.0);
}

/// The readout of a stacked plot follows applicability, so this pins what
/// "applicable" answers there: the contour, not whichever series is first.
#[test]
fn applicability_names_the_contour_under_a_heatmap_drawn_first() {
    let (mut app, contour, heatmap) = stacked_app();
    // Put the heatmap first, which is what a heatmap with a contour drawn over
    // it looks like.
    {
        let plot = app.doc.canvases[0]
            .object_mut(object_of(&contour))
            .and_then(|object| object.plot_mut())
            .expect("plot");
        plot.binding.series.reverse();
    }
    let targets = app.series_targets(0, object_of(&contour));
    assert_eq!(
        targets.first(),
        Some(&heatmap),
        "the heatmap is drawn first"
    );
    let set = app.resolve_property_set(contour::BASE_MAGNITUDE, &targets);
    assert_eq!(
        set.applicable_targets
            .iter()
            .map(|address| address.target.clone())
            .collect::<Vec<_>>(),
        vec![contour],
        "only the series that draws a contour has a lowest contour level"
    );
}

/// "Reset contour" is a control in the same section as the individual settings,
/// and has to answer the same way they do. Reporting "Updated 1 contour series."
/// for a series that was already at its factory encoding told the user something
/// happened when nothing did.
#[test]
fn resetting_an_encoding_that_is_already_the_factorys_reports_a_skip() {
    let (mut app, contour) = contour_app();
    let commit = app
        .plan_encoding_reset(EncodingKind::Contour, std::slice::from_ref(&contour))
        .expect("the reset plans");
    assert!(
        commit.applied.is_empty(),
        "a fresh series is what the factory produces: {:?}",
        commit.applied
    );
    assert_eq!(commit.skipped.len(), 1);
    assert_eq!(commit.skipped[0].reason, SkipReason::AlreadyAtValue);
    assert_eq!(app.commit_property(commit), 0);

    // And it still applies once the series has actually moved.
    let levels = contour_spec(&app, &contour).positive.count;
    let commit = app
        .plan_property_write(
            contour::COUNT,
            std::slice::from_ref(&contour),
            &PropertyValue::Int(i64::from(levels) + 1),
        )
        .expect("the level count is writable");
    app.commit_property(commit);
    let commit = app
        .plan_encoding_reset(EncodingKind::Contour, std::slice::from_ref(&contour))
        .expect("the reset plans");
    assert_eq!(commit.applied.len(), 1);
    assert!(commit.skipped.is_empty());
}
