use super::*;
use nmr::{axis::AxisDomain, dataset::DescriptorRef};

/// Inspect source data without constructing a processing recipe or running FFT.
/// The NMR branch uses the checked library model, including its logical NUS shape.
pub fn inspect_file(path: &Path) -> Result<InspectionReport, WorkflowError> {
    match plotx_io::nmr_bridge::read_options().detect(path) {
        Ok(_) => {}
        Err(error) if error.kind() == nmr::ReadErrorKind::Unrecognized => {
            // A recognized but unsupported NMR input must never fall back to an
            // older vendor reader. Detection of other scientific families stays here.
            if matches!(plotx_io::detect_format(path)?, DataFormat::Nmr(_)) {
                return Err(plotx_io::IoError::Nmr(Box::new(error)).into());
            }
            let loaded = plotx_io::load_path(path)?;
            return Ok(inspection_report(
                loaded.format,
                &loaded.provenance,
                &loaded.warnings,
                &loaded.acquisition,
            ));
        }
        Err(error) => return Err(plotx_io::IoError::Nmr(Box::new(error)).into()),
    }
    let dataset = plotx_io::nmr_bridge::read(path, &mut nmr::ExecutionContext::default())?;
    inspect_nmr_dataset(&dataset)
}

pub fn inspect_nmr_dataset(dataset: &nmr::Dataset) -> Result<InspectionReport, WorkflowError> {
    let domains: Vec<_> = match dataset.descriptor() {
        DescriptorRef::Raw(descriptor) => {
            descriptor.axes().iter().map(|axis| axis.domain()).collect()
        }
        DescriptorRef::Processed(descriptor) => {
            descriptor.axes().iter().map(|axis| axis.domain()).collect()
        }
        _ => {
            return Err(
                plotx_io::IoError::NmrConversion("unsupported NMR descriptor".into()).into(),
            );
        }
    };
    let domain = if domains.iter().all(|domain| *domain == AxisDomain::Time) {
        "time"
    } else if domains
        .iter()
        .all(|domain| *domain == AxisDomain::Frequency)
    {
        "frequency"
    } else {
        "mixed"
    };
    let provenance = plotx_io::nmr_bridge::provenance(dataset)?;
    let shape = plotx_io::nmr_bridge::shape(dataset)?;
    Ok(InspectionReport {
        schema: INSPECTION_SCHEMA,
        format: plotx_io::nmr_bridge::format(dataset)?.as_str().to_owned(),
        provenance: ProvenanceReport {
            selected_path: provenance.selected_path,
            data_path: provenance.data_path,
            parameter_paths: provenance.parameter_paths,
            companion_paths: provenance.companion_paths,
        },
        dimension: DimensionReport {
            count: shape.len(),
            shape,
        },
        domain: domain.to_owned(),
        warnings: plotx_io::nmr_bridge::warnings(dataset)
            .iter()
            .map(warning_report)
            .collect(),
        electrophysiology: None,
        afm: None,
        mass_spectrometry: None,
        xrd: None,
        xps: None,
    })
}
