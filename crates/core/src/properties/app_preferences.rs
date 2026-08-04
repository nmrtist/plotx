//! Persistent application preferences exposed through the property catalog.

use super::provider::PropertyProvider;
use super::target::require_app_target;
use super::{
    AggregateValue, Applicability, Availability, ComponentKind, DefaultPolicy, EditOp, EnumVariant,
    PropertyAccess, PropertyAddress, PropertyDefinition, PropertyError, PropertyId,
    PropertyTransaction, PropertyValue, ResolvedProperty, ResolvedSchema, ScopeKind, Tier,
    ValueCopies, ValueSchema, definition,
};
use crate::settings::{GraphicsPowerPreference, MAX_PROJECT_BACKUP_GENERATIONS, ThemeMode};
use crate::state::PlotxApp;
use crate::update::{UpdateChannel, UpdateChannelSetting};
use plotx_figure::Color;

pub const SNAP_ENABLED: PropertyId = PropertyId("settings.general.snap_enabled");
pub const EQUAL_SCALE_HOMONUCLEAR_2D_IMPORTS: PropertyId =
    PropertyId("settings.general.equal_scale_homonuclear_2d_imports");
pub const KEEP_EMPTY_SOURCE_CANVAS: PropertyId =
    PropertyId("settings.general.keep_empty_source_canvas");
pub const PROJECT_BACKUP_GENERATIONS: PropertyId =
    PropertyId("settings.general.project_backup_generations");
pub const THEME: PropertyId = PropertyId("settings.appearance.theme");
pub const GRAPHICS_POWER: PropertyId = PropertyId("settings.appearance.graphics_power");
pub const ACCENT_COLOR: PropertyId = PropertyId("settings.appearance.accent.color");
pub const INCLUDE_VIEW_SNAPSHOTS: PropertyId = PropertyId("settings.export.include_view_snapshots");
pub const TRIM_TO_VISIBLE_CONTENT: PropertyId =
    PropertyId("settings.export.trim_to_visible_content");
pub const SCALE_CONTENT: PropertyId = PropertyId("settings.canvas_size.scale_content");
pub const AUTO_CHECK_UPDATES: PropertyId = PropertyId("settings.updates.auto_check");
pub const UPDATE_CHANNEL: PropertyId = PropertyId("settings.updates.channel");

pub const THEME_SYSTEM: &str = "system";
pub const THEME_LIGHT: &str = "light";
pub const THEME_DARK: &str = "dark";
pub const GRAPHICS_LOW_POWER: &str = "low_power";
pub const GRAPHICS_HIGH_PERFORMANCE: &str = "high_performance";
pub const UPDATE_AUTO: &str = "auto";
pub const UPDATE_STABLE: &str = "stable";
pub const UPDATE_BETA: &str = "beta";
pub const UPDATE_ALPHA: &str = "alpha";

/// Core reports this derived default as a headless-environment substitute,
/// because no live theme colour exists there. The desktop presentation layer
/// replaces it with the current theme colour.
pub const ACCENT_PLACEHOLDER: Color = Color::BLACK;

