//! Worker-only execution for jobs declared by `compute`.

use super::*;
use crate::state::{MONO_EXP_SNR_FRAC, ilt_provenance, mono_exp_provenance};

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
            values,
            meta,
            nucleus,
            source,
        } => {
            let cancelled = || token.is_cancelled();
            let provenance = ilt_provenance(&stack, &values, &meta, params);
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
                        provenance,
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
            let cancelled = || token.is_cancelled();
            let provenance = mono_exp_provenance(&stack, &values, &meta);
            match diffusion_map_cancellable(&*stack, &values, &meta, MONO_EXP_SNR_FRAC, &cancelled)
            {
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
                        provenance,
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
        Job::Craft {
            generation,
            dataset,
            epoch,
            token,
            data,
            invocation,
            parent_run,
        } => {
            let cancelled = || token.is_cancelled();
            match process_craft_cancellable(&data, &invocation, &cancelled) {
                Ok(result) if !cancelled() => Done::Craft {
                    generation,
                    dataset,
                    epoch,
                    result,
                    invocation,
                    parent_run,
                },
                Err(plotx_processing::craft::CraftError::Cancelled) | Ok(_) => Done::Cancelled {
                    generation,
                    dataset,
                    kind: ComputeKind::Craft,
                },
                Err(error) => Done::CraftFailed {
                    generation,
                    dataset,
                    epoch,
                    message: error.to_string(),
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
            let cancelled = || token.is_cancelled();
            let mut work = plotx_processing::nmr_execution::processing_2d_work_ledger();
            let mut context =
                nmr::ExecutionContext::new(&mut work).with_cancellation(token.clone());
            #[cfg(test)]
            let processing_timer = crate::contour_probe::Timer::new("processing + view");
            let result = (|| match input {
                ProcessingInput::Full(input) => {
                    let base = execute_2d(
                        &input.source,
                        &params,
                        input.delay,
                        RecipeRange::Base,
                        input.nus,
                        &mut context,
                    )?;
                    let processed = execute_2d(
                        &base.source,
                        &params,
                        DelayPolicy::Disabled,
                        RecipeRange::Frequency,
                        None,
                        &mut context,
                    )?;
                    Ok((Some(base), processed))
                }
                ProcessingInput::Reapply(base) => {
                    let processed = execute_2d(
                        &base,
                        &params,
                        DelayPolicy::Disabled,
                        RecipeRange::Frequency,
                        None,
                        &mut context,
                    )?;
                    Ok((None, processed))
                }
            })();
            #[cfg(test)]
            drop(processing_timer);
            let (base, processed) = match result {
                Ok(output) => output,
                Err(error)
                    if plotx_processing::nmr_execution::ExecutionError::is_cancelled(&error) =>
                {
                    return cancelled_done(version.0, dataset);
                }
                Err(error) => {
                    return Done::Processing2DFailed {
                        version,
                        dataset,
                        message: error.to_string(),
                    };
                }
            };
            if cancelled() {
                return cancelled_done(version.0, dataset);
            }
            let fields = processed_field_artifacts(&processed.view, &fields);
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
    #[cfg(test)]
    let _timer = crate::contour_probe::Timer::new("field artifacts");
    fields
        .iter()
        .map(|field| {
            let grid = match processed {
                Processed2D::Ft(spectrum) => {
                    let values = match field.component {
                        ProcessedFieldComponent::Real => spectrum.real(),
                        ProcessedFieldComponent::Magnitude => spectrum.magnitude(),
                    };
                    Some(Arc::new(nmr_scalar_grid(spectrum, values)))
                }
                Processed2D::Stack(_) => None,
            };
            ProcessedFieldArtifact {
                source: field.source,
                summary: grid.as_ref().and_then(|grid| grid.summary()),
                grid,
            }
        })
        .collect()
}
