use crate::{ColumnId, LogicalType, RowId};

#[derive(Debug, thiserror::Error)]
pub enum DataError {
    #[error("invalid schema: {0}")]
    InvalidSchema(String),
    #[error("invalid array: {0}")]
    InvalidArray(String),
    #[error("column {0} does not exist")]
    MissingColumn(ColumnId),
    #[error("row {0} does not exist")]
    MissingRow(RowId),
    #[error("expected {expected:?}, found {actual:?}")]
    TypeMismatch {
        expected: LogicalType,
        actual: LogicalType,
    },
    #[error("incompatible units: {left} and {right}")]
    IncompatibleUnits { left: String, right: String },
    #[error("unknown or unsupported codec {0:?}")]
    UnknownCodec(String),
    #[error("data block {0} is missing")]
    MissingBlock(String),
    #[error("data block is corrupt: {0}")]
    CorruptBlock(String),
    #[error("invalid relation plan: {0}")]
    InvalidPlan(String),
    #[error("operation was cancelled")]
    Cancelled,
    #[error("memory budget exceeded: requested {requested} bytes with {limit} byte limit")]
    MemoryBudget { requested: u64, limit: u64 },
    #[error("patch conflict: {0}")]
    PatchConflict(String),
    #[error("unsupported operation: {0}")]
    Unsupported(String),
    #[error("backend failure: {0}")]
    Backend(String),
}

pub type Result<T> = std::result::Result<T, DataError>;
