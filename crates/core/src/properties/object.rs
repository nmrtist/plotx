//! Persistent properties owned by canvas objects and plot-object children.

use super::provider::PropertyProvider;
use super::target::{require_object_target, resolved_schema, series_context_unchecked};
use super::*;
use crate::state::{
    DataDomain, FieldCapabilities, ObjectStyle, PlotObject, PlotxApp, ShapeKind, StackKind,
    StackMode, TextAlign, default_chart_type,
};
use plotx_analysis::statistics::{BinRule, histogram};
use plotx_figure::ColormapId;

#[path = "object_definitions.rs"]
mod definitions;
pub use definitions::{
    ALIGN_CENTER, ALIGN_LEFT, ALIGN_RIGHT, CHART_BINS_AUTO, CHART_BINS_COUNT, CHART_COLORMAP,
    CHART_STACKED, CHART_TYPE_ID, CHART_VIEW_AZIMUTH, CHART_VIEW_ELEVATION, COLOR_OVERLAY, LOCKED,
    OFFSET, PANEL_USER_NOTE, PANEL_VISIBLE, SERIES_VISIBLE, SHAPE_ARROW, SHAPE_ELLIPSE,
    SHAPE_FILL_COLOR, SHAPE_FILL_ENABLED, SHAPE_KIND, SHAPE_LINE, SHAPE_RECT, SHAPE_STROKE,
    SHAPE_STROKE_WIDTH, STACK_MODE, STACK_NORMALIZE, STACK_SHEAR_X, STACK_SPACING_Y, SUPERIMPOSED,
    TEXT, TEXT_ALIGN, TEXT_BOLD, TEXT_COLOR, TEXT_FONT_SIZE,
};
use definitions::{DEFINITIONS, FILL_FALLBACK, STACK_MODES};

type ResolvedObjectValue = (
    PropertyValue,
    Option<PropertyValue>,
    Option<bool>,
    Availability,
    ResolvedSchema,
);

pub(crate) struct ObjectProvider;
pub(crate) static PROVIDER: ObjectProvider = ObjectProvider;

impl PropertyProvider for ObjectProvider {
    fn definitions(&self) -> &'static [PropertyDefinition] {
        DEFINITIONS
    }

    fn read(
        &self,
        app: &PlotxApp,
        address: &PropertyAddress,
    ) -> Result<ResolvedProperty, PropertyError> {
        let definition = property_definition(address.definition)?;
        if definition.id == SERIES_VISIBLE {
            return read_series(app, address, definition);
        }
        let (canvas, object) = require_object_target(app, &address.target, definition)?;
        let object_ref = app.doc.canvases[canvas]
            .object(object)
            .ok_or_else(|| PropertyError::UnknownTarget(address.target.describe()))?;
        let (value, default_value, modified, availability, schema) =
            object_value(app, canvas, object_ref, definition)?;
        Ok(ResolvedProperty {
            address: address.clone(),
            value: AggregateValue::Uniform(value),
            default_value,
            modified,
            availability,
            schema,
        })
    }

    fn edit(
        &self,
        app: &PlotxApp,
        transaction: &mut PropertyTransaction,
        address: &PropertyAddress,
        operation: &EditOp<'_>,
    ) -> Result<(), PropertyError> {
        let definition = property_definition(address.definition)?;
        if definition.id == SERIES_VISIBLE {
            return edit_series(app, transaction, address, definition, operation);
        }
        let (canvas, object) = require_object_target(app, &address.target, definition)?;
        let object_ref = app.doc.canvases[canvas]
            .object(object)
            .ok_or_else(|| PropertyError::UnknownTarget(address.target.describe()))?;
        let (_, default, _, availability, schema) =
            object_value(app, canvas, object_ref, definition)?;
        if let Availability::Disabled(reason) = availability {
            return Err(PropertyError::NotApplicable(reason.to_owned()));
        }
        let value = match operation {
            EditOp::Set(value) => {
                let checked = checked_value(definition, value)?;
                if let (ResolvedSchema::Enum { variants }, PropertyValue::Enum(value)) =
                    (&schema, &checked)
                    && !variants.iter().any(|variant| variant.id == *value)
                {
                    return Err(PropertyError::NotApplicable(format!(
                        "'{value}' is not available for this object"
                    )));
                }
                checked
            }
            EditOp::Reset => reset_value(definition, default)?,
            EditOp::Step(_) => return Err(no_step(definition)),
        };
        write_object(app, transaction, canvas, object, definition.id, value)
    }
}

