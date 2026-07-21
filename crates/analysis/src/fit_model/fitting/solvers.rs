use super::*;
use crate::fit::{jac_step, solve_linear};

pub(super) fn integrate_ode(
    compiled: &CompiledModel,
    definition: &FitModelDefinition,
    environment: &BTreeMap<String, f64>,
    target: f64,
    options: &FitOptions,
) -> Result<BTreeMap<String, f64>, EvaluationError> {
    let FitModelKind::OdeSystem {
        independent,
        initial_conditions,
        ..
    } = &definition.kind
    else {
        return Err(EvaluationError::WrongModelKind);
    };
    let mut base = environment.clone();
    let mut initial_time = None;
    let mut states = BTreeMap::new();
    for condition in initial_conditions {
        let at = parse_expression(&condition.at)?.evaluate(&base)?;
        if initial_time.is_some_and(|value: f64| (value - at).abs() > 1e-12) {
            return Err(EvaluationError::NonFinite {
                output: "ODE initial times disagree".into(),
            });
        }
        initial_time = Some(at);
        base.insert(independent.clone(), at);
        states.insert(
            condition.state.clone(),
            parse_expression(&condition.value)?.evaluate(&base)?,
        );
    }
    let mut time =
        initial_time.ok_or_else(|| EvaluationError::MissingSymbol("initial condition".into()))?;
    if time == target {
        return Ok(states);
    }
    let direction = (target - time).signum();
    let span = (target - time).abs();
    let mut step = direction
        * (span / 20.0)
            .max(options.absolute_tolerance.sqrt())
            .min(span);
    let mut steps = 0;
    while direction * (target - time) > 0.0 {
        steps += 1;
        if steps > 100_000 {
            return Err(EvaluationError::NonFinite {
                output: "ODE step resource limit reached".into(),
            });
        }
        if direction * (time + step - target) > 0.0 {
            step = target - time;
        }
        let attempt = match options.ode_solver {
            OdeSolver::Bdf => bdf_step(compiled, independent, environment, time, &states, step),
            OdeSolver::Auto | OdeSolver::DormandPrince45 => adaptive_rk_step(
                compiled,
                independent,
                environment,
                time,
                &states,
                step,
                options,
            ),
        };
        match attempt {
            Ok((next, error)) => {
                if error <= 1.0 || matches!(options.ode_solver, OdeSolver::Bdf) {
                    states = next;
                    time += step;
                    let factor = if error > 0.0 {
                        (0.9 * error.powf(-0.2)).clamp(0.2, 4.0)
                    } else {
                        2.0
                    };
                    step *= factor;
                } else {
                    step *= (0.9 * error.powf(-0.2)).clamp(0.1, 0.5);
                }
            }
            Err(_error) if options.ode_solver == OdeSolver::Auto => {
                let (next, _) = bdf_step(compiled, independent, environment, time, &states, step)?;
                states = next;
                time += step;
            }
            Err(error) => return Err(error),
        }
        if step.abs() < f64::EPSILON * time.abs().max(1.0) {
            return Err(EvaluationError::NonFinite {
                output: "ODE step underflow".into(),
            });
        }
    }
    Ok(states)
}

fn derivatives(
    compiled: &CompiledModel,
    independent: &str,
    base: &BTreeMap<String, f64>,
    time: f64,
    states: &BTreeMap<String, f64>,
) -> Result<BTreeMap<String, f64>, EvaluationError> {
    let mut environment = base.clone();
    environment.insert(independent.into(), time);
    environment.extend(states.iter().map(|(name, value)| (name.clone(), *value)));
    compiled.evaluate_derivatives(&environment)
}

fn rk4_step(
    compiled: &CompiledModel,
    independent: &str,
    base: &BTreeMap<String, f64>,
    time: f64,
    states: &BTreeMap<String, f64>,
    step: f64,
) -> Result<BTreeMap<String, f64>, EvaluationError> {
    let k1 = derivatives(compiled, independent, base, time, states)?;
    let s2 = combine_state(states, &[(&k1, step * 0.5)]);
    let k2 = derivatives(compiled, independent, base, time + step * 0.5, &s2)?;
    let s3 = combine_state(states, &[(&k2, step * 0.5)]);
    let k3 = derivatives(compiled, independent, base, time + step * 0.5, &s3)?;
    let s4 = combine_state(states, &[(&k3, step)]);
    let k4 = derivatives(compiled, independent, base, time + step, &s4)?;
    Ok(states
        .iter()
        .map(|(name, value)| {
            (
                name.clone(),
                value + step * (k1[name] + 2.0 * k2[name] + 2.0 * k3[name] + k4[name]) / 6.0,
            )
        })
        .collect())
}