const THEME_VARIANTS: &[EnumVariant] = &[
    EnumVariant::new(THEME_SYSTEM, "Follow system"),
    EnumVariant::new(THEME_LIGHT, "Light"),
    EnumVariant::new(THEME_DARK, "Dark"),
];
const GRAPHICS_VARIANTS: &[EnumVariant] = &[
    EnumVariant::new(GRAPHICS_LOW_POWER, "Power saving"),
    EnumVariant::new(GRAPHICS_HIGH_PERFORMANCE, "High performance"),
];
const UPDATE_VARIANTS: &[EnumVariant] = &[
    EnumVariant::new(UPDATE_AUTO, "Follow build"),
    EnumVariant::new(UPDATE_STABLE, "Stable"),
    EnumVariant::new(UPDATE_BETA, "Beta"),
    EnumVariant::new(UPDATE_ALPHA, "Alpha"),
];
const UPDATE_VARIANTS_STABLE: &[EnumVariant] = &[
    EnumVariant::new(UPDATE_AUTO, "Follow build (stable)"),
    EnumVariant::new(UPDATE_STABLE, "Stable"),
    EnumVariant::new(UPDATE_BETA, "Beta"),
    EnumVariant::new(UPDATE_ALPHA, "Alpha"),
];
const UPDATE_VARIANTS_BETA: &[EnumVariant] = &[
    EnumVariant::new(UPDATE_AUTO, "Follow build (beta)"),
    EnumVariant::new(UPDATE_STABLE, "Stable"),
    EnumVariant::new(UPDATE_BETA, "Beta"),
    EnumVariant::new(UPDATE_ALPHA, "Alpha"),
];
const UPDATE_VARIANTS_ALPHA: &[EnumVariant] = &[
    EnumVariant::new(UPDATE_AUTO, "Follow build (alpha)"),
    EnumVariant::new(UPDATE_STABLE, "Stable"),
    EnumVariant::new(UPDATE_BETA, "Beta"),
    EnumVariant::new(UPDATE_ALPHA, "Alpha"),
];

const fn app_definition(
    id: PropertyId,
    value_schema: ValueSchema,
    default_policy: DefaultPolicy,
    canonical_label: &'static str,
    canonical_aliases: &'static [&'static str],
) -> PropertyDefinition {
    PropertyDefinition {
        id,
        scope_kind: ScopeKind::App,
        value_schema,
        access: PropertyAccess::ReadWrite,
        applicability: Applicability::component(ComponentKind::None),
        default_policy,
        tier: Tier::Essential,
        copies: ValueCopies::PerTarget,
        canonical_label,
        canonical_aliases,
    }
}

pub(crate) const DEFINITIONS: &[PropertyDefinition] = &[
    app_definition(
        SNAP_ENABLED,
        ValueSchema::Bool,
        DefaultPolicy::Fixed(PropertyValue::Bool(true)),
        "Object snapping",
        &["snap", "snap to guides"],
    ),
    app_definition(
        EQUAL_SCALE_HOMONUCLEAR_2D_IMPORTS,
        ValueSchema::Bool,
        DefaultPolicy::Fixed(PropertyValue::Bool(true)),
        "Equal scale for homonuclear 2D imports",
        &[
            "1:1 F1 F2 scale",
            "lock homonuclear aspect",
            "equal axis scale",
        ],
    ),
    app_definition(
        KEEP_EMPTY_SOURCE_CANVAS,
        ValueSchema::Bool,
        DefaultPolicy::Fixed(PropertyValue::Bool(false)),
        "Keep empty source canvas",
        &["keep source canvas", "empty canvas after tiling"],
    ),
    app_definition(
        PROJECT_BACKUP_GENERATIONS,
        ValueSchema::Int {
            min: 0,
            max: MAX_PROJECT_BACKUP_GENERATIONS as i64,
        },
        DefaultPolicy::Fixed(PropertyValue::Int(1)),
        "Project backup copies",
        &["backup generations", "previous project saves"],
    ),
    app_definition(
        THEME,
        ValueSchema::Enum {
            variants: THEME_VARIANTS,
        },
        DefaultPolicy::Fixed(PropertyValue::Enum(THEME_SYSTEM)),
        "Chrome theme",
        &["appearance theme", "light mode", "dark mode"],
    ),
    app_definition(
        GRAPHICS_POWER,
        ValueSchema::Enum {
            variants: GRAPHICS_VARIANTS,
        },
        DefaultPolicy::Fixed(PropertyValue::Enum(GRAPHICS_LOW_POWER)),
        "Graphics mode",
        &["GPU preference", "graphics power", "graphics processor"],
    ),
    app_definition(
        ACCENT_COLOR,
        ValueSchema::Color,
        DefaultPolicy::Derived,
        "Canvas accent",
        &["selection colour", "guide color", "accent color"],
    ),
    app_definition(
        INCLUDE_VIEW_SNAPSHOTS,
        ValueSchema::Bool,
        DefaultPolicy::Fixed(PropertyValue::Bool(false)),
        "Embed view snapshots",
        &["save view snapshots", "project snapshots"],
    ),
    app_definition(
        TRIM_TO_VISIBLE_CONTENT,
        ValueSchema::Bool,
        DefaultPolicy::Fixed(PropertyValue::Bool(false)),
        "Trim to visible content",
        &["trim export", "remove page whitespace"],
    ),
    app_definition(
        SCALE_CONTENT,
        ValueSchema::Bool,
        DefaultPolicy::Fixed(PropertyValue::Bool(false)),
        "Scale content with page size",
        &["scale page content", "resize canvas content"],
    ),
    app_definition(
        AUTO_CHECK_UPDATES,
        ValueSchema::Bool,
        DefaultPolicy::Fixed(PropertyValue::Bool(true)),
        "Automatic updates",
        &["check for updates", "background updates"],
    ),
    app_definition(
        UPDATE_CHANNEL,
        ValueSchema::Enum {
            variants: UPDATE_VARIANTS,
        },
        DefaultPolicy::Fixed(PropertyValue::Enum(UPDATE_AUTO)),
        "Update channel",
        &["release channel", "update train"],
    ),
];