fn property_definition(id: PropertyId) -> Result<&'static PropertyDefinition, PropertyError> {
    super::definition(id).ok_or_else(|| PropertyError::UnknownProperty(id.to_string()))
}

fn plot_context<'a>(
    app: &'a PlotxApp,
    canvas: usize,
    object: &'a crate::state::CanvasObject,
) -> Result<
    (
        &'a PlotObject,
        &'a crate::state::Dataset,
        DataDomain,
        FieldCapabilities,
    ),
    PropertyError,
> {
    let plot = object.plot().ok_or_else(|| {
        PropertyError::NotApplicable("This property belongs to a plot object.".to_owned())
    })?;
    let series = app
        .display_binding(plot.display_owner, &plot.binding)
        .series
        .first()
        .map(|series| series.source)
        .ok_or_else(|| {
            PropertyError::NotApplicable("The plot has no primary series.".to_owned())
        })?;
    let dataset = app
        .doc
        .dataset_by_id(series.resource)
        .ok_or_else(|| PropertyError::UnknownTarget(format!("{canvas}/{}", series.resource)))?;
    let capabilities = dataset
        .field_descriptor(series.field)
        .map(|field| field.capabilities)
        .unwrap_or_default();
    Ok((plot, dataset, dataset.domain(), capabilities))
}

#[allow(clippy::type_complexity)]
fn object_value(
    app: &PlotxApp,
    canvas: usize,
    object: &crate::state::CanvasObject,
    definition: &'static PropertyDefinition,
) -> Result<
    (
        PropertyValue,
        Option<PropertyValue>,
        Option<bool>,
        Availability,
        ResolvedSchema,
    ),
    PropertyError,
> {
    let fixed = fixed_default(definition);
    let standard_schema = resolved_schema(definition, &FieldCapabilities::default());
    match definition.id {
        LOCKED => Ok((
            PropertyValue::Bool(object.locked),
            fixed,
            None,
            Availability::Editable,
            standard_schema,
        )),
        TEXT | TEXT_FONT_SIZE | TEXT_BOLD | TEXT_ALIGN | TEXT_COLOR => {
            let text = object.text().ok_or_else(|| {
                PropertyError::NotApplicable(
                    "This property belongs to a text or panel-label object.".to_owned(),
                )
            })?;
            let default = crate::state::TextBox::panel_label(String::new());
            let default = if object.is_panel_label() {
                default
            } else {
                crate::state::TextBox::label(String::new())
            };
            let value = text_value(definition.id, text)?;
            let default = Some(text_value(definition.id, &default)?);
            Ok((
                value,
                default,
                None,
                Availability::Editable,
                standard_schema,
            ))
        }
        SHAPE_KIND | SHAPE_STROKE | SHAPE_STROKE_WIDTH | SHAPE_FILL_ENABLED | SHAPE_FILL_COLOR => {
            let shape = object.shape().ok_or_else(|| {
                PropertyError::NotApplicable("This property belongs to a shape object.".to_owned())
            })?;
            let availability = if definition.id == SHAPE_FILL_COLOR && shape.fill.is_none() {
                Availability::Disabled("Turn on Fill to choose a fill color.")
            } else {
                Availability::Editable
            };
            Ok((
                shape_value(definition.id, shape)?,
                fixed,
                None,
                availability,
                standard_schema,
            ))
        }
        _ => {
            let (plot, _, domain, capabilities) = plot_context(app, canvas, object)?;
            plot_value(app, definition, plot, domain, capabilities)
        }
    }
}

