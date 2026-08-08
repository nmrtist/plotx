use super::{Dataset, FieldId};

impl Dataset {
    pub fn trace_collection(&self, field: FieldId) -> Option<&plotx_data::TraceCollectionCatalog> {
        match self {
            Self::Nmr(data) => data.field_catalog.trace_collection(field),
            Self::Table(data) => data.field_catalog.trace_collection(field),
            Self::Nmr2D(data) => data.field_catalog.trace_collection(field),
            Self::Electrophysiology(data) => data.field_catalog.trace_collection(field),
            Self::Afm(data) => data.field_catalog.trace_collection(field),
            Self::MassSpec(data) => data.field_catalog.trace_collection(field),
            Self::Xrd(data) => data.field_catalog.trace_collection(field),
            Self::Xps(data) => data.field_catalog.trace_collection(field),
        }
    }

    /// The trace collection currently represented by this dataset's live
    /// display. Providers resolve their own display ownership; generic trace
    /// workflows only consume the resulting field identity.
    pub fn active_trace_collection_field(&self) -> Option<FieldId> {
        if let Self::Electrophysiology(recording) = self {
            return recording
                .field_key(recording.selected_channel)
                .and_then(|key| recording.field_catalog.id_for_key(key));
        }
        self.default_field_id()
            .filter(|field| self.trace_collection(*field).is_some())
            .or_else(|| {
                self.field_descriptors()
                    .into_iter()
                    .map(|field| field.id)
                    .find(|field| self.trace_collection(*field).is_some())
            })
    }

    pub(super) fn validate_trace_collections(&self) -> Result<(), String> {
        let catalog = self.field_catalog();
        match self {
            Self::Nmr2D(dataset) => {
                let field = catalog
                    .id_for_key("nmr.stack")
                    .ok_or_else(|| "NMR stack field is missing".to_owned())?;
                let collection = catalog.trace_collection(field).ok_or_else(|| {
                    "NMR stack field is missing its trace collection catalog".to_owned()
                })?;
                if collection.items.len() != dataset.data.rows {
                    return Err(
                        "NMR trace collection item count does not match the acquisition".to_owned(),
                    );
                }
            }
            Self::Electrophysiology(dataset) => {
                for field in self
                    .field_descriptors()
                    .into_iter()
                    .map(|descriptor| descriptor.id)
                {
                    let collection = catalog.trace_collection(field).ok_or_else(|| format!("electrophysiology field {field} is missing its trace collection catalog"))?;
                    if collection.items.len() != dataset.data.sweeps.len() {
                        return Err(format!(
                            "electrophysiology field {field} trace item count does not match its sweeps"
                        ));
                    }
                }
                if let Some(selected) = &dataset.invocation.analysis_selection {
                    let collection = dataset
                        .field_key(dataset.selected_channel)
                        .and_then(|key| catalog.id_for_key(key))
                        .and_then(|field| catalog.trace_collection(field))
                        .ok_or_else(|| {
                            "electrophysiology analysis selection has no channel collection"
                                .to_owned()
                        })?;
                    let unique = selected
                        .iter()
                        .copied()
                        .collect::<std::collections::BTreeSet<_>>();
                    if unique.len() != selected.len() {
                        return Err(
                            "electrophysiology analysis selection contains duplicate item IDs"
                                .to_owned(),
                        );
                    }
                    if !selected.iter().all(|item| collection.item(*item).is_some()) {
                        return Err(
                            "electrophysiology analysis selection contains an unknown item ID"
                                .to_owned(),
                        );
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }
}
