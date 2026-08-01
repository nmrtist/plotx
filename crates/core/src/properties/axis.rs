//! Plot-object axis labels and visibility overrides.

use super::provider::PropertyProvider;
use super::target::{require_plot_object_target, resolved_schema};
use super::{
    AggregateValue, Applicability, Availability, ComponentKind, DefaultPolicy, EditOp,
    PropertyAccess, PropertyAddress, PropertyDefinition, PropertyError, PropertyId,
    PropertyTransaction, PropertyValue, ResolvedProperty, ScopeKind, Tier, ValueCopies,
    ValueSchema,
};
use crate::state::{FieldCapabilities, PlotObject, PlotxApp};
use plotx_figure::AxisFrame;

pub const X_LABEL: PropertyId = PropertyId("object.axes.x_label");
pub const Y_LABEL: PropertyId = PropertyId("object.axes.y_label");
pub const EQUAL_F1_F2_SCALE: PropertyId = PropertyId("object.axes.equal_f1_f2_scale");
pub const X_SHOW_TICK_LABELS: PropertyId = PropertyId("object.axes.x_show_tick_labels");
pub const X_SHOW_LABEL: PropertyId = PropertyId("object.axes.x_show_label");
pub const Y_SHOW_TICK_LABELS: PropertyId = PropertyId("object.axes.y_show_tick_labels");
pub const Y_SHOW_LABEL: PropertyId = PropertyId("object.axes.y_show_label");

const OBJECT: Applicability = Applicability::component(ComponentKind::None);

const fn axis_definition(
    id: PropertyId,
    value_schema: ValueSchema,
    canonical_label: &'static str,
    canonical_aliases: &'static [&'static str],
    tier: Tier,
) -> PropertyDefinition {
    PropertyDefinition {
        id,
        scope_kind: ScopeKind::Object,
        value_schema,
        access: PropertyAccess::ReadWrite,
        applicability: OBJECT,
        default_policy: DefaultPolicy::Derived,
        tier,
        copies: ValueCopies::PerTarget,
        canonical_label,
        canonical_aliases,
    }
}

pub(crate) const DEFINITIONS: &[PropertyDefinition] = &[
    PropertyDefinition {
        id: EQUAL_F1_F2_SCALE,
        scope_kind: ScopeKind::Object,
        value_schema: ValueSchema::Bool,
        access: PropertyAccess::ReadWrite,
        applicability: OBJECT,
        default_policy: DefaultPolicy::None,
        tier: Tier::Essential,
        copies: ValueCopies::PerTarget,
        canonical_label: "Equal F1/F2 scale",
        canonical_aliases: &["1:1 scale", "equal axis scale", "aspect lock"],
    },
    axis_definition(
        X_LABEL,
        ValueSchema::Text,
        "X-axis title",
        &["x label"],
        Tier::Essential,
    ),
    axis_definition(
        Y_LABEL,
        ValueSchema::Text,
        "Y-axis title",
        &["y label"],
        Tier::Essential,
    ),
    axis_definition(
        X_SHOW_TICK_LABELS,
        ValueSchema::Bool,
        "Show x-axis tick labels",
        &["x ticks", "x tick labels"],
        Tier::Essential,
    ),
    axis_definition(
        X_SHOW_LABEL,
        ValueSchema::Bool,
        "Show x-axis title",
        &["x title visibility"],
        Tier::Advanced,
    ),
    axis_definition(
        Y_SHOW_TICK_LABELS,
        ValueSchema::Bool,
        "Show y-axis tick labels",
        &["y ticks", "y tick labels"],
        Tier::Essential,
    ),
    axis_definition(
        Y_SHOW_LABEL,
        ValueSchema::Bool,
        "Show y-axis title",
        &["y title visibility"],
        Tier::Advanced,
    ),
];

pub(crate) struct AxisProvider;

pub(crate) static PROVIDER: AxisProvider = AxisProvider;