fn plot_value(
    app: &PlotxApp,
    definition: &'static PropertyDefinition,
    plot: &PlotObject,
    domain: DataDomain,
    capabilities: FieldCapabilities,
) -> Result<ResolvedObjectValue, PropertyError> {
    let fixed = fixed_default(definition);
    let schema = resolved_schema(definition, &capabilities);
    match definition.id {
        STACK_MODE | STACK_SPACING_Y | STACK_SHEAR_X | STACK_NORMALIZE => {
            let binding = app.display_binding(plot.display_owner, &plot.binding);
            if binding.series.len() <= 1 || !app.series_stackable(&binding) {
                return Err(PropertyError::NotApplicable(
                    "Stack settings require a stackable plot with multiple series.".to_owned(),
                ));
            }
            let kind =
                if binding.series.iter().all(|series| {
                    matches!(series.encoding, plotx_figure::SeriesEncoding::Contour(_))
                }) {
                    Some(StackKind::Field)
                } else {
                    Some(StackKind::Line)
                };
            if definition.id != STACK_MODE
                && (kind != Some(StackKind::Line) || plot.stack.mode != StackMode::Offset)
            {
                return Err(PropertyError::NotApplicable(
                    "Spacing, shear, and normalization apply to Offset line stacks.".to_owned(),
                ));
            }
            let availability = if definition.id == STACK_MODE && kind == Some(StackKind::Field) {
                Availability::Disabled("Field stacks always use Color overlay.")
            } else {
                Availability::Editable
            };
            let schema = if definition.id == STACK_MODE {
                let variants = match kind {
                    Some(StackKind::Line) => STACK_MODES[..2].iter().collect(),
                    Some(StackKind::Field) => STACK_MODES[2..].iter().collect(),
                    None => Vec::new(),
                };
                ResolvedSchema::Enum { variants }
            } else {
                schema
            };
            Ok((
                stack_value(definition.id, plot)?,
                fixed,
                None,
                availability,
                schema,
            ))
        }
        CHART_TYPE_ID | CHART_BINS_AUTO | CHART_BINS_COUNT | CHART_STACKED | CHART_COLORMAP
        | CHART_VIEW_AZIMUTH | CHART_VIEW_ELEVATION => {
            let current_id = crate::state::resolved_chart_type_for_field(
                &capabilities,
                domain,
                &plot.chart.type_id,
            )
            .id;
            if !chart_property_applies_to_type(definition.id, current_id) {
                return Err(PropertyError::NotApplicable(
                    "This option does not apply to the selected chart type.".to_owned(),
                ));
            }
            let availability = if definition.id == CHART_BINS_COUNT && plot.chart.bins.is_none() {
                Availability::Disabled(
                    "Turn off Automatic histogram bins to set the count manually.",
                )
            } else {
                Availability::Editable
            };
            let default = if definition.id == CHART_TYPE_ID {
                Some(PropertyValue::Enum(default_chart_type(domain).id))
            } else {
                fixed
            };
            let modified =
                (definition.id == CHART_TYPE_ID).then_some(!plot.chart.type_id.is_empty());
            Ok((
                chart_value(app, definition.id, plot)?,
                default,
                modified,
                availability,
                schema,
            ))
        }
        PANEL_USER_NOTE | PANEL_VISIBLE => Ok((
            if definition.id == PANEL_USER_NOTE {
                PropertyValue::Text(plot.panel.user_note.clone())
            } else {
                PropertyValue::Bool(plot.panel.visible)
            },
            fixed,
            None,
            Availability::Editable,
            schema,
        )),
        _ => Err(PropertyError::UnknownProperty(definition.id.to_string())),
    }
}

/// Whether a Chart-section property is rendered for a resolved chart type.
///
/// The panel-density calibration calls this same predicate while enumerating
/// the chart-type discriminator, so the budget cannot drift from the provider
/// that decides which rows a user actually sees.
#[doc(hidden)]
pub fn chart_property_applies_to_type(property: PropertyId, chart_type: &str) -> bool {
    match property {
        CHART_TYPE_ID => true,
        CHART_BINS_AUTO | CHART_BINS_COUNT => chart_type == "table_histogram",
        CHART_STACKED => chart_type == "table_bar_grouped",
        CHART_COLORMAP => matches!(chart_type, "table_heatmap" | "table_surface"),
        CHART_VIEW_AZIMUTH | CHART_VIEW_ELEVATION => chart_type == "table_surface",
        _ => false,
    }
}

