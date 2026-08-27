use num_complex::Complex64;
use plotx_analysis::craft::{
    CraftFitBounds, CraftFitError, CraftFitOptions, DampedSinusoid, backward_linear_predict,
    fit_damped_sinusoids_initialized_cancellable, matrix_pencil_estimates,
};
use std::f64::consts::{PI, TAU};

use super::regions::ModelingWindow;
use super::{CraftError, CraftModelingPolicy, CraftParams, CraftProfile, CraftWarningKind};

pub(super) struct ModelingWindowResult {
    pub(super) components: Vec<DampedSinusoid>,
    pub(super) center_hz: f64,
    pub(super) training_bic: Option<f64>,
    pub(super) condition_number: f64,
    pub(super) decimation: usize,
    pub(super) modeled_sample_count: usize,
    pub(super) evaluated_model_orders: usize,
    pub(super) modeled_duration_s: f64,
    pub(super) training_normalized_residual: f64,
    pub(super) validation_normalized_residual: f64,
    pub(super) warning: Option<(CraftWarningKind, String)>,
}

pub(super) struct CraftModelingContext<'a> {
    pub(super) input: &'a [Complex64],
    pub(super) skipped_points: usize,
    pub(super) group_delay_points: f64,
    pub(super) spectral_width_hz: f64,
    pub(super) params: &'a CraftParams,
    pub(super) policy: CraftModelingPolicy,
}

struct ValidatedCandidate {
    order: usize,
    components: Vec<DampedSinusoid>,
    training_bic: f64,
    condition_number: f64,
    training_normalized_residual: f64,
    validation_normalized_residual: f64,
}

