//! Worker-only execution for jobs declared by `compute`.

use super::*;

pub(super) fn run_job(job: Job) -> Done {
    match job {
        Job::Ilt {
            generation,
            dataset,
            epoch,
            token,
            stack,
            b_factors,
            d_grid,
            lambda,
            params,
            nucleus,
            source,
        } => {
            let cancelled = || token.load(Ordering::Relaxed);
            match ilt_map_cancellable(&*stack, &b_factors, &d_grid, lambda, &cancelled) {
                Some(result) if !cancelled() => {
                    let Some(figure) =
                        build_ilt_figure_cancellable(&result, &nucleus, &source, &cancelled)
                            .map(Arc::new)
                    else {
                        return Done::Cancelled {
                            generation,
                            dataset,
                            kind: ComputeKind::Ilt,
                        };
                    };
                    Done::Ilt {
                        generation,
                        dataset,
                        epoch,
                        result,
                        params,
                        figure,
                    }
                }
                None | Some(_) => Done::Cancelled {
                    generation,
                    dataset,
                    kind: ComputeKind::Ilt,
                },
            }
        }
        Job::Dosy {
            generation,
            dataset,
            epoch,
            token,
            stack,
            values,
            meta,
            nucleus,
            source,
        } => {
            let cancelled = || token.load(Ordering::Relaxed);
            match diffusion_map_cancellable(&*stack, &values, &meta, 0.05, &cancelled) {
                Some(result) if !cancelled() => {
                    let Some(figure) =
                        build_dosy_figure_cancellable(&result, &nucleus, &source, &cancelled)
                            .map(Arc::new)
                    else {
                        return Done::Cancelled {
                            generation,
                            dataset,
                            kind: ComputeKind::Dosy,
                        };
                    };
                    Done::Dosy {
                        generation,
                        dataset,
                        epoch,
                        result,
                        figure,
                    }
                }
                None | Some(_) => Done::Cancelled {
                    generation,
                    dataset,
                    kind: ComputeKind::Dosy,
                },
            }
        }
        Job::Process2D {
            version,
            dataset,
            token,
            input,
            params,
            fields,
        } => {
            let cancelled = || token.load(Ordering::Relaxed);
            let (base, processed) = match input {
                ProcessingInput::Full(data) => {
                    let Some(base) = process_2d_cancellable(&data, &params, &cancelled) else {
                        return cancelled_done(version.0, dataset);
                    };
                    let Some(processed) = reapply_2d_cancellable(&base, &params, &cancelled) else {
                        return cancelled_done(version.0, dataset);
                    };
                    (Some(base), processed)
                }
                ProcessingInput::Reapply(base) => {
                    let Some(processed) = reapply_2d_cancellable(&base, &params, &cancelled) else {
                        return cancelled_done(version.0, dataset);
                    };
                    (None, processed)
                }
            };
            if cancelled() {
                return cancelled_done(version.0, dataset);
            }
            let fields = processed_field_artifacts(&processed, &fields);
            Done::Processing2D {
                version,
                dataset,
                base,
                processed,
                fields,
                params,
            }
        }
        Job::EstimateField { key, grid } => {
            match compute_field::run_estimate_field(key.clone(), grid) {
                Ok(result) => Done::EstimateField { key, result },
                Err(message) => Done::EstimateFieldFailed { key, message },
            }
        }
        Job::BuildContour { key, grid } => {
            match compute_field::run_build_contour(key.clone(), grid) {
                Ok(geometry) => Done::BuildContour { key, geometry },
                Err(message) => Done::BuildContourFailed { key, message },
            }
        }
    }
}

fn cancelled_done(generation: u64, dataset: DatasetId) -> Done {
    Done::Cancelled {
        generation,
        dataset,
        kind: ComputeKind::Processing2D,
    }
}

fn processed_field_artifacts(
    processed: &Processed2D,
    fields: &[VersionedProcessingField],
) -> Vec<ProcessedFieldArtifact> {
    fields
        .iter()
        .map(|field| {
            let summary = match processed {
                Processed2D::Ft(spectrum) => {
                    let values = match field.component {
                        ProcessedFieldComponent::Real => spectrum.real(),
                        ProcessedFieldComponent::Magnitude => spectrum.magnitude(),
                    };
                    nmr_scalar_grid(spectrum, values).summary()
                }
                Processed2D::Stack(_) => None,
            };
            ProcessedFieldArtifact {
                source: field.source,
                summary,
            }
        })
        .collect()
}
