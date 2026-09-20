//! Checked NMR import boundary. The library dataset owns all scientific facts;
//! the values below are presentation summaries, never processing inputs.

use crate::{
    AcquisitionIdentity, DataFormat, IoError, LoadWarning, LoadWarningCode, NmrFormat, Provenance,
};
use nmr::dataset::DescriptorRef;
use nmr::provenance::SourceKind;
use nmr::{Dataset, ExecutionContext, Format, ReadOptions, ReadPreference, ReadWarning};
use std::{path::Path, sync::Arc};

#[path = "nmr_bridge_snapshot.rs"]
pub mod snapshot;

/// PlotX's agreed import policy. Exact file selections are resolved by nmr;
/// ordinary experiment directories prefer raw and same-kind ambiguity is an error.
pub fn read_options() -> ReadOptions {
    ReadOptions::new()
        .preference(ReadPreference::PreferRaw)
        .allow_experimental_vendor_semantics(true)
}

pub fn read(path: &Path, context: &mut ExecutionContext<'_>) -> Result<Arc<Dataset>, IoError> {
    read_options()
        .read_with_context(path, context)
        .map(Arc::new)
        .map_err(|error| IoError::Nmr(Box::new(error)))
}

pub fn load(path: &Path) -> Result<crate::LoadResult, IoError> {
    let dataset = read(path, &mut ExecutionContext::default())?;
    loaded(dataset)
}

pub(super) fn loaded(dataset: Arc<Dataset>) -> Result<crate::LoadResult, IoError> {
    Ok(crate::LoadResult {
        acquisition: crate::Acquisition::Nmr(crate::nmr_view::NmrSource::new(dataset.clone())?),
        acquisition_identity: identity(&dataset),
        format: format(&dataset)?,
        provenance: provenance(&dataset)?,
        warnings: warnings(&dataset),
    })
}

pub fn format(dataset: &Dataset) -> Result<DataFormat, IoError> {
    use nmr::{processed::Format as Processed, raw::RawFormat as Raw};
    let format = match dataset.source_format() {
        Some(Format::Raw(Raw::BrukerRaw)) => NmrFormat::BrukerRaw,
        Some(Format::Raw(Raw::VarianRaw)) => NmrFormat::VarianAgilentRaw,
        Some(Format::Raw(Raw::JeolDelta) | Format::Processed(Processed::JeolDelta)) => {
            NmrFormat::JeolDelta
        }
        Some(Format::Processed(Processed::JcampDx)) => NmrFormat::JcampDx1D,
        Some(Format::Processed(Processed::BrukerTopSpin)) => match shape(dataset)?.len() {
            1 => NmrFormat::BrukerProcessed1D,
            2 => NmrFormat::BrukerProcessed2D,
            _ => {
                return Err(IoError::NmrConversion(
                    "unsupported Bruker spectrum rank".into(),
                ));
            }
        },
        _ => {
            return Err(IoError::NmrConversion(
                "dataset has no supported import format".into(),
            ));
        }
    };
    Ok(DataFormat::Nmr(format))
}

pub fn shape(dataset: &Dataset) -> Result<Vec<usize>, IoError> {
    match dataset.descriptor() {
        DescriptorRef::Raw(descriptor) => Ok(descriptor.logical_shape()),
        DescriptorRef::Processed(descriptor) => Ok(descriptor.logical_shape()),
        _ => Err(IoError::NmrConversion("unsupported NMR descriptor".into())),
    }
}

pub fn identity(dataset: &Dataset) -> AcquisitionIdentity {
    let identity = dataset.identity();
    AcquisitionIdentity {
        subject: identity.subject().map(str::to_owned),
        acquisition: identity.acquisition().map(str::to_owned),
        source_label: identity
            .source_label()
            .map(str::to_owned)
            .unwrap_or_else(|| {
                AcquisitionIdentity::from_path(dataset.selected_path().unwrap_or(Path::new("")))
                    .source_label
            }),
    }
}

