//! PlotX's typed, immutable tabular data foundation.
//!
//! Public types in this crate deliberately do not expose Arrow or an execution
//! engine. Backends are codecs and execution adapters, so persisted PlotX v1
//! semantics remain stable when those dependencies change.

mod array;
mod column_lineage;
mod error;
#[doc(hidden)]
pub mod execute;
#[doc(hidden)]
pub mod execute_expr;
#[doc(hidden)]
pub mod execute_relations;
mod execute_reshape;
mod execution_input;
#[doc(hidden)]
pub mod id;
mod materialized_snapshot;
mod patch_rebase;
mod plan;
mod revision;
mod row_provenance;
mod schema;
mod snapshot;
mod source;
#[doc(hidden)]
pub mod storage;
mod typecheck;
mod unit_registry;

pub use array::*;
pub use column_lineage::*;
pub use error::*;
pub use execute::*;
pub use execution_input::*;
pub use id::*;
pub use materialized_snapshot::*;
pub use patch_rebase::*;
pub use plan::*;
pub use revision::*;
pub use row_provenance::*;
pub use schema::*;
pub use snapshot::*;
pub use source::*;
pub use storage::*;
pub use typecheck::*;
pub use unit_registry::*;

/// Schema version for every PlotX data envelope and relation plan in this
/// crate. Product redesigns do not change the project schema version.
pub const DATA_SCHEMA_VERSION: u32 = 1;
