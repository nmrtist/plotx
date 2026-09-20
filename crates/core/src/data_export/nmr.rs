use super::*;

pub(super) fn snapshot_series(
    nmr: &crate::state::Nmr2DDataset,
) -> Result<SnapshotData, DataExportError> {
    match &nmr.processed {
        Processed2D::Ft(spectrum) => Ok(SnapshotData::True2D(Arc::clone(spectrum))),
        Processed2D::Stack(spectrum) => {
            let axis = nmr.data.pseudo_axis.as_ref();
            if nmr.stack_field_key() == "nmr.observations" {
                let nus = nmr
                    .data
                    .nus
                    .as_ref()
                    .ok_or(DataExportError::ContentUnavailable)?;
                return Ok(SnapshotData::Pseudo2D {
                    spectrum: Arc::clone(spectrum),
                    ruler_name: "Acquired grid index (zero based)".into(),
                    ruler_unit: String::new(),
                    ruler: nus.schedule.iter().map(|index| *index as f64).collect(),
                });
            }

            Ok(SnapshotData::Pseudo2D {
                spectrum: Arc::clone(spectrum),
                ruler_name: axis
                    .map(|axis| axis.name.clone())
                    .filter(|name| !name.is_empty())
                    .unwrap_or_else(|| "Ruler".into()),
                ruler_unit: axis.map(|axis| axis.unit.clone()).unwrap_or_default(),
                ruler: axis
                    .map(|axis| axis.values.clone())
                    .unwrap_or_else(|| (0..spectrum.increments()).map(|i| i as f64).collect()),
            })
        }
    }
}
