//! Field-versioned estimate and contour work layered onto `ComputeService`.

use super::*;
use crate::state::{
    Dataset, EstimateKind, EstimateProvenance, EstimatedScale, FieldPayload, FieldProvenance,
    FieldRef, FieldSnapshot, FieldSummary, FieldVersion, FiniteF64, LocationScaleEstimate,
    ScaleEstimate, VersionedFieldRef,
};
use plotx_analysis::robust::{
    DEPLANED_LOCATION_SCALE_ID, DEPLANED_LOCATION_SCALE_VERSION, ROBUST_DIFFERENCE_MAD_ID,
    ROBUST_DIFFERENCE_MAD_VERSION, deplaned_location_scale, robust_difference_mad,
};

/// Failure to queue field-derived work. Unlike a contour miss, this reaches the
/// application status path because a requested render cannot eventually appear.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldEnqueueError {
    WorkersUnavailable,
    VersionExhausted,
}

impl ComputeService {
    /// Import/load boundary for a field provider that has no processing
    /// pipeline (for example an RGB image). It still receives a session version
    /// from `ComputeService`; payload type decides whether it has a summary or
    /// can ever reach scalar contour work.
    pub(crate) fn register_imported_field(
        &mut self,
        field: FieldRef,
        payload: FieldPayload,
        provenance: FieldProvenance,
    ) -> Result<FieldSnapshot, FieldEnqueueError> {
        let version = self.field_version_for(field)?;
        let source = VersionedFieldRef { field, version };
        let cached = self.field_runtime.summary(source);
        let snapshot = FieldSnapshot::new(source, payload, provenance, cached);
        self.remember_field_summary(&snapshot);
        Ok(snapshot)
    }

    /// Register versions for a newly loaded immutable dataset before any plot
    /// asks for it. This is the import/load allocation point for raw fields;
    /// processing results use [`Self::reserve_field_version`] and promotion
    /// instead. Registering a scalar snapshot also makes its cheap summary
    /// available immediately, without scheduling an estimate.
    pub(crate) fn register_loaded_dataset_fields(
        &mut self,
        dataset: &Dataset,
    ) -> Result<(), FieldEnqueueError> {
        for descriptor in dataset.field_descriptors() {
            let field = FieldRef {
                resource: dataset.resource_id(),
                field: descriptor.id,
            };
            let version = self.field_version_for(field)?;
            let cached = self.cached_field_summary(VersionedFieldRef { field, version });
            if let Some(snapshot) = dataset.field_snapshot(descriptor.id, version, cached) {
                // Dataset adapters already construct their provenance; the
                // same import path is used by providers without a Dataset
                // implementation yet.
                let FieldSnapshot {
                    payload,
                    provenance,
                    ..
                } = snapshot;
                let imported = self.register_imported_field(field, payload, provenance)?;
                debug_assert_eq!(imported.source.version, version);
            }
        }
        Ok(())
    }

    pub(crate) fn field_version_for(
        &mut self,
        field: FieldRef,
    ) -> Result<FieldVersion, FieldEnqueueError> {
        self.field_runtime
            .version_for(field)
            .ok_or(FieldEnqueueError::VersionExhausted)
    }

    pub(crate) fn reserve_field_version(&mut self) -> Result<FieldVersion, FieldEnqueueError> {
        self.field_runtime
            .reserve_version()
            .ok_or(FieldEnqueueError::VersionExhausted)
    }

    /// The summary already known for this exact `(field, version)`, if any.
    /// Callers consult it *before* building a snapshot so a hit skips the full
    /// min/max scan instead of discarding one that already ran.
    pub(crate) fn cached_field_summary(
        &mut self,
        source: VersionedFieldRef,
    ) -> Option<FieldSummary> {
        self.field_runtime.summary(source)
    }

    pub(crate) fn remember_field_summary(&mut self, snapshot: &FieldSnapshot) {
        if let Some(summary) = snapshot.summary {
            self.field_runtime
                .remember_summary(snapshot.source, summary);
        }
    }

    pub(crate) fn promote_field_version(
        &mut self,
        source: VersionedFieldRef,
        summary: Option<FieldSummary>,
    ) {
        self.field_runtime.promote(source, summary);
    }

    pub(crate) fn current_field_version(&self, field: FieldRef) -> Option<FieldVersion> {
        self.field_runtime.current_version(field)
    }

    // Cache reads take `&mut self`: the derived-data caches are bounded and
    // least-recently-used, so a read is also a recency update.
    pub(crate) fn estimate_for(&mut self, key: &EstimateKey) -> Option<&EstimateResult> {
        self.field_runtime.estimate(key)
    }

    pub(crate) fn geometry_for(
        &mut self,
        key: &ContourGeometryCacheKey,
    ) -> Option<Arc<ContourGeometry>> {
        self.field_runtime.geometry(key)
    }

    pub(crate) fn enqueue_estimate(
        &mut self,
        key: EstimateKey,
        grid: Arc<ScalarGrid2D>,
    ) -> Result<bool, FieldEnqueueError> {
        if self.field_runtime.estimate(&key).is_some()
            || !self.field_runtime.begin_estimate(key.clone())
        {
            return Ok(false);
        }
        #[cfg(test)]
        crate::contour_probe::record_queued_estimate();
        if self
            .job_tx
            .send(Job::EstimateField {
                key: key.clone(),
                grid,
            })
            .is_ok()
        {
            return Ok(true);
        }
        self.field_runtime.finish_estimate_request(&key);
        Err(FieldEnqueueError::WorkersUnavailable)
    }