pub(crate) struct AppPreferencesProvider;
pub(crate) static PROVIDER: AppPreferencesProvider = AppPreferencesProvider;

impl PropertyProvider for AppPreferencesProvider {
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
        let default_value = if definition.id == ACCENT_COLOR {
            Some(PropertyValue::Color(ACCENT_PLACEHOLDER))
        } else {
            fixed_default(definition)
        };
        Ok(ResolvedProperty {
            address: address.clone(),
            modified: (definition.id == ACCENT_COLOR)
                .then_some(app.settings.appearance.canvas_accent.is_some()),
            value: AggregateValue::Uniform(value_of(app, definition.id)?),
            default_value,
            availability: Availability::Editable,
            schema: resolved_schema(definition),
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
        require_app_target(&address.target, definition)?;
        if definition.id == ACCENT_COLOR && matches!(operation, EditOp::Reset) {
            transaction.app_preferences(app).appearance.canvas_accent = None;
            return Ok(());
        }
        let value = match operation {
            EditOp::Set(value) => checked_value(definition, value)?,
            EditOp::Reset => {
                fixed_default(definition).ok_or_else(|| PropertyError::InvalidValue {
                    property: definition.id,
                    message: "this preference has no reset value".to_owned(),
                })?
            }
            EditOp::Step(_) => {
                return Err(PropertyError::InvalidValue {
                    property: definition.id,
                    message: "this preference has no step gesture".to_owned(),
                });
            }
        };
        write_value(transaction.app_preferences(app), definition.id, value)
    }
}

fn property_definition(id: PropertyId) -> Result<&'static PropertyDefinition, PropertyError> {
    definition(id).ok_or_else(|| PropertyError::UnknownProperty(id.as_str().to_owned()))
}

fn fixed_default(definition: &'static PropertyDefinition) -> Option<PropertyValue> {
    match &definition.default_policy {
        DefaultPolicy::Fixed(value) => Some(value.clone()),
        DefaultPolicy::EncodingFactory
        | DefaultPolicy::ProcessingFactory
        | DefaultPolicy::Derived
        | DefaultPolicy::None => None,
    }
}

