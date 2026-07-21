//! Parsing and formatting between on-screen cell text and typed literal values.

use plotx_core::data::{ColumnSchema, FiniteOrSpecial, LiteralValue, LogicalType, ScalarValue};

pub(super) fn parse_typed_cell(text: &str, schema: &ColumnSchema) -> Result<LiteralValue, String> {
    let text = text.trim();
    if text.is_empty() || text.eq_ignore_ascii_case("null") {
        return schema
            .nullable
            .then_some(LiteralValue::Null)
            .ok_or_else(|| format!("{} is not nullable", schema.name));
    }
    let invalid = || format!("{text:?} is not valid for {}", schema.name);
    match &schema.logical_type {
        LogicalType::Null => Err(invalid()),
        LogicalType::Boolean => text
            .parse()
            .map(LiteralValue::Boolean)
            .map_err(|_| invalid()),
        LogicalType::Int64 => text.parse().map(LiteralValue::Int64).map_err(|_| invalid()),
        LogicalType::Float64 => parse_float(text)
            .map(|value| LiteralValue::Float64(FiniteOrSpecial::new(value)))
            .ok_or_else(invalid),
        LogicalType::Utf8 => Ok(LiteralValue::Utf8(text.to_owned())),
        LogicalType::Categorical { levels } => levels
            .iter()
            .position(|level| {
                level.value == text || level.label.as_deref().is_some_and(|label| label == text)
            })
            .and_then(|index| u32::try_from(index).ok())
            .map(LiteralValue::Categorical)
            .ok_or_else(invalid),
        LogicalType::Date => text.parse().map(LiteralValue::Date).map_err(|_| invalid()),
        LogicalType::Time => text.parse().map(LiteralValue::Time).map_err(|_| invalid()),
        LogicalType::Timestamp { .. } => text
            .parse()
            .map(LiteralValue::Timestamp)
            .map_err(|_| invalid()),
        LogicalType::Duration => text
            .parse()
            .map(LiteralValue::Duration)
            .map_err(|_| invalid()),
        LogicalType::Extension(_) => Err(format!(
            "{} uses an extension type that this editor cannot modify",
            schema.name
        )),
    }
}

fn parse_float(text: &str) -> Option<f64> {
    match text.to_ascii_lowercase().as_str() {
        "nan" => Some(f64::NAN),
        "+inf" | "inf" | "+infinity" | "infinity" => Some(f64::INFINITY),
        "-inf" | "-infinity" => Some(f64::NEG_INFINITY),
        _ => text.parse().ok(),
    }
}

pub(super) fn scalar_to_literal(value: &ScalarValue) -> Result<LiteralValue, String> {
    Ok(match value {
        ScalarValue::Null => LiteralValue::Null,
        ScalarValue::Boolean(value) => LiteralValue::Boolean(*value),
        ScalarValue::Int64(value) => LiteralValue::Int64(*value),
        ScalarValue::Float64(value) => LiteralValue::Float64(FiniteOrSpecial::new(*value)),
        ScalarValue::Utf8(value) => LiteralValue::Utf8(value.clone()),
        ScalarValue::Categorical(value) => LiteralValue::Categorical(*value),
        ScalarValue::Date(value) => LiteralValue::Date(*value),
        ScalarValue::Time(value) => LiteralValue::Time(*value),
        ScalarValue::Timestamp(value) => LiteralValue::Timestamp(*value),
        ScalarValue::Duration(value) => LiteralValue::Duration(*value),
        ScalarValue::Extension { .. } => return Err("extension values are read-only".into()),
    })
}

pub(super) fn typed_cell_text(value: &ScalarValue, logical_type: &LogicalType) -> String {
    match value {
        ScalarValue::Null => String::new(),
        ScalarValue::Boolean(value) => value.to_string(),
        ScalarValue::Int64(value) => value.to_string(),
        ScalarValue::Float64(value) if value.is_nan() => "NaN".into(),
        ScalarValue::Float64(value) if *value == f64::INFINITY => "+Inf".into(),
        ScalarValue::Float64(value) if *value == f64::NEG_INFINITY => "-Inf".into(),
        ScalarValue::Float64(value) => value.to_string(),
        ScalarValue::Utf8(value) => value.clone(),
        ScalarValue::Categorical(index) => match logical_type {
            LogicalType::Categorical { levels } => levels.get(*index as usize).map_or_else(
                || format!("#{index}"),
                |level| level.label.clone().unwrap_or_else(|| level.value.clone()),
            ),
            _ => format!("#{index}"),
        },
        ScalarValue::Date(value) => value.to_string(),
        ScalarValue::Time(value) | ScalarValue::Timestamp(value) | ScalarValue::Duration(value) => {
            value.to_string()
        }
        ScalarValue::Extension { storage, .. } => typed_cell_text(storage, logical_type),
    }
}

/// True when the value is a placeholder (NULL or non-finite) that should be
/// rendered de-emphasized rather than as ordinary data.
pub(super) fn is_placeholder(value: &ScalarValue) -> bool {
    match value {
        ScalarValue::Null => true,
        ScalarValue::Float64(value) => !value.is_finite(),
        _ => false,
    }
}

/// Numeric and temporal cells right-align so magnitudes line up.
pub(super) fn right_aligned(logical_type: &LogicalType) -> bool {
    matches!(
        logical_type,
        LogicalType::Int64
            | LogicalType::Float64
            | LogicalType::Date
            | LogicalType::Time
            | LogicalType::Timestamp { .. }
            | LogicalType::Duration
    )
}

/// One-word type name for the header's secondary line.
pub(super) fn type_label(logical_type: &LogicalType) -> &'static str {
    match logical_type {
        LogicalType::Null => "Null",
        LogicalType::Boolean => "Boolean",
        LogicalType::Int64 => "Integer",
        LogicalType::Float64 => "Number",
        LogicalType::Utf8 => "Text",
        LogicalType::Categorical { .. } => "Category",
        LogicalType::Date => "Date",
        LogicalType::Time => "Time",
        LogicalType::Timestamp { .. } => "Timestamp",
        LogicalType::Duration => "Duration",
        LogicalType::Extension(_) => "Extension",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_parser_distinguishes_null_from_non_finite_float_values() {
        let mut schema = ColumnSchema::new("value", LogicalType::Float64);
        schema.nullable = true;
        assert_eq!(parse_typed_cell("", &schema).unwrap(), LiteralValue::Null);
        assert_eq!(
            parse_typed_cell("NaN", &schema).unwrap(),
            LiteralValue::Float64(FiniteOrSpecial::Nan)
        );
        assert_eq!(
            parse_typed_cell("-Inf", &schema).unwrap(),
            LiteralValue::Float64(FiniteOrSpecial::NegativeInfinity)
        );
    }
}
