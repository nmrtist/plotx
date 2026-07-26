//! The narrow dispatch seam between the catalog service and one property's
//! owning domain model.
//!
//! A provider owns one target at a time: it resolves its canonical state,
//! validates an operation, and asks the transaction for the storage it owns.
//! Selection aggregation, skipped-target reporting, and atomic commits remain
//! in `service`, so providers cannot grow parallel planners.

use super::{
    EditOp, PropertyAddress, PropertyDefinition, PropertyError, PropertyReadout,
    PropertyTransaction, ResolvedProperty,
};
use crate::state::PlotxApp;

/// The implementation seam for one registered family of properties.
///
/// This is deliberately crate-private. It makes existing domain providers
/// explicit without turning the catalog into a third-party plugin system.
pub(crate) trait PropertyProvider: Sync {
    fn definitions(&self) -> &'static [PropertyDefinition];

    fn read(
        &self,
        app: &PlotxApp,
        address: &PropertyAddress,
    ) -> Result<ResolvedProperty, PropertyError>;

    fn edit(
        &self,
        app: &PlotxApp,
        transaction: &mut PropertyTransaction,
        address: &PropertyAddress,
        operation: EditOp,
    ) -> Result<(), PropertyError>;

    /// Return the value one canvas or panel label should show for this exact
    /// address. The normal case is the resolved scalar; providers with cached
    /// semantic context may override it without adding an encoding branch to
    /// the service.
    fn readout(
        &self,
        app: &PlotxApp,
        address: &PropertyAddress,
    ) -> Result<PropertyReadout, PropertyError> {
        super::readout::uniform_readout(self.read(app, address)?)
    }
}

/// One explicit registration entry. Adding a property family means adding its
/// module and exactly one item to `GROUPS`; `catalog()` derives all definitions
/// from this slice.
pub(crate) struct PropertyProviderGroup {
    pub provider: &'static dyn PropertyProvider,
}
