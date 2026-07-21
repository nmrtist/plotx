//! Declarative, serialisable curve-fit models and their safe execution engine.
//!
//! Model expressions are parsed into a restricted AST. They cannot access the
//! filesystem, network, clock, random numbers, or execute user code.

mod definition;
mod dsl;
mod fitting;
mod prediction;
mod program;

pub use definition::*;
pub use dsl::{Expression, SourceError, SourcePosition, discover_symbols, parse_expression};
pub use fitting::*;
pub use prediction::evaluate_fit_result_on_grid;
pub use program::{CompiledModel, EvaluationError, ModelValidationError};