pub(super) fn fit_modeling_window(
    context: &CraftModelingContext<'_>,
    window: ModelingWindow,
    cancelled: &impl Fn() -> bool,
) -> Result<ModelingWindowResult, CraftError> {
    let CraftModelingContext {
        input,
        skipped_points,
        group_delay_points,
        spectral_width_hz: sw,
        params,
        policy,
    } = context;
    let center_hz = (window.modeling_band_hz.0 + window.modeling_band_hz.1) * 0.5;
    let modeled_bandwidth_hz = window.modeling_band_hz.1 - window.modeling_band_hz.0;
    // Include one filter length of guard samples so the modeled interval has
    // complete centered-FIR support at both ends.
    let modeled_points = (policy.modeling_duration_s * *sw).ceil().max(1.0) as usize;
    let filter_input_len = input
        .len()
        .min(modeled_points.saturating_add(params.fir_filter_taps));
    let mixed: Vec<Complex64> = input[..filter_input_len]
        .iter()
        .enumerate()
        .map(|(index, &value)| {
            // Raw point numbers preserve the fractional group-delay time origin
            // after the digital-filter transient has been skipped.
            let time = (*skipped_points as f64 + index as f64 - *group_delay_points) / *sw;
            value * Complex64::from_polar(1.0, -TAU * center_hz * time)
        })
        .collect();
    let filtered = low_pass_fir(
        &mixed,
        *sw,
        modeled_bandwidth_hz * 0.5,
        params.fir_filter_taps,
        cancelled,
    )?;
    let mut decimation = (*sw / (2.0 * modeled_bandwidth_hz).max(f64::MIN_POSITIVE))
        .floor()
        .max(1.0) as usize;
    decimation = decimation.max(filtered.len().div_ceil(params.maximum_modeled_sample_count));
    let filter_half = effective_filter_taps(params.fir_filter_taps, mixed.len()) / 2;
    let valid_end = filtered.len().saturating_sub(filter_half);
    // Match the established CRAFT digital-filter workflow: retain the early
    // record through a phase-preserving FIR precharge, then replace the five
    // boundary-dependent downsampled points by backward linear prediction.
    let modeling_start = 0_usize;
    let useful_end = modeling_start.saturating_add(modeled_points).min(valid_end);
    let mut samples: Vec<Complex64> = filtered[modeling_start..useful_end]
        .iter()
        .step_by(decimation)
        .copied()
        .collect();
    let predicted_count = samples.len().min(5);
    let training_count = samples.len().saturating_sub(predicted_count).min(256);
    let configured_order = if samples.len() > 261 { 32 } else { 16 };
    let prediction_order = configured_order.min(training_count.saturating_sub(1) / 2);
    if params.profile == CraftProfile::Conventional && predicted_count > 0 && prediction_order > 0 {
        match backward_linear_predict(
            &mut samples,
            predicted_count,
            training_count,
            prediction_order,
        ) {
            Ok(()) => {}
            // A rankless no-signal record has nothing to predict. Keep the
            // phase-preserving FIR precharge so exploratory runs can report
            // the empty window instead of failing the complete invocation.
            Err(CraftFitError::Singular) => {}
            Err(CraftFitError::Cancelled) => return Err(CraftError::Cancelled),
            Err(error) => return Err(CraftError::Fit(error)),
        }
    }
    let times: Vec<f64> = (0..samples.len())
        .map(|index| {
            (*skipped_points as f64 + modeling_start as f64 + (index * decimation) as f64
                - *group_delay_points)
                / *sw
        })
        .collect();
    if samples.len() < 16 {
        return Ok(ModelingWindowResult {
            components: Vec::new(),
            center_hz,
            training_bic: None,
            condition_number: 1.0,
            decimation,
            modeled_sample_count: samples.len(),
            evaluated_model_orders: 0,
            modeled_duration_s: samples.len() as f64 * decimation as f64 / *sw,
            training_normalized_residual: 1.0,
            validation_normalized_residual: 1.0,
            warning: Some((
                CraftWarningKind::ModelingWindowFailure,
                "too few samples remained after filtering".to_owned(),
            )),
        });
    }

    let relative_frequency_bounds = (
        window.modeling_band_hz.0 - center_hz,
        window.modeling_band_hz.1 - center_hz,
    );
    let validation_count = if samples.len() >= 32 {
        ((samples.len() as f64 * policy.validation_tail_fraction).round() as usize).max(8)
    } else {
        0
    };
    let validation_start = samples.len() - validation_count;
    let training_samples = samples.as_slice();
    let training_times = times.as_slice();
    let validation_samples = &samples[validation_start..];
    let validation_times = &times[validation_start..];
    let training_energy = training_samples
        .iter()
        .map(Complex64::norm_sqr)
        .sum::<f64>()
        .max(f64::MIN_POSITIVE);
    let validation_energy = validation_samples
        .iter()
        .map(Complex64::norm_sqr)
        .sum::<f64>()
        .max(f64::MIN_POSITIVE);
    let observation_count = (training_samples.len() * 2) as f64;
    let mut candidates = Vec::new();
    let mut warning = None;

    let fit_bounds = CraftFitBounds {
        frequency_hz: relative_frequency_bounds,
        linewidth_hz: policy.component_linewidth_bounds_hz,
    };
    let dwell_s = decimation as f64 / *sw;
    let minimum_separation_hz = *sw / input.len() as f64;
    let maximum_order = params
        .maximum_model_order
        .min(training_samples.len() / 2 - 1);
    let mut evaluated_model_orders = 0;
    // Candidate generation is deliberately bounded because the Hankel SVD is
    // cubic. Final amplitudes, evidence, covariance, and residuals use the
    // complete sub-FID, matching the established CRAFT workflow.
    let pencil_samples = &training_samples[..training_samples.len().min(256)];
    for order in 1..=maximum_order {
        evaluated_model_orders += 1;
        let Ok(candidate) = matrix_pencil_estimates(pencil_samples, dwell_s, order, fit_bounds)
        else {
            continue;
        };
        if candidate.components.len() != order {
            continue;
        }
        let fit = match fit_damped_sinusoids_initialized_cancellable(
            training_samples,
            training_times,
            &candidate.components,
            fit_bounds,
            CraftFitOptions::default(),
            cancelled,
        ) {
            Ok(fit) => Some(fit),
            Err(CraftFitError::Cancelled) => return Err(CraftError::Cancelled),
            Err(error) => {
                warning = Some((CraftWarningKind::ModelingWindowFailure, error.to_string()));
                None
            }
        };
        if let Some(mut fit) = fit {
            fit.components
                .sort_by(|left, right| left.frequency_hz.total_cmp(&right.frequency_hz));
            let candidate_bic = bic(
                fit.rss,
                observation_count,
                (fit.components.len() * 4 + 1) as f64,
            );
            let sufficiently_separated = fit.components.windows(2).all(|pair| {
                (pair[1].frequency_hz - pair[0].frequency_hz).abs() >= minimum_separation_hz
            });
            let linewidths_are_interior = fit.components.iter().all(|component| {
                component.linewidth_hz > fit_bounds.linewidth_hz.0 + 1e-6
                    && component.linewidth_hz < fit_bounds.linewidth_hz.1 - 1e-6
            });
            let uncertainties_are_bounded = fit.components.iter().all(|component| {
                component.amplitude_std.is_some()
                    && component.frequency_std_hz.is_some()
                    && component.linewidth_std_hz.is_some()
                    && component.phase_std_rad.is_some()
            });
            let model_amplitude_to_noise = coherent_amplitude(&fit.components)
                / fit
                    .components
                    .iter()
                    .filter_map(|component| component.amplitude_std)
                    .map(|value| value * value)
                    .sum::<f64>()
                    .sqrt()
                    .max(f64::MIN_POSITIVE);
            if sufficiently_separated
                && linewidths_are_interior
                && uncertainties_are_bounded
                && model_amplitude_to_noise >= params.minimum_amplitude_to_noise
                && fit.condition_number <= 1e8
            {
                let validation_rss = if validation_samples.is_empty() {
                    fit.rss
                } else {
                    validation_samples
                        .iter()
                        .zip(validation_times)
                        .map(|(&sample, &time)| {
                            (sample - damped_model_at(&fit.components, time)).norm_sqr()
                        })
                        .sum()
                };
                candidates.push(ValidatedCandidate {
                    order,
                    components: fit.components,
                    training_bic: candidate_bic,
                    condition_number: fit.condition_number,
                    training_normalized_residual: (fit.rss / training_energy).sqrt(),
                    validation_normalized_residual: if validation_samples.is_empty() {
                        (fit.rss / training_energy).sqrt()
                    } else {
                        (validation_rss / validation_energy).sqrt()
                    },
                });
            }
        }
    }

    let minimum_training_bic = candidates
        .iter()
        .map(|candidate| candidate.training_bic)
        .min_by(f64::total_cmp)
        .unwrap_or_else(|| bic(training_energy, observation_count, 1.0));
    // Bretthorst CRAFT selects model order from the evidence in the modeled
    // record. BIC is the deterministic evidence approximation used here; a
    // two-unit band retains the simplest statistically comparable model.
    let comparable_bic_limit = minimum_training_bic + 2.0;
    candidates.sort_by_key(|candidate| candidate.order);
    let selected = candidates
        .into_iter()
        .find(|candidate| candidate.training_bic <= comparable_bic_limit);
    let (mut components, training_bic, condition_number, training_residual, validation_residual) =
        selected.map_or_else(
            || {
                (
                    Vec::new(),
                    bic(training_energy, observation_count, 1.0),
                    1.0,
                    1.0,
                    1.0,
                )
            },
            |candidate| {
                (
                    candidate.components,
                    candidate.training_bic,
                    candidate.condition_number,
                    candidate.training_normalized_residual,
                    candidate.validation_normalized_residual,
                )
            },
        );
    // Very small poles at a window edge are commonly transition-band leakage
    // or a split of the dominant line, not an independently quantifiable
    // resonance. Apply the threshold to the selected multiplet so weak lines
    // are retained relative to their local partner rather than compared with
    // a global raw-FID noise estimate.
    if let Some(maximum_amplitude) = components
        .iter()
        .map(|component| component.amplitude)
        .max_by(f64::total_cmp)
    {
        let minimum_amplitude = maximum_amplitude * 0.05;
        components.retain(|component| component.amplitude >= minimum_amplitude);
    }
    if !components.is_empty()
        && warning
            .as_ref()
            .is_some_and(|(kind, _)| *kind == CraftWarningKind::ModelingWindowFailure)
    {
        warning = None;
    }
    let actual_taps = effective_filter_taps(params.fir_filter_taps, mixed.len());
    for component in &mut components {
        let gain = fir_response(
            *sw,
            modeled_bandwidth_hz * 0.5,
            actual_taps,
            component.frequency_hz,
            component.decay_rate_s_inv,
        );
        let gain_norm = gain.norm();
        if gain_norm > 0.1 {
            component.amplitude /= gain_norm;
            component.amplitude_std = component.amplitude_std.map(|value| value / gain_norm);
            component.phase_rad -= gain.arg();
        }
    }
    if components.len() == maximum_order {
        warning = Some((
            CraftWarningKind::ModelOrderLimit,
            "model order reached the modeling-window limit; inspect the residual before quantitation"
                .to_owned(),
        ));
    }
    if !components.is_empty() && training_residual > 0.25 {
        warning = Some((
            CraftWarningKind::ModelingWindowFailure,
            format!(
                "modeled-record residual {training_residual:.3} exceeded the quantitative limit"
            ),
        ));
    }
    Ok(ModelingWindowResult {
        components,
        center_hz,
        training_bic: Some(training_bic),
        condition_number,
        decimation,
        modeled_sample_count: samples.len(),
        evaluated_model_orders,
        modeled_duration_s: samples.len() as f64 * decimation as f64 / *sw,
        training_normalized_residual: training_residual,
        validation_normalized_residual: validation_residual,
        warning,
    })
}

