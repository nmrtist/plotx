//! Constrained time-domain fitting of damped complex sinusoids for CRAFT.

use crate::fit::{
    LmProblem, levenberg_marquardt_problem_cancellable, problem_parameter_covariance,
};
use nalgebra::{DMatrix, DVector};
use num_complex::Complex64;
use serde::{Deserialize, Serialize};
use std::f64::consts::{PI, TAU};

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct CraftFitBounds {
    pub frequency_hz: (f64, f64),
    pub linewidth_hz: (f64, f64),
}

impl CraftFitBounds {
    pub fn validate(self) -> Result<Self, CraftFitError> {
        let (f0, f1) = self.frequency_hz;
        let (l0, l1) = self.linewidth_hz;
        if !f0.is_finite()
            || !f1.is_finite()
            || f0 >= f1
            || !l0.is_finite()
            || !l1.is_finite()
            || l0 <= 0.0
            || l0 >= l1
        {
            return Err(CraftFitError::InvalidBounds);
        }
        Ok(self)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct CraftFitOptions {
    pub max_iterations: usize,
    pub initial_linewidth_hz: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DampedSinusoidEstimate {
    pub frequency_hz: f64,
    pub linewidth_hz: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MatrixPencilEstimate {
    pub components: Vec<DampedSinusoidEstimate>,
    pub rss: f64,
}

impl Default for CraftFitOptions {
    fn default() -> Self {
        Self {
            max_iterations: 80,
            initial_linewidth_hz: 1.0,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DampedSinusoid {
    pub frequency_hz: f64,
    pub amplitude: f64,
    pub phase_rad: f64,
    pub decay_rate_s_inv: f64,
    pub linewidth_hz: f64,
    /// `None` when the fit covariance cannot support an uncertainty estimate.
    pub amplitude_std: Option<f64>,
    pub frequency_std_hz: Option<f64>,
    pub linewidth_std_hz: Option<f64>,
    pub phase_std_rad: Option<f64>,
}

#[derive(Clone, Debug)]
pub struct DampedSinusoidFit {
    pub components: Vec<DampedSinusoid>,
    pub residual: Vec<Complex64>,
    pub rss: f64,
    pub iterations: usize,
    pub condition_number: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum CraftFitError {
    #[error("CRAFT fitting requires matching, non-empty sample and time arrays")]
    InvalidInput,
    #[error("CRAFT fitting bounds are invalid")]
    InvalidBounds,
    #[error("CRAFT fitting was cancelled")]
    Cancelled,
    #[error("CRAFT fitting could not solve the component system")]
    Singular,
}

/// Replace the leading samples of a complex record by backward linear
/// prediction from the immediately following observed samples.
///
/// CRAFT uses this after digital filtering because a finite FIR must invent a
/// prehistory at the acquisition boundary. The prediction is fitted in reverse
/// time, so the supplied autoregressive order has the same meaning as a
/// conventional forward linear predictor.
pub fn backward_linear_predict(
    samples: &mut [Complex64],
    predicted_count: usize,
    training_count: usize,
    order: usize,
) -> Result<(), CraftFitError> {
    if predicted_count == 0 {
        return Ok(());
    }
    if order == 0
        || training_count <= order
        || predicted_count
            .checked_add(training_count)
            .is_none_or(|required| required > samples.len())
        || samples
            .iter()
            .take(predicted_count + training_count)
            .any(|value| !value.re.is_finite() || !value.im.is_finite())
    {
        return Err(CraftFitError::InvalidInput);
    }

    let training = &samples[predicted_count..predicted_count + training_count];
    let reversed = training.iter().rev().copied().collect::<Vec<_>>();
    let scale = reversed
        .iter()
        .map(|value| value.norm())
        .fold(0.0_f64, f64::max);
    if scale <= f64::MIN_POSITIVE {
        return Err(CraftFitError::Singular);
    }
    let equation_count = reversed.len() - order;
    let mut design = DMatrix::<f64>::zeros(equation_count * 2, order * 2);
    let mut observed = DVector::<f64>::zeros(equation_count * 2);
    for row in 0..equation_count {
        let target = reversed[row + order];
        observed[row * 2] = target.re / scale;
        observed[row * 2 + 1] = target.im / scale;
        for lag in 0..order {
            let basis = reversed[row + order - lag - 1] / scale;
            design[(row * 2, lag * 2)] = basis.re;
            design[(row * 2, lag * 2 + 1)] = -basis.im;
            design[(row * 2 + 1, lag * 2)] = basis.im;
            design[(row * 2 + 1, lag * 2 + 1)] = basis.re;
        }
    }
    // Scale the singular-value cutoff by the design energy so rank detection
    // remains stable across differently normalized input records.
    let rank_tolerance = (5e-14 * design.norm_squared()).sqrt().max(1e-12);
    let solution = design
        .svd(true, true)
        .solve(&observed, rank_tolerance)
        .map_err(|_| CraftFitError::Singular)?;
    let coefficients = solution
        .as_slice()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| Complex64::new(pair[0], pair[1]))
        .collect::<Vec<_>>();
    let mut history = reversed;
    for index in 0..predicted_count {
        let predicted = coefficients
            .iter()
            .enumerate()
            .fold(Complex64::new(0.0, 0.0), |sum, (lag, coefficient)| {
                sum + coefficient * history[history.len() - lag - 1]
            });
        if !predicted.re.is_finite() || !predicted.im.is_finite() {
            return Err(CraftFitError::Singular);
        }
        samples[predicted_count - index - 1] = predicted;
        history.push(predicted);
    }
    Ok(())
}

/// Fit a fixed set of initial component frequencies. Model-order selection and
/// residual candidate discovery live in `plotx-processing`, beside its FFT.
pub fn fit_damped_sinusoids_cancellable(
    samples: &[Complex64],
    times_s: &[f64],
    initial_frequencies_hz: &[f64],
    bounds: CraftFitBounds,
    options: CraftFitOptions,
    cancelled: &impl Fn() -> bool,
) -> Result<DampedSinusoidFit, CraftFitError> {
    let estimates = initial_frequencies_hz
        .iter()
        .map(|&frequency_hz| DampedSinusoidEstimate {
            frequency_hz,
            linewidth_hz: options.initial_linewidth_hz,
        })
        .collect::<Vec<_>>();
    fit_damped_sinusoids_initialized_cancellable(
        samples, times_s, &estimates, bounds, options, cancelled,
    )
}

/// Fit initialized frequency/linewidth pairs. Matrix-pencil or other spectral
/// estimators can use this entry point while retaining the bounded LM and
/// covariance implementation shared by all CRAFT callers.
pub fn fit_damped_sinusoids_initialized_cancellable(
    samples: &[Complex64],
    times_s: &[f64],
    initial: &[DampedSinusoidEstimate],
    bounds: CraftFitBounds,
    options: CraftFitOptions,
    cancelled: &impl Fn() -> bool,
) -> Result<DampedSinusoidFit, CraftFitError> {
    if samples.is_empty()
        || samples.len() != times_s.len()
        || samples
            .iter()
            .any(|value| !value.re.is_finite() || !value.im.is_finite())
        || times_s.iter().any(|value| !value.is_finite())
    {
        return Err(CraftFitError::InvalidInput);
    }
    let bounds = bounds.validate()?;
    if cancelled() {
        return Err(CraftFitError::Cancelled);
    }
    if initial.is_empty() {
        let rss = samples.iter().map(Complex64::norm_sqr).sum();
        return Ok(DampedSinusoidFit {
            components: Vec::new(),
            residual: samples.to_vec(),
            rss,
            iterations: 0,
            condition_number: 1.0,
        });
    }

    let mut estimates = initial.to_vec();
    estimates.sort_by(|left, right| left.frequency_hz.total_cmp(&right.frequency_hz));
    estimates.dedup_by(|left, right| (left.frequency_hz - right.frequency_hz).abs() <= 1e-9);
    if estimates.iter().any(|estimate| {
        estimate.frequency_hz < bounds.frequency_hz.0
            || estimate.frequency_hz > bounds.frequency_hz.1
            || !estimate.linewidth_hz.is_finite()
    }) {
        return Err(CraftFitError::InvalidBounds);
    }
    let frequencies = estimates
        .iter()
        .map(|estimate| estimate.frequency_hz)
        .collect::<Vec<_>>();
    let decay_rates = estimates
        .iter()
        .map(|estimate| {
            PI * estimate
                .linewidth_hz
                .clamp(bounds.linewidth_hz.0, bounds.linewidth_hz.1)
        })
        .collect::<Vec<_>>();
    let amplitudes = solve_complex_amplitudes(samples, times_s, &frequencies, &decay_rates)
        .ok_or(CraftFitError::Singular)?;

    let mut params = Vec::with_capacity(frequencies.len() * 4);
    for ((frequency, decay_rate), amplitude) in
        frequencies.iter().zip(&decay_rates).zip(&amplitudes)
    {
        params.push(amplitude.re);
        params.push(amplitude.im);
        params.push(to_unbounded(*frequency, bounds.frequency_hz));
        params.push(to_unbounded(*decay_rate / PI, bounds.linewidth_hz));
    }
    let mut problem = CraftProblem {
        samples,
        times_s,
        bounds,
        derivatives: vec![Complex64::new(0.0, 0.0); params.len()],
    };
    let Some((params, iterations)) = levenberg_marquardt_problem_cancellable(
        &mut problem,
        &params,
        options.max_iterations,
        cancelled,
    ) else {
        return Err(if cancelled() {
            CraftFitError::Cancelled
        } else {
            CraftFitError::Singular
        });
    };
    if cancelled() {
        return Err(CraftFitError::Cancelled);
    }

    let covariance = problem_parameter_covariance(&mut problem, &params, samples.len() * 2);
    let decoded = problem.decode(&params);
    let residual = residuals(samples, times_s, &decoded);
    let rss = residual.iter().map(Complex64::norm_sqr).sum();
    let condition_number = design_condition_number(times_s, &decoded);
    let components = decoded
        .iter()
        .enumerate()
        .map(|(index, component)| {
            component_with_uncertainty(component, index, covariance.as_deref(), bounds)
        })
        .collect();
    Ok(DampedSinusoidFit {
        components,
        residual,
        rss,
        iterations,
        condition_number,
    })
}

/// Evaluate matrix-pencil poles without moving them to a nearby, potentially
/// degenerate nonlinear minimum. Linear complex amplitudes and the full local
/// covariance are still solved from the supplied observations.
pub fn evaluate_damped_sinusoids_cancellable(
    samples: &[Complex64],
    times_s: &[f64],
    initial: &[DampedSinusoidEstimate],
    bounds: CraftFitBounds,
    cancelled: &impl Fn() -> bool,
) -> Result<DampedSinusoidFit, CraftFitError> {
    if samples.is_empty()
        || samples.len() != times_s.len()
        || initial.is_empty()
        || samples
            .iter()
            .any(|value| !value.re.is_finite() || !value.im.is_finite())
        || times_s.iter().any(|value| !value.is_finite())
    {
        return Err(CraftFitError::InvalidInput);
    }
    let bounds = bounds.validate()?;
    if cancelled() {
        return Err(CraftFitError::Cancelled);
    }
    let mut estimates = initial.to_vec();
    estimates.sort_by(|left, right| left.frequency_hz.total_cmp(&right.frequency_hz));
    if estimates.iter().any(|estimate| {
        estimate.frequency_hz < bounds.frequency_hz.0
            || estimate.frequency_hz > bounds.frequency_hz.1
            || estimate.linewidth_hz < bounds.linewidth_hz.0
            || estimate.linewidth_hz > bounds.linewidth_hz.1
    }) {
        return Err(CraftFitError::InvalidBounds);
    }
    let frequencies = estimates
        .iter()
        .map(|estimate| estimate.frequency_hz)
        .collect::<Vec<_>>();
    let decay_rates = estimates
        .iter()
        .map(|estimate| PI * estimate.linewidth_hz)
        .collect::<Vec<_>>();
    let amplitudes = solve_complex_amplitudes(samples, times_s, &frequencies, &decay_rates)
        .ok_or(CraftFitError::Singular)?;
    let mut params = Vec::with_capacity(estimates.len() * 4);
    for ((estimate, decay_rate), amplitude) in estimates.iter().zip(&decay_rates).zip(&amplitudes) {
        params.push(amplitude.re);
        params.push(amplitude.im);
        params.push(to_unbounded(estimate.frequency_hz, bounds.frequency_hz));
        params.push(to_unbounded(*decay_rate / PI, bounds.linewidth_hz));
    }
    let mut problem = CraftProblem {
        samples,
        times_s,
        bounds,
        derivatives: vec![Complex64::new(0.0, 0.0); params.len()],
    };
    let covariance = problem_parameter_covariance(&mut problem, &params, samples.len() * 2);
    let decoded = problem.decode(&params);
    let residual = residuals(samples, times_s, &decoded);
    let rss = residual.iter().map(Complex64::norm_sqr).sum();
    let condition_number = design_condition_number(times_s, &decoded);
    let components = decoded
        .iter()
        .enumerate()
        .map(|(index, component)| {
            component_with_uncertainty(component, index, covariance.as_deref(), bounds)
        })
        .collect();
    Ok(DampedSinusoidFit {
        components,
        residual,
        rss,
        iterations: 0,
        condition_number,
    })
}

/// Estimate damped complex exponentials with a reduced matrix pencil.
///
/// The estimates seed the bounded nonlinear fit; they are not returned as final
/// CRAFT values because covariance and physical bounds are handled by that fit.
pub fn matrix_pencil_estimates(
    samples: &[Complex64],
    dwell_s: f64,
    order: usize,
    bounds: CraftFitBounds,
) -> Result<MatrixPencilEstimate, CraftFitError> {
    let bounds = bounds.validate()?;
    if samples.len() < 6
        || order == 0
        || order * 2 >= samples.len()
        || !dwell_s.is_finite()
        || dwell_s <= 0.0
    {
        return Err(CraftFitError::InvalidInput);
    }
    let rows = samples.len() / 2;
    let cols = samples.len() - rows;
    let h0 = DMatrix::<Complex64>::from_fn(rows, cols, |row, col| samples[row + col]);
    let h1 = DMatrix::<Complex64>::from_fn(rows, cols, |row, col| samples[row + col + 1]);
    let svd = h0.svd(true, true);
    let u = svd
        .u
        .ok_or(CraftFitError::Singular)?
        .columns(0, order)
        .into_owned();
    let v = svd
        .v_t
        .ok_or(CraftFitError::Singular)?
        .adjoint()
        .columns(0, order)
        .into_owned();
    let largest = svd.singular_values[0];
    let mut inverse = DMatrix::<Complex64>::zeros(order, order);
    for index in 0..order {
        let singular = svd.singular_values[index];
        if !singular.is_finite() || singular <= largest * 1e-12 {
            return Err(CraftFitError::Singular);
        }
        inverse[(index, index)] = Complex64::new(1.0 / singular, 0.0);
    }
    let reduced = u.adjoint() * h1 * v * inverse;
    let eigenvalues = reduced.eigenvalues().ok_or(CraftFitError::Singular)?;
    let mut estimates = eigenvalues
        .iter()
        .filter_map(|value| {
            let magnitude = value.norm();
            if !magnitude.is_finite() || magnitude <= 0.0 || magnitude >= 1.0 {
                return None;
            }
            let frequency_hz = value.arg() / (TAU * dwell_s);
            let linewidth_hz = -magnitude.ln() / (PI * dwell_s);
            (frequency_hz >= bounds.frequency_hz.0
                && frequency_hz <= bounds.frequency_hz.1
                && linewidth_hz >= bounds.linewidth_hz.0
                && linewidth_hz <= bounds.linewidth_hz.1)
                .then_some(DampedSinusoidEstimate {
                    frequency_hz,
                    linewidth_hz,
                })
        })
        .collect::<Vec<_>>();
    estimates.sort_by(|left, right| left.frequency_hz.total_cmp(&right.frequency_hz));
    let times = (0..samples.len())
        .map(|index| index as f64 * dwell_s)
        .collect::<Vec<_>>();
    let frequencies = estimates
        .iter()
        .map(|estimate| estimate.frequency_hz)
        .collect::<Vec<_>>();
    let decay_rates = estimates
        .iter()
        .map(|estimate| PI * estimate.linewidth_hz)
        .collect::<Vec<_>>();
    if estimates.is_empty() {
        return Err(CraftFitError::Singular);
    }
    let coefficients = solve_complex_amplitudes(samples, &times, &frequencies, &decay_rates)
        .ok_or(CraftFitError::Singular)?;
    let rss = samples
        .iter()
        .zip(&times)
        .map(|(&sample, &time)| {
            let predicted = coefficients
                .iter()
                .zip(&frequencies)
                .zip(&decay_rates)
                .fold(
                    Complex64::new(0.0, 0.0),
                    |sum, ((coefficient, frequency), decay)| {
                        sum + coefficient
                            * Complex64::from_polar((-decay * time).exp(), TAU * frequency * time)
                    },
                );
            (sample - predicted).norm_sqr()
        })
        .sum();
    Ok(MatrixPencilEstimate {
        components: estimates,
        rss,
    })
}

#[derive(Clone, Copy)]
struct DecodedComponent {
    coefficient: Complex64,
    frequency_hz: f64,
    decay_rate: f64,
    frequency_scale: f64,
    linewidth_scale: f64,
}

struct CraftProblem<'a> {
    samples: &'a [Complex64],
    times_s: &'a [f64],
    bounds: CraftFitBounds,
    derivatives: Vec<Complex64>,
}

impl CraftProblem<'_> {
    fn decode(&self, params: &[f64]) -> Vec<DecodedComponent> {
        params
            .as_chunks::<4>()
            .0
            .iter()
            .map(|chunk| {
                let (frequency_hz, frequency_scale) =
                    from_unbounded(chunk[2], self.bounds.frequency_hz);
                let (linewidth_hz, linewidth_scale) =
                    from_unbounded(chunk[3], self.bounds.linewidth_hz);
                DecodedComponent {
                    coefficient: Complex64::new(chunk[0], chunk[1]),
                    frequency_hz,
                    decay_rate: PI * linewidth_hz,
                    frequency_scale,
                    linewidth_scale: PI * linewidth_scale,
                }
            })
            .collect()
    }
}

impl LmProblem for CraftProblem<'_> {
    fn cost(&mut self, params: &[f64]) -> f64 {
        let components = self.decode(params);
        residuals(self.samples, self.times_s, &components)
            .iter()
            .map(Complex64::norm_sqr)
            .sum()
    }

    #[allow(clippy::needless_range_loop)]
    fn normal_equations(&mut self, params: &[f64], jtj: &mut [Vec<f64>], jtr: &mut [f64]) {
        for row in jtj.iter_mut() {
            row.fill(0.0);
        }
        jtr.fill(0.0);
        let components = self.decode(params);
        for (&sample, &time) in self.samples.iter().zip(self.times_s) {
            let mut predicted = Complex64::new(0.0, 0.0);
            for (index, component) in components.iter().enumerate() {
                let basis = Complex64::from_polar(
                    (-component.decay_rate * time).exp(),
                    TAU * component.frequency_hz * time,
                );
                predicted += component.coefficient * basis;
                let offset = index * 4;
                self.derivatives[offset] = basis;
                self.derivatives[offset + 1] = Complex64::new(-basis.im, basis.re);
                self.derivatives[offset + 2] = component.coefficient
                    * basis
                    * Complex64::new(0.0, TAU * time * component.frequency_scale);
                self.derivatives[offset + 3] =
                    component.coefficient * basis * (-time * component.linewidth_scale);
            }
            let residual = sample - predicted;
            for a in 0..params.len() {
                let ja = self.derivatives[a];
                jtr[a] += ja.re * residual.re + ja.im * residual.im;
                for b in a..params.len() {
                    let jb = self.derivatives[b];
                    jtj[a][b] += ja.re * jb.re + ja.im * jb.im;
                }
            }
        }
        for a in 1..params.len() {
            for b in 0..a {
                jtj[a][b] = jtj[b][a];
            }
        }
    }
}

fn solve_complex_amplitudes(
    samples: &[Complex64],
    times_s: &[f64],
    frequencies_hz: &[f64],
    decay_rates: &[f64],
) -> Option<Vec<Complex64>> {
    let rows = samples.len() * 2;
    let cols = frequencies_hz.len() * 2;
    let mut design = DMatrix::<f64>::zeros(rows, cols);
    let mut observed = DVector::<f64>::zeros(rows);
    for (row, (&sample, &time)) in samples.iter().zip(times_s).enumerate() {
        observed[row * 2] = sample.re;
        observed[row * 2 + 1] = sample.im;
        for component in 0..frequencies_hz.len() {
            let basis = Complex64::from_polar(
                (-decay_rates[component] * time).exp(),
                TAU * frequencies_hz[component] * time,
            );
            design[(row * 2, component * 2)] = basis.re;
            design[(row * 2, component * 2 + 1)] = -basis.im;
            design[(row * 2 + 1, component * 2)] = basis.im;
            design[(row * 2 + 1, component * 2 + 1)] = basis.re;
        }
    }
    // nalgebra's direct QR solve is square-only. Thin SVD handles the tall
    // least-squares system and gives the rank tolerance required for strongly
    // overlapping components without forming ill-conditioned normal equations.
    let solution = design.svd(true, true).solve(&observed, 1e-12).ok()?;
    Some(
        solution
            .as_slice()
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| Complex64::new(pair[0], pair[1]))
            .collect(),
    )
}

fn residuals(
    samples: &[Complex64],
    times_s: &[f64],
    components: &[DecodedComponent],
) -> Vec<Complex64> {
    samples
        .iter()
        .zip(times_s)
        .map(|(&sample, &time)| {
            let predicted = components
                .iter()
                .fold(Complex64::new(0.0, 0.0), |sum, component| {
                    sum + component.coefficient
                        * Complex64::from_polar(
                            (-component.decay_rate * time).exp(),
                            TAU * component.frequency_hz * time,
                        )
                });
            sample - predicted
        })
        .collect()
}

fn component_with_uncertainty(
    component: &DecodedComponent,
    index: usize,
    covariance: Option<&[Vec<f64>]>,
    bounds: CraftFitBounds,
) -> DampedSinusoid {
    let amplitude = component.coefficient.norm();
    let offset = index * 4;
    let variance = |parameter: usize| {
        covariance
            .and_then(|matrix| matrix.get(parameter))
            .and_then(|row| row.get(parameter))
            .copied()
            .filter(|value| value.is_finite())
            .map(|value| value.max(0.0))
    };
    let (re, im) = (component.coefficient.re, component.coefficient.im);
    let amplitude_std = if amplitude > 0.0 {
        let covariance_re_im = covariance
            .and_then(|matrix| matrix.get(offset))
            .and_then(|row| row.get(offset + 1))
            .copied()
            .filter(|value| value.is_finite());
        variance(offset)
            .zip(variance(offset + 1))
            .zip(covariance_re_im)
            .map(|((variance_re, variance_im), covariance_re_im)| {
                ((re * re * variance_re + im * im * variance_im + 2.0 * re * im * covariance_re_im)
                    / amplitude.powi(2))
                .max(0.0)
                .sqrt()
            })
    } else {
        None
    };
    let phase_std_rad = if amplitude > 0.0 {
        let covariance_re_im = covariance
            .and_then(|matrix| matrix.get(offset))
            .and_then(|row| row.get(offset + 1))
            .copied()
            .filter(|value| value.is_finite());
        variance(offset)
            .zip(variance(offset + 1))
            .zip(covariance_re_im)
            .map(|((variance_re, variance_im), covariance_re_im)| {
                ((im * im * variance_re + re * re * variance_im - 2.0 * re * im * covariance_re_im)
                    / amplitude.powi(4))
                .max(0.0)
                .sqrt()
            })
    } else {
        None
    };
    let frequency_std_hz =
        variance(offset + 2).map(|variance| variance.sqrt() * component.frequency_scale);
    let linewidth_scale = component.linewidth_scale / PI;
    let linewidth_std_hz = variance(offset + 3).map(|variance| variance.sqrt() * linewidth_scale);
    DampedSinusoid {
        frequency_hz: component
            .frequency_hz
            .clamp(bounds.frequency_hz.0, bounds.frequency_hz.1),
        amplitude,
        phase_rad: component.coefficient.arg(),
        decay_rate_s_inv: component.decay_rate,
        linewidth_hz: component.decay_rate / PI,
        amplitude_std,
        frequency_std_hz,
        linewidth_std_hz,
        phase_std_rad,
    }
}

fn design_condition_number(times_s: &[f64], components: &[DecodedComponent]) -> f64 {
    if components.is_empty() {
        return 1.0;
    }
    let design = DMatrix::from_fn(times_s.len() * 2, components.len() * 2, |row, col| {
        let component = components[col / 2];
        let time = times_s[row / 2];
        let basis = Complex64::from_polar(
            (-component.decay_rate * time).exp(),
            TAU * component.frequency_hz * time,
        );
        match (row % 2, col % 2) {
            (0, 0) => basis.re,
            (0, 1) => -basis.im,
            (1, 0) => basis.im,
            (1, 1) => basis.re,
            _ => unreachable!(),
        }
    });
    let columns = design.ncols();
    let singular = design.svd(false, false).singular_values;
    if singular.len() < columns {
        return f64::INFINITY;
    }
    let largest = singular.iter().copied().fold(0.0_f64, f64::max);
    let smallest = singular.iter().copied().fold(f64::INFINITY, f64::min);
    // Use the same relative rank tolerance as the amplitude SVD. A discarded
    // singular direction is evidence of rank deficiency, not a reason to
    // report the condition number of the remaining subspace.
    if largest.is_finite() && largest > 0.0 && smallest.is_finite() && smallest > largest * 1e-12 {
        largest / smallest
    } else {
        f64::INFINITY
    }
}

fn to_unbounded(value: f64, bounds: (f64, f64)) -> f64 {
    let ratio = ((value - bounds.0) / (bounds.1 - bounds.0)).clamp(1e-9, 1.0 - 1e-9);
    (ratio / (1.0 - ratio)).ln()
}

fn from_unbounded(value: f64, bounds: (f64, f64)) -> (f64, f64) {
    let sigmoid = if value >= 0.0 {
        1.0 / (1.0 + (-value).exp())
    } else {
        let exp = value.exp();
        exp / (1.0 + exp)
    };
    let span = bounds.1 - bounds.0;
    (bounds.0 + span * sigmoid, span * sigmoid * (1.0 - sigmoid))
}

#[cfg(test)]
#[path = "craft_tests.rs"]
mod tests;
