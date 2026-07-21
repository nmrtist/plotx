//! DataFusion adapter kept outside PlotX's lightweight data-model crate.
//!
//! This crate owns engine-specific lowering and streaming storage output. Its
//! public surface consists only of the backend adapter; PlotX IR and storage
//! types remain owned by `plotx-data`.

// Internal convenience only: the adapter reaches PlotX IR and storage helpers
// through `crate::`. Consumers depend on plotx-data directly rather than having
// its whole API re-exported from here.
pub(crate) use plotx_data::*;

pub mod backend;

pub use backend::*;