pub fn provenance(dataset: &Dataset) -> Result<Provenance, IoError> {
    let selected_path = dataset
        .selected_path()
        .ok_or_else(|| IoError::NmrConversion("dataset has no original read selection".into()))?;
    let mut data = dataset
        .sources()
        .iter()
        .filter(|source| source.kind() == SourceKind::Data);
    let primary = data.next().ok_or_else(|| {
        IoError::NmrConversion("imported dataset has no data source record".into())
    })?;
    Ok(Provenance {
        selected_path: selected_path.to_owned(),
        data_path: primary.path().to_owned(),
        parameter_paths: dataset
            .sources()
            .iter()
            .filter(|source| source.kind() == SourceKind::Parameters)
            .map(|source| source.path().to_owned())
            .collect(),
        companion_paths: data
            .map(|source| source.path().to_owned())
            .chain(
                dataset
                    .sources()
                    .iter()
                    .filter(|source| {
                        !matches!(source.kind(), SourceKind::Data | SourceKind::Parameters)
                    })
                    .map(|source| source.path().to_owned()),
            )
            .collect(),
    })
}

pub fn warnings(dataset: &Dataset) -> Vec<LoadWarning> {
    dataset.warnings().iter().filter_map(|warning| {
        let (code, message, path) = match warning {
            // The opt-in policy is documented; retain this evidence on the NMR
            // dataset without turning every successful import into an alert.
            ReadWarning::ExperimentalVendorSemantics { .. } => return None,
            ReadWarning::MissingOptionalSource { role, path, impact, .. } => (
                LoadWarningCode::MissingCompanion,
                format!("Optional {role} is missing (affects {impact:?})."),
                Some(path.clone()),
            ),
            ReadWarning::MissingMetadata { field, axis, impact, .. } => (
                if *impact == nmr::WarningImpact::AxisCalibration { LoadWarningCode::MissingCalibration } else { LoadWarningCode::InvalidMetadata },
                format!("NMR metadata {field:?} is missing on axis {axis:?} (affects {impact:?}); no value was inferred."),
                dataset.selected_path().map(Path::to_owned),
            ),
            _ => (LoadWarningCode::InvalidMetadata, format!("NMR import: {warning:?}"), dataset.selected_path().map(Path::to_owned)),
        };
        Some(LoadWarning { code, message, path })
    }).collect()
}

/// Recognized NMR selections, including malformed/ambiguous acquisitions that
/// must reach the reader's diagnostic rather than be descended into as folders.
pub fn is_candidate(path: &Path) -> bool {
    match read_options().detect(path) {
        Ok(_) => true,
        Err(error) => {
            error.format().is_some() || matches!(error.kind(), nmr::ReadErrorKind::Ambiguous)
        }
    }
}

pub(crate) fn detected_format(format_id: Format, path: &Path) -> Result<DataFormat, IoError> {
    use nmr::{processed::Format as Processed, raw::RawFormat as Raw};
    Ok(DataFormat::Nmr(match format_id {
        Format::Raw(Raw::BrukerRaw) => NmrFormat::BrukerRaw,
        Format::Raw(Raw::VarianRaw) => NmrFormat::VarianAgilentRaw,
        Format::Raw(Raw::JeolDelta) | Format::Processed(Processed::JeolDelta) => {
            NmrFormat::JeolDelta
        }
        Format::Processed(Processed::JcampDx) => NmrFormat::JcampDx1D,
        // The host's format catalog distinguishes spectrum ranks. Obtain that
        // fact from the library descriptor; do not inspect vendor parameters.
        Format::Processed(Processed::BrukerTopSpin) => {
            return self::format(read(path, &mut ExecutionContext::default())?.as_ref());
        }
        _ => return Err(IoError::Unsupported(format!("NMR format {format_id:?}"))),
    }))
}
