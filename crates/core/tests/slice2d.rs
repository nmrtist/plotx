//! End-to-end 2D slice: synthetic 2D FID → 2D FFT → contour / stack figure →
//! SVG export. No files needed.
//!
//! The same harness also drives the property catalog's two entry points against
//! one document, because the thing automation can break here is not the
//! numerics — those are the same code either way — but the JSON adapter quietly
//! growing a second copy of the planner.

use num_complex::Complex64;
use plotx_core::automation::{
    CallerType, DocumentRevision, TOOL_RESET, TOOL_SET, TargetOutcome, TargetRef, TargetSelector,
    ToolRequest, execute_tool, plan_tool,
};
use plotx_core::build_stack_figure;
use plotx_core::properties::{PropertyId, PropertyValue, contour};
use plotx_core::state::{
    CanvasDocument, Dataset, Nmr2DDataset, ObjectFrame, ObjectId, PlotxApp, SeriesBinding,
};
use plotx_io::{Dim, Domain, NmrData2D, QuadMode};
use plotx_processing::{Layout2D, Params2D, Preset2D, Processed2D, process_2d, recommend_preset};
use std::f64::consts::TAU;
use std::thread;
use std::time::{Duration, Instant};

fn dim(sw: f64, obs: f64, nucleus: &str) -> Dim {
    Dim {
        spectral_width_hz: sw,
        observe_freq_mhz: obs,
        carrier_ppm: 0.0,
        nucleus: nucleus.into(),
        group_delay: 0.0,
    }
}

/// A phase-modulated 2D FID with a single cross peak at `(f2_ppm, f1_ppm)`.
fn synthetic_hsqc(f2_ppm: f64, f1_ppm: f64, experiment: &str) -> NmrData2D {
    let (cols, rows) = (256usize, 128usize);
    let direct = dim(4000.0, 400.0, "1H");
    let indirect = dim(4000.0, 100.0, "13C");
    let dt2 = 1.0 / direct.spectral_width_hz;
    let dt1 = 1.0 / indirect.spectral_width_hz;
    let f2_hz = f2_ppm * direct.observe_freq_mhz;
    let f1_hz = f1_ppm * indirect.observe_freq_mhz;
    let mut data = Vec::with_capacity(rows * cols);
    for k in 0..rows {
        let t1 = k as f64 * dt1;
        for j in 0..cols {
            let t2 = j as f64 * dt2;
            let decay = (-t2 / 0.3 - t1 / 0.3).exp();
            data.push(Complex64::from_polar(
                decay,
                TAU * (f2_hz * t2 + f1_hz * t1),
            ));
        }
    }
    NmrData2D {
        data,
        rows,
        cols,
        domain: Domain::Time,
        direct,
        indirect,
        quad: QuadMode::Complex,
        indirect_conjugate: false,
        experiment: Some(experiment.to_owned()),
        pseudo_axis: None,
        diffusion: None,
        nus: None,
        source: "synthetic HSQC".into(),
    }
}

