use super::engine::*;
use super::*;
use crate::fit::{jac_step, solve_linear};
use crate::fit_model::Expression;

#[allow(clippy::too_many_arguments)]
pub(super) fn assemble_result(
    definition: FitModelDefinition,
    datasets: Vec<FitDataset>,
    options: FitOptions,
    compiled: CompiledModel,
    rows: Vec<PreparedRow>,
    slots: Vec<Slot>,
    values: Vec<f64>,
    attempts: Vec<FitAttempt>,
    iterations: usize,
    excluded: usize,
    mut notices: Vec<String>,
    weight_expression: Option<&Expression>,
) -> Result<FitResult, FitError> {
    let residuals = residual_vector(
        &compiled,
        &definition,
        &datasets,
        &rows,
        &slots,
        &values,
        &options,
        weight_expression,
    )?;
    let jacobian = numeric_jacobian(
        &compiled,
        &definition,
        &datasets,
        &rows,
        &slots,
        &values,
        &residuals,
        &options,
        weight_expression,
    )?;
    let (jtj, _) = normal_equations(&jacobian, &residuals);
    let degrees = residuals.len() as isize - slots.len() as isize;
    let chi_squared = residuals.iter().map(|v| v * v).sum::<f64>();
    let variance = if degrees > 0 {
        chi_squared / degrees as f64
    } else {
        f64::NAN
    };
    let inverse_jtj = inverse(&jtj);
    let covariance_available = inverse_jtj.is_some();
    let covariance = inverse_jtj
        .map(|mut matrix| {
            for row in &mut matrix {
                for value in row {
                    *value *= variance;
                }
            }
            matrix
        })
        .unwrap_or_else(|| vec![vec![0.0; slots.len()]; slots.len()]);
    if !covariance_available {
        notices.push("Covariance is unavailable because the final Jacobian is singular.".into());
    }
    let correlation = correlation_matrix(&covariance);
    let parameters = parameter_estimates(
        &compiled,
        &definition,
        &datasets,
        &slots,
        &values,
        &covariance,
    )?;
    let mut points = Vec::new();
    for row in &rows {
        let environment = environment_for(
            &definition,
            &datasets,
            &slots,
            &values,
            row.dataset,
            &row.environment,
            &compiled,
        )?;
        let predicted = predict_row(
            &compiled,
            &definition,
            &environment,
            &row.observed,
            &options,
        )
        .map_err(|error| FitError::Evaluation {
            dataset: datasets[row.dataset].id.clone(),
            row: row.row,
            reason: error.to_string(),
        })?;
        for response in &definition.responses {
            let observed = row.observed[&response.id];
            let prediction = predicted[&response.id];
            let raw = observed - prediction;
            let weight = point_weight(
                &options.weights,
                observed,
                prediction,
                row.sigmas.get(&response.id).copied(),
                raw,
                weight_expression,
            )?;
            points.push(PointResult {
                dataset_id: datasets[row.dataset].id.clone(),
                response: response.id.clone(),
                row: row.row,
                observed,
                predicted: prediction,
                residual: raw,
                weight,
            });
        }
    }
    let response_statistics = response_statistics(&points, &slots, &datasets);
    let n = residuals.len() as f64;
    let k = slots.len() as f64;
    let log_likelihood_term = n * (chi_squared / n).max(f64::MIN_POSITIVE).ln();
    let aic = log_likelihood_term + 2.0 * k;
    let aicc = if n > k + 1.0 {
        Some(aic + 2.0 * k * (k + 1.0) / (n - k - 1.0))
    } else {
        None
    };
    let bic = log_likelihood_term + k * n.ln();
    let observed_mean =
        points.iter().map(|point| point.observed).sum::<f64>() / points.len() as f64;
    let total_sum = points
        .iter()
        .map(|point| (point.observed - observed_mean).powi(2))
        .sum::<f64>();
    let residual_sum = points
        .iter()
        .map(|point| point.residual.powi(2))
        .sum::<f64>();
    let r_squared = if total_sum > 0.0 {
        1.0 - residual_sum / total_sum
    } else {
        0.0
    };
    let statistics = FitStatistics {
        observations: residuals.len(),
        free_parameters: slots.len(),
        degrees_of_freedom: degrees,
        chi_squared,
        reduced_chi_squared: variance,
        r_squared,
        aic,
        aicc,
        bic,
        responses: response_statistics,
    };
    let derived = derived_estimates(
        &compiled,
        &definition,
        &datasets,
        &slots,
        &values,
        &covariance,
    )?;
    Ok(FitResult {
        model: definition,
        datasets,
        options,
        parameters,
        derived,
        covariance,
        correlation,
        points,
        statistics,
        attempts,
        iterations,
        converged: true,
        excluded_rows: excluded,
        notices,
    })
}

