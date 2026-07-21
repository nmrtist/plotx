use super::{
    CompiledModel, EvaluationError, FitModelDefinition, FitModelKind, InitialValueRule,
    ParameterMode, ParameterSharing, parse_expression,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;

mod engine;
mod results;
mod solvers;

use engine::optimize;
use results::assemble_result;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FitDataset {
    pub id: String,
    pub inputs: BTreeMap<String, Vec<f64>>,
    pub responses: BTreeMap<String, Vec<f64>>,
    #[serde(default)]
    pub sigmas: BTreeMap<String, Vec<f64>>,
    #[serde(default)]
    pub constants: BTreeMap<String, f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum FitSolver {
    #[default]
    BoundedTrustRegion,
    LevenbergMarquardt,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum OdeSolver {
    #[default]
    Auto,
    DormandPrince45,
    Bdf,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(tag = "type", content = "expression", rename_all = "snake_case")]
pub enum WeightMode {
    #[default]
    Auto,
    Equal,
    MeasurementSigma,
    Relative,
    Poisson,
    Expression(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
#[serde(tag = "type", content = "scale", rename_all = "snake_case")]
pub enum RobustLoss {
    #[default]
    None,
    Huber(f64),
    SoftL1(f64),
    Cauchy(f64),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum NonFinitePolicy {
    #[default]
    Reject,
    ExcludeRows,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FitOptions {
    pub solver: FitSolver,
    pub weights: WeightMode,
    pub robust_loss: RobustLoss,
    pub non_finite: NonFinitePolicy,
    pub max_iterations: usize,
    pub tolerance: f64,
    pub multi_start: usize,
    pub ode_solver: OdeSolver,
    pub relative_tolerance: f64,
    pub absolute_tolerance: f64,
}

impl Default for FitOptions {
    fn default() -> Self {
        Self {
            solver: FitSolver::BoundedTrustRegion,
            weights: WeightMode::Auto,
            robust_loss: RobustLoss::None,
            non_finite: NonFinitePolicy::Reject,
            max_iterations: 300,
            tolerance: 1e-8,
            multi_start: 1,
            ode_solver: OdeSolver::Auto,
            relative_tolerance: 1e-6,
            absolute_tolerance: 1e-9,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParameterOverride {
    pub dataset_id: Option<String>,
    pub parameter: String,
    pub initial: Option<f64>,
    pub lower_bound: Option<f64>,
    pub upper_bound: Option<f64>,
    pub mode: Option<ParameterMode>,
    pub sharing: Option<ParameterSharing>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParameterEstimate {
    pub parameter: String,
    pub dataset_id: Option<String>,
    pub value: f64,
    pub standard_error: f64,
    pub initial_value: f64,
    pub lower_bound: Option<f64>,
    pub upper_bound: Option<f64>,
    pub mode: ParameterMode,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DerivedEstimate {
    pub name: String,
    pub dataset_id: String,
    pub value: f64,
    pub standard_error: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PointResult {
    pub dataset_id: String,
    pub response: String,
    pub row: usize,
    pub observed: f64,
    pub predicted: f64,
    pub residual: f64,
    pub weight: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResponseStatistics {
    pub dataset_id: String,
    pub response: String,
    pub points: usize,
    pub chi_squared: f64,
    pub reduced_chi_squared: f64,
    pub r_squared: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FitStatistics {
    pub observations: usize,
    pub free_parameters: usize,
    pub degrees_of_freedom: isize,
    pub chi_squared: f64,
    pub reduced_chi_squared: f64,
    pub r_squared: f64,
    pub aic: f64,
    pub aicc: Option<f64>,
    pub bic: f64,
    pub responses: Vec<ResponseStatistics>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FitAttempt {
    pub start: usize,
    pub converged: bool,
    pub iterations: usize,
    pub cost: f64,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FitResult {
    pub model: FitModelDefinition,
    pub datasets: Vec<FitDataset>,
    pub options: FitOptions,
    pub parameters: Vec<ParameterEstimate>,
    pub derived: Vec<DerivedEstimate>,
    pub covariance: Vec<Vec<f64>>,
    pub correlation: Vec<Vec<f64>>,
    pub points: Vec<PointResult>,
    pub statistics: FitStatistics,
    pub attempts: Vec<FitAttempt>,
    pub iterations: usize,
    pub converged: bool,
    pub excluded_rows: usize,
    pub notices: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InitialFitPreview {
    pub parameters: Vec<ParameterEstimate>,
    pub points: Vec<PointResult>,
    pub excluded_rows: usize,
    pub notices: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum FitError {
    InvalidModel(String),
    InvalidData(String),
    InvalidInitialValue {
        parameter: String,
        reason: String,
    },
    Evaluation {
        dataset: String,
        row: usize,
        reason: String,
    },
    Cancelled,
    NoConvergedStart(Vec<FitAttempt>),
}

impl fmt::Display for FitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidModel(reason) => write!(f, "invalid model: {reason}"),
            Self::InvalidData(reason) => write!(f, "invalid fit data: {reason}"),
            Self::InvalidInitialValue { parameter, reason } => {
                write!(f, "invalid initial value for '{parameter}': {reason}")
            }
            Self::Evaluation {
                dataset,
                row,
                reason,
            } => write!(
                f,
                "model failed for dataset '{dataset}', row {}: {reason}",
                row + 1
            ),
            Self::Cancelled => f.write_str("fit was cancelled"),
            Self::NoConvergedStart(_) => {
                f.write_str("no multi-start attempt produced a finite solution")
            }
        }
    }
}
impl std::error::Error for FitError {}

#[derive(Clone)]
struct Slot {
    parameter: usize,
    dataset: Option<usize>,
    initial: f64,
    lower: f64,
    upper: f64,
}

#[derive(Clone)]
struct PreparedRow {
    dataset: usize,
    row: usize,
    environment: BTreeMap<String, f64>,
    observed: BTreeMap<String, f64>,
    sigmas: BTreeMap<String, f64>,
}

pub fn fit_model(
    definition: FitModelDefinition,
    datasets: Vec<FitDataset>,
    overrides: &[ParameterOverride],
    options: FitOptions,
) -> Result<FitResult, FitError> {
    fit_model_cancellable(definition, datasets, overrides, options, &|| false)
}

/// Evaluates the effective initial values without running the optimiser.
/// Domain and row errors use the same validation path as a real fit.
pub fn preview_initial_model(
    definition: FitModelDefinition,
    datasets: Vec<FitDataset>,
    overrides: &[ParameterOverride],
    mut options: FitOptions,
) -> Result<InitialFitPreview, FitError> {
    let compiled = CompiledModel::compile(definition.clone())
        .map_err(|error| FitError::InvalidModel(error.to_string()))?;
    if !compiled.unknown_symbols().is_empty() {
        return Err(FitError::InvalidModel(format!(
            "unclassified symbols: {}",
            compiled.unknown_symbols().join(", ")
        )));
    }
    let (rows, excluded_rows, mut notices) =
        prepare_rows(&definition, &datasets, options.non_finite)?;
    let slots = build_slots(&definition, &datasets, overrides)?;
    let values = start_values(&slots, 0);
    if matches!(options.weights, WeightMode::Auto) {
        let all_sigma = rows.iter().all(|row| {
            definition.responses.iter().all(|response| {
                row.sigmas
                    .get(&response.id)
                    .is_some_and(|value| value.is_finite() && *value > 0.0)
            })
        });
        options.weights = if all_sigma {
            WeightMode::MeasurementSigma
        } else {
            WeightMode::Equal
        };
        notices.push(format!(
            "Initial preview uses {} weights.",
            if all_sigma {
                "inverse-variance"
            } else {
                "equal"
            }
        ));
    }
    let expression = match &options.weights {
        WeightMode::Expression(source) => Some(
            parse_expression(source).map_err(|error| FitError::InvalidModel(error.to_string()))?,
        ),
        _ => None,
    };
    let parameters = slots
        .iter()
        .enumerate()
        .map(|(index, slot)| {
            let parameter = &definition.parameters[slot.parameter];
            ParameterEstimate {
                parameter: parameter.id.clone(),
                dataset_id: slot.dataset.map(|di| datasets[di].id.clone()),
                value: values[index],
                standard_error: 0.0,
                initial_value: slot.initial,
                lower_bound: slot.lower.is_finite().then_some(slot.lower),
                upper_bound: slot.upper.is_finite().then_some(slot.upper),
                mode: ParameterMode::Free,
            }
        })
        .collect();
    let mut points = Vec::new();
    for row in rows {
        let environment = engine::environment_for(
            &definition,
            &datasets,
            &slots,
            &values,
            row.dataset,
            &row.environment,
            &compiled,
        )?;
        let predicted = engine::predict_row(
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
            let residual = observed - prediction;
            let weight = engine::point_weight(
                &options.weights,
                observed,
                prediction,
                row.sigmas.get(&response.id).copied(),
                residual,
                expression.as_ref(),
            )?;
            points.push(PointResult {
                dataset_id: datasets[row.dataset].id.clone(),
                response: response.id.clone(),
                row: row.row,
                observed,
                predicted: prediction,
                residual,
                weight,
            });
        }
    }
    Ok(InitialFitPreview {
        parameters,
        points,
        excluded_rows,
        notices,
    })
}

pub fn fit_model_cancellable(
    definition: FitModelDefinition,
    datasets: Vec<FitDataset>,
    overrides: &[ParameterOverride],
    options: FitOptions,
    cancelled: &impl Fn() -> bool,
) -> Result<FitResult, FitError> {
    let compiled = CompiledModel::compile(definition.clone())
        .map_err(|error| FitError::InvalidModel(error.to_string()))?;
    if !compiled.unknown_symbols().is_empty() {
        return Err(FitError::InvalidModel(format!(
            "unclassified symbols: {}",
            compiled.unknown_symbols().join(", ")
        )));
    }
    if datasets.is_empty() {
        return Err(FitError::InvalidData(
            "at least one dataset is required".into(),
        ));
    }
    let (rows, excluded, mut notices) = prepare_rows(&definition, &datasets, options.non_finite)?;
    let mut options = options;
    if matches!(options.weights, WeightMode::Auto) {
        let all_sigma = rows.iter().all(|row| {
            definition.responses.iter().all(|response| {
                row.sigmas
                    .get(&response.id)
                    .is_some_and(|value| value.is_finite() && *value > 0.0)
            })
        });
        options.weights = if all_sigma {
            WeightMode::MeasurementSigma
        } else {
            WeightMode::Equal
        };
        notices.push(if all_sigma {
            "Auto weighting selected inverse-variance weights because every point has valid sigma.".into()
        } else {
            "Auto weighting selected equal weights because sigma is missing or invalid for at least one point.".into()
        });
    }
    let slots = build_slots(&definition, &datasets, overrides)?;
    // Each prepared row contributes one residual per response.
    if rows.len() * definition.responses.len() <= slots.len() {
        return Err(FitError::InvalidData(
            "observations must outnumber free parameters".into(),
        ));
    }
    if options.solver == FitSolver::LevenbergMarquardt
        && slots
            .iter()
            .any(|slot| slot.lower.is_finite() || slot.upper.is_finite())
    {
        return Err(FitError::InvalidModel(
            "Levenberg–Marquardt is only available for unbounded parameters".into(),
        ));
    }
    let weight_expression = match &options.weights {
        WeightMode::Expression(source) => Some(
            parse_expression(source).map_err(|error| FitError::InvalidModel(error.to_string()))?,
        ),
        _ => None,
    };
    let mut attempts = Vec::new();
    let mut best: Option<(Vec<f64>, f64, usize)> = None;
    for start in 0..options.multi_start.max(1) {
        if cancelled() {
            return Err(FitError::Cancelled);
        }
        let initial = start_values(&slots, start);
        let outcome = optimize(
            &compiled,
            &definition,
            &datasets,
            &rows,
            &slots,
            initial,
            &options,
            weight_expression.as_ref(),
            cancelled,
        )?;
        attempts.push(FitAttempt {
            start: start + 1,
            converged: outcome.3,
            iterations: outcome.2,
            cost: outcome.1,
            reason: outcome.4.clone(),
        });
        if outcome.1.is_finite() && best.as_ref().is_none_or(|best| outcome.1 < best.1) {
            best = Some((outcome.0, outcome.1, outcome.2));
        }
    }
    let Some((values, _, iterations)) = best else {
        return Err(FitError::NoConvergedStart(attempts));
    };
    assemble_result(
        definition,
        datasets,
        options,
        compiled,
        rows,
        slots,
        values,
        attempts,
        iterations,
        excluded,
        notices,
        weight_expression.as_ref(),
    )
}

fn prepare_rows(
    definition: &FitModelDefinition,
    datasets: &[FitDataset],
    policy: NonFinitePolicy,
) -> Result<(Vec<PreparedRow>, usize, Vec<String>), FitError> {
    let mut rows = Vec::new();
    let mut excluded = 0;
    for (di, dataset) in datasets.iter().enumerate() {
        for constant in &definition.constants {
            engine::dataset_constant(dataset, constant)?;
        }
        let lengths: Vec<usize> = definition
            .independent_variables
            .iter()
            .filter_map(|v| dataset.inputs.get(&v.id).map(Vec::len))
            .chain(
                definition
                    .responses
                    .iter()
                    .filter_map(|v| dataset.responses.get(&v.id).map(Vec::len)),
            )
            .collect();
        if lengths.len() != definition.independent_variables.len() + definition.responses.len() {
            return Err(FitError::InvalidData(format!(
                "dataset '{}' is missing a required variable or response",
                dataset.id
            )));
        }
        if lengths.iter().any(|length| *length != lengths[0]) {
            return Err(FitError::InvalidData(format!(
                "dataset '{}' has columns with different lengths",
                dataset.id
            )));
        }
        for row in 0..lengths[0] {
            let environment: BTreeMap<String, f64> = definition
                .independent_variables
                .iter()
                .map(|v| (v.id.clone(), dataset.inputs[&v.id][row]))
                .chain(definition.constants.iter().map(|c| {
                    (
                        c.id.clone(),
                        engine::dataset_constant(dataset, c)
                            .expect("constants were validated before preparing rows"),
                    )
                }))
                .collect();
            let observed: BTreeMap<String, f64> = definition
                .responses
                .iter()
                .map(|v| (v.id.clone(), dataset.responses[&v.id][row]))
                .collect();
            let sigmas: BTreeMap<String, f64> = definition
                .responses
                .iter()
                .filter_map(|v| {
                    dataset
                        .sigmas
                        .get(&v.id)
                        .and_then(|s| s.get(row))
                        .map(|value| (v.id.clone(), *value))
                })
                .collect();
            let finite = environment
                .values()
                .chain(observed.values())
                .all(|value| value.is_finite());
            if !finite {
                if policy == NonFinitePolicy::Reject {
                    return Err(FitError::InvalidData(format!(
                        "dataset '{}' row {} contains a non-finite value",
                        dataset.id,
                        row + 1
                    )));
                }
                excluded += 1;
                continue;
            }
            rows.push(PreparedRow {
                dataset: di,
                row,
                environment,
                observed,
                sigmas,
            });
        }
    }
    let notices = (excluded > 0)
        .then(|| format!("Excluded {excluded} row(s) containing non-finite input."))
        .into_iter()
        .collect();
    Ok((rows, excluded, notices))
}

fn build_slots(
    definition: &FitModelDefinition,
    datasets: &[FitDataset],
    overrides: &[ParameterOverride],
) -> Result<Vec<Slot>, FitError> {
    let mut slots = Vec::new();
    for (pi, parameter) in definition.parameters.iter().enumerate() {
        let global = override_for(overrides, None, &parameter.id);
        let mode = global
            .and_then(|value| value.mode.as_ref())
            .unwrap_or(&parameter.mode);
        if !matches!(mode, ParameterMode::Free) {
            continue;
        }
        let sharing = global
            .and_then(|value| value.sharing)
            .unwrap_or(parameter.sharing);
        let dataset_indices: Vec<Option<usize>> = if sharing == ParameterSharing::Shared {
            vec![None]
        } else {
            (0..datasets.len()).map(Some).collect()
        };
        for dataset_index in dataset_indices {
            let dataset_override = dataset_index
                .and_then(|index| override_for(overrides, Some(&datasets[index].id), &parameter.id))
                .or(global);
            let initial = dataset_override
                .and_then(|value| value.initial)
                .unwrap_or_else(|| {
                    initial_value(
                        &parameter.initial,
                        dataset_index.map(|index| &datasets[index]),
                    )
                    .unwrap_or(f64::NAN)
                });
            let lower = dataset_override
                .and_then(|value| value.lower_bound)
                .or(parameter.lower_bound)
                .unwrap_or(f64::NEG_INFINITY);
            let upper = dataset_override
                .and_then(|value| value.upper_bound)
                .or(parameter.upper_bound)
                .unwrap_or(f64::INFINITY);
            if !initial.is_finite() {
                return Err(FitError::InvalidInitialValue {
                    parameter: parameter.id.clone(),
                    reason: "value is not finite".into(),
                });
            }
            if lower > upper || initial < lower || initial > upper {
                return Err(FitError::InvalidInitialValue {
                    parameter: parameter.id.clone(),
                    reason: format!("{initial} is outside [{lower}, {upper}]"),
                });
            }
            slots.push(Slot {
                parameter: pi,
                dataset: dataset_index,
                initial,
                lower,
                upper,
            });
        }
    }
    Ok(slots)
}

fn override_for<'a>(
    overrides: &'a [ParameterOverride],
    dataset: Option<&str>,
    parameter: &str,
) -> Option<&'a ParameterOverride> {
    overrides
        .iter()
        .rev()
        .find(|value| value.parameter == parameter && value.dataset_id.as_deref() == dataset)
}

fn initial_value(rule: &InitialValueRule, dataset: Option<&FitDataset>) -> Option<f64> {
    match rule {
        InitialValueRule::Value(value) => Some(*value),
        InitialValueRule::DataExpression(source) => data_initial(source, dataset?),
    }
}

fn data_initial(source: &str, dataset: &FitDataset) -> Option<f64> {
    let mut resolved = source.trim().to_owned();
    while let Some(open) = resolved.rfind('(') {
        let close = open + resolved[open..].find(')')?;
        let name_start = resolved[..open]
            .char_indices()
            .rev()
            .find(|(_, character)| !character.is_ascii_alphanumeric() && *character != '_')
            .map_or(0, |(index, character)| index + character.len_utf8());
        let function = &resolved[name_start..open];
        let arguments = &resolved[open + 1..close];
        let value = evaluate_data_call(function, arguments, dataset)?;
        resolved.replace_range(name_start..=close, &format!("{value:.17e}"));
    }
    parse_expression(&resolved)
        .ok()?
        .evaluate(&BTreeMap::new())
        .ok()
}

fn evaluate_data_call(function: &str, arguments: &str, dataset: &FitDataset) -> Option<f64> {
    let args: Vec<&str> = arguments.split(',').map(str::trim).collect();
    let values = dataset
        .responses
        .get(*args.first()?)
        .or_else(|| dataset.inputs.get(*args.first()?))?;
    if matches!(function, "start_slope" | "mid_slope" | "end_slope") {
        let x = dataset.inputs.values().next()?;
        if x.len() != values.len() || x.len() < 2 {
            return None;
        }
        let center = match function {
            "start_slope" => 0,
            "mid_slope" => x.len() / 2,
            _ => x.len() - 1,
        };
        let left = center.saturating_sub(1);
        let right = (center + 1).min(x.len() - 1);
        return (x[right] != x[left])
            .then(|| (values[right] - values[left]) / (x[right] - x[left]));
    }
    if function == "x_at_y" {
        let target: f64 = args.get(1)?.parse().ok()?;
        let x = dataset.inputs.values().next()?;
        for index in 1..values.len().min(x.len()) {
            let (a, b) = (values[index - 1] - target, values[index] - target);
            if a == 0.0 {
                return Some(x[index - 1]);
            }
            if a.signum() != b.signum() {
                let fraction = a.abs() / (a.abs() + b.abs());
                return Some(x[index - 1] + fraction * (x[index] - x[index - 1]));
            }
        }
        return None;
    }
    let mut sorted: Vec<f64> = values
        .iter()
        .copied()
        .filter(|value| value.is_finite())
        .collect();
    if sorted.is_empty() {
        return None;
    }
    Some(match function {
        "min" => sorted.into_iter().reduce(f64::min)?,
        "max" => sorted.into_iter().reduce(f64::max)?,
        "mean" => sorted.iter().sum::<f64>() / sorted.len() as f64,
        "median" => {
            sorted.sort_by(f64::total_cmp);
            quantile_sorted(&sorted, 0.5)
        }
        "quantile" => {
            sorted.sort_by(f64::total_cmp);
            quantile_sorted(&sorted, args.get(1)?.parse().ok()?)
        }
        "span" => {
            sorted.iter().copied().reduce(f64::max)? - sorted.iter().copied().reduce(f64::min)?
        }
        _ => return None,
    })
}

fn quantile_sorted(values: &[f64], q: f64) -> f64 {
    let position = q.clamp(0.0, 1.0) * (values.len() - 1) as f64;
    let low = position.floor() as usize;
    let fraction = position - low as f64;
    values[low] + fraction * (values[(low + 1).min(values.len() - 1)] - values[low])
}

fn start_values(slots: &[Slot], start: usize) -> Vec<f64> {
    slots
        .iter()
        .enumerate()
        .map(|(index, slot)| {
            if start == 0 {
                return slot.initial;
            }
            let phase =
                (((start * 104_729 + index * 15_485) % 10_007) as f64 / 10_007.0) * 2.0 - 1.0;
            if slot.lower.is_finite() && slot.upper.is_finite() {
                slot.lower + (phase + 1.0) * 0.5 * (slot.upper - slot.lower)
            } else {
                (slot.initial + phase * slot.initial.abs().max(1.0) * (start as f64).sqrt())
                    .clamp(slot.lower, slot.upper)
            }
        })
        .collect()
}

#[cfg(test)]
mod tests;
