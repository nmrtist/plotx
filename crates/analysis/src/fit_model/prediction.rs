use super::{CompiledModel, FitError, FitModelKind, FitResult};
use std::collections::BTreeMap;

/// Evaluate an explicit fitted model on caller-provided model inputs.
///
/// The caller owns any mapping from display coordinates or application data
/// sources into `inputs`. Parameter scopes and per-dataset constants are
/// reconstructed exclusively from the immutable fit result.
pub fn evaluate_fit_result_on_grid(
    result: &FitResult,
    dataset_id: &str,
    inputs: &BTreeMap<String, Vec<f64>>,
) -> Result<BTreeMap<String, Vec<f64>>, FitError> {
    if !matches!(result.model.kind, FitModelKind::Explicit { .. }) {
        return Err(FitError::InvalidModel(
            "grid prediction currently requires an explicit model".into(),
        ));
    }
    let matching_datasets: Vec<_> = result
        .datasets
        .iter()
        .filter(|dataset| dataset.id == dataset_id)
        .collect();
    let [dataset] = matching_datasets.as_slice() else {
        return Err(FitError::InvalidData(format!(
            "fit result must contain exactly one dataset named '{dataset_id}'"
        )));
    };
    let lengths: Vec<usize> = result
        .model
        .independent_variables
        .iter()
        .map(|variable| {
            inputs.get(&variable.id).map(Vec::len).ok_or_else(|| {
                FitError::InvalidData(format!(
                    "prediction grid is missing input '{}'",
                    variable.id
                ))
            })
        })
        .collect::<Result<_, _>>()?;
    let Some(&rows) = lengths.first() else {
        return Err(FitError::InvalidData(
            "prediction grid requires at least one independent variable".into(),
        ));
    };
    if lengths.iter().any(|length| *length != rows) {
        return Err(FitError::InvalidData(
            "prediction grid inputs have different lengths".into(),
        ));
    }

    let compiled = CompiledModel::compile(result.model.clone())
        .map_err(|error| FitError::InvalidModel(error.to_string()))?;
    if !compiled.unknown_symbols().is_empty() {
        return Err(FitError::InvalidModel(format!(
            "unclassified symbols: {}",
            compiled.unknown_symbols().join(", ")
        )));
    }
    let mut fitted_environment = BTreeMap::new();
    for constant in &result.model.constants {
        let value = dataset
            .constants
            .get(&constant.id)
            .copied()
            .or(constant.default_value)
            .ok_or_else(|| {
                FitError::InvalidData(format!(
                    "dataset '{}' is missing required constant '{}'",
                    dataset.id, constant.id
                ))
            })?;
        fitted_environment.insert(constant.id.clone(), value);
    }
    for parameter in &result.model.parameters {
        let exact: Vec<_> = result
            .parameters
            .iter()
            .filter(|estimate| {
                estimate.parameter == parameter.id
                    && estimate.dataset_id.as_deref() == Some(dataset_id)
            })
            .collect();
        let shared: Vec<_> = result
            .parameters
            .iter()
            .filter(|estimate| estimate.parameter == parameter.id && estimate.dataset_id.is_none())
            .collect();
        let estimate = match (exact.as_slice(), shared.as_slice()) {
            ([estimate], []) | ([], [estimate]) => *estimate,
            _ => {
                return Err(FitError::InvalidData(format!(
                    "fit result has an ambiguous value for parameter '{}' in dataset '{}'",
                    parameter.id, dataset_id
                )));
            }
        };
        fitted_environment.insert(parameter.id.clone(), estimate.value);
    }

    let mut predictions: BTreeMap<String, Vec<f64>> = result
        .model
        .responses
        .iter()
        .map(|response| (response.id.clone(), Vec::with_capacity(rows)))
        .collect();
    let mut input_iters: Vec<_> = result
        .model
        .independent_variables
        .iter()
        .map(|variable| (variable.id.as_str(), inputs[&variable.id].iter()))
        .collect();
    for row in 0..rows {
        let mut environment = fitted_environment.clone();
        for (variable, values) in &mut input_iters {
            let value = *values
                .next()
                .expect("prediction input lengths were validated above");
            if !value.is_finite() {
                return Err(FitError::InvalidData(format!(
                    "prediction grid row {} contains a non-finite input",
                    row + 1
                )));
            }
            environment.insert((*variable).to_owned(), value);
        }
        let values =
            compiled
                .evaluate_explicit(&environment)
                .map_err(|error| FitError::Evaluation {
                    dataset: dataset_id.into(),
                    row,
                    reason: error.to_string(),
                })?;
        for response in &result.model.responses {
            predictions
                .get_mut(&response.id)
                .expect("response map follows the model definition")
                .push(values[&response.id]);
        }
    }
    Ok(predictions)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fit_model::{
        FitDataset, FitModelDefinition, FitOptions, ParameterDefinition, ParameterSharing,
        VariableDefinition, fit_model,
    };

    #[test]
    fn grid_prediction_resolves_shared_and_per_dataset_parameters() {
        let mut model = FitModelDefinition::explicit(
            "12345678-1234-4234-8234-123456789abc",
            "Shared line",
            "y = intercept + slope*x",
        );
        model.independent_variables = vec![VariableDefinition::new("x")];
        model.responses = vec![VariableDefinition::new("y")];
        model.parameters = vec![
            ParameterDefinition::free("intercept", 0.0),
            ParameterDefinition {
                sharing: ParameterSharing::Shared,
                ..ParameterDefinition::free("slope", 1.0)
            },
        ];
        let dataset = |id: &str, intercept: f64| {
            let x: Vec<f64> = (0..12).map(|value| value as f64).collect();
            FitDataset {
                id: id.into(),
                responses: BTreeMap::from([(
                    "y".into(),
                    x.iter().map(|value| intercept + 2.0 * value).collect(),
                )]),
                inputs: BTreeMap::from([("x".into(), x)]),
                sigmas: BTreeMap::new(),
                constants: BTreeMap::new(),
            }
        };
        let result = fit_model(
            model,
            vec![dataset("a", 1.0), dataset("b", 5.0)],
            &[],
            FitOptions::default(),
        )
        .unwrap();
        let values = evaluate_fit_result_on_grid(
            &result,
            "b",
            &BTreeMap::from([("x".into(), vec![0.5, 1.5])]),
        )
        .unwrap();
        assert!((values["y"][0] - 6.0).abs() < 1e-5);
        assert!((values["y"][1] - 8.0).abs() < 1e-5);
    }
}
