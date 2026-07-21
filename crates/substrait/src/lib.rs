//! Opt-in Substrait exchange boundary. It is never linked by default.

pub(crate) use plotx_data::*;
pub(crate) use plotx_datafusion::compile_for_interop;

mod adapter;

pub use adapter::*;
