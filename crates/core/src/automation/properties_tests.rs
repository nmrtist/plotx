//! The JSON boundary of the property catalog.
//!
//! Every test here targets the one way this stage can break: the adapter
//! growing its own copy of a planning or validation rule and drifting from the
//! planner the panel controls use.

use super::*;
use crate::properties::ilt_tests::ilt_app;
use crate::properties::tests::{contour_app, contour_spec};
use crate::properties::{
    AggregateValue, PropertyAddress, PropertyValue, apodization, app_preferences, axis, contour,
    definition_by_key, ilt, phase, smooth, typography,
};
use crate::state::{
    CONTOUR_BASE_FRACTION_OF_RANGE, CONTOUR_BASE_NOISE_FLOOR, CanvasObject, CanvasObjectKind,
    Dataset, NmrDataset, ObjectFrame, PlotxApp, SeriesBinding, TextBox,
};
use plotx_io::{Domain, NmrData};

pub(super) fn request(
    app: &PlotxApp,
    tool_id: &str,
    parameters: serde_json::Value,
    ids: Vec<String>,
    caller: CallerType,
) -> ToolRequest {
    ToolRequest {
        tool_id: tool_id.to_owned(),
        tool_version: 1,
        parameters,
        targets: TargetSelector::Explicit { ids },
        expected_revision: DocumentRevision(app.doc.automation_revision),
        caller,
    }
}

/// The stable id of the fixture's one plot object, as a selector names it.
pub(super) fn plot_resource_id(app: &PlotxApp) -> String {
    let canvas = &app.doc.canvases[0];
    format!("{}/{}", canvas.resource_id, canvas.objects[0].id)
}

pub(super) fn set_request(app: &PlotxApp, key: &str, value: serde_json::Value) -> ToolRequest {
    request(
        app,
        TOOL_SET,
        serde_json::json!({"key": key, "value": value}),
        vec![plot_resource_id(app)],
        CallerType::Agent,
    )
}

/// Plan and execute in one step, the way a headless caller does.
pub(super) fn run(app: &mut PlotxApp, request: ToolRequest) -> Result<ToolResult, AutomationError> {
    let plan = plan_tool(app, request)?;
    let authority = plan.required_authority;
    execute_tool(app, plan, authority)
}

/// Add a second series drawn as a line, so the object holds one series the
/// contour catalog applies to and one it does not.
fn add_line_series(app: &mut PlotxApp) {
    let object = &mut app.doc.canvases[0].objects[0];
    let plot = object.plot_mut().expect("the fixture holds a plot");
    let mut series = plot.binding.series[0].clone();
    series.encoding = plotx_figure::SeriesEncoding::Line(plotx_figure::LineEncoding::default());
    let id = plot.allocate_series_id();
    let mut series = SeriesBinding { id, ..series };
    series.label = Some("line".to_owned());
    plot.binding.series.push(series);
}

#[path = "properties_tests_inbound_id.rs"]
mod inbound_id_tests;
#[path = "properties_tests_inbound_value.rs"]
mod inbound_value_tests;
#[path = "properties_tests_outbound.rs"]
mod outbound_tests;
#[path = "properties_tests_planning.rs"]
mod planning_tests;
#[path = "properties_tests_rejections.rs"]
mod rejection_tests;
