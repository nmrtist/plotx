//! Read-only built-in models expressed through the same declarative model
//! language as user-created models.

use crate::fit_model::{
    ConstantDefinition, FitModelDefinition, InitialValueRule, ParameterDefinition,
    VariableDefinition,
};
use std::sync::OnceLock;

/// Stable ids for the built-in models, so integrations select a builtin by
/// name instead of repeating the UUID literal.
pub const MONO_EXPONENTIAL_ID: &str = "11111111-1111-4111-8111-111111111111";
pub const INVERSION_RECOVERY_ID: &str = "22222222-2222-4222-8222-222222222222";
pub const SATURATION_RECOVERY_ID: &str = "33333333-3333-4333-8333-333333333333";
pub const STEJSKAL_TANNER_ID: &str = "44444444-4444-4444-8444-444444444444";
pub const BI_EXPONENTIAL_ID: &str = "55555555-5555-4555-8555-555555555555";
pub const STRETCHED_EXPONENTIAL_ID: &str = "66666666-6666-4666-8666-666666666666";
pub const LINEAR_ID: &str = "77777777-7777-4777-8777-777777777777";

pub fn builtin_models() -> &'static [FitModelDefinition] {
    static MODELS: OnceLock<Vec<FitModelDefinition>> = OnceLock::new();
    MODELS.get_or_init(build_builtin_models)
}

fn build_builtin_models() -> Vec<FitModelDefinition> {
    vec![
        explicit(
            MONO_EXPONENTIAL_ID,
            "Mono-exponential",
            "Relaxation",
            "y = a*exp(-x/T)",
            &["x"],
            &["y"],
            vec![
                data_parameter("a", "max(y)"),
                positive_data_parameter("T", "span(x)"),
            ],
        ),
        explicit(
            INVERSION_RECOVERY_ID,
            "Inversion recovery",
            "Relaxation",
            "y = a + b*exp(-x/T)",
            &["x"],
            &["y"],
            vec![
                data_parameter("a", "max(y)"),
                data_parameter("b", "min(y) - max(y)"),
                positive_data_parameter("T", "span(x)"),
            ],
        ),
        explicit(
            SATURATION_RECOVERY_ID,
            "Saturation recovery",
            "Relaxation",
            "y = a*(1 - exp(-x/T))",
            &["x"],
            &["y"],
            vec![
                data_parameter("a", "max(y)"),
                positive_data_parameter("T", "span(x)"),
            ],
        ),
        stejskal_tanner(),
        explicit(
            BI_EXPONENTIAL_ID,
            "Bi-exponential",
            "Relaxation",
            "y = a1*exp(-x/T1) + a2*exp(-x/T2)",
            &["x"],
            &["y"],
            vec![
                data_parameter("a1", "0.6*max(y)"),
                positive_data_parameter("T1", "span(x)"),
                data_parameter("a2", "0.4*max(y)"),
                positive_data_parameter("T2", "span(x)"),
            ],
        ),
        explicit(
            STRETCHED_EXPONENTIAL_ID,
            "Stretched exponential",
            "Relaxation",
            "y = a*exp(-(x/T)^beta)",
            &["x"],
            &["y"],
            vec![
                data_parameter("a", "max(y)"),
                positive_data_parameter("T", "span(x)"),
                positive_parameter("beta", 1.0),
            ],
        ),
        explicit(
            LINEAR_ID,
            "Linear",
            "General",
            "y = a + b*x",
            &["x"],
            &["y"],
            vec![
                ParameterDefinition::free("a", 0.0),
                ParameterDefinition::free("b", 1.0),
            ],
        ),
    ]
}

fn stejskal_tanner() -> FitModelDefinition {
    let mut definition = explicit(
        STEJSKAL_TANNER_ID,
        "Stejskal–Tanner",
        "Diffusion",
        "let effective_delay = big_delta - shape_factor*delta - tau/2\n\
         let b = (gamma*delta*g)^2 * effective_delay\n\
         y = I0*exp(-D*b)",
        &["g"],
        &["y"],
        vec![
            data_parameter("I0", "max(y)"),
            positive_parameter("D", 1e-10),
        ],
    );
    definition.independent_variables[0].display_name = "Gradient strength".into();
    definition.independent_variables[0].unit = "T/m".into();
    definition.constants = vec![
        required_constant("gamma", "Gyromagnetic ratio", "rad s^-1 T^-1"),
        required_constant("delta", "Gradient pulse duration", "s"),
        required_constant("big_delta", "Diffusion delay", "s"),
        required_constant("tau", "Bipolar recovery delay", "s"),
        required_constant("shape_factor", "Gradient shape factor", ""),
    ];
    definition
}

fn required_constant(id: &str, display_name: &str, unit: &str) -> ConstantDefinition {
    ConstantDefinition {
        id: id.into(),
        display_name: display_name.into(),
        unit: unit.into(),
        description: String::new(),
        default_value: None,
    }
}

pub fn builtin_model(id: &str) -> Option<FitModelDefinition> {
    builtin_models()
        .iter()
        .find(|model| model.id == id)
        .cloned()
}

pub fn builtin_model_by_name(name: &str) -> Option<FitModelDefinition> {
    builtin_models()
        .iter()
        .find(|model| model.name == name)
        .cloned()
}

