//! A single snapshot frame for embedding in a bounded project entry.
//! The project writer owns transaction publication and compression.

use nmr::{
    Dataset, ExecutionContext,
    snapshot::{AcceptRecordedHistory, SnapshotError, SnapshotLimits},
};
use std::{
    io::{Read, Write},
    sync::Arc,
};

pub fn write(
    input: &Dataset,
    writer: &mut impl Write,
    limits: SnapshotLimits,
    context: &mut ExecutionContext<'_>,
) -> Result<(), SnapshotError> {
    nmr::snapshot::write_snapshot_with_context(input, writer, limits, context)
}

/// Accept recorded history only after integrity/model checks. Restore never
/// consults source paths. The caller must supply one bounded archive entry.
pub fn read(
    reader: &mut impl Read,
    limits: SnapshotLimits,
    context: &mut ExecutionContext<'_>,
) -> Result<Arc<Dataset>, SnapshotError> {
    let checked = nmr::snapshot::read_snapshot_with_context(reader, limits, context)?;
    context.check_cancelled()?;
    let mut trailing = [0u8; 1];
    if reader.read(&mut trailing)? != 0 {
        return Err(SnapshotError::Structure);
    }
    Ok(Arc::new(checked.restore(AcceptRecordedHistory)))
}
