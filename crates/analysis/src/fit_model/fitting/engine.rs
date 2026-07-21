use super::solvers::{integrate_ode, solve_implicit};
use super::*;
use crate::fit::{jac_step, solve_linear};
use crate::fit_model::{ConstantDefinition, Expression};

pub(super) fn dataset_constant(
    dataset: &FitDataset,
    constant: &ConstantDefinition,
) -> Result<f64, FitError> {
    dataset
        .constants
        .get(&constant.id)
        .copied()
        .or(constant.default_value)
        .ok_or_else(|| {
            FitError::InvalidData(format!(
                "dataset '{}' is missing required constant '{}'",
                dataset.id, constant.id
            ))
        })
}

#[allow(clippy::too_many_arguments)]
pub(super) fn optimize(
    compiled: &CompiledModel,
    definition: &FitModelDefinition,
    datasets: &[FitDataset],
    rows: &[PreparedRow],
    slots: &[Slot],
    mut parameters: Vec<f64>,
    options: &FitOptions,
    weight_expression: Option<&Expression>,
    cancelled: &impl Fn() -> bool,
) -> Result<(Vec<f64>, f64, usize, bool, String), FitError> {
    let mut residuals = residual_vector(
        compiled,
        definition,
        datasets,
        rows,
        slots,
        &parameters,
        options,
        weight_expression,
    )?;
    let mut cost = residuals.iter().map(|value| value * value).sum::<f64>();
    let mut lambda = 1e-3;
    for iteration in 0..options.max_iterations {
        if cancelled() {
            return Err(FitError::Cancelled);
        }
        let jacobian = numeric_jacobian(
            compiled,
            definition,
            datasets,
            rows,
            slots,
            &parameters,
            &residuals,
            options,
            weight_expression,
        )?;
        let (mut jtj, mut jtr) = normal_equations(&jacobian, &residuals);
        for index in 0..slots.len() {
            jtj[index][index] += lambda * jtj[index][index].max(1e-12);
            jtr[index] = -jtr[index];
        }
        let Some(step) = solve_linear(&jtj, &jtr) else {
            return Ok((
                parameters,
                cost,
                iteration,
                false,
                "singular normal equations".into(),
            ));
        };
        let trial: Vec<f64> = parameters
            .iter()
            .zip(&step)
            .zip(slots)
            .map(|((&value, &change), slot)| (value + change).clamp(slot.lower, slot.upper))
            .collect();
        // A trial point outside the model's domain is a rejected step, not a
        // fatal error: shrink the trust region and try again from the current
        // (valid) parameters.
        let trial_residuals = match residual_vector(
            compiled,
            definition,
            datasets,
            rows,
            slots,
            &trial,
            options,
            weight_expression,
        ) {
            Ok(values) => values,
            Err(_) => {
                lambda *= 10.0;
                if lambda > 1e16 {
                    return Ok((
                        parameters,
                        cost,
                        iteration + 1,
                        false,
                        "trust region collapsed".into(),
                    ));
                }
                continue;
            }
        };
        let trial_cost = trial_residuals
            .iter()
            .map(|value| value * value)
            .sum::<f64>();
        if trial_cost < cost {
            let relative = (cost - trial_cost) / cost.max(f64::MIN_POSITIVE);
            parameters = trial;
            residuals = trial_residuals;
            cost = trial_cost;
            lambda = (lambda * 0.35).max(1e-12);
            if relative < options.tolerance {
                return Ok((
                    parameters,
                    cost,
                    iteration + 1,
                    true,
                    "relative cost tolerance reached".into(),
                ));
            }
        } else {
            lambda *= 10.0;
            if lambda > 1e16 {
                return Ok((
                    parameters,
                    cost,
                    iteration + 1,
                    false,
                    "trust region collapsed".into(),
                ));
            }
        }
    }
    Ok((
        parameters,
        cost,
        options.max_iterations,
        false,
        "iteration limit reached".into(),
    ))
}