#[test]
fn contour_slice_places_peak_and_exports_svg() {
    // Shifts stay inside the ±SW/2 Nyquist range (F1: 10 ppm × 100 MHz = 1 kHz).
    let data = synthetic_hsqc(3.0, 10.0, "hsqcetgpsisp");
    let preset = recommend_preset(&data);
    assert_eq!(preset, Preset2D::Hsqc);
    assert_eq!(preset.layout(), Layout2D::Ft);

    let spec = match process_2d(&data, &Params2D::default()) {
        Processed2D::Ft(s) => s,
        Processed2D::Stack(_) => panic!("expected Ft"),
    };
    let mag = spec.magnitude();
    let (mut best, mut br, mut bc) = (f32::MIN, 0, 0);
    for r in 0..spec.f1_size {
        for c in 0..spec.f2_size {
            let v = mag[r * spec.f2_size + c];
            if v > best {
                best = v;
                br = r;
                bc = c;
            }
        }
    }
    assert!(
        (spec.f2_ppm[bc] - 3.0).abs() < 0.1,
        "F2 {}",
        spec.f2_ppm[bc]
    );
    assert!(
        (spec.f1_ppm[br] - 10.0).abs() < 0.5,
        "F1 {}",
        spec.f1_ppm[br]
    );

    let mut app = PlotxApp::new();
    app.doc
        .datasets
        .push(Dataset::Nmr2D(Box::new(Nmr2DDataset::load(data))));
    let mut canvas = CanvasDocument::new("contour".to_owned(), [120.0, 80.0]);
    let [width, height] = canvas.size_pt();
    let object = app.build_plot_object(
        0,
        ObjectFrame::new(0.0, 0.0, width, height),
        canvas.allocate_object_id(),
        "Contour".to_owned(),
    );
    canvas.objects.push(object);
    app.doc.canvases.push(canvas);

    let deadline = Instant::now() + Duration::from_secs(2);
    while app.compute_busy() && Instant::now() < deadline {
        app.poll_compute();
        thread::sleep(Duration::from_millis(5));
    }
    app.poll_compute();
    assert!(!app.compute_busy(), "contour jobs did not settle");
    let fig = app.doc.canvases[0].objects[0]
        .plot()
        .unwrap()
        .figure
        .clone();
    assert!(!fig.contours.is_empty());
    assert!(!fig.contours[0].segments.is_empty());

    let svg = plotx_render::svg::export(&fig);
    assert!(svg.starts_with("<svg"));
    assert!(svg.contains("<path"), "contour path present");
    assert!(svg.contains("chemical shift"));
}

// ---------------------------------------------------------------------------
// The property catalog's two entry points, over the same real contour
// ---------------------------------------------------------------------------

/// Wait for the asynchronous contour work a commit triggers.
///
/// Asserting geometry before the build settles reads the *previous* answer and
/// passes for the wrong reason, so every check that looks at a figure goes
/// through here first.
fn settle(app: &mut PlotxApp) {
    let deadline = Instant::now() + Duration::from_secs(2);
    while app.compute_busy() && Instant::now() < deadline {
        app.poll_compute();
        thread::sleep(Duration::from_millis(5));
    }
    app.poll_compute();
    assert!(!app.compute_busy(), "contour jobs did not settle");
}

/// A page holding one plot of a real processed 2D spectrum, settled.
///
/// Built from the same synthetic FID as the slice above and run through the
/// same `process_2d`, so the field the catalog sees is a genuine spectrum with
/// a genuine noise estimate rather than a hand-written grid.
fn contour_page() -> (PlotxApp, ObjectId) {
    let mut app = PlotxApp::new();
    app.doc
        .datasets
        .push(Dataset::Nmr2D(Box::new(Nmr2DDataset::load(
            synthetic_hsqc(3.0, 10.0, "hsqcetgpsisp"),
        ))));
    let mut canvas = CanvasDocument::new("contour".to_owned(), [120.0, 80.0]);
    let [width, height] = canvas.size_pt();
    let id = canvas.allocate_object_id();
    let object = app.build_plot_object(
        0,
        ObjectFrame::new(0.0, 0.0, width, height),
        id,
        "Contour".to_owned(),
    );
    canvas.objects.push(object);
    app.doc.canvases.push(canvas);
    app.session.active_canvas = Some(0);
    settle(&mut app);
    (app, id)
}

fn object_resource_id(app: &PlotxApp, object: ObjectId) -> String {
    format!("{}/{object}", app.doc.canvases[0].resource_id)
}

fn series_targets(app: &PlotxApp, object: ObjectId) -> Vec<TargetRef> {
    app.series_targets(0, object)
}