fn resolved_schema(definition: &'static PropertyDefinition) -> ResolvedSchema {
    if definition.id == UPDATE_CHANNEL {
        let variants = match UpdateChannel::built_in() {
            UpdateChannel::Stable => UPDATE_VARIANTS_STABLE,
            UpdateChannel::Beta => UPDATE_VARIANTS_BETA,
            UpdateChannel::Alpha => UPDATE_VARIANTS_ALPHA,
        };
        return ResolvedSchema::Enum {
            variants: variants.iter().collect(),
        };
    }
    match definition.value_schema {
        ValueSchema::Bool => ResolvedSchema::Bool,
        ValueSchema::Int { min, max } => ResolvedSchema::Int { min, max, unit: "" },
        ValueSchema::Enum { variants } => ResolvedSchema::Enum {
            variants: variants.iter().collect(),
        },
        ValueSchema::Color => ResolvedSchema::Color,
        ValueSchema::Text
        | ValueSchema::IntWithDrag { .. }
        | ValueSchema::SteppedInt { .. }
        | ValueSchema::Float { .. } => {
            unreachable!("application preference definitions use bool, int, enum, or color")
        }
    }
}

fn value_of(app: &PlotxApp, id: PropertyId) -> Result<PropertyValue, PropertyError> {
    let settings = &app.settings;
    Ok(match id {
        SNAP_ENABLED => PropertyValue::Bool(settings.general.snap_enabled),
        EQUAL_SCALE_HOMONUCLEAR_2D_IMPORTS => {
            PropertyValue::Bool(settings.general.equal_scale_homonuclear_2d_imports)
        }
        KEEP_EMPTY_SOURCE_CANVAS => PropertyValue::Bool(settings.general.keep_empty_source_canvas),
        PROJECT_BACKUP_GENERATIONS => {
            PropertyValue::Int(i64::from(settings.general.project_backup_generations))
        }
        THEME => PropertyValue::Enum(theme_key(settings.appearance.theme)),
        GRAPHICS_POWER => PropertyValue::Enum(graphics_key(settings.appearance.graphics_power)),
        ACCENT_COLOR => {
            let [r, g, b] = settings.appearance.canvas_accent.unwrap_or([
                ACCENT_PLACEHOLDER.r,
                ACCENT_PLACEHOLDER.g,
                ACCENT_PLACEHOLDER.b,
            ]);
            PropertyValue::Color(Color::rgb(r, g, b))
        }
        INCLUDE_VIEW_SNAPSHOTS => PropertyValue::Bool(settings.export.include_view_snapshots),
        TRIM_TO_VISIBLE_CONTENT => PropertyValue::Bool(settings.export.trim_to_visible_content),
        SCALE_CONTENT => PropertyValue::Bool(settings.canvas_size.scale_content),
        AUTO_CHECK_UPDATES => PropertyValue::Bool(settings.updates.auto_check),
        UPDATE_CHANNEL => PropertyValue::Enum(update_key(settings.updates.channel)),
        _ => return Err(PropertyError::UnknownProperty(id.to_string())),
    })
}

fn checked_value(
    definition: &'static PropertyDefinition,
    value: &PropertyValue,
) -> Result<PropertyValue, PropertyError> {
    match (definition.value_schema, value) {
        (ValueSchema::Bool, PropertyValue::Bool(value)) => Ok(PropertyValue::Bool(*value)),
        (ValueSchema::Int { min, max }, PropertyValue::Int(value))
            if (min..=max).contains(value) =>
        {
            Ok(PropertyValue::Int(*value))
        }
        (ValueSchema::Int { min, max }, PropertyValue::Int(value)) => {
            Err(PropertyError::InvalidValue {
                property: definition.id,
                message: format!(
                    "{} {value} is out of range: it must be between {min} and {max}",
                    definition.canonical_label
                ),
            })
        }
        (ValueSchema::Enum { variants }, PropertyValue::Enum(value))
            if variants.iter().any(|variant| variant.id == *value) =>
        {
            Ok(PropertyValue::Enum(value))
        }
        (ValueSchema::Color, PropertyValue::Color(value)) => Ok(PropertyValue::Color(*value)),
        (_, value) => Err(PropertyError::InvalidValue {
            property: definition.id,
            message: format!(
                "{} does not accept {}",
                definition.canonical_label,
                value.kind()
            ),
        }),
    }
}