impl PropertyProvider for AxisProvider {
    fn definitions(&self) -> &'static [PropertyDefinition] {
        DEFINITIONS
    }

    fn read(
        &self,
        app: &PlotxApp,
        address: &PropertyAddress,
    ) -> Result<ResolvedProperty, PropertyError> {
        let definition = property_definition(address.definition)?;
        let (canvas, object) = require_plot_object_target(app, &address.target, definition)?;
        let plot = app.doc.canvases[canvas]
            .object(object)
            .and_then(|object| object.plot())
            .ok_or_else(|| PropertyError::UnknownTarget(address.target.describe()))?;
        if definition.id == EQUAL_F1_F2_SCALE {
            require_2d_nmr_plot(app, plot, definition)?;
        }
        let availability = if matches!(definition.id, X_LABEL | Y_LABEL)
            && plot.figure().axis_frame == AxisFrame::Hidden
        {
            Availability::Disabled("Choose a chart with visible axes to edit axis settings.")
        } else {
            Availability::Editable
        };
        let (default_value, modified) = if definition.id == EQUAL_F1_F2_SCALE {
            (None, None)
        } else {
            (
                Some(default_value(definition.id, plot)?),
                Some(has_override(definition.id, plot)?),
            )
        };
        Ok(ResolvedProperty {
            address: address.clone(),
            value: AggregateValue::Uniform(value_of(definition.id, plot)?),
            default_value,
            modified,
            availability,
            schema: resolved_schema(definition, &FieldCapabilities::default()),
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
        let (canvas, object) = require_plot_object_target(app, &address.target, definition)?;
        if definition.id == EQUAL_F1_F2_SCALE {
            let plot = app.doc.canvases[canvas]
                .object(object)
                .and_then(|object| object.plot())
                .ok_or_else(|| PropertyError::UnknownTarget(address.target.describe()))?;
            require_2d_nmr_plot(app, plot, definition)?;
        }
        if matches!(definition.id, X_LABEL | Y_LABEL)
            && app.doc.canvases[canvas]
                .object(object)
                .and_then(|object| object.plot())
                .is_some_and(|plot| plot.figure().axis_frame == AxisFrame::Hidden)
        {
            return Err(PropertyError::NotApplicable(
                "Choose a chart with visible axes to edit axis settings.".to_owned(),
            ));
        }
        let overrides = transaction.axis_overrides(app, canvas, object)?;
        match (definition.id, operation) {
            (EQUAL_F1_F2_SCALE, EditOp::Set(PropertyValue::Bool(value))) => {
                overrides.lock_aspect = Some(*value)
            }
            (X_LABEL, EditOp::Set(PropertyValue::Text(value))) => {
                overrides.x_label = Some(value.clone())
            }
            (Y_LABEL, EditOp::Set(PropertyValue::Text(value))) => {
                overrides.y_label = Some(value.clone())
            }
            (X_SHOW_TICK_LABELS, EditOp::Set(PropertyValue::Bool(value))) => {
                overrides.x_show_tick_labels = Some(*value)
            }
            (X_SHOW_LABEL, EditOp::Set(PropertyValue::Bool(value))) => {
                overrides.x_show_label = Some(*value)
            }
            (Y_SHOW_TICK_LABELS, EditOp::Set(PropertyValue::Bool(value))) => {
                overrides.y_show_tick_labels = Some(*value)
            }
            (Y_SHOW_LABEL, EditOp::Set(PropertyValue::Bool(value))) => {
                overrides.y_show_label = Some(*value)
            }
            (EQUAL_F1_F2_SCALE, EditOp::Reset) => overrides.lock_aspect = None,
            (X_LABEL, EditOp::Reset) => overrides.x_label = None,
            (Y_LABEL, EditOp::Reset) => overrides.y_label = None,
            (X_SHOW_TICK_LABELS, EditOp::Reset) => overrides.x_show_tick_labels = None,
            (X_SHOW_LABEL, EditOp::Reset) => overrides.x_show_label = None,
            (Y_SHOW_TICK_LABELS, EditOp::Reset) => overrides.y_show_tick_labels = None,
            (Y_SHOW_LABEL, EditOp::Reset) => overrides.y_show_label = None,
            (_, EditOp::Step(_)) => {
                return Err(PropertyError::InvalidValue {
                    property: definition.id,
                    message: "axis settings have no step gesture".to_owned(),
                });
            }
            (_, EditOp::Set(value)) => {
                return Err(PropertyError::InvalidValue {
                    property: definition.id,
                    message: format!(
                        "{} does not accept a value of kind {}",
                        definition.canonical_label,
                        value.kind()
                    ),
                });
            }
            (_, EditOp::Reset) => {
                return Err(PropertyError::UnknownProperty(
                    definition.id.as_str().to_owned(),
                ));
            }
        }
        Ok(())
    }
}

fn property_definition(id: PropertyId) -> Result<&'static PropertyDefinition, PropertyError> {
    super::definition(id).ok_or_else(|| PropertyError::UnknownProperty(id.as_str().to_owned()))
}