fn read_series(
    app: &PlotxApp,
    address: &PropertyAddress,
    definition: &'static PropertyDefinition,
) -> Result<ResolvedProperty, PropertyError> {
    let context = series_context_unchecked(app, &address.target)?;
    let plot = app.doc.canvases[context.canvas]
        .object(context.object)
        .and_then(|object| object.plot())
        .ok_or_else(|| PropertyError::UnknownTarget(address.target.describe()))?;
    let binding = app.display_binding(plot.display_owner, &plot.binding);
    if binding.series.len() <= 1 || !app.series_stackable(&binding) {
        return Err(PropertyError::NotApplicable(
            "Series visibility is available only on stackable multi-series plots.".to_owned(),
        ));
    }
    let visible = binding
        .series
        .iter()
        .find(|series| series.id == context.series)
        .map(|series| series.visible)
        .ok_or_else(|| PropertyError::UnknownTarget(address.target.describe()))?;
    Ok(ResolvedProperty {
        address: address.clone(),
        value: AggregateValue::Uniform(PropertyValue::Bool(visible)),
        default_value: Some(PropertyValue::Bool(true)),
        modified: None,
        availability: Availability::Editable,
        schema: resolved_schema(definition, &context.capabilities),
    })
}

fn edit_series(
    app: &PlotxApp,
    transaction: &mut PropertyTransaction,
    address: &PropertyAddress,
    definition: &'static PropertyDefinition,
    operation: &EditOp<'_>,
) -> Result<(), PropertyError> {
    let context = series_context_unchecked(app, &address.target)?;
    let plot = app.doc.canvases[context.canvas]
        .object(context.object)
        .and_then(|object| object.plot())
        .ok_or_else(|| PropertyError::UnknownTarget(address.target.describe()))?;
    let binding = app.display_binding(plot.display_owner, &plot.binding);
    if binding.series.len() <= 1 || !app.series_stackable(&binding) {
        return Err(PropertyError::NotApplicable(
            "Series visibility is available only on stackable multi-series plots.".to_owned(),
        ));
    }
    let visible = match operation {
        EditOp::Set(PropertyValue::Bool(value)) => *value,
        EditOp::Reset => true,
        EditOp::Set(value) => return Err(wrong_kind(definition, value)),
        EditOp::Step(_) => return Err(no_step(definition)),
    };
    let binding = transaction.data_binding(app, context.canvas, context.object)?;
    let series = binding
        .series
        .iter_mut()
        .find(|series| series.id == context.series)
        .ok_or_else(|| PropertyError::UnknownTarget(address.target.describe()))?;
    series.visible = visible;
    Ok(())
}

fn write_object(
    app: &PlotxApp,
    transaction: &mut PropertyTransaction,
    canvas: usize,
    object: crate::state::ObjectId,
    id: PropertyId,
    value: PropertyValue,
) -> Result<(), PropertyError> {
    match id {
        STACK_MODE | STACK_SPACING_Y | STACK_SHEAR_X | STACK_NORMALIZE => {
            let stack = transaction.stack_spec(app, canvas, object)?;
            match (id, value) {
                (STACK_MODE, PropertyValue::Enum(value)) => {
                    stack.mode = stack_mode(value).expect("validated stack mode")
                }
                (STACK_SPACING_Y, PropertyValue::Float(value)) => stack.spacing_y = value,
                (STACK_SHEAR_X, PropertyValue::Float(value)) => stack.shear_x = value,
                (STACK_NORMALIZE, PropertyValue::Bool(value)) => stack.normalize = value,
                _ => unreachable!("validated stack value"),
            }
        }
        CHART_TYPE_ID | CHART_BINS_AUTO | CHART_BINS_COUNT | CHART_STACKED | CHART_COLORMAP
        | CHART_VIEW_AZIMUTH | CHART_VIEW_ELEVATION => {
            let automatic_seed = (id == CHART_BINS_AUTO && value == PropertyValue::Bool(false))
                .then(|| live_auto_bins(app, canvas, object))
                .flatten()
                .unwrap_or(20);
            let chart = transaction.chart_spec(app, canvas, object)?;
            match (id, value) {
                (CHART_TYPE_ID, PropertyValue::Enum(value)) => chart.type_id = value.to_owned(),
                (CHART_TYPE_ID, PropertyValue::Text(value)) if value.is_empty() => {
                    chart.type_id.clear()
                }
                (CHART_BINS_AUTO, PropertyValue::Bool(true)) => chart.bins = None,
                (CHART_BINS_AUTO, PropertyValue::Bool(false)) => chart.bins = Some(automatic_seed),
                (CHART_BINS_COUNT, PropertyValue::Int(value)) => chart.bins = Some(value as usize),
                (CHART_STACKED, PropertyValue::Bool(value)) => chart.stacked = value,
                (CHART_COLORMAP, PropertyValue::Enum(value)) => {
                    chart.colormap = ColormapId::from_id(value).expect("validated colormap")
                }
                (CHART_VIEW_AZIMUTH, PropertyValue::Float(value)) => {
                    chart.view_angles[0] = value.to_degrees() as f32
                }
                (CHART_VIEW_ELEVATION, PropertyValue::Float(value)) => {
                    chart.view_angles[1] = value.to_degrees() as f32
                }
                _ => unreachable!("validated chart value"),
            }
        }
        PANEL_USER_NOTE | PANEL_VISIBLE => {
            let panel = transaction.panel_meta(app, canvas, object)?;
            match (id, value) {
                (PANEL_USER_NOTE, PropertyValue::Text(value)) => panel.user_note = value,
                (PANEL_VISIBLE, PropertyValue::Bool(value)) => panel.visible = value,
                _ => unreachable!("validated panel value"),
            }
        }
        LOCKED => {
            let flags = transaction.object_flags(app, canvas, object)?;
            let PropertyValue::Bool(value) = value else {
                unreachable!("validated lock value")
            };
            flags.1 = value;
        }
        TEXT | TEXT_FONT_SIZE | TEXT_BOLD | TEXT_ALIGN | TEXT_COLOR | SHAPE_KIND | SHAPE_STROKE
        | SHAPE_STROKE_WIDTH | SHAPE_FILL_ENABLED | SHAPE_FILL_COLOR => {
            write_style(transaction.object_style(app, canvas, object)?, id, value)?;
        }
        _ => return Err(PropertyError::UnknownProperty(id.to_string())),
    }
    Ok(())
}