/// Every series of the plot, reduced to the state a property write owns.
///
/// Comparing this whole rather than just the number that was written is what
/// makes the differential check meaningful: an adapter that also rounded a
/// neighbouring field, wrote only one half of the ladder, or touched a series
/// it should have skipped would still pass "did the value change?" and fails
/// here. The dataset's own UUID is excluded because it is minted per document
/// and identifies the fixture, not the edit.
fn binding(app: &PlotxApp, object: ObjectId) -> Vec<Comparable> {
    app.doc.canvases[0]
        .object(object)
        .and_then(|object| object.plot())
        .expect("the page holds a plot")
        .binding
        .series
        .iter()
        .map(Comparable::of)
        .collect()
}

#[derive(Clone, Debug, PartialEq)]
struct Comparable {
    series: plotx_core::state::SeriesId,
    field: plotx_core::state::FieldId,
    visible: bool,
    label: Option<String>,
    encoding: plotx_figure::SeriesEncoding,
}

impl Comparable {
    fn of(series: &SeriesBinding) -> Self {
        Self {
            series: series.id,
            field: series.source.field,
            visible: series.visible,
            label: series.label.clone(),
            encoding: series.encoding.clone(),
        }
    }
}

fn tool_request(
    app: &PlotxApp,
    tool_id: &str,
    object: ObjectId,
    parameters: serde_json::Value,
) -> ToolRequest {
    ToolRequest {
        tool_id: tool_id.to_owned(),
        tool_version: 1,
        parameters,
        targets: TargetSelector::Explicit {
            ids: vec![object_resource_id(app, object)],
        },
        expected_revision: DocumentRevision(app.doc.automation_revision),
        caller: CallerType::Agent,
    }
}

fn run_tool(
    app: &mut PlotxApp,
    tool_id: &str,
    object: ObjectId,
    parameters: serde_json::Value,
) -> plotx_core::automation::ToolResult {
    let request = tool_request(app, tool_id, object, parameters);
    let plan = plan_tool(app, request).expect("the tool plans");
    let authority = plan.required_authority;
    execute_tool(app, plan, authority).expect("the tool executes")
}

/// Write one property through the typed entry point the panel uses.
fn write_typed(app: &mut PlotxApp, object: ObjectId, property: PropertyId, value: PropertyValue) {
    let targets = series_targets(app, object);
    let commit = app
        .plan_property_write(property, &targets, &value)
        .expect("the typed planner accepts the value");
    app.commit_property(commit);
}

/// The differential check this stage exists for.
///
/// One document, one target, one value, through the panel's typed planner and
/// through `properties.set`. The resulting document state must be identical
/// field for field. An adapter that re-implemented planning would still make
/// "did the number change?" pass; it cannot make this pass, because any
/// difference in clamping, rounding, which halves of the ladder are written, or
/// which series are touched shows up in the compared bindings.
#[test]
fn typed_and_json_property_writes_produce_identical_documents() {
    for (property, typed, json) in [
        (
            contour::COUNT,
            PropertyValue::Int(11),
            serde_json::json!(11),
        ),
        (
            contour::RATIO,
            PropertyValue::Float(1.75),
            serde_json::json!(1.75),
        ),
        (
            contour::BASE_MAGNITUDE,
            PropertyValue::Float(7.5),
            serde_json::json!(7.5),
        ),
        (
            contour::NEGATIVE_ENABLED,
            PropertyValue::Bool(false),
            serde_json::json!(false),
        ),
        (
            contour::LINE_WIDTH,
            PropertyValue::Float(0.9),
            serde_json::json!(0.9),
        ),
    ] {
        let (mut typed_app, typed_object) = contour_page();
        let (mut json_app, json_object) = contour_page();
        assert_eq!(
            binding(&typed_app, typed_object),
            binding(&json_app, json_object),
            "the two pages start identical"
        );

        write_typed(&mut typed_app, typed_object, property, typed);
        settle(&mut typed_app);

        let result = run_tool(
            &mut json_app,
            TOOL_SET,
            json_object,
            serde_json::json!({"key": property.as_str(), "value": json}),
        );
        settle(&mut json_app);

        assert_eq!(
            binding(&json_app, json_object),
            binding(&typed_app, typed_object),
            "{property} must land identically through both entry points"
        );
        assert!(
            result
                .targets
                .iter()
                .any(|target| target.outcome == TargetOutcome::Succeeded),
            "{property} applied through the JSON entry point"
        );
        assert_eq!(
            result
                .targets
                .iter()
                .filter(|target| target.outcome == TargetOutcome::Skipped)
                .count(),
            0,
            "{property} skipped nothing on a page whose only series is a contour"
        );
    }
}

