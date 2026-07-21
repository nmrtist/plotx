//! Model-neutral automation primitives shared by the desktop, CLI and future
//! agents. The public boundary is deliberately semantic: callers can inspect,
//! select and invoke registered tools, but cannot construct document actions.

mod registry;
mod resources;
mod tasks;
mod tools;
mod types;
mod workflow;

pub use registry::ToolRegistry;
pub use resources::*;
pub use tasks::*;
pub use tools::*;
pub use types::*;
pub use workflow::*;

pub const WORKFLOW_SCHEMA: &str = "plotx.workflow.v1";
pub const RUN_MANIFEST_SCHEMA: &str = "plotx.run-manifest.v1";

#[cfg(test)]
mod tests;
