use crate::{DataError, LogicalType, Result};
use serde::{Deserialize, Serialize};

/// Independent null bitmap. A false bit is null; floating-point NaN and
/// infinities remain ordinary, valid values.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Validity {
    len: usize,
    bytes: Vec<u8>,
}

impl Validity {
    pub fn all_valid(len: usize) -> Self {
        let mut bytes = vec![u8::MAX; len.div_ceil(8)];
        clear_unused_bits(&mut bytes, len);
        Self { len, bytes }
    }

    pub fn all_null(len: usize) -> Self {
        Self {
            len,
            bytes: vec![0; len.div_ceil(8)],
        }
    }

    pub fn from_valid(valid: impl IntoIterator<Item = bool>) -> Self {
        let values: Vec<bool> = valid.into_iter().collect();
        let mut bitmap = Self::all_null(values.len());
        for (index, value) in values.into_iter().enumerate() {
            bitmap.set(index, value);
        }
        bitmap
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn is_valid(&self, index: usize) -> Option<bool> {
        (index < self.len).then(|| self.bytes[index / 8] & (1 << (index % 8)) != 0)
    }

    pub fn null_count(&self) -> usize {
        self.len
            - self
                .bytes
                .iter()
                .map(|byte| byte.count_ones() as usize)
                .sum::<usize>()
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub(crate) fn set(&mut self, index: usize, valid: bool) {
        if valid {
            self.bytes[index / 8] |= 1 << (index % 8);
        } else {
            self.bytes[index / 8] &= !(1 << (index % 8));
        }
    }
}

fn clear_unused_bits(bytes: &mut [u8], len: usize) {
    if let Some(last) = bytes.last_mut()
        && !len.is_multiple_of(8)
    {
        *last &= (1 << (len % 8)) - 1;
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum ColumnValues {
    Null(usize),
    Boolean(Vec<bool>),
    Int64(Vec<i64>),
    Float64(Vec<f64>),
    Utf8(Vec<String>),
    Categorical(Vec<u32>),
    Date(Vec<i32>),
    Time(Vec<i64>),
    Timestamp(Vec<i64>),
    Duration(Vec<i64>),
    Extension {
        type_id: String,
        storage: Box<ColumnValues>,
    },
}

impl ColumnValues {
    pub fn len(&self) -> usize {
        match self {
            Self::Null(len) => *len,
            Self::Boolean(values) => values.len(),
            Self::Int64(values)
            | Self::Time(values)
            | Self::Timestamp(values)
            | Self::Duration(values) => values.len(),
            Self::Float64(values) => values.len(),
            Self::Utf8(values) => values.len(),
            Self::Categorical(values) => values.len(),
            Self::Date(values) => values.len(),
            Self::Extension { storage, .. } => storage.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn logical_type(&self) -> LogicalType {
        match self {
            Self::Null(_) => LogicalType::Null,
            Self::Boolean(_) => LogicalType::Boolean,
            Self::Int64(_) => LogicalType::Int64,
            Self::Float64(_) => LogicalType::Float64,
            Self::Utf8(_) => LogicalType::Utf8,
            Self::Categorical(_) => LogicalType::Categorical { levels: Vec::new() },
            Self::Date(_) => LogicalType::Date,
            Self::Time(_) => LogicalType::Time,
            Self::Timestamp(_) => LogicalType::Timestamp {
                display_timezone: "UTC".into(),
            },
            Self::Duration(_) => LogicalType::Duration,
            Self::Extension { type_id, storage } => LogicalType::Extension(crate::ExtensionType {
                id: type_id.clone(),
                version: 1,
                storage: Box::new(storage.logical_type()),
                semantics_critical: true,
            }),
        }
    }

    pub fn scalar(&self, index: usize) -> Option<ScalarValue> {
        match self {
            Self::Null(len) => (index < *len).then_some(ScalarValue::Null),
            Self::Boolean(values) => values.get(index).copied().map(ScalarValue::Boolean),
            Self::Int64(values) => values.get(index).copied().map(ScalarValue::Int64),
            Self::Float64(values) => values.get(index).copied().map(ScalarValue::Float64),
            Self::Utf8(values) => values.get(index).cloned().map(ScalarValue::Utf8),
            Self::Categorical(values) => values.get(index).copied().map(ScalarValue::Categorical),
            Self::Date(values) => values.get(index).copied().map(ScalarValue::Date),
            Self::Time(values) => values.get(index).copied().map(ScalarValue::Time),
            Self::Timestamp(values) => values.get(index).copied().map(ScalarValue::Timestamp),
            Self::Duration(values) => values.get(index).copied().map(ScalarValue::Duration),
            Self::Extension { type_id, storage } => {
                storage.scalar(index).map(|value| ScalarValue::Extension {
                    type_id: type_id.clone(),
                    storage: Box::new(value),
                })
            }
        }
    }

    fn set_scalar(&mut self, index: usize, value: ScalarValue) -> Result<()> {
        macro_rules! set_value {
            ($values:expr, $value:expr) => {{
                let target = $values.get_mut(index).ok_or_else(|| {
                    DataError::InvalidArray(format!("array index {index} is out of bounds"))
                })?;
                *target = $value;
                Ok(())
            }};
        }
        match (self, value) {
            (Self::Boolean(values), ScalarValue::Boolean(value)) => set_value!(values, value),
            (Self::Int64(values), ScalarValue::Int64(value)) => set_value!(values, value),
            (Self::Float64(values), ScalarValue::Float64(value)) => set_value!(values, value),
            (Self::Utf8(values), ScalarValue::Utf8(value)) => set_value!(values, value),
            (Self::Categorical(values), ScalarValue::Categorical(value)) => {
                set_value!(values, value)
            }
            (Self::Date(values), ScalarValue::Date(value)) => set_value!(values, value),
            (Self::Time(values), ScalarValue::Time(value))
            | (Self::Timestamp(values), ScalarValue::Timestamp(value))
            | (Self::Duration(values), ScalarValue::Duration(value)) => set_value!(values, value),
            (
                Self::Extension { type_id, storage },
                ScalarValue::Extension {
                    type_id: value_type,
                    storage: value,
                },
            ) if *type_id == value_type => storage.set_scalar(index, *value),
            (values, value) => Err(DataError::TypeMismatch {
                expected: values.logical_type(),
                actual: value.logical_type(),
            }),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum ScalarValue {
    Null,
    Boolean(bool),
    Int64(i64),
    Float64(f64),
    Utf8(String),
    Categorical(u32),
    Date(i32),
    Time(i64),
    Timestamp(i64),
    Duration(i64),
    Extension {
        type_id: String,
        storage: Box<ScalarValue>,
    },
}

impl ScalarValue {
    pub fn logical_type(&self) -> LogicalType {
        match self {
            Self::Null => LogicalType::Null,
            Self::Boolean(_) => LogicalType::Boolean,
            Self::Int64(_) => LogicalType::Int64,
            Self::Float64(_) => LogicalType::Float64,
            Self::Utf8(_) => LogicalType::Utf8,
            Self::Categorical(_) => LogicalType::Categorical { levels: Vec::new() },
            Self::Date(_) => LogicalType::Date,
            Self::Time(_) => LogicalType::Time,
            Self::Timestamp(_) => LogicalType::Timestamp {
                display_timezone: "UTC".into(),
            },
            Self::Duration(_) => LogicalType::Duration,
            Self::Extension { type_id, storage } => LogicalType::Extension(crate::ExtensionType {
                id: type_id.clone(),
                version: 1,
                storage: Box::new(storage.logical_type()),
                semantics_critical: true,
            }),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ColumnChunk {
    values: ColumnValues,
    validity: Validity,
}

impl ColumnChunk {
    pub fn new(values: ColumnValues, validity: Validity) -> Result<Self> {
        if values.len() != validity.len() {
            return Err(DataError::InvalidArray(format!(
                "{} values but {} validity bits",
                values.len(),
                validity.len()
            )));
        }
        if matches!(values, ColumnValues::Null(_)) && validity.null_count() != validity.len() {
            return Err(DataError::InvalidArray(
                "Null arrays cannot contain valid values".into(),
            ));
        }
        Ok(Self { values, validity })
    }

    pub fn all_valid(values: ColumnValues) -> Self {
        let validity = Validity::all_valid(values.len());
        Self { values, validity }
    }

    /// Build a Float64 array whose nulls are represented only by validity
    /// bits. Present NaN and infinities remain valid IEEE values.
    pub fn optional_f64(values: impl IntoIterator<Item = Option<f64>>) -> Self {
        let values = values.into_iter().collect::<Vec<_>>();
        let validity = Validity::from_valid(values.iter().map(Option::is_some));
        let values = ColumnValues::Float64(
            values
                .into_iter()
                .map(|value| value.unwrap_or_default())
                .collect(),
        );
        Self { values, validity }
    }

    pub fn len(&self) -> usize {
        self.values.len()
    }

    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    pub fn values(&self) -> &ColumnValues {
        &self.values
    }

    pub fn validity(&self) -> &Validity {
        &self.validity
    }

    pub fn value(&self, index: usize) -> Option<ScalarValue> {
        match self.validity.is_valid(index)? {
            true => self.values.scalar(index),
            false => Some(ScalarValue::Null),
        }
    }

    pub fn set_value(&mut self, index: usize, value: ScalarValue) -> Result<()> {
        if index >= self.len() {
            return Err(DataError::InvalidArray(format!(
                "array index {index} is out of bounds"
            )));
        }
        if matches!(value, ScalarValue::Null) {
            self.validity.set(index, false);
            return Ok(());
        }
        self.values.set_scalar(index, value)?;
        self.validity.set(index, true);
        Ok(())
    }

    pub fn validate_type(&self, expected: &LogicalType) -> Result<()> {
        let compatible = matches!(
            (expected, &self.values),
            (LogicalType::Null, ColumnValues::Null(_))
                | (LogicalType::Boolean, ColumnValues::Boolean(_))
                | (LogicalType::Int64, ColumnValues::Int64(_))
                | (LogicalType::Float64, ColumnValues::Float64(_))
                | (LogicalType::Utf8, ColumnValues::Utf8(_))
                | (
                    LogicalType::Categorical { .. },
                    ColumnValues::Categorical(_)
                )
                | (LogicalType::Date, ColumnValues::Date(_))
                | (LogicalType::Time, ColumnValues::Time(_))
                | (LogicalType::Timestamp { .. }, ColumnValues::Timestamp(_))
                | (LogicalType::Duration, ColumnValues::Duration(_))
        ) || matches!(
            (expected, &self.values),
            (
                LogicalType::Extension(expected),
                ColumnValues::Extension { type_id, .. }
            ) if expected.id == *type_id
        );
        if !compatible {
            return Err(DataError::TypeMismatch {
                expected: expected.clone(),
                actual: self.values.logical_type(),
            });
        }
        if !matches!(expected, LogicalType::Null) && !self.validity.is_empty() {
            // Nullability belongs to the schema and is checked by snapshots.
        }
        if let (LogicalType::Categorical { levels }, ColumnValues::Categorical(values)) =
            (expected, &self.values)
            && values.iter().any(|value| *value as usize >= levels.len())
        {
            return Err(DataError::InvalidArray(
                "categorical value is outside the registered levels".into(),
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn null_is_independent_from_non_finite_float_values() {
        let chunk = ColumnChunk::new(
            ColumnValues::Float64(vec![f64::NAN, f64::INFINITY, 3.0]),
            Validity::from_valid([true, true, false]),
        )
        .unwrap();
        assert!(matches!(chunk.value(0), Some(ScalarValue::Float64(v)) if v.is_nan()));
        assert_eq!(chunk.value(1), Some(ScalarValue::Float64(f64::INFINITY)));
        assert_eq!(chunk.value(2), Some(ScalarValue::Null));
    }

    #[test]
    fn validity_masks_unused_bits() {
        let valid = Validity::all_valid(9);
        assert_eq!(valid.null_count(), 0);
        assert_eq!(valid.bytes(), &[255, 1]);
    }
}