fn write_style(
    style: &mut ObjectStyle,
    id: PropertyId,
    value: PropertyValue,
) -> Result<(), PropertyError> {
    match (style, id, value) {
        (ObjectStyle::Text(text), TEXT, PropertyValue::Text(value)) => text.text = value,
        (ObjectStyle::Text(text), TEXT_FONT_SIZE, PropertyValue::Float(value)) => {
            text.font_size = value as f32
        }
        (ObjectStyle::Text(text), TEXT_BOLD, PropertyValue::Bool(value)) => text.bold = value,
        (ObjectStyle::Text(text), TEXT_ALIGN, PropertyValue::Enum(value)) => {
            text.align = text_align(value).expect("validated alignment")
        }
        (ObjectStyle::Text(text), TEXT_COLOR, PropertyValue::Color(value)) => text.color = value,
        (ObjectStyle::Shape(shape), SHAPE_KIND, PropertyValue::Enum(value)) => {
            shape.shape = shape_kind(value).expect("validated shape kind")
        }
        (ObjectStyle::Shape(shape), SHAPE_STROKE, PropertyValue::Color(value)) => {
            shape.stroke = value
        }
        (ObjectStyle::Shape(shape), SHAPE_STROKE_WIDTH, PropertyValue::Float(value)) => {
            shape.stroke_width = value as f32
        }
        (ObjectStyle::Shape(shape), SHAPE_FILL_ENABLED, PropertyValue::Bool(true)) => {
            shape.fill = Some(shape.fill.unwrap_or(FILL_FALLBACK));
        }
        (ObjectStyle::Shape(shape), SHAPE_FILL_ENABLED, PropertyValue::Bool(false)) => {
            shape.fill = None
        }
        (ObjectStyle::Shape(shape), SHAPE_FILL_COLOR, PropertyValue::Color(value)) => {
            shape.fill = Some(value)
        }
        (_, _, _) => {
            return Err(PropertyError::NotApplicable(
                "The object style changed kind before the edit was applied.".to_owned(),
            ));
        }
    }
    Ok(())
}

fn live_auto_bins(app: &PlotxApp, canvas: usize, object: crate::state::ObjectId) -> Option<usize> {
    let plot = app.doc.canvases.get(canvas)?.object(object)?.plot()?;
    let dataset = app
        .doc
        .dataset_by_id(plot.binding.primary_dataset()?)?
        .as_table()?;
    let data = dataset.typed_plot_data(100_000).ok()?;
    let index = plot
        .chart
        .column
        .and_then(|column| {
            data.series
                .iter()
                .position(|series| series.binding.value_column == column)
        })
        .unwrap_or(0);
    let values = data
        .series
        .get(index)?
        .y
        .iter()
        .copied()
        .filter(|value| value.is_finite())
        .collect::<Vec<_>>();
    histogram(&values, BinRule::Auto)
        .ok()
        .map(|histogram| histogram.counts.len())
}