    pub(crate) fn enqueue_contour(
        &mut self,
        key: ContourGeometryCacheKey,
        grid: Arc<ScalarGrid2D>,
    ) -> Result<bool, FieldEnqueueError> {
        if self.field_runtime.geometry(&key).is_some()
            || !self.field_runtime.begin_geometry(key.clone())
        {
            return Ok(false);
        }
        #[cfg(test)]
        crate::contour_probe::record_queued_contour_build();
        if self
            .job_tx
            .send(Job::BuildContour {
                key: key.clone(),
                grid,
            })
            .is_ok()
        {
            return Ok(true);
        }
        self.field_runtime.finish_geometry_request(&key);
        Err(FieldEnqueueError::WorkersUnavailable)
    }

    pub(crate) fn finish_estimate(
        &mut self,
        key: EstimateKey,
        result: EstimateResult,
        current: Option<FieldVersion>,
    ) -> bool {
        self.field_runtime.install_estimate(key, result, current)
    }

    pub(crate) fn finish_contour(
        &mut self,
        key: ContourGeometryCacheKey,
        geometry: ContourGeometry,
        current: Option<FieldVersion>,
    ) -> bool {
        self.field_runtime.install_geometry(key, geometry, current)
    }
}

pub(super) fn run_estimate_field(
    key: EstimateKey,
    grid: Arc<ScalarGrid2D>,
) -> Result<EstimateResult, String> {
    if !grid.has_valid_shape() {
        return Err("scalar grid dimensions do not match its row-major values".to_owned());
    }
    let provenance = resolved_estimator(&key)?;
    match key.kind {
        EstimateKind::Noise => {
            let scale = robust_difference_mad(&grid.values, grid.rows, grid.cols);
            // A zero scale measures a flat field; it is an answer, not a
            // failure. Caching it is what stops an identically failing job from
            // being re-queued on every canvas rebuild. Only a non-finite or
            // negative scale means the estimator itself misbehaved.
            let scale = EstimatedScale::new(scale)
                .ok_or_else(|| "noise estimator returned a non-finite scale".to_owned())?;
            Ok(EstimateResult::Scale(ScaleEstimate { scale, provenance }))
        }
        EstimateKind::Background => {
            let (location, scale) = deplaned_location_scale(&grid.values, grid.rows, grid.cols);
            let location = FiniteF64::new(location)
                .ok_or_else(|| "background estimator returned a non-finite location".to_owned())?;
            let scale = EstimatedScale::new(scale)
                .ok_or_else(|| "background estimator returned a non-finite scale".to_owned())?;
            Ok(EstimateResult::LocationScale(LocationScaleEstimate {
                location,
                scale,
                provenance,
            }))
        }
    }
}

fn resolved_estimator(key: &EstimateKey) -> Result<EstimateProvenance, String> {
    let (latest_id, latest_version) = match key.kind {
        EstimateKind::Noise => (ROBUST_DIFFERENCE_MAD_ID, ROBUST_DIFFERENCE_MAD_VERSION),
        EstimateKind::Background => (DEPLANED_LOCATION_SCALE_ID, DEPLANED_LOCATION_SCALE_VERSION),
    };
    match &key.estimator {
        plotx_figure::EstimatorSelection::FollowLatest => Ok(EstimateProvenance {
            estimator: latest_id.to_owned(),
            version: latest_version,
        }),
        plotx_figure::EstimatorSelection::Frozen { estimator, version }
            if estimator == latest_id && *version == latest_version =>
        {
            Ok(EstimateProvenance {
                estimator: estimator.clone(),
                version: *version,
            })
        }
        plotx_figure::EstimatorSelection::Frozen { estimator, version } => Err(format!(
            "{} v{version} is not available for this field estimate",
            estimator
        )),
    }
}

pub(super) fn run_build_contour(
    key: ContourGeometryCacheKey,
    grid: Arc<ScalarGrid2D>,
) -> Result<ContourGeometry, String> {
    if !grid.has_valid_shape() {
        return Err("scalar grid dimensions do not match its row-major values".to_owned());
    }
    let Some([x0, x1, y0, y1]) = grid.linear_bounds() else {
        return Err("contour geometry requires finite linear axis bounds".to_owned());
    };
    let levels = &key.levels;
    #[cfg(test)]
    crate::contour_probe::record_marching_squares();
    let positive = plotx_render::contour::segments(
        &grid.values,
        grid.rows,
        grid.cols,
        x0,
        x1,
        y0,
        y1,
        &levels
            .positive
            .iter()
            .map(|value| value.get())
            .collect::<Vec<_>>(),
    );
    #[cfg(test)]
    crate::contour_probe::record_marching_squares();
    let negative = plotx_render::contour::segments(
        &grid.values,
        grid.rows,
        grid.cols,
        x0,
        x1,
        y0,
        y1,
        &levels
            .negative
            .iter()
            .map(|value| value.get())
            .collect::<Vec<_>>(),
    );
    Ok(ContourGeometry {
        positive: Arc::from(positive),
        negative: Arc::from(negative),
        positive_levels: u16::try_from(levels.positive.len()).unwrap_or(u16::MAX),
        negative_levels: u16::try_from(levels.negative.len()).unwrap_or(u16::MAX),
    })
}