/// The same equivalence for a reset, which derives its value from the default
/// policy in the target's current context rather than taking one from the
/// caller. A reset is where an adapter is most tempted to reach for a literal.
#[test]
fn typed_and_json_property_resets_produce_identical_documents() {
    let property = contour::COUNT;
    let (mut typed_app, typed_object) = contour_page();
    let (mut json_app, json_object) = contour_page();

    // Move both pages off the default first, or the reset would be a no-op and
    // the comparison would hold for the wrong reason.
    write_typed(
        &mut typed_app,
        typed_object,
        property,
        PropertyValue::Int(3),
    );
    settle(&mut typed_app);
    run_tool(
        &mut json_app,
        TOOL_SET,
        json_object,
        serde_json::json!({"key": property.as_str(), "value": 3}),
    );
    settle(&mut json_app);
    let moved = binding(&typed_app, typed_object);
    assert_eq!(binding(&json_app, json_object), moved);

    let targets = series_targets(&typed_app, typed_object);
    let commit = typed_app
        .plan_property_reset(property, &targets)
        .expect("the typed planner resets");
    typed_app.commit_property(commit);
    settle(&mut typed_app);

    run_tool(
        &mut json_app,
        TOOL_RESET,
        json_object,
        serde_json::json!({"key": property.as_str()}),
    );
    settle(&mut json_app);

    let reset = binding(&typed_app, typed_object);
    assert_ne!(reset, moved, "the reset actually moved the value back");
    assert_eq!(
        binding(&json_app, json_object),
        reset,
        "a reset must land identically through both entry points"
    );
}

/// Planning expands one plot object into one target per series, and a series
/// the property does not apply to is reported rather than dropped.
#[test]
fn planning_expands_series_and_reports_the_ones_it_skips() {
    let (mut app, object) = contour_page();
    // A second series over the same field, drawn as a line: the contour catalog
    // does not apply to it, and it must be visible as a skip rather than vanish.
    {
        let plot = app.doc.canvases[0]
            .objects
            .iter_mut()
            .find(|candidate| candidate.id == object)
            .and_then(|object| object.plot_mut())
            .expect("the page holds a plot");
        let mut line = plot.binding.series[0].clone();
        line.encoding = plotx_figure::SeriesEncoding::Line(plotx_figure::LineEncoding::default());
        line.id = plot.allocate_series_id();
        plot.binding.series.push(line);
    }
    settle(&mut app);

    let request = tool_request(
        &app,
        TOOL_SET,
        object,
        serde_json::json!({"key": contour::COUNT.as_str(), "value": 8}),
    );
    let plan = plan_tool(&app, request).expect("the tool plans");
    assert_eq!(
        plan.targets.len(),
        2,
        "one plot object expands into one target per series"
    );
    assert!(
        plan.targets
            .iter()
            .all(|target| target.target.component.is_some()),
        "every expanded target names its component"
    );

    let authority = plan.required_authority;
    let result = execute_tool(&mut app, plan, authority).expect("the tool executes");
    settle(&mut app);

    let succeeded = result
        .targets
        .iter()
        .filter(|target| target.outcome == TargetOutcome::Succeeded)
        .collect::<Vec<_>>();
    let skipped = result
        .targets
        .iter()
        .filter(|target| target.outcome == TargetOutcome::Skipped)
        .collect::<Vec<_>>();
    assert_eq!(succeeded.len(), 1, "{:?}", result.targets);
    assert_eq!(skipped.len(), 1, "{:?}", result.targets);
    assert!(
        !skipped[0].message.is_empty(),
        "the skip carries its reason to the caller"
    );
    assert_ne!(
        succeeded[0].target.describe(),
        skipped[0].target.describe(),
        "the two rows are distinguishable in a result list"
    );
}