fn explicit(
    id: &str,
    name: &str,
    category: &str,
    source: &str,
    inputs: &[&str],
    responses: &[&str],
    parameters: Vec<ParameterDefinition>,
) -> FitModelDefinition {
    let mut definition = FitModelDefinition::explicit(id, name, source);
    definition.category = category.into();
    definition.summary = source.into();
    definition.independent_variables = inputs
        .iter()
        .map(|id| VariableDefinition::new(*id))
        .collect();
    definition.responses = responses
        .iter()
        .map(|id| VariableDefinition::new(*id))
        .collect();
    definition.parameters = parameters;
    definition
}

fn data_parameter(id: &str, expression: &str) -> ParameterDefinition {
    ParameterDefinition {
        initial: InitialValueRule::DataExpression(expression.into()),
        ..ParameterDefinition::free(id, 1.0)
    }
}

fn positive_data_parameter(id: &str, expression: &str) -> ParameterDefinition {
    ParameterDefinition {
        lower_bound: Some(f64::MIN_POSITIVE),
        ..data_parameter(id, expression)
    }
}

fn positive_parameter(id: &str, value: f64) -> ParameterDefinition {
    ParameterDefinition {
        lower_bound: Some(f64::MIN_POSITIVE),
        ..ParameterDefinition::free(id, value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fit_model::{
        CompiledModel, FitDataset, FitOptions, evaluate_fit_result_on_grid, fit_model,
    };
    use std::collections::BTreeMap;

    #[test]
    fn every_builtin_is_a_valid_declarative_model() {
        for model in builtin_models() {
            let compiled = CompiledModel::compile(model.clone()).unwrap();
            assert!(compiled.unknown_symbols().is_empty());
        }
    }

    #[test]
    fn mono_exponential_recovers_reference_parameters() {
        let model = builtin_model_by_name("Mono-exponential").unwrap();
        let x: Vec<f64> = (0..30).map(|index| index as f64 * 0.1).collect();
        let y = x.iter().map(|x| 8.0 * (-x / 1.2).exp()).collect();
        let dataset = FitDataset {
            id: "curve".into(),
            inputs: BTreeMap::from([("x".into(), x)]),
            responses: BTreeMap::from([("y".into(), y)]),
            sigmas: BTreeMap::new(),
            constants: BTreeMap::new(),
        };
        let result = fit_model(model, vec![dataset], &[], FitOptions::default()).unwrap();
        let parameter = |name: &str| {
            result
                .parameters
                .iter()
                .find(|parameter| parameter.parameter == name)
                .unwrap()
                .value
        };
        assert!((parameter("a") - 8.0).abs() < 1e-5);
        assert!((parameter("T") - 1.2).abs() < 1e-5);
    }

    #[test]
    fn stejskal_tanner_fits_in_gradient_space_and_predicts_negative_gradients() {
        let model = builtin_model_by_name("Stejskal–Tanner").unwrap();
        assert_eq!(model.independent_variables[0].id, "g");
        let gamma = 2.675_222_005e8;
        let delta = 2.0e-3;
        let big_delta = 80.0e-3;
        let tau = 1.0e-3;
        let shape_factor = 1.0 / 3.0;
        let effective_delay = big_delta - shape_factor * delta - 0.5 * tau;
        let gradients = vec![-0.2, -0.12, -0.04, 0.0, 0.06, 0.14, 0.22];
        let attenuation = |g: f64| {
            let b = (gamma * delta * g).powi(2) * effective_delay;
            12.0 * (-1.2e-9 * b).exp()
        };
        let responses = gradients.iter().copied().map(attenuation).collect();
        let constants = BTreeMap::from([
            ("gamma".into(), gamma),
            ("delta".into(), delta),
            ("big_delta".into(), big_delta),
            ("tau".into(), tau),
            ("shape_factor".into(), shape_factor),
        ]);
        let result = fit_model(
            model,
            vec![FitDataset {
                id: "dosy".into(),
                inputs: BTreeMap::from([("g".into(), gradients)]),
                responses: BTreeMap::from([("y".into(), responses)]),
                sigmas: BTreeMap::new(),
                constants,
            }],
            &[],
            FitOptions::default(),
        )
        .unwrap();
        let predicted = evaluate_fit_result_on_grid(
            &result,
            "dosy",
            &BTreeMap::from([("g".into(), vec![-0.1, 0.1])]),
        )
        .unwrap();
        assert!((predicted["y"][0] - predicted["y"][1]).abs() < 1e-12);
        let diffusion = result
            .parameters
            .iter()
            .find(|parameter| parameter.parameter == "D")
            .unwrap();
        assert!((diffusion.value - 1.2e-9).abs() < 1e-12);
    }

    #[test]
    fn stejskal_tanner_rejects_missing_acquisition_constants() {
        let model = builtin_model_by_name("Stejskal–Tanner").unwrap();
        let result = fit_model(
            model,
            vec![FitDataset {
                id: "dosy".into(),
                inputs: BTreeMap::from([("g".into(), vec![0.0, 0.1, 0.2])]),
                responses: BTreeMap::from([("y".into(), vec![1.0, 0.8, 0.4])]),
                sigmas: BTreeMap::new(),
                constants: BTreeMap::new(),
            }],
            &[],
            FitOptions::default(),
        );
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("required constant 'gamma'")
        );
    }
}
