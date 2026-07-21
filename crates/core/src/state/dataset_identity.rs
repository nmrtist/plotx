use super::Dataset;

impl Dataset {
    pub fn resource_id(&self) -> &str {
        match self {
            Dataset::Nmr(dataset) => &dataset.resource_id,
            Dataset::Nmr2D(dataset) => &dataset.resource_id,
            Dataset::Table(dataset) => &dataset.resource_id,
            Dataset::Electrophysiology(dataset) => &dataset.resource_id,
        }
    }

    pub(crate) fn set_resource_id(&mut self, id: String) {
        match self {
            Dataset::Nmr(dataset) => dataset.resource_id = id,
            Dataset::Nmr2D(dataset) => dataset.resource_id = id,
            Dataset::Table(dataset) => dataset.resource_id = id,
            Dataset::Electrophysiology(dataset) => dataset.resource_id = id,
        }
    }
}
