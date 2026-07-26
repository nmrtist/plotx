//! Document-owned figure typography properties.

use super::provider::PropertyProvider;
use super::target::require_document_target;
use super::{
    AggregateValue, Applicability, Availability, ComponentKind, DefaultPolicy, EditOp, FloatBounds,
    PropertyAccess, PropertyAddress, PropertyDefinition, PropertyError, PropertyId,
    PropertyTransaction, PropertyValue, ResolvedProperty, ResolvedSchema, ScopeKind, Tier,
    ValueCopies, ValueSchema, definition,
};
use crate::state::PlotxApp;

pub const TICK_PT: PropertyId = PropertyId("document.figure.typography.tick_pt");

const POINT_BOUNDS: FloatBounds = FloatBounds::inclusive(1.0, 72.0);
/// A quarter point per drag notch: point sizes are chosen to a half point, and
/// the admissible range spans seventy-one of them, so the range says nothing
/// about how finely the value is usually set.
const POINT_STEP: f64 = 0.25;

pub(crate) const DEFINITIONS: &[PropertyDefinition] = &[PropertyDefinition {
    id: TICK_PT,
    scope_kind: ScopeKind::Document,
    value_schema: ValueSchema::Float {
        bounds: POINT_BOUNDS,
        log: false,
        drag_step: Some(POINT_STEP),
    },
    access: PropertyAccess::ReadWrite,
    applicability: Applicability::component(ComponentKind::None),
    default_policy: DefaultPolicy::Fixed(PropertyValue::Float(7.0)),
    tier: Tier::Essential,
    copies: ValueCopies::PerTarget,
    canonical_label: "Figure tick-label size",
    canonical_aliases: &["figure typography", "tick size", "font size", "points"],
}];

pub(crate) struct TypographyProvider;

pub(crate) static PROVIDER: TypographyProvider = TypographyProvider;

impl PropertyProvider for TypographyProvider {
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
        require_document_target(&address.target, definition)?;
        Ok(ResolvedProperty {
            address: address.clone(),
            value: AggregateValue::Uniform(PropertyValue::Float(f64::from(
                app.doc.style_library.figure_typography.tick_pt,
            ))),
            default_value: match definition.default_policy {
                DefaultPolicy::Fixed(value) => Some(value),
                DefaultPolicy::EncodingFactory
                | DefaultPolicy::ProcessingFactory
                | DefaultPolicy::None => None,
            },
            availability: Availability::Editable,
            schema: ResolvedSchema::Float {
                bounds: POINT_BOUNDS,
                log: false,
                unit: "pt",
            },
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
        require_document_target(&address.target, definition)?;
        let value = match operation {
            EditOp::Set(PropertyValue::Float(value)) => {
                POINT_BOUNDS.check(definition.id, "tick-label size", value)?
            }
            EditOp::Set(value) => {
                return Err(PropertyError::InvalidValue {
                    property: definition.id,
                    message: format!("expected a number, got {}", value.kind()),
                });
            }
            EditOp::Reset => match definition.default_policy {
                DefaultPolicy::Fixed(PropertyValue::Float(value)) => value,
                DefaultPolicy::Fixed(_)
                | DefaultPolicy::EncodingFactory
                | DefaultPolicy::ProcessingFactory
                | DefaultPolicy::None => {
                    return Err(PropertyError::InvalidValue {
                        property: definition.id,
                        message: "the default policy has no numeric value".to_owned(),
                    });
                }
            },
            EditOp::Step(_) => {
                return Err(PropertyError::InvalidValue {
                    property: definition.id,
                    message: "this setting has no step gesture".to_owned(),
                });
            }
        };
        transaction.figure_typography(app).tick_pt = value as f32;
        Ok(())
    }
}
