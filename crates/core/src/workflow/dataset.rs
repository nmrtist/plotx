use super::*;
use crate::state::{Nmr2DDataset, NmrDataset};

pub fn dataset_from_loaded_acquisition(
    acquisition: Acquisition,
    acquisition_identity: plotx_io::AcquisitionIdentity,
    nmr_origin: Option<plotx_io::NmrOrigin>,
    equal_scale_homonuclear_2d_imports: bool,
) -> (Dataset, String) {
    let (mut dataset, source) = dataset_from_acquisition_with_origin(
        acquisition,
        nmr_origin,
        equal_scale_homonuclear_2d_imports,
    );
    dataset.set_acquisition_identity(acquisition_identity);
    (dataset, source)
}

pub fn dataset_from_acquisition(acquisition: Acquisition) -> (Dataset, String) {
    dataset_from_acquisition_with_equal_scale_preference(acquisition, true)
}

pub fn dataset_from_acquisition_with_equal_scale_preference(
    acquisition: Acquisition,
    equal_scale_homonuclear_2d_imports: bool,
) -> (Dataset, String) {
    dataset_from_acquisition_with_origin(acquisition, None, equal_scale_homonuclear_2d_imports)
}

fn dataset_from_acquisition_with_origin(
    acquisition: Acquisition,
    nmr_origin: Option<plotx_io::NmrOrigin>,
    equal_scale_homonuclear_2d_imports: bool,
) -> (Dataset, String) {
    match acquisition {
        Acquisition::D1(data) => {
            let source = data.source.clone();
            (
                Dataset::Nmr(Box::new(NmrDataset::load_with_origin(
                    data,
                    nmr_origin.unwrap_or(plotx_io::NmrOrigin::Derived),
                ))),
                source,
            )
        }
        Acquisition::D2(data) => {
            let source = data.source.clone();
            (
                Dataset::Nmr2D(Box::new(
                    Nmr2DDataset::load_with_origin_and_equal_scale_preference(
                        *data,
                        nmr_origin.unwrap_or(plotx_io::NmrOrigin::Derived),
                        equal_scale_homonuclear_2d_imports,
                    ),
                )),
                source,
            )
        }
        Acquisition::Electrophysiology(data) => {
            let source = data.source.clone();
            (
                Dataset::Electrophysiology(Box::new(crate::state::ElectrophysiologyDataset::load(
                    *data,
                ))),
                source,
            )
        }
        Acquisition::Afm(data) => {
            let source = data.source.clone();
            (
                Dataset::Afm(Box::new(crate::state::AfmDataset::load(*data))),
                source,
            )
        }
        Acquisition::MassSpec(data) => {
            let source = data.source.clone();
            (
                Dataset::MassSpec(Box::new(crate::state::MassSpecDataset::load(*data))),
                source,
            )
        }
        Acquisition::Xrd(data) => {
            let source = data.source.clone();
            (
                Dataset::Xrd(Box::new(crate::state::XrdDataset::load(*data))),
                source,
            )
        }
        Acquisition::Xps(data) => {
            let source = data.source.clone();
            (
                Dataset::Xps(Box::new(crate::state::XpsDataset::load(*data))),
                source,
            )
        }
    }
}

pub fn dataset_title(dataset: &Dataset) -> String {
    match dataset {
        Dataset::Nmr(nmr) => nmr
            .name
            .clone()
            .unwrap_or_else(|| short_name(&nmr.data.source)),
        Dataset::Nmr2D(nmr) => nmr
            .name
            .clone()
            .unwrap_or_else(|| short_name(&nmr.data.source)),
        Dataset::Table(table) => table.name.clone().unwrap_or_else(|| table.summary()),
        Dataset::Electrophysiology(data) => data
            .name
            .clone()
            .unwrap_or_else(|| short_name(&data.data.source)),
        Dataset::Afm(data) => data
            .name
            .clone()
            .unwrap_or_else(|| short_name(&data.data.source)),
        Dataset::MassSpec(data) => data
            .name
            .clone()
            .unwrap_or_else(|| short_name(&data.run.source)),
        Dataset::Xrd(data) => data
            .name
            .clone()
            .unwrap_or_else(|| short_name(&data.data.source)),
        Dataset::Xps(data) => data
            .name
            .clone()
            .unwrap_or_else(|| short_name(&data.experiment.source)),
    }
}
