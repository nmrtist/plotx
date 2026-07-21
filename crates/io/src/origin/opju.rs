use super::{OriginError, OriginProbe, OriginProject};

/// OPJU is detection-only until a complete, bounded container profile has
/// public evidence. In particular, this path must not scan markers or attempt
/// to decode records after the validated first line.
pub(super) fn read(_probe: OriginProbe) -> Result<OriginProject, OriginError> {
    Err(OriginError::UnsupportedOpjuVariant {
        message: "This OPJU file uses a record layout that PlotX does not support yet. No data was imported."
            .to_owned(),
    })
}
