//! The NMR snapshot is the sole persisted scientific payload.

use super::*;
use plotx_io::nmr_view::NmrSource;

pub(super) const STORAGE: &str = "nmr_snapshot_v1";

/// Evidence of the last completed output, which may precede a queued recipe
/// edit. It is never used as a sample cache or replayed in place of the recipe.
pub(super) fn execution_evidence(
    source: &NmrSource,
    phases: &[plotx_processing::nmr_bridge::PhaseReport],
) -> Result<serde_json::Value> {
    let Some(processed) = source.dataset().as_processed() else {
        return Ok(serde_json::Value::Null);
    };
    let mut report = Vec::new();
    nmr::execution_report::write_json(
        processed,
        &[],
        &mut report,
        ProjectLoadLimits::default().max_metadata_bytes as usize,
    )
    .map_err(|error| ProjectError::Invalid(format!("NMR execution evidence: {error}")))?;
    let report: serde_json::Value = serde_json::from_slice(&report)?;
    let digest = |value: nmr::provenance::CanonicalDatasetDigests| {
        value
            .dataset()
            .as_bytes()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    };
    let phases: Vec<_> = phases.iter().map(|phase| serde_json::json!({
        "step_id": phase.step.get(), "axis": phase.axis, "points": phase.points,
        "display_pivot": phase.display_pivot, "method": format!("{:?}", phase.method),
        "algorithm": phase.method.algorithm_version(), "input": digest(phase.input),
        "correction": { "p0_degrees": phase.correction.p0_degrees(), "p1_degrees": phase.correction.p1_degrees(),
            "pivot_fraction": phase.correction.pivot_fraction(), "convention": "exp(+i phase), i/N" },
        "objective": phase.objective, "evaluations": phase.evaluations,
        "representative": phase.representative.as_ref().map(|trace| serde_json::json!({
            "policy": "strongest-cartesian-component.v1", "removed_axis": trace.removed_axis,
            "logical_index": trace.index, "component": trace.component, "input": digest(trace.input)
        }))
    })).collect();
    Ok(serde_json::json!({ "library": report, "automatic_phase": phases }))
}

fn limits() -> nmr::snapshot::SnapshotLimits {
    let project = ProjectLoadLimits::default();
    nmr::snapshot::SnapshotLimits {
        max_bytes: project.max_entry_bytes,
        max_sample_bytes: project.max_materialized_bytes as usize,
        max_metadata_bytes: project.max_metadata_bytes as usize,
        max_working_bytes: (project.max_materialized_bytes + project.max_metadata_bytes) as usize,
        ..Default::default()
    }
}

pub(super) fn write(writer: &mut impl Write, source: &nmr::Dataset) -> Result<()> {
    plotx_io::nmr_bridge::snapshot::write(
        source,
        writer,
        limits(),
        &mut nmr::ExecutionContext::default(),
    )
    .map_err(|error| ProjectError::Invalid(format!("NMR snapshot: {error}")))
}

pub(super) fn read(zip: &mut ZipArchive<File>, data: &DataObject) -> Result<NmrSource> {
    if data.payload.storage != STORAGE {
        return Err(ProjectError::Unsupported(format!(
            "NMR payload storage {}",
            data.payload.storage
        )));
    }
    if !data.dimensions.is_empty() || data.payload.domain != "nmr" {
        return Err(ProjectError::Invalid(
            "NMR dimensions and calibration belong to the snapshot".into(),
        ));
    }
    let source = read_entry(
        zip,
        &data.payload.blob,
        "NMR snapshot",
        limits().max_bytes,
        |reader| {
            plotx_io::nmr_bridge::snapshot::read(
                reader,
                limits(),
                &mut nmr::ExecutionContext::default(),
            )
            .map_err(|error| ProjectError::Invalid(format!("NMR snapshot: {error}")))
        },
    )?;
    let shape = plotx_io::nmr_bridge::shape(&source)
        .map_err(|error| ProjectError::Invalid(error.to_string()))?;
    if shape != data.payload.shape {
        return Err(ProjectError::Invalid(
            "NMR snapshot shape differs from the object index".into(),
        ));
    }
    let identity = read_acquisition_identity(data)?;
    NmrSource::new(source)
        .map(|source| source.with_display_label(identity.source_label))
        .map_err(|error| ProjectError::Invalid(error.to_string()))
}

pub(super) fn read_1d(
    zip: &mut ZipArchive<File>,
    data: &DataObject,
    recipe: &RecipeObject,
) -> Result<Dataset> {
    let source = read(zip, data)?;
    let mut dataset = NmrDataset::load_with_pipeline(
        source,
        Some(AxisPipeline { steps: Vec::new() }),
        Some(false),
    )
    .map_err(ProjectError::Invalid)?;
    dataset.acquisition_identity = read_acquisition_identity(data)?;
    dataset.field_catalog = super::field_catalog::read(data)?;
    apply_1d_recipe(&mut dataset, recipe)?;
    dataset.name = data.label.clone();
    dataset.retransform().map_err(ProjectError::Invalid)?;
    let dataset = Dataset::Nmr(Box::new(dataset));
    dataset
        .validate_field_catalog()
        .map_err(ProjectError::Invalid)?;
    Ok(dataset)
}