fn value_of(id: PropertyId, plot: &PlotObject) -> Result<PropertyValue, PropertyError> {
    match id {
        EQUAL_F1_F2_SCALE => Ok(PropertyValue::Bool(plot.figure().lock_aspect)),
        X_LABEL => Ok(PropertyValue::Text(plot.figure().x.label.clone())),
        Y_LABEL => Ok(PropertyValue::Text(plot.figure().y.label.clone())),
        X_SHOW_TICK_LABELS => Ok(PropertyValue::Bool(plot.figure().x.show_tick_labels)),
        X_SHOW_LABEL => Ok(PropertyValue::Bool(plot.figure().x.show_label)),
        Y_SHOW_TICK_LABELS => Ok(PropertyValue::Bool(plot.figure().y.show_tick_labels)),
        Y_SHOW_LABEL => Ok(PropertyValue::Bool(plot.figure().y.show_label)),
        _ => Err(PropertyError::UnknownProperty(id.as_str().to_owned())),
    }
}

fn require_2d_nmr_plot(
    app: &PlotxApp,
    plot: &PlotObject,
    definition: &'static PropertyDefinition,
) -> Result<(), PropertyError> {
    let applicable = plot
        .primary_dataset()
        .and_then(|dataset| app.doc.dataset_by_id(dataset))
        .is_some_and(|dataset| {
            matches!(dataset, crate::state::Dataset::Nmr2D(dataset) if dataset.is_true_2d())
        });
    if applicable {
        Ok(())
    } else {
        Err(PropertyError::NotApplicable(format!(
            "{} belongs to a true 2D NMR spectrum",
            definition.canonical_label
        )))
    }
}

fn default_value(id: PropertyId, plot: &PlotObject) -> Result<PropertyValue, PropertyError> {
    match id {
        X_LABEL => Ok(PropertyValue::Text(plot.derived_axes().x_label.clone())),
        Y_LABEL => Ok(PropertyValue::Text(plot.derived_axes().y_label.clone())),
        X_SHOW_TICK_LABELS => Ok(PropertyValue::Bool(plot.derived_axes().x_show_tick_labels)),
        X_SHOW_LABEL => Ok(PropertyValue::Bool(plot.derived_axes().x_show_label)),
        Y_SHOW_TICK_LABELS => Ok(PropertyValue::Bool(plot.derived_axes().y_show_tick_labels)),
        Y_SHOW_LABEL => Ok(PropertyValue::Bool(plot.derived_axes().y_show_label)),
        _ => Err(PropertyError::UnknownProperty(id.as_str().to_owned())),
    }
}

fn has_override(id: PropertyId, plot: &PlotObject) -> Result<bool, PropertyError> {
    match id {
        X_LABEL => Ok(plot.axis_overrides.x_label.is_some()),
        Y_LABEL => Ok(plot.axis_overrides.y_label.is_some()),
        X_SHOW_TICK_LABELS => Ok(plot.axis_overrides.x_show_tick_labels.is_some()),
        X_SHOW_LABEL => Ok(plot.axis_overrides.x_show_label.is_some()),
        Y_SHOW_TICK_LABELS => Ok(plot.axis_overrides.y_show_tick_labels.is_some()),
        Y_SHOW_LABEL => Ok(plot.axis_overrides.y_show_label.is_some()),
        _ => Err(PropertyError::UnknownProperty(id.as_str().to_owned())),
    }
}