fn damped_model_at(components: &[DampedSinusoid], time_s: f64) -> Complex64 {
    components
        .iter()
        .fold(Complex64::new(0.0, 0.0), |sum, component| {
            sum + Complex64::from_polar(
                component.amplitude * (-component.decay_rate_s_inv * time_s).exp(),
                component.phase_rad + TAU * component.frequency_hz * time_s,
            )
        })
}

fn coherent_amplitude(components: &[DampedSinusoid]) -> f64 {
    components
        .iter()
        .fold(Complex64::new(0.0, 0.0), |sum, component| {
            sum + Complex64::from_polar(component.amplitude, component.phase_rad)
        })
        .norm()
}

fn low_pass_fir(
    input: &[Complex64],
    sample_rate_hz: f64,
    cutoff_hz: f64,
    requested_taps: usize,
    cancelled: &impl Fn() -> bool,
) -> Result<Vec<Complex64>, CraftError> {
    if cutoff_hz * 2.0 >= sample_rate_hz * 0.999 {
        return Ok(input.to_vec());
    }
    let taps = effective_filter_taps(requested_taps, input.len());
    if taps < 3 {
        return Ok(input.to_vec());
    }
    let half = taps / 2;
    let kernel = fir_kernel(sample_rate_hz, cutoff_hz, taps);
    let mut output = vec![Complex64::new(0.0, 0.0); input.len()];
    let first = input[0];
    let reflection = if first.norm_sqr() > f64::MIN_POSITIVE {
        first / first.conj()
    } else {
        Complex64::new(1.0, 0.0)
    };
    for (center, filtered) in output
        .iter_mut()
        .enumerate()
        .take(input.len().saturating_sub(half))
    {
        if center % 64 == 0 && cancelled() {
            return Err(CraftError::Cancelled);
        }
        *filtered = kernel.iter().enumerate().fold(
            Complex64::new(0.0, 0.0),
            |sum, (index, &coefficient)| {
                let source = center as isize + index as isize - half as isize;
                let sample = if source >= 0 {
                    input[source as usize]
                } else {
                    reflection * input[source.unsigned_abs()].conj()
                };
                sum + sample * coefficient
            },
        );
    }
    Ok(output)
}

