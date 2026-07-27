//! The language-neutral half of the property catalog.
//!
//! A [`PropertyDefinition`] describes *what a property is* — its identity,
//! schema, ownership scope, applicability and default policy. It deliberately
//! carries no target, no current value and no stored "is default" flag: the
//! current value and the default are derived from the document on demand, so
//! there is never a second copy of a value that already lives in a typed domain
//! model. There is, for the same reason, no generic value store here; writes are
//! compiled into typed commits for their owning persistence boundary.

mod identity;
mod resolution;
mod schema;

pub use identity::*;
pub use resolution::*;
pub use schema::*;
