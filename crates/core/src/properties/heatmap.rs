//! Scalar heatmap display-range properties.
//!
//! The range belongs to the series encoding, not to the source field. `None`
//! keeps the encoding data-driven by using the field summary; an explicit range
//! is a presentation override and is therefore safe to edit without changing
//! the scientific data.

use super::provider::PropertyProvider;
use super::target::{not_applicable_encoding, resolved_schema, series_context};
use super::{
    AggregateValue, Applicability, Availability, ComponentKind, DefaultPolicy, EditOp,
    EncodingKind, FloatBounds, FloatDisplay, PropertyAccess, PropertyAddress, PropertyDefinition,
    PropertyError, PropertyId, PropertyStep, PropertyTransaction, PropertyValue, ResolvedProperty,
    ScopeKind, Tier, ValueCopies, ValueSchema, definition,
};
use crate::automation::CAP_FIELD_SCALAR_GRID_2D_REGULAR;
use crate::state::{Dataset, FieldId, PlotxApp};
use plotx_figure::{HeatmapSpec, SeriesEncoding};

pub const RANGE_SPAN: PropertyId = PropertyId("series.heatmap.range_span");
pub const RANGE_CENTER: PropertyId = PropertyId("series.heatmap.range_center");

const MAX_VALUE: f64 = f32::MAX as f64;
const SPAN_BOUNDS: FloatBounds = FloatBounds::above(0.0, MAX_VALUE);
const CENTER_BOUNDS: FloatBounds = FloatBounds::inclusive(-MAX_VALUE, MAX_VALUE);
const SPAN_STEP_RATIO: f64 = 1.2;

const HEATMAP: Applicability =
    Applicability::encoding(ComponentKind::Series, EncodingKind::Heatmap)
        .requiring(&[CAP_FIELD_SCALAR_GRID_2D_REGULAR]);

pub(crate) const DEFINITIONS: &[PropertyDefinition] = &[
    PropertyDefinition {
        id: RANGE_SPAN,
        scope_kind: ScopeKind::Object,
        value_schema: ValueSchema::Float {
            bounds: SPAN_BOUNDS,
            display: FloatDisplay::Linear("intensity"),
            drag_step: None,
        },
        access: PropertyAccess::ReadWrite,
        applicability: HEATMAP,
        default_policy: DefaultPolicy::Derived,
        tier: Tier::Essential,
        copies: ValueCopies::PerTarget,
        canonical_label: "Colour range span",
        canonical_aliases: &[
            "heatmap range",
            "colour scale",
            "color scale",
            "contrast",
            "dynamic range",
        ],
    },
    PropertyDefinition {
        id: RANGE_CENTER,
        scope_kind: ScopeKind::Object,
        value_schema: ValueSchema::Float {
            bounds: CENTER_BOUNDS,
            display: FloatDisplay::Linear("intensity"),
            drag_step: None,
        },
        access: PropertyAccess::ReadWrite,
        applicability: HEATMAP,
        default_policy: DefaultPolicy::Derived,
        tier: Tier::Advanced,
        copies: ValueCopies::PerTarget,
        canonical_label: "Colour range centre",
        canonical_aliases: &[
            "heatmap centre",
            "color range center",
            "colour scale midpoint",
        ],
    },
];

pub(crate) struct HeatmapProvider;

pub(crate) static PROVIDER: HeatmapProvider = HeatmapProvider;

