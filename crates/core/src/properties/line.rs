//! Line-encoding properties on a plot object's series.

use super::provider::PropertyProvider;
use super::target::{SeriesContext, not_applicable_encoding, resolved_schema, series_context};
use super::{
    AggregateValue, Applicability, Availability, ComponentKind, DefaultPolicy, EditOp,
    EncodingKind, FloatBounds, PropertyAccess, PropertyAddress, PropertyDefinition, PropertyError,
    PropertyId, PropertyTransaction, PropertyValue, ResolvedProperty, ScopeKind, Tier, ValueCopies,
    ValueSchema, definition,
};
use crate::state::{
    PlotxApp, PresentationProfile, RequestedChart, default_encoding, field_peak_magnitude,
};
use plotx_figure::{PositiveFiniteF32, SeriesEncoding};

pub const STROKE_WIDTH: PropertyId = PropertyId("series.line.stroke_width");

const WIDTH_BOUNDS: FloatBounds = FloatBounds::inclusive(0.05, 10.0);
const WIDTH_STEP: f64 = 0.25;

pub(crate) const DEFINITIONS: &[PropertyDefinition] = &[PropertyDefinition {
    id: STROKE_WIDTH,
    scope_kind: ScopeKind::Object,
    value_schema: ValueSchema::Float {
        bounds: WIDTH_BOUNDS,
        log: false,
        drag_step: Some(WIDTH_STEP),
    },
    access: PropertyAccess::ReadWrite,
    applicability: Applicability::encoding(ComponentKind::Series, EncodingKind::Line),
    default_policy: DefaultPolicy::EncodingFactory,
    tier: Tier::Essential,
    copies: ValueCopies::PerTarget,
    canonical_label: "Line stroke width",
    canonical_aliases: &["line width", "stroke width", "trace thickness"],
}];

pub(crate) struct LineProvider;

pub(crate) static PROVIDER: LineProvider = LineProvider;

impl PropertyProvider for LineProvider {
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
        let SeriesEncoding::Line(line) = context.encoding else {
            return Err(not_applicable_encoding(definition, context.encoding));
        };
        Ok(ResolvedProperty {
            address: address.clone(),
            value: AggregateValue::Uniform(PropertyValue::Float(f64::from(line.width.get()))),
            default_value: factory_width(&context).map(PropertyValue::Float),
            availability: Availability::Editable,
            schema: resolved_schema(definition, &context.capabilities),
        })
    }

    fn edit(
        &self,
        app: &PlotxApp,
        transaction: &mut PropertyTransaction,
        address: &PropertyAddress,
        operation: EditOp,
    ) -> Result<(), PropertyError> {
        let definition = definition(address.definition).ok_or_else(|| {
            PropertyError::UnknownProperty(address.definition.as_str().to_owned())
        })?;
        let context = series_context(app, &address.target, definition)?;
        let SeriesEncoding::Line(current) = context.encoding else {
            return Err(not_applicable_encoding(definition, context.encoding));
        };
        let width = match operation {
            EditOp::Set(PropertyValue::Float(value)) => {
                WIDTH_BOUNDS.check(definition.id, "line width", value)?
            }
            EditOp::Set(value) => {
                return Err(PropertyError::InvalidValue {
                    property: definition.id,
                    message: format!("expected a number, got {}", value.kind()),
                });
            }
            // `DefaultPolicy::EncodingFactory` is a promise about *where* the
            // default comes from. Restating a literal here would keep that
            // promise only until the factory started choosing a width from the
            // field it is drawing.
            EditOp::Reset => factory_width(&context).ok_or_else(|| {
                PropertyError::NotApplicable(
                    "the source field is gone, so there is no factory line to rebuild".to_owned(),
                )
            })?,
            EditOp::Step(step) => {
                let current = f64::from(current.width.get());
                let next = match step {
                    super::PropertyStep::Raise => (current + WIDTH_STEP).min(WIDTH_BOUNDS.max),
                    super::PropertyStep::Lower => (current - WIDTH_STEP).max(WIDTH_BOUNDS.min),
                };
                if next == current {
                    return Err(PropertyError::InvalidValue {
                        property: definition.id,
                        message: format!(
                            "the line width is already at the {} bound ({current})",
                            match step {
                                super::PropertyStep::Raise => "upper",
                                super::PropertyStep::Lower => "lower",
                            }
                        ),
                    });
                }
                next
            }
        };
        let width = PositiveFiniteF32::new(width as f32).ok_or(PropertyError::InvalidValue {
            property: definition.id,
            message: "line width must be a finite positive number".to_owned(),
        })?;
        let binding = transaction.data_binding(app, context.canvas, context.object)?;
        let series = binding
            .series
            .iter_mut()
            .find(|series| series.id == context.series)
            .ok_or_else(|| PropertyError::UnknownTarget(address.target.describe()))?;
        let SeriesEncoding::Line(line) = &mut series.encoding else {
            return Err(PropertyError::NotApplicable(
                "the series is no longer a line".to_owned(),
            ));
        };
        line.width = width;
        Ok(())
    }
}

/// The stroke width the encoding factory would give this series right now.
///
/// Asked of the same factory that materializes a new encoding, in the target's
/// own context, which is exactly what `DefaultPolicy::EncodingFactory` declares.
fn factory_width(context: &SeriesContext<'_>) -> Option<f64> {
    let descriptor = context.dataset.field_descriptor(context.field)?;
    let encoding = default_encoding(
        &descriptor.capabilities,
        &descriptor.metadata,
        RequestedChart::Line,
        &PresentationProfile::default(),
        &|| field_peak_magnitude(context.dataset, context.field),
    );
    match encoding {
        SeriesEncoding::Line(line) => Some(f64::from(line.width.get())),
        _ => None,
    }
}
