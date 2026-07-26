//! Application-owned default resolution for raster exports.

use super::provider::PropertyProvider;
use super::target::require_app_target;
use super::{
    AggregateValue, Applicability, Availability, ComponentKind, DefaultPolicy, EditOp,
    PropertyAccess, PropertyAddress, PropertyDefinition, PropertyError, PropertyId,
    PropertyTransaction, PropertyValue, ResolvedProperty, ResolvedSchema, ScopeKind, Tier,
    ValueCopies, ValueSchema, definition,
};
use crate::settings::{MAX_EXPORT_DPI, MIN_EXPORT_DPI};
use crate::state::PlotxApp;

pub const DPI: PropertyId = PropertyId("settings.export.dpi");

pub(crate) const DEFINITIONS: &[PropertyDefinition] = &[PropertyDefinition {
    id: DPI,
    scope_kind: ScopeKind::App,
    value_schema: ValueSchema::Int {
        min: MIN_EXPORT_DPI as i64,
        max: MAX_EXPORT_DPI as i64,
    },
    access: PropertyAccess::ReadWrite,
    applicability: Applicability::component(ComponentKind::None),
    default_policy: DefaultPolicy::Fixed(PropertyValue::Int(
        crate::export::DEFAULT_BITMAP_DPI as i64,
    )),
    tier: Tier::Essential,
    copies: ValueCopies::PerTarget,
    canonical_label: "Raster export resolution",
    canonical_aliases: &["export DPI", "bitmap resolution", "raster resolution"],
}];

pub(crate) struct ExportDpiProvider;

pub(crate) static PROVIDER: ExportDpiProvider = ExportDpiProvider;

impl PropertyProvider for ExportDpiProvider {
    fn definitions(&self) -> &'static [PropertyDefinition] {
        DEFINITIONS
    }

    fn read(
        &self,
        app: &PlotxApp,
        address: &PropertyAddress,
    ) -> Result<ResolvedProperty, PropertyError> {
        let definition = property_definition(address.definition)?;
        require_app_target(&address.target, definition)?;
        Ok(ResolvedProperty {
            address: address.clone(),
            value: AggregateValue::Uniform(PropertyValue::Int(i64::from(app.settings.export.dpi))),
            default_value: match definition.default_policy {
                DefaultPolicy::Fixed(value) => Some(value),
                DefaultPolicy::EncodingFactory
                | DefaultPolicy::ProcessingFactory
                | DefaultPolicy::None => None,
            },
            availability: Availability::Editable,
            schema: ResolvedSchema::Int {
                min: i64::from(MIN_EXPORT_DPI),
                max: i64::from(MAX_EXPORT_DPI),
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
        let definition = property_definition(address.definition)?;
        require_app_target(&address.target, definition)?;
        let value = match operation {
            EditOp::Set(PropertyValue::Int(value)) => checked_dpi(definition.id, value)?,
            EditOp::Set(value) => {
                return Err(PropertyError::InvalidValue {
                    property: definition.id,
                    message: format!("expected an integer, got {}", value.kind()),
                });
            }
            EditOp::Reset => match definition.default_policy {
                DefaultPolicy::Fixed(PropertyValue::Int(value)) => {
                    checked_dpi(definition.id, value)?
                }
                DefaultPolicy::Fixed(_)
                | DefaultPolicy::EncodingFactory
                | DefaultPolicy::ProcessingFactory
                | DefaultPolicy::None => {
                    return Err(PropertyError::InvalidValue {
                        property: definition.id,
                        message: "the default policy has no integer value".to_owned(),
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
        transaction.app_preferences(app).export.dpi = value;
        Ok(())
    }
}

fn property_definition(id: PropertyId) -> Result<&'static PropertyDefinition, PropertyError> {
    definition(id).ok_or_else(|| PropertyError::UnknownProperty(id.as_str().to_owned()))
}

fn checked_dpi(property: PropertyId, value: i64) -> Result<u16, PropertyError> {
    if (i64::from(MIN_EXPORT_DPI)..=i64::from(MAX_EXPORT_DPI)).contains(&value) {
        return Ok(value as u16);
    }
    Err(PropertyError::InvalidValue {
        property,
        message: format!(
            "raster export resolution {value} is outside {MIN_EXPORT_DPI}–{MAX_EXPORT_DPI} dpi"
        ),
    })
}