/// Geometry diagnostics cannot reach the result of the call that caused them.
///
/// A property write commits a spec; the geometry is rebuilt by a background
/// job, and the renderer's segment budget — which drops whole levels and
/// reports how many (`ContourGeometry::omitted`) — is applied inside that job.
/// `execute_tool` has already returned by then, so a `ToolResult` reports a
/// clean success for a write whose plot is later cut down. Today that diagnostic
/// reaches `session.status` only (`app_impl_figures.rs`), and there is no
/// channel from it to a `ToolResult`.
///
/// This test pins the ordering that makes the gap structural rather than a
/// missing wire: it is not that someone forgot to copy a field across, it is
/// that the answer does not exist yet when the result is built. Closing it needs
/// a deferred or task-completion-shaped tool result, which is a change to the
/// tool contract and out of this stage's scope.
#[test]
fn a_property_write_returns_before_its_geometry_is_built() {
    let (mut app, object) = contour_page();
    let request = tool_request(
        &app,
        TOOL_SET,
        object,
        serde_json::json!({"key": contour::BASE_MAGNITUDE.as_str(), "value": 2.0}),
    );
    let plan = plan_tool(&app, request).expect("the tool plans");
    let authority = plan.required_authority;
    let result = execute_tool(&mut app, plan, authority).expect("the tool executes");
    assert!(
        result.diagnostics.is_empty(),
        "the result carries no geometry diagnostic, because none exists yet"
    );
    assert!(
        app.compute_busy(),
        "geometry for the value just written is still outstanding when the tool returns"
    );
    settle(&mut app);
}

/// A real contour, driven from the JSON entry point, still ends up as drawn
/// geometry — the tool does not merely mutate a spec that nothing consumes.
#[test]
fn a_json_property_write_reaches_the_drawn_figure() {
    let (mut app, object) = contour_page();
    let before = app.doc.canvases[0]
        .object(object)
        .and_then(|object| object.plot())
        .expect("plot")
        .figure
        .contours
        .len();
    assert!(before > 0, "the page draws contours to begin with");

    run_tool(
        &mut app,
        TOOL_SET,
        object,
        serde_json::json!({"key": contour::NEGATIVE_ENABLED.as_str(), "value": false}),
    );
    settle(&mut app);

    let plot = app.doc.canvases[0]
        .object(object)
        .and_then(|object| object.plot())
        .expect("plot");
    assert!(
        !plot.figure.contours.is_empty(),
        "the positive half is still drawn"
    );
    let svg = plotx_render::svg::export(&plot.figure);
    assert!(svg.contains("<path"), "the figure still exports geometry");
}

#[test]
fn stack_slice_exports_waterfall() {
    let mut data = synthetic_hsqc(3.0, 40.0, "ledbpgp2s");
    // A DOSY-style hint should recommend the stacked (pseudo-2D) layout.
    data.experiment = Some("ledbpgp2s".into());
    assert_eq!(recommend_preset(&data).layout(), Layout2D::Stack);

    let stack = match process_2d(
        &data,
        &Params2D {
            layout: Layout2D::Stack,
            ..Params2D::default()
        },
    ) {
        Processed2D::Stack(s) => s,
        Processed2D::Ft(_) => panic!("expected Stack"),
    };
    assert_eq!(stack.increments(), data.rows);

    let fig = build_stack_figure(&stack);
    assert!(!fig.series.is_empty());
    let svg = plotx_render::svg::export(&fig);
    assert!(svg.contains("<polyline"), "stack traces present");
}