fn parameter_estimates(
    compiled: &CompiledModel,
    definition: &FitModelDefinition,
    datasets: &[FitDataset],
    slots: &[Slot],
    values: &[f64],
    covariance: &[Vec<f64>],
) -> Result<Vec<ParameterEstimate>, FitError> {
    let mut estimates: Vec<ParameterEstimate> = slots
        .iter()
        .enumerate()
        .map(|(index, slot)| {
            let parameter = &definition.parameters[slot.parameter];
            ParameterEstimate {
                parameter: parameter.id.clone(),
                dataset_id: slot.dataset.map(|di| datasets[di].id.clone()),
                value: values[index],
                standard_error: covariance[index][index].max(0.0).sqrt(),
                initial_value: slot.initial,
                lower_bound: slot.lower.is_finite().then_some(slot.lower),
                upper_bound: slot.upper.is_finite().then_some(slot.upper),
                mode: ParameterMode::Free,
            }
        })
        .collect();
    for parameter in definition
        .parameters
        .iter()
        .filter(|parameter| !matches!(parameter.mode, ParameterMode::Free))
    {
        let dataset_indices: Vec<Option<usize>> = if parameter.sharing == ParameterSharing::Shared {
            vec![None]
        } else {
            (0..datasets.len()).map(Some).collect()
        };
        for dataset_index in dataset_indices {
            let di = dataset_index.unwrap_or(0);
            let environment = environment_for(
                definition,
                datasets,
                slots,
                values,
                di,
                &datasets[di].constants,
                compiled,
            )?;
            let value =
                *environment
                    .get(&parameter.id)
                    .ok_or_else(|| FitError::InvalidInitialValue {
                        parameter: parameter.id.clone(),
                        reason: "value could not be evaluated from the fit data".into(),
                    })?;
            let standard_error = if matches!(parameter.mode, ParameterMode::Fixed) {
                0.0
            } else {
                let mut gradient = vec![0.0; slots.len()];
                for slot_index in 0..slots.len() {
                    if slots[slot_index]
                        .dataset
                        .is_some_and(|dataset| dataset != di)
                    {
                        continue;
                    }
                    let h = jac_step(values[slot_index]);
                    let mut plus = values.to_vec();
                    let mut minus = values.to_vec();
                    plus[slot_index] += h;
                    minus[slot_index] -= h;
                    let plus_value = environment_for(
                        definition,
                        datasets,
                        slots,
                        &plus,
                        di,
                        &datasets[di].constants,
                        compiled,
                    )?[&parameter.id];
                    let minus_value = environment_for(
                        definition,
                        datasets,
                        slots,
                        &minus,
                        di,
                        &datasets[di].constants,
                        compiled,
                    )?[&parameter.id];
                    gradient[slot_index] = (plus_value - minus_value) / (2.0 * h);
                }
                let mut variance = 0.0;
                for a in 0..slots.len() {
                    for b in 0..slots.len() {
                        variance += gradient[a] * covariance[a][b] * gradient[b];
                    }
                }
                variance.max(0.0).sqrt()
            };
            let initial = initial_value(&parameter.initial, Some(&datasets[di])).unwrap_or(value);
            estimates.push(ParameterEstimate {
                parameter: parameter.id.clone(),
                dataset_id: dataset_index.map(|index| datasets[index].id.clone()),
                value,
                standard_error,
                initial_value: initial,
                lower_bound: parameter.lower_bound,
                upper_bound: parameter.upper_bound,
                mode: parameter.mode.clone(),
            });
        }
    }
    Ok(estimates)
}

fn inverse(matrix: &[Vec<f64>]) -> Option<Vec<Vec<f64>>> {
    let n = matrix.len();
    let mut inverse = vec![vec![0.0; n]; n];
    for column in 0..n {
        let mut unit = vec![0.0; n];
        unit[column] = 1.0;
        let result = solve_linear(matrix, &unit)?;
        for row in 0..n {
            inverse[row][column] = result[row];
        }
    }
    Some(inverse)
}
fn correlation_matrix(covariance: &[Vec<f64>]) -> Vec<Vec<f64>> {
    (0..covariance.len())
        .map(|a| {
            (0..covariance.len())
                .map(|b| {
                    let denominator = (covariance[a][a] * covariance[b][b]).sqrt();
                    if denominator > 0.0 {
                        covariance[a][b] / denominator
                    } else if a == b {
                        1.0
                    } else {
                        0.0
                    }
                })
                .collect()
        })
        .collect()
}

