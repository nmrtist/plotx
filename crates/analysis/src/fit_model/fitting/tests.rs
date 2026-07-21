use super::*;
use crate::fit_model::{ParameterDefinition, VariableDefinition};

fn line() -> FitModelDefinition {
    let mut model = FitModelDefinition::explicit(
        "12345678-1234-1234-1234-123456789abc",
        "Line",
        "y = a + b*x",
    );
    model.independent_variables = vec![VariableDefinition::new("x")];
    model.responses = vec![VariableDefinition::new("y")];
    model.parameters = vec![
        ParameterDefinition::free("a", 0.0),
        ParameterDefinition::free("b", 1.0),
    ];
    model
}
fn dataset(id: &str, intercept: f64, slope: f64) -> FitDataset {
    let x: Vec<f64> = (0..20).map(|v| v as f64).collect();
    let y = x.iter().map(|x| intercept + slope * x).collect();
    FitDataset {
        id: id.into(),
        inputs: BTreeMap::from([("x".into(), x)]),
        responses: BTreeMap::from([("y".into(), y)]),
        sigmas: BTreeMap::new(),
        constants: BTreeMap::new(),
    }
}

#[test]
fn data_initial_rules_compose_in_arithmetic_expressions() {
    let data = dataset("seed", -2.0, 0.5);
    let value = data_initial("max(y) - min(y) + median(y)", &data).unwrap();
    assert!((value - 12.25).abs() < 1e-12);
}

#[test]
fn bounded_explicit_fit_recovers_line_and_full_statistics() {
    let result = fit_model(
        line(),
        vec![dataset("one", 3.0, 2.0)],
        &[],
        FitOptions::default(),
    )
    .unwrap();
    assert!((result.parameters[0].value - 3.0).abs() < 1e-6);
    assert!((result.parameters[1].value - 2.0).abs() < 1e-6);
    assert_eq!(result.points.len(), 20);
    assert!(result.statistics.aic.is_finite());
    assert_eq!(result.covariance.len(), 2);
}

#[test]
fn global_fit_can_share_one_parameter() {
    let mut model = line();
    model.parameters[1].sharing = ParameterSharing::Shared;
    let result = fit_model(
        model,
        vec![dataset("a", 1.0, 4.0), dataset("b", 8.0, 4.0)],
        &[],
        FitOptions::default(),
    )
    .unwrap();
    assert_eq!(result.parameters.len(), 3);
    let slope = result
        .parameters
        .iter()
        .find(|p| p.parameter == "b")
        .unwrap();
    assert!((slope.value - 4.0).abs() < 1e-6);
}

#[test]
fn invalid_rows_are_never_silently_discarded() {
    let mut data = dataset("bad", 1.0, 2.0);
    data.responses.get_mut("y").unwrap()[3] = f64::NAN;
    assert!(fit_model(line(), vec![data.clone()], &[], FitOptions::default()).is_err());
    let result = fit_model(
        line(),
        vec![data],
        &[],
        FitOptions {
            non_finite: NonFinitePolicy::ExcludeRows,
            ..FitOptions::default()
        },
    )
    .unwrap();
    assert_eq!(result.excluded_rows, 1);
    assert!(!result.notices.is_empty());
}

#[test]
fn implicit_equation_is_solved_at_each_observation() {
    let mut model =
        FitModelDefinition::explicit("23456789-1234-4234-8234-123456789abc", "Implicit", "");
    model.kind = FitModelKind::Implicit {
        source: "y^2 = a*x".into(),
    };
    model.independent_variables = vec![VariableDefinition::new("x")];
    model.responses = vec![VariableDefinition::new("y")];
    model.parameters = vec![ParameterDefinition {
        lower_bound: Some(0.0),
        ..ParameterDefinition::free("a", 2.0)
    }];
    let x: Vec<f64> = (1..15).map(|v| v as f64).collect();
    let y = x.iter().map(|x| (3.0 * x).sqrt()).collect();
    let data = FitDataset {
        id: "implicit".into(),
        inputs: BTreeMap::from([("x".into(), x)]),
        responses: BTreeMap::from([("y".into(), y)]),
        sigmas: BTreeMap::new(),
        constants: BTreeMap::new(),
    };
    let result = fit_model(model, vec![data], &[], FitOptions::default()).unwrap();
    assert!((result.parameters[0].value - 3.0).abs() < 1e-5);
}

#[test]
fn ode_system_fits_bidirectional_observation_times() {
    use crate::fit_model::InitialConditionDefinition;
    let mut model = FitModelDefinition::explicit("34567891-1234-4234-8234-123456789abc", "ODE", "");
    model.kind = FitModelKind::OdeSystem {
        source: "d(y)/d(t) = -k*y".into(),
        independent: "t".into(),
        initial_conditions: vec![InitialConditionDefinition {
            state: "y".into(),
            at: "0".into(),
            value: "1".into(),
        }],
    };
    model.independent_variables = vec![VariableDefinition::new("t")];
    model.responses = vec![VariableDefinition::new("y")];
    model.parameters = vec![ParameterDefinition {
        lower_bound: Some(0.0),
        ..ParameterDefinition::free("k", 0.5)
    }];
    let t: Vec<f64> = (-5..=10).map(|v| v as f64 * 0.1).collect();
    let y = t.iter().map(|t| (-0.8 * t).exp()).collect();
    let data = FitDataset {
        id: "ode".into(),
        inputs: BTreeMap::from([("t".into(), t)]),
        responses: BTreeMap::from([("y".into(), y)]),
        sigmas: BTreeMap::new(),
        constants: BTreeMap::new(),
    };
    let result = fit_model(
        model,
        vec![data],
        &[],
        FitOptions {
            max_iterations: 80,
            ..FitOptions::default()
        },
    )
    .unwrap();
    assert!(
        (result.parameters[0].value - 0.8).abs() < 2e-4,
        "{}",
        result.parameters[0].value
    );
}