fn chart_value(
    app: &PlotxApp,
    id: PropertyId,
    plot: &PlotObject,
) -> Result<PropertyValue, PropertyError> {
    match id {
        CHART_TYPE_ID => {
            let series = plot.binding.series.first().ok_or_else(|| {
                PropertyError::NotApplicable("The plot has no primary series.".to_owned())
            })?;
            let dataset = app
                .doc
                .dataset_by_id(series.source.resource)
                .ok_or_else(|| PropertyError::UnknownTarget(series.source.resource.to_string()))?;
            let capabilities = dataset
                .field_descriptor(series.source.field)
                .map(|field| field.capabilities)
                .unwrap_or_default();
            Ok(PropertyValue::Enum(
                crate::state::resolved_chart_type_for_field(
                    &capabilities,
                    dataset.domain(),
                    &plot.chart.type_id,
                )
                .id,
            ))
        }
        CHART_BINS_AUTO => Ok(PropertyValue::Bool(plot.chart.bins.is_none())),
        CHART_BINS_COUNT => Ok(PropertyValue::Int(plot.chart.bins.unwrap_or(20) as i64)),
        CHART_STACKED => Ok(PropertyValue::Bool(plot.chart.stacked)),
        CHART_COLORMAP => Ok(PropertyValue::Enum(plot.chart.colormap.id())),
        CHART_VIEW_AZIMUTH => Ok(PropertyValue::Float(
            f64::from(plot.chart.view_angles[0]).to_radians(),
        )),
        CHART_VIEW_ELEVATION => Ok(PropertyValue::Float(
            f64::from(plot.chart.view_angles[1]).to_radians(),
        )),
        _ => Err(PropertyError::UnknownProperty(id.to_string())),
    }
}

fn stack_value(id: PropertyId, plot: &PlotObject) -> Result<PropertyValue, PropertyError> {
    match id {
        STACK_MODE => Ok(PropertyValue::Enum(stack_mode_key(plot.stack.mode))),
        STACK_SPACING_Y => Ok(PropertyValue::Float(plot.stack.spacing_y)),
        STACK_SHEAR_X => Ok(PropertyValue::Float(plot.stack.shear_x)),
        STACK_NORMALIZE => Ok(PropertyValue::Bool(plot.stack.normalize)),
        _ => Err(PropertyError::UnknownProperty(id.to_string())),
    }
}

fn text_value(
    id: PropertyId,
    text: &crate::state::TextBox,
) -> Result<PropertyValue, PropertyError> {
    match id {
        TEXT => Ok(PropertyValue::Text(text.text.clone())),
        TEXT_FONT_SIZE => Ok(PropertyValue::Float(f64::from(text.font_size))),
        TEXT_BOLD => Ok(PropertyValue::Bool(text.bold)),
        TEXT_ALIGN => Ok(PropertyValue::Enum(text_align_key(text.align))),
        TEXT_COLOR => Ok(PropertyValue::Color(text.color)),
        _ => Err(PropertyError::UnknownProperty(id.to_string())),
    }
}

fn shape_value(
    id: PropertyId,
    shape: &crate::state::ShapeObject,
) -> Result<PropertyValue, PropertyError> {
    match id {
        SHAPE_KIND => Ok(PropertyValue::Enum(shape_kind_key(shape.shape))),
        SHAPE_STROKE => Ok(PropertyValue::Color(shape.stroke)),
        SHAPE_STROKE_WIDTH => Ok(PropertyValue::Float(f64::from(shape.stroke_width))),
        SHAPE_FILL_ENABLED => Ok(PropertyValue::Bool(shape.fill.is_some())),
        SHAPE_FILL_COLOR => Ok(PropertyValue::Color(shape.fill.unwrap_or(FILL_FALLBACK))),
        _ => Err(PropertyError::UnknownProperty(id.to_string())),
    }
}

