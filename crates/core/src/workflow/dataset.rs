use super::*;
use crate::state::{Nmr2DDataset, NmrDataset};

pub fn dataset_from_loaded_acquisition(
    acquisition: Acquisition,
    acquisition_identity: plotx_io::AcquisitionIdentity,
    equal_scale_homonuclear_2d_imports: bool,
) -> Result<(Dataset, String), WorkflowError> {
    let (mut dataset, source) =
        convert_acquisition(acquisition, equal_scale_homonuclear_2d_imports)?;
    dataset.set_acquisition_identity(acquisition_identity);
    Ok((dataset, source))
}

pub fn dataset_from_acquisition(
    acquisition: Acquisition,
) -> Result<(Dataset, String), WorkflowError> {
    dataset_from_acquisition_with_equal_scale_preference(acquisition, true)
}

pub fn dataset_from_acquisition_with_equal_scale_preference(
    acquisition: Acquisition,
    equal_scale_homonuclear_2d_imports: bool,
) -> Result<(Dataset, String), WorkflowError> {
    convert_acquisition(acquisition, equal_scale_homonuclear_2d_imports)
}

fn convert_acquisition(
    acquisition: Acquisition,
    equal_scale_homonuclear_2d_imports: bool,
) -> Result<(Dataset, String), WorkflowError> {
    Ok(match acquisition {
        Acquisition::Nmr(data) => {
            let source = data.source().to_owned();
            let dataset = match data.axes().len() {
                1 => Dataset::Nmr(Box::new(
                    NmrDataset::load(data).map_err(WorkflowError::Nmr)?,
                )),
                2 => Dataset::Nmr2D(Box::new(
                    Nmr2DDataset::load_with_equal_scale_preference(
                        data,
                        equal_scale_homonuclear_2d_imports,
                    )
                    .map_err(WorkflowError::Nmr)?,
                )),
                rank => {
                    return Err(WorkflowError::Nmr(format!(
                        "PlotX does not yet display rank-{rank} NMR data"
                    )));
                }
            };
            (dataset, source)
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
    })
}

pub fn dataset_title(dataset: &Dataset) -> String {
    match dataset {
        Dataset::Nmr(nmr) => nmr
            .name
            .clone()
            .unwrap_or_else(|| short_name(nmr.data.source())),
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
