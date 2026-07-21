use crate::{ColumnId, DataError, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LogicalType {
    Null,
    Boolean,
    Int64,
    Float64,
    Utf8,
    Categorical { levels: Vec<CategoryLevel> },
    Date,
    Time,
    Timestamp { display_timezone: String },
    Duration,
    Extension(ExtensionType),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CategoryLevel {
    pub value: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExtensionType {
    /// Reverse-domain registered identifier such as `space.nmrtist.nmr.shift`.
    pub id: String,
    pub version: u32,
    pub storage: Box<LogicalType>,
    /// If true, an environment that does not understand this extension may
    /// preserve it, but must refuse calculations involving the column.
    pub semantics_critical: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SemanticRole {
    #[default]
    Value,
    Identifier,
    Label,
    Group,
    Subject,
    Timepoint,
    QualityControl,
    Excluded,
    ExclusionReason,
    Custom(String),
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ColumnConstraints {
    #[serde(default)]
    pub unique: bool,
}

/// Affine conversion from displayed values to a canonical value:
/// `canonical = value * scale + offset`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UnitSpec {
    pub quantity: String,
    /// Canonicalized base dimensions and integer exponents.
    pub dimension: BTreeMap<String, i8>,
    pub canonical_unit: String,
    pub display_unit: String,
    pub scale: f64,
    pub offset: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ucum: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extension_id: Option<String>,
}

impl UnitSpec {
    pub fn dimensionless(display: impl Into<String>) -> Self {
        let display = display.into();
        Self {
            quantity: "dimensionless".into(),
            dimension: BTreeMap::new(),
            canonical_unit: "1".into(),
            display_unit: display.clone(),
            scale: 1.0,
            offset: 0.0,
            ucum: (display == "1").then(|| "1".into()),
            extension_id: None,
        }
    }

    pub fn ppm() -> Self {
        let mut unit = Self::dimensionless("ppm");
        unit.quantity = "ratio".into();
        unit.scale = 1e-6;
        unit.ucum = Some("[ppm]".into());
        unit
    }

    pub fn validate(&self) -> Result<()> {
        if self.quantity.trim().is_empty()
            || self.canonical_unit.trim().is_empty()
            || self.display_unit.trim().is_empty()
        {
            return Err(DataError::InvalidSchema(
                "unit names and quantity must not be empty".into(),
            ));
        }
        if !self.scale.is_finite() || self.scale == 0.0 || !self.offset.is_finite() {
            return Err(DataError::InvalidSchema(
                "unit scale must be finite and non-zero; offset must be finite".into(),
            ));
        }
        Ok(())
    }

    pub fn is_compatible_with(&self, other: &Self) -> bool {
        self.dimension == other.dimension
            && self.canonical_unit == other.canonical_unit
            && self.extension_id == other.extension_id
    }

    pub fn convert_value(&self, value: f64, target: &Self) -> Result<f64> {
        if !self.is_compatible_with(target) {
            return Err(DataError::IncompatibleUnits {
                left: self.display_unit.clone(),
                right: target.display_unit.clone(),
            });
        }
        Ok((value * self.scale + self.offset - target.offset) / target.scale)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ColumnSchema {
    pub id: ColumnId,
    pub name: String,
    pub logical_type: LogicalType,
    pub nullable: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<UnitSpec>,
    #[serde(default)]
    pub role: SemanticRole,
    #[serde(default)]
    pub constraints: ColumnConstraints,
    #[serde(default)]
    pub metadata: BTreeMap<String, serde_json::Value>,
}

impl ColumnSchema {
    pub fn new(name: impl Into<String>, logical_type: LogicalType) -> Self {
        Self {
            id: ColumnId::new(),
            name: name.into(),
            logical_type,
            nullable: true,
            unit: None,
            role: SemanticRole::Value,
            constraints: ColumnConstraints::default(),
            metadata: BTreeMap::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BusinessKey {
    pub name: String,
    pub columns: Vec<ColumnId>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TableSchema {
    pub columns: Vec<ColumnSchema>,
    #[serde(default)]
    pub business_keys: Vec<BusinessKey>,
    #[serde(default)]
    pub metadata: BTreeMap<String, serde_json::Value>,
}

impl TableSchema {
    pub fn new(columns: Vec<ColumnSchema>) -> Result<Self> {
        let schema = Self {
            columns,
            business_keys: Vec::new(),
            metadata: BTreeMap::new(),
        };
        schema.validate()?;
        Ok(schema)
    }

    pub fn column(&self, id: ColumnId) -> Option<&ColumnSchema> {
        self.columns.iter().find(|column| column.id == id)
    }

    pub fn column_index(&self, id: ColumnId) -> Option<usize> {
        self.columns.iter().position(|column| column.id == id)
    }

    pub fn validate(&self) -> Result<()> {
        let mut ids = BTreeSet::new();
        let mut names = BTreeSet::new();
        for column in &self.columns {
            if column.name.trim().is_empty() {
                return Err(DataError::InvalidSchema("column name is empty".into()));
            }
            if !ids.insert(column.id) {
                return Err(DataError::InvalidSchema(format!(
                    "duplicate column id {}",
                    column.id
                )));
            }
            if !names.insert(column.name.clone()) {
                return Err(DataError::InvalidSchema(format!(
                    "duplicate column name {:?}",
                    column.name
                )));
            }
            if let Some(unit) = &column.unit {
                unit.validate()?;
                if !matches!(
                    column.logical_type,
                    LogicalType::Int64
                        | LogicalType::Float64
                        | LogicalType::Duration
                        | LogicalType::Extension(_)
                ) {
                    return Err(DataError::InvalidSchema(format!(
                        "column {:?} has a unit but is not numeric",
                        column.name
                    )));
                }
            }
            validate_logical_type(&column.logical_type)?;
        }
        for key in &self.business_keys {
            if key.name.trim().is_empty() || key.columns.is_empty() {
                return Err(DataError::InvalidSchema(
                    "business keys require a name and at least one column".into(),
                ));
            }
            let mut key_ids = BTreeSet::new();
            for id in &key.columns {
                let column = self.column(*id).ok_or(DataError::MissingColumn(*id))?;
                if column.nullable {
                    return Err(DataError::InvalidSchema(format!(
                        "business key {:?} uses nullable column {:?}",
                        key.name, column.name
                    )));
                }
                if !key_ids.insert(*id) {
                    return Err(DataError::InvalidSchema(format!(
                        "business key {:?} repeats column {}",
                        key.name, id
                    )));
                }
            }
        }
        Ok(())
    }
}

fn validate_logical_type(logical_type: &LogicalType) -> Result<()> {
    match logical_type {
        LogicalType::Categorical { levels } => {
            let unique: BTreeSet<_> = levels.iter().map(|level| &level.value).collect();
            if unique.len() != levels.len() {
                return Err(DataError::InvalidSchema(
                    "categorical levels must be unique".into(),
                ));
            }
        }
        LogicalType::Timestamp { display_timezone }
            if display_timezone.parse::<chrono_tz::Tz>().is_err() =>
        {
            return Err(DataError::InvalidSchema(format!(
                "timestamp display timezone {display_timezone:?} is not an IANA name"
            )));
        }
        LogicalType::Extension(extension) => {
            if extension.version != 1 || !extension.id.contains('.') {
                return Err(DataError::InvalidSchema(
                    "extension types require a reverse-domain id and v1 contract".into(),
                ));
            }
            if matches!(*extension.storage, LogicalType::Extension(_)) {
                return Err(DataError::InvalidSchema(
                    "extension storage cannot itself be an extension".into(),
                ));
            }
            validate_logical_type(&extension.storage)?;
        }
        _ => {}
    }
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UncertaintyRelation {
    pub value: ColumnId,
    pub kind: UncertaintyKind,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum UncertaintyKind {
    Symmetric {
        column: ColumnId,
        meaning: UncertaintyMeaning,
    },
    Asymmetric {
        lower: ColumnId,
        upper: ColumnId,
        meaning: UncertaintyMeaning,
    },
    ConfidenceInterval {
        lower: ColumnId,
        upper: ColumnId,
        level: f64,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UncertaintyMeaning {
    MeasurementStandardDeviation,
    SampleStandardDeviation,
    StandardError,
    Custom(String),
}

impl UncertaintyRelation {
    pub fn validate(&self, schema: &TableSchema) -> Result<()> {
        let value = numeric_column(schema, self.value)?;
        let related: Vec<ColumnId> = match &self.kind {
            UncertaintyKind::Symmetric { column, .. } => vec![*column],
            UncertaintyKind::Asymmetric { lower, upper, .. }
            | UncertaintyKind::ConfidenceInterval { lower, upper, .. } => vec![*lower, *upper],
        };
        if let UncertaintyKind::ConfidenceInterval { level, .. } = self.kind
            && (!(0.0..1.0).contains(&level) || !level.is_finite())
        {
            return Err(DataError::InvalidSchema(
                "confidence level must be finite and between zero and one".into(),
            ));
        }
        for id in related {
            let uncertainty = numeric_column(schema, id)?;
            match (&value.unit, &uncertainty.unit) {
                (Some(left), Some(right)) if left.is_compatible_with(right) => {}
                (None, None) => {}
                _ => {
                    return Err(DataError::IncompatibleUnits {
                        left: value
                            .unit
                            .as_ref()
                            .map_or_else(|| "(none)".into(), |unit| unit.display_unit.clone()),
                        right: uncertainty
                            .unit
                            .as_ref()
                            .map_or_else(|| "(none)".into(), |unit| unit.display_unit.clone()),
                    });
                }
            }
        }
        Ok(())
    }
}

fn numeric_column(schema: &TableSchema, id: ColumnId) -> Result<&ColumnSchema> {
    let column = schema.column(id).ok_or(DataError::MissingColumn(id))?;
    if !matches!(
        column.logical_type,
        LogicalType::Int64 | LogicalType::Float64
    ) {
        return Err(DataError::InvalidSchema(format!(
            "uncertainty column {:?} is not numeric",
            column.name
        )));
    }
    Ok(column)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn length_unit(display: &str, scale: f64) -> UnitSpec {
        UnitSpec {
            quantity: "length".into(),
            dimension: BTreeMap::from([("m".into(), 1)]),
            canonical_unit: "m".into(),
            display_unit: display.into(),
            scale,
            offset: 0.0,
            ucum: Some(display.into()),
            extension_id: None,
        }
    }

    #[test]
    fn units_convert_affinely_and_reject_other_dimensions() {
        let metre = length_unit("m", 1.0);
        let millimetre = length_unit("mm", 1e-3);
        assert_eq!(millimetre.convert_value(250.0, &metre).unwrap(), 0.25);
        assert!(millimetre.convert_value(1.0, &UnitSpec::ppm()).is_err());
    }

    #[test]
    fn business_key_columns_must_be_non_null() {
        let column = ColumnSchema::new("sample", LogicalType::Utf8);
        let id = column.id;
        let mut schema = TableSchema::new(vec![column]).unwrap();
        schema.business_keys.push(BusinessKey {
            name: "sample".into(),
            columns: vec![id],
        });
        assert!(schema.validate().is_err());
    }

    #[test]
    fn asymmetric_and_confidence_uncertainty_validate_units_and_levels() {
        let mut value = ColumnSchema::new("value", LogicalType::Float64);
        let mut lower = ColumnSchema::new("lower", LogicalType::Float64);
        let mut upper = ColumnSchema::new("upper", LogicalType::Float64);
        value.unit = Some(length_unit("m", 1.0));
        lower.unit = Some(length_unit("mm", 1e-3));
        upper.unit = Some(length_unit("m", 1.0));
        let schema = TableSchema::new(vec![value.clone(), lower.clone(), upper.clone()]).unwrap();

        UncertaintyRelation {
            value: value.id,
            kind: UncertaintyKind::Asymmetric {
                lower: lower.id,
                upper: upper.id,
                meaning: UncertaintyMeaning::StandardError,
            },
        }
        .validate(&schema)
        .unwrap();
        UncertaintyRelation {
            value: value.id,
            kind: UncertaintyKind::ConfidenceInterval {
                lower: lower.id,
                upper: upper.id,
                level: 0.95,
            },
        }
        .validate(&schema)
        .unwrap();

        let invalid_level = UncertaintyRelation {
            value: value.id,
            kind: UncertaintyKind::ConfidenceInterval {
                lower: lower.id,
                upper: upper.id,
                level: 1.0,
            },
        };
        assert!(invalid_level.validate(&schema).is_err());

        upper.unit = Some(UnitSpec::ppm());
        let incompatible = TableSchema::new(vec![value.clone(), lower.clone(), upper]).unwrap();
        assert!(matches!(
            UncertaintyRelation {
                value: value.id,
                kind: UncertaintyKind::Asymmetric {
                    lower: lower.id,
                    upper: incompatible.columns[2].id,
                    meaning: UncertaintyMeaning::SampleStandardDeviation,
                },
            }
            .validate(&incompatible),
            Err(DataError::IncompatibleUnits { .. })
        ));
    }

    #[test]
    fn timestamp_display_timezone_must_be_registered_by_iana() {
        TableSchema::new(vec![ColumnSchema::new(
            "instant",
            LogicalType::Timestamp {
                display_timezone: "Asia/Singapore".into(),
            },
        )])
        .unwrap();
        assert!(
            TableSchema::new(vec![ColumnSchema::new(
                "instant",
                LogicalType::Timestamp {
                    display_timezone: "Mars/Olympus".into(),
                },
            )])
            .is_err()
        );
    }
}