fn fir_response(
    sample_rate_hz: f64,
    cutoff_hz: f64,
    taps: usize,
    frequency_hz: f64,
    decay_rate_s_inv: f64,
) -> Complex64 {
    if cutoff_hz * 2.0 >= sample_rate_hz * 0.999 || taps < 3 {
        return Complex64::new(1.0, 0.0);
    }
    let half = taps / 2;
    fir_kernel(sample_rate_hz, cutoff_hz, taps)
        .iter()
        .enumerate()
        .fold(Complex64::new(0.0, 0.0), |sum, (index, coefficient)| {
            let offset_s = (index as f64 - half as f64) / sample_rate_hz;
            sum + Complex64::from_polar(
                coefficient * (-decay_rate_s_inv * offset_s).exp(),
                TAU * frequency_hz * offset_s,
            )
        })
}

fn fir_kernel(sample_rate_hz: f64, cutoff_hz: f64, taps: usize) -> Vec<f64> {
    let half = taps / 2;
    let normalized = cutoff_hz / sample_rate_hz;
    let mut kernel = (0..taps)
        .map(|index| {
            let x = index as isize - half as isize;
            let sinc = if x == 0 {
                2.0 * normalized
            } else {
                (TAU * normalized * x as f64).sin() / (PI * x as f64)
            };
            let window = 0.42 - 0.5 * (TAU * index as f64 / (taps - 1) as f64).cos()
                + 0.08 * (2.0 * TAU * index as f64 / (taps - 1) as f64).cos();
            sinc * window
        })
        .collect::<Vec<_>>();
    let sum = kernel.iter().sum::<f64>();
    for coefficient in &mut kernel {
        *coefficient /= sum;
    }
    kernel
}

fn effective_filter_taps(requested_taps: usize, input_len: usize) -> usize {
    let taps = requested_taps.min(input_len.saturating_sub(1));
    if taps.is_multiple_of(2) {
        taps.saturating_sub(1)
    } else {
        taps
    }
}

fn bic(rss: f64, observations: f64, parameters: f64) -> f64 {
    observations * (rss.max(f64::MIN_POSITIVE) / observations).ln() + parameters * observations.ln()
}
