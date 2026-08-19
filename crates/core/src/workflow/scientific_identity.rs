use super::*;

pub fn dataset_from_loaded_acquisition(
    acquisition: Acquisition,
    scientific_identity: plotx_io::ImportedScientificIdentity,
    equal_scale_homonuclear_2d_imports: bool,
) -> (Dataset, String) {
    let (mut dataset, source) = dataset_from_acquisition_with_equal_scale_preference(
        acquisition,
        equal_scale_homonuclear_2d_imports,
    );
    dataset.set_scientific_identity(scientific_identity);
    (dataset, source)
}