fn response_statistics(
    points: &[PointResult],
    slots: &[Slot],
    datasets: &[FitDataset],
) -> Vec<ResponseStatistics> {
    // Each group's degrees of freedom subtract only the parameters that act on
    // its dataset: the shared slots plus that dataset's own slots.
    let shared = slots.iter().filter(|slot| slot.dataset.is_none()).count();
    let per_dataset: BTreeMap<&str, usize> = datasets
        .iter()
        .enumerate()
        .map(|(di, dataset)| {
            let own = slots.iter().filter(|slot| slot.dataset == Some(di)).count();
            (dataset.id.as_str(), shared + own)
        })
        .collect();
    let mut groups: BTreeMap<(&str, &str), Vec<&PointResult>> = BTreeMap::new();
    for point in points {
        groups
            .entry((&point.dataset_id, &point.response))
            .or_default()
            .push(point);
    }
    groups
        .into_iter()
        .map(|((dataset, response), points)| {
            let mean = points.iter().map(|p| p.observed).sum::<f64>() / points.len() as f64;
            let chi = points
                .iter()
                .map(|p| p.residual * p.residual * p.weight)
                .sum::<f64>();
            let total = points
                .iter()
                .map(|p| (p.observed - mean).powi(2))
                .sum::<f64>();
            let parameter_count = per_dataset.get(dataset).copied().unwrap_or(slots.len());
            let dof = points.len().saturating_sub(parameter_count).max(1);
            ResponseStatistics {
                dataset_id: dataset.into(),
                response: response.into(),
                points: points.len(),
                chi_squared: chi,
                reduced_chi_squared: chi / dof as f64,
                r_squared: if total > 0.0 {
                    1.0 - points.iter().map(|p| p.residual.powi(2)).sum::<f64>() / total
                } else {
                    0.0
                },
            }
        })
        .collect()
}

fn derived_estimates(
    compiled: &CompiledModel,
    definition: &FitModelDefinition,
    datasets: &[FitDataset],
    slots: &[Slot],
    values: &[f64],
    covariance: &[Vec<f64>],
) -> Result<Vec<DerivedEstimate>, FitError> {
    let mut output = Vec::new();
    for (di, dataset) in datasets.iter().enumerate() {
        let environment = environment_for(
            definition,
            datasets,
            slots,
            values,
            di,
            &dataset.constants,
            compiled,
        )?;
        let derived = compiled
            .evaluate_derived(&environment)
            .map_err(|error| FitError::InvalidModel(error.to_string()))?;
        for (name, value) in derived {
            let mut gradient = vec![0.0; slots.len()];
            for slot_index in 0..slots.len() {
                if slots[slot_index]
                    .dataset
                    .is_some_and(|dataset| dataset != di)
                {
                    continue;
                }
                let h = jac_step(values[slot_index]);
                let mut plus = values.to_vec();
                let mut minus = values.to_vec();
                plus[slot_index] += h;
                minus[slot_index] -= h;
                let plus_environment = environment_for(
                    definition,
                    datasets,
                    slots,
                    &plus,
                    di,
                    &dataset.constants,
                    compiled,
                )?;
                let minus_environment = environment_for(
                    definition,
                    datasets,
                    slots,
                    &minus,
                    di,
                    &dataset.constants,
                    compiled,
                )?;
                let plus_value = compiled
                    .evaluate_derived(&plus_environment)
                    .map_err(|error| FitError::InvalidModel(error.to_string()))?[&name];
                let minus_value = compiled
                    .evaluate_derived(&minus_environment)
                    .map_err(|error| FitError::InvalidModel(error.to_string()))?[&name];
                gradient[slot_index] = (plus_value - minus_value) / (2.0 * h);
            }
            let mut variance = 0.0;
            for a in 0..slots.len() {
                for b in 0..slots.len() {
                    variance += gradient[a] * covariance[a][b] * gradient[b];
                }
            }
            output.push(DerivedEstimate {
                name,
                dataset_id: dataset.id.clone(),
                value,
                standard_error: variance.max(0.0).sqrt(),
            });
        }
    }
    Ok(output)
}