fn checked_value(
    definition: &'static PropertyDefinition,
    value: &PropertyValue,
) -> Result<PropertyValue, PropertyError> {
    match (definition.value_schema, value) {
        (ValueSchema::Bool, PropertyValue::Bool(value)) => Ok(PropertyValue::Bool(*value)),
        (ValueSchema::Text, PropertyValue::Text(value)) => Ok(PropertyValue::Text(value.clone())),
        (
            ValueSchema::Int { min, max } | ValueSchema::IntWithDrag { min, max, .. },
            PropertyValue::Int(value),
        ) if (min..=max).contains(value) => Ok(PropertyValue::Int(*value)),
        (ValueSchema::Float { bounds, .. }, PropertyValue::Float(value)) => {
            bounds.check(definition.id, definition.canonical_label, *value)?;
            Ok(PropertyValue::Float(*value))
        }
        (ValueSchema::Enum { variants }, PropertyValue::Enum(value))
            if variants.iter().any(|variant| variant.id == *value) =>
        {
            Ok(PropertyValue::Enum(value))
        }
        (ValueSchema::Color, PropertyValue::Color(value)) => Ok(PropertyValue::Color(*value)),
        (_, value) => Err(wrong_kind(definition, value)),
    }
}

fn reset_value(
    definition: &'static PropertyDefinition,
    resolved_default: Option<PropertyValue>,
) -> Result<PropertyValue, PropertyError> {
    if definition.id == CHART_TYPE_ID {
        // The write layer interprets the default chart as a concrete selection.
        // Reset needs the persisted sentinel, so it is handled before validation.
        return Ok(PropertyValue::Text(String::new()));
    }
    resolved_default.ok_or_else(|| PropertyError::InvalidValue {
        property: definition.id,
        message: "this property has no reset value".to_owned(),
    })
}

fn fixed_default(definition: &'static PropertyDefinition) -> Option<PropertyValue> {
    match &definition.default_policy {
        DefaultPolicy::Fixed(value) => Some(value.clone()),
        _ => None,
    }
}

fn wrong_kind(definition: &'static PropertyDefinition, value: &PropertyValue) -> PropertyError {
    PropertyError::InvalidValue {
        property: definition.id,
        message: format!(
            "{} does not accept {}",
            definition.canonical_label,
            value.kind()
        ),
    }
}

fn no_step(definition: &'static PropertyDefinition) -> PropertyError {
    PropertyError::InvalidValue {
        property: definition.id,
        message: "this object property has no step gesture".to_owned(),
    }
}

fn stack_mode_key(value: StackMode) -> &'static str {
    match value {
        StackMode::Superimposed => SUPERIMPOSED,
        StackMode::Offset => OFFSET,
        StackMode::ColorOverlay => COLOR_OVERLAY,
    }
}

fn stack_mode(value: &str) -> Option<StackMode> {
    match value {
        SUPERIMPOSED => Some(StackMode::Superimposed),
        OFFSET => Some(StackMode::Offset),
        COLOR_OVERLAY => Some(StackMode::ColorOverlay),
        _ => None,
    }
}

fn text_align_key(value: TextAlign) -> &'static str {
    match value {
        TextAlign::Left => ALIGN_LEFT,
        TextAlign::Center => ALIGN_CENTER,
        TextAlign::Right => ALIGN_RIGHT,
    }
}

fn text_align(value: &str) -> Option<TextAlign> {
    match value {
        ALIGN_LEFT => Some(TextAlign::Left),
        ALIGN_CENTER => Some(TextAlign::Center),
        ALIGN_RIGHT => Some(TextAlign::Right),
        _ => None,
    }
}

fn shape_kind_key(value: ShapeKind) -> &'static str {
    match value {
        ShapeKind::Rect => SHAPE_RECT,
        ShapeKind::Ellipse => SHAPE_ELLIPSE,
        ShapeKind::Line => SHAPE_LINE,
        ShapeKind::Arrow => SHAPE_ARROW,
    }
}

fn shape_kind(value: &str) -> Option<ShapeKind> {
    match value {
        SHAPE_RECT => Some(ShapeKind::Rect),
        SHAPE_ELLIPSE => Some(ShapeKind::Ellipse),
        SHAPE_LINE => Some(ShapeKind::Line),
        SHAPE_ARROW => Some(ShapeKind::Arrow),
        _ => None,
    }
}