#[allow(clippy::too_many_arguments)]
pub(super) fn residual_vector(
    compiled: &CompiledModel,
    definition: &FitModelDefinition,
    datasets: &[FitDataset],
    rows: &[PreparedRow],
    slots: &[Slot],
    values: &[f64],
    options: &FitOptions,
    weight_expression: Option<&Expression>,
) -> Result<Vec<f64>, FitError> {
    let mut output = Vec::new();
    for row in rows {
        let environment = environment_for(
            definition,
            datasets,
            slots,
            values,
            row.dataset,
            &row.environment,
            compiled,
        )?;
        let predicted = predict_row(compiled, definition, &environment, &row.observed, options)
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
            output.push(robust_residual(raw * weight.sqrt(), options.robust_loss));
        }
    }
    Ok(output)
}

pub(super) fn environment_for(
    definition: &FitModelDefinition,
    datasets: &[FitDataset],
    slots: &[Slot],
    values: &[f64],
    dataset: usize,
    base: &BTreeMap<String, f64>,
    compiled: &CompiledModel,
) -> Result<BTreeMap<String, f64>, FitError> {
    let mut environment = base.clone();
    // Post-fit callers pass a base without the declared constants, so fill the
    // per-dataset value or default for any constant the base does not carry.
    for constant in &definition.constants {
        if !environment.contains_key(&constant.id) {
            let value = dataset_constant(&datasets[dataset], constant)?;
            environment.insert(constant.id.clone(), value);
        }
    }
    for (pi, parameter) in definition.parameters.iter().enumerate() {
        // Constrained parameters must come from `apply_constraints` below;
        // pre-seeding them with initial values would satisfy the dependency
        // check in `evaluate_ordered` and let chains read stale values.
        if matches!(parameter.mode, ParameterMode::Constrained(_)) {
            continue;
        }
        let value = slots
            .iter()
            .enumerate()
            .find(|(_, slot)| {
                slot.parameter == pi && (slot.dataset.is_none() || slot.dataset == Some(dataset))
            })
            .map(|(index, _)| values[index]);
        let value = value.or_else(|| match parameter.initial {
            InitialValueRule::Value(value) => Some(value),
            _ => initial_value(&parameter.initial, Some(&datasets[dataset])),
        });
        if let Some(value) = value {
            environment.insert(parameter.id.clone(), value);
        }
    }
    compiled
        .apply_constraints(&mut environment)
        .map_err(|error| FitError::InvalidModel(error.to_string()))?;
    Ok(environment)
}

pub(super) fn predict_row(
    compiled: &CompiledModel,
    definition: &FitModelDefinition,
    environment: &BTreeMap<String, f64>,
    observed: &BTreeMap<String, f64>,
    options: &FitOptions,
) -> Result<BTreeMap<String, f64>, EvaluationError> {
    match &definition.kind {
        FitModelKind::Explicit { .. } => compiled.evaluate_explicit(environment),
        FitModelKind::Implicit { .. } => {
            solve_implicit(compiled, definition, environment, observed)
        }
        FitModelKind::OdeSystem { independent, .. } => integrate_ode(
            compiled,
            definition,
            environment,
            environment[independent],
            options,
        ),
    }
}