fn write_value(
    settings: &mut crate::settings::Settings,
    id: PropertyId,
    value: PropertyValue,
) -> Result<(), PropertyError> {
    match (id, value) {
        (SNAP_ENABLED, PropertyValue::Bool(value)) => settings.general.snap_enabled = value,
        (EQUAL_SCALE_HOMONUCLEAR_2D_IMPORTS, PropertyValue::Bool(value)) => {
            settings.general.equal_scale_homonuclear_2d_imports = value
        }
        (KEEP_EMPTY_SOURCE_CANVAS, PropertyValue::Bool(value)) => {
            settings.general.keep_empty_source_canvas = value
        }
        (PROJECT_BACKUP_GENERATIONS, PropertyValue::Int(value)) => {
            settings.general.project_backup_generations = value as u8
        }
        (THEME, PropertyValue::Enum(value)) => {
            settings.appearance.theme = theme(value).expect("validated theme")
        }
        (GRAPHICS_POWER, PropertyValue::Enum(value)) => {
            settings.appearance.graphics_power = graphics(value).expect("validated graphics power")
        }
        (ACCENT_COLOR, PropertyValue::Color(value)) => {
            settings.appearance.canvas_accent = Some([value.r, value.g, value.b])
        }
        (INCLUDE_VIEW_SNAPSHOTS, PropertyValue::Bool(value)) => {
            settings.export.include_view_snapshots = value
        }
        (TRIM_TO_VISIBLE_CONTENT, PropertyValue::Bool(value)) => {
            settings.export.trim_to_visible_content = value
        }
        (SCALE_CONTENT, PropertyValue::Bool(value)) => settings.canvas_size.scale_content = value,
        (AUTO_CHECK_UPDATES, PropertyValue::Bool(value)) => settings.updates.auto_check = value,
        (UPDATE_CHANNEL, PropertyValue::Enum(value)) => {
            settings.updates.channel = update(value).expect("validated update channel")
        }
        _ => {
            return Err(PropertyError::InvalidValue {
                property: id,
                message: "the validated preference value changed shape".to_owned(),
            });
        }
    }
    Ok(())
}

fn theme_key(value: ThemeMode) -> &'static str {
    match value {
        ThemeMode::System => THEME_SYSTEM,
        ThemeMode::Light => THEME_LIGHT,
        ThemeMode::Dark => THEME_DARK,
    }
}

fn theme(value: &str) -> Option<ThemeMode> {
    match value {
        THEME_SYSTEM => Some(ThemeMode::System),
        THEME_LIGHT => Some(ThemeMode::Light),
        THEME_DARK => Some(ThemeMode::Dark),
        _ => None,
    }
}

fn graphics_key(value: GraphicsPowerPreference) -> &'static str {
    match value {
        GraphicsPowerPreference::LowPower => GRAPHICS_LOW_POWER,
        GraphicsPowerPreference::HighPerformance => GRAPHICS_HIGH_PERFORMANCE,
    }
}

fn graphics(value: &str) -> Option<GraphicsPowerPreference> {
    match value {
        GRAPHICS_LOW_POWER => Some(GraphicsPowerPreference::LowPower),
        GRAPHICS_HIGH_PERFORMANCE => Some(GraphicsPowerPreference::HighPerformance),
        _ => None,
    }
}

fn update_key(value: UpdateChannelSetting) -> &'static str {
    match value {
        UpdateChannelSetting::Auto => UPDATE_AUTO,
        UpdateChannelSetting::Stable => UPDATE_STABLE,
        UpdateChannelSetting::Beta => UPDATE_BETA,
        UpdateChannelSetting::Alpha => UPDATE_ALPHA,
    }
}

fn update(value: &str) -> Option<UpdateChannelSetting> {
    match value {
        UPDATE_AUTO => Some(UpdateChannelSetting::Auto),
        UPDATE_STABLE => Some(UpdateChannelSetting::Stable),
        UPDATE_BETA => Some(UpdateChannelSetting::Beta),
        UPDATE_ALPHA => Some(UpdateChannelSetting::Alpha),
        _ => None,
    }
}

#[cfg(test)]
#[path = "app_preferences_tests.rs"]
mod tests;