impl PropertyProvider for HeatmapProvider {
    fn definitions(&self) -> &'static [PropertyDefinition] {
        DEFINITIONS
    }

    fn read(
        &self,
        app: &PlotxApp,
        address: &PropertyAddress,
    ) -> Result<ResolvedProperty, PropertyError> {
        let definition = definition(address.definition).ok_or_else(|| {
            PropertyError::UnknownProperty(address.definition.as_str().to_owned())
        })?;
        let context = series_context(app, &address.target, definition)?;
        let SeriesEncoding::Heatmap(spec) = context.encoding else {
            return Err(not_applicable_encoding(definition, context.encoding));
        };
        let range = effective_range(spec, context.dataset, context.field, definition.id)?;
        let summary = summary_range(context.dataset, context.field, definition.id)?;
        Ok(ResolvedProperty {
            address: address.clone(),
            modified: Some(spec.value_range.is_some()),
            value: AggregateValue::Uniform(read_value(definition.id, range)?),
            default_value: Some(read_value(definition.id, summary)?),
            availability: Availability::Editable,
            schema: resolved_schema(definition, &context.capabilities),
        })
    }

    fn edit(
        &self,
        app: &PlotxApp,
        transaction: &mut PropertyTransaction,
        address: &PropertyAddress,
        operation: &EditOp<'_>,
    ) -> Result<(), PropertyError> {
        let definition = definition(address.definition).ok_or_else(|| {
            PropertyError::UnknownProperty(address.definition.as_str().to_owned())
        })?;
        let context = series_context(app, &address.target, definition)?;
        let SeriesEncoding::Heatmap(current) = context.encoding else {
            return Err(not_applicable_encoding(definition, context.encoding));
        };
        let current_range =
            effective_range(current, context.dataset, context.field, definition.id)?;

        let next_range = match operation {
            EditOp::Reset => None,
            EditOp::Set(value) => {
                let value = value
                    .as_float()
                    .ok_or_else(|| PropertyError::InvalidValue {
                        property: definition.id,
                        message: format!("expected a number, got {}", value.kind()),
                    })?;
                Some(range_with_value(definition.id, current_range, value)?)
            }
            EditOp::Step(direction) => {
                if definition.id != RANGE_SPAN {
                    return Err(PropertyError::InvalidValue {
                        property: definition.id,
                        message: "this setting has no step gesture".to_owned(),
                    });
                }
                let span = f64::from(current_range[1]) - f64::from(current_range[0]);
                let stepped = match direction {
                    PropertyStep::Raise => span * SPAN_STEP_RATIO,
                    PropertyStep::Lower => span / SPAN_STEP_RATIO,
                };
                Some(range_with_value(definition.id, current_range, stepped)?)
            }
        };

        let binding = transaction.data_binding(app, context.canvas, context.object)?;
        let series = binding
            .series
            .iter_mut()
            .find(|series| series.id == context.series)
            .ok_or_else(|| PropertyError::UnknownTarget(address.target.describe()))?;
        let SeriesEncoding::Heatmap(spec) = &mut series.encoding else {
            return Err(PropertyError::NotApplicable(
                "the series is no longer a heatmap".to_owned(),
            ));
        };
        spec.value_range = next_range;
        Ok(())
    }
}

fn summary_range(
    dataset: &Dataset,
    field: FieldId,
    property: PropertyId,
) -> Result<[f32; 2], PropertyError> {
    let summary = dataset
        .field_payload(field)
        .and_then(|payload| payload.summary())
        .ok_or_else(|| {
            PropertyError::NotApplicable(
                "the scalar field has no finite range to drive a colour scale".to_owned(),
            )
        })?;
    checked_range([summary.min.get(), summary.max.get()], property)
}

fn effective_range(
    spec: &HeatmapSpec,
    dataset: &Dataset,
    field: FieldId,
    property: PropertyId,
) -> Result<[f32; 2], PropertyError> {
    match spec.value_range {
        Some([lo, hi]) => checked_range([f64::from(lo), f64::from(hi)], property),
        None => summary_range(dataset, field, property),
    }
}

fn checked_range(range: [f64; 2], property: PropertyId) -> Result<[f32; 2], PropertyError> {
    let [lo, hi] = range;
    if !lo.is_finite() || !hi.is_finite() || lo < -MAX_VALUE || hi > MAX_VALUE || lo >= hi {
        return Err(PropertyError::InvalidValue {
            property,
            message: format!(
                "colour range [{lo}, {hi}] must contain two finite, increasing f32 values"
            ),
        });
    }
    let range = [lo as f32, hi as f32];
    if range[0] >= range[1] {
        return Err(PropertyError::InvalidValue {
            property,
            message: "colour range is too narrow to represent with f32 field values".to_owned(),
        });
    }
    Ok(range)
}

fn read_value(property: PropertyId, [lo, hi]: [f32; 2]) -> Result<PropertyValue, PropertyError> {
    match property {
        RANGE_SPAN => Ok(PropertyValue::Float(f64::from(hi) - f64::from(lo))),
        RANGE_CENTER => Ok(PropertyValue::Float((f64::from(lo) + f64::from(hi)) * 0.5)),
        _ => Err(PropertyError::UnknownProperty(property.as_str().to_owned())),
    }
}

fn range_with_value(
    property: PropertyId,
    current: [f32; 2],
    value: f64,
) -> Result<[f32; 2], PropertyError> {
    let center = (f64::from(current[0]) + f64::from(current[1])) * 0.5;
    let span = f64::from(current[1]) - f64::from(current[0]);
    let (center, span) = match property {
        RANGE_SPAN => (
            center,
            SPAN_BOUNDS.check(property, "colour range span", value)?,
        ),
        RANGE_CENTER => (
            CENTER_BOUNDS.check(property, "colour range centre", value)?,
            span,
        ),
        _ => return Err(PropertyError::UnknownProperty(property.as_str().to_owned())),
    };
    checked_range([center - span * 0.5, center + span * 0.5], property)
}