fn combine_state(
    states: &BTreeMap<String, f64>,
    terms: &[(&BTreeMap<String, f64>, f64)],
) -> BTreeMap<String, f64> {
    states
        .iter()
        .map(|(name, value)| {
            (
                name.clone(),
                terms.iter().fold(*value, |sum, (derivative, scale)| {
                    sum + scale * derivative[name]
                }),
            )
        })
        .collect()
}

fn adaptive_rk_step(
    compiled: &CompiledModel,
    independent: &str,
    base: &BTreeMap<String, f64>,
    time: f64,
    states: &BTreeMap<String, f64>,
    step: f64,
    options: &FitOptions,
) -> Result<(BTreeMap<String, f64>, f64), EvaluationError> {
    let full = rk4_step(compiled, independent, base, time, states, step)?;
    let half = rk4_step(compiled, independent, base, time, states, step * 0.5)?;
    let refined = rk4_step(
        compiled,
        independent,
        base,
        time + step * 0.5,
        &half,
        step * 0.5,
    )?;
    let error = refined
        .iter()
        .map(|(name, value)| {
            let scale = options.absolute_tolerance
                + options.relative_tolerance * value.abs().max(full[name].abs());
            ((value - full[name]) / scale).abs()
        })
        .fold(0.0, f64::max);
    Ok((refined, error))
}

fn bdf_step(
    compiled: &CompiledModel,
    independent: &str,
    base: &BTreeMap<String, f64>,
    time: f64,
    states: &BTreeMap<String, f64>,
    step: f64,
) -> Result<(BTreeMap<String, f64>, f64), EvaluationError> {
    let start_derivative = derivatives(compiled, independent, base, time, states)?;
    let mut next = combine_state(states, &[(&start_derivative, step)]);
    for _ in 0..20 {
        let derivative = derivatives(compiled, independent, base, time + step, &next)?;
        let updated = combine_state(states, &[(&derivative, step)]);
        let error = updated
            .iter()
            .map(|(name, value)| (value - next[name]).abs())
            .fold(0.0, f64::max);
        next = updated;
        if error < 1e-10 {
            return Ok((next, 0.0));
        }
    }
    Err(EvaluationError::NonFinite {
        output: "BDF iteration did not converge".into(),
    })
}

pub(super) fn solve_implicit(
    compiled: &CompiledModel,
    definition: &FitModelDefinition,
    environment: &BTreeMap<String, f64>,
    observed: &BTreeMap<String, f64>,
) -> Result<BTreeMap<String, f64>, EvaluationError> {
    let names: Vec<&str> = definition
        .responses
        .iter()
        .map(|response| response.id.as_str())
        .collect();
    let mut root: Vec<f64> = names.iter().map(|name| observed[*name]).collect();
    for _ in 0..40 {
        let mut values = environment.clone();
        for (name, value) in names.iter().zip(&root) {
            values.insert((*name).into(), *value);
        }
        let residual = compiled.evaluate_implicit(&values)?;
        if residual.iter().map(|v| v * v).sum::<f64>().sqrt() < 1e-10 {
            return Ok(names
                .iter()
                .zip(root)
                .map(|(n, v)| ((*n).into(), v))
                .collect());
        }
        let mut jacobian = vec![vec![0.0; root.len()]; root.len()];
        for column in 0..root.len() {
            let h = jac_step(root[column]);
            let mut shifted = values.clone();
            shifted.insert(names[column].into(), root[column] + h);
            let plus = compiled.evaluate_implicit(&shifted)?;
            shifted.insert(names[column].into(), root[column] - h);
            let minus = compiled.evaluate_implicit(&shifted)?;
            for row in 0..root.len() {
                jacobian[row][column] = (plus[row] - minus[row]) / (2.0 * h);
            }
        }
        let rhs: Vec<f64> = residual.iter().map(|value| -*value).collect();
        let Some(step) = solve_linear(&jacobian, &rhs) else {
            return Err(EvaluationError::NonFinite {
                output: "implicit root".into(),
            });
        };
        for (value, step) in root.iter_mut().zip(step) {
            *value += step;
        }
    }
    Err(EvaluationError::NonFinite {
        output: "implicit root did not converge".into(),
    })
}