pub(super) fn point_weight(
    mode: &WeightMode,
    observed: f64,
    predicted: f64,
    sigma: Option<f64>,
    residual: f64,
    expression: Option<&Expression>,
) -> Result<f64, FitError> {
    let valid_sigma = sigma.filter(|value| value.is_finite() && *value > 0.0);
    let weight = match mode {
        WeightMode::Auto => valid_sigma.map_or(1.0, |value| 1.0 / (value * value)),
        WeightMode::Equal => 1.0,
        WeightMode::MeasurementSigma => {
            1.0 / valid_sigma
                .ok_or_else(|| {
                    FitError::InvalidData(
                        "sigma weighting requires a positive finite sigma for every point".into(),
                    )
                })?
                .powi(2)
        }
        WeightMode::Relative => {
            // observed² must stay a normal float or the reciprocal overflows
            // to infinity (MIN_POSITIVE² underflows to zero).
            let denominator = observed * observed;
            if !denominator.is_normal() {
                return Err(FitError::InvalidData(
                    "relative weighting requires every observed value to be nonzero".into(),
                ));
            }
            1.0 / denominator
        }
        WeightMode::Poisson => 1.0 / predicted.abs().max(1.0),
        WeightMode::Expression(_) => expression
            .unwrap()
            .evaluate(&BTreeMap::from([
                ("observed".into(), observed),
                ("predicted".into(), predicted),
                ("residual".into(), residual),
                ("sigma".into(), sigma.unwrap_or(f64::NAN)),
            ]))
            .map_err(|error| FitError::InvalidModel(error.to_string()))?,
    };
    if !weight.is_finite() || weight <= 0.0 {
        return Err(FitError::InvalidData(
            "residual weight must be positive and finite".into(),
        ));
    }
    Ok(weight)
}

pub(super) fn robust_residual(value: f64, loss: RobustLoss) -> f64 {
    let (magnitude, sign) = (value.abs(), value.signum());
    sign * match loss {
        RobustLoss::None => magnitude,
        RobustLoss::Huber(scale) => {
            if magnitude <= scale {
                magnitude
            } else {
                (2.0 * scale * magnitude - scale * scale).sqrt()
            }
        }
        RobustLoss::SoftL1(scale) => {
            (2.0 * scale * scale * ((1.0 + (magnitude / scale).powi(2)).sqrt() - 1.0)).sqrt()
        }
        RobustLoss::Cauchy(scale) => {
            (scale * scale * (1.0 + (magnitude / scale).powi(2)).ln()).sqrt()
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn numeric_jacobian(
    compiled: &CompiledModel,
    definition: &FitModelDefinition,
    datasets: &[FitDataset],
    rows: &[PreparedRow],
    slots: &[Slot],
    values: &[f64],
    residuals: &[f64],
    options: &FitOptions,
    weight_expression: Option<&Expression>,
) -> Result<Vec<Vec<f64>>, FitError> {
    let mut jacobian = vec![vec![0.0; slots.len()]; residuals.len()];
    let mut shifted = values.to_vec();
    for column in 0..slots.len() {
        let h = jac_step(values[column]);
        shifted[column] = (values[column] + h).min(slots[column].upper);
        let plus = residual_vector(
            compiled,
            definition,
            datasets,
            rows,
            slots,
            &shifted,
            options,
            weight_expression,
        )?;
        shifted[column] = (values[column] - h).max(slots[column].lower);
        let minus = residual_vector(
            compiled,
            definition,
            datasets,
            rows,
            slots,
            &shifted,
            options,
            weight_expression,
        )?;
        let denominator = ((values[column] + h).min(slots[column].upper)
            - (values[column] - h).max(slots[column].lower))
        .max(f64::MIN_POSITIVE);
        shifted[column] = values[column];
        for row in 0..residuals.len() {
            jacobian[row][column] = (plus[row] - minus[row]) / denominator;
        }
    }
    Ok(jacobian)
}

pub(super) fn normal_equations(
    jacobian: &[Vec<f64>],
    residuals: &[f64],
) -> (Vec<Vec<f64>>, Vec<f64>) {
    let width = jacobian.first().map_or(0, Vec::len);
    let mut jtj = vec![vec![0.0; width]; width];
    let mut jtr = vec![0.0; width];
    for (row, residual) in jacobian.iter().zip(residuals) {
        for a in 0..width {
            jtr[a] += row[a] * residual;
            for b in 0..width {
                jtj[a][b] += row[a] * row[b];
            }
        }
    }
    (jtj, jtr)
}
