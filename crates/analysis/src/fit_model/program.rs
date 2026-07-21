use super::definition::{FitModelDefinition, FitModelKind, ParameterMode};
use super::dsl::{Expression, SourceError, SourcePosition, parse_expression};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelValidationError {
    pub message: String,
    pub position: Option<SourcePosition>,
}

impl ModelValidationError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            position: None,
        }
    }
    fn at(message: impl Into<String>, position: SourcePosition) -> Self {
        Self {
            message: message.into(),
            position: Some(position),
        }
    }
}

impl fmt::Display for ModelValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(position) = self.position {
            write!(
                f,
                "{} at line {}, column {}",
                self.message, position.line, position.column
            )
        } else {
            f.write_str(&self.message)
        }
    }
}

impl std::error::Error for ModelValidationError {}

#[derive(Debug, Clone, PartialEq)]
pub enum EvaluationError {
    MissingSymbol(String),
    NonFinite { output: String },
    Expression(SourceError),
    WrongModelKind,
}

impl fmt::Display for EvaluationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingSymbol(name) => write!(f, "missing value for '{name}'"),
            Self::NonFinite { output } => write!(f, "model output '{output}' is not finite"),
            Self::Expression(error) => error.fmt(f),
            Self::WrongModelKind => f.write_str("operation is not valid for this model type"),
        }
    }
}

impl std::error::Error for EvaluationError {}

impl From<SourceError> for EvaluationError {
    fn from(value: SourceError) -> Self {
        Self::Expression(value)
    }
}

#[derive(Debug, Clone)]
struct NamedExpression {
    name: String,
    expression: Expression,
}

#[derive(Debug, Clone)]
pub struct CompiledModel {
    definition: FitModelDefinition,
    helpers: Vec<NamedExpression>,
    equations: Vec<NamedExpression>,
    constraints: Vec<NamedExpression>,
    derived: Vec<NamedExpression>,
    unknown_symbols: Vec<String>,
}

impl CompiledModel {
    pub fn compile(definition: FitModelDefinition) -> Result<Self, ModelValidationError> {
        validate_metadata(&definition)?;
        let (helpers, equations) = compile_source(&definition)?;
        let roles = declared_roles(&definition)?;
        validate_equations(&definition, &equations)?;
        let helper_names: BTreeSet<&str> = helpers.iter().map(|item| item.name.as_str()).collect();
        let mut used = BTreeSet::new();
        for item in helpers.iter().chain(&equations) {
            item.expression.collect_symbols(&mut used);
        }
        let unknown_symbols = used
            .into_iter()
            .filter(|name| !roles.contains(name.as_str()) && !helper_names.contains(name.as_str()))
            .collect();

        let constraints = definition
            .parameters
            .iter()
            .filter_map(|parameter| match &parameter.mode {
                ParameterMode::Constrained(source) => Some(parse_named(&parameter.id, source)),
                _ => None,
            })
            .collect::<Result<Vec<_>, _>>()?;
        validate_dependency_dag(&constraints, "parameter constraint")?;
        let derived = definition
            .derived_results
            .iter()
            .map(|result| parse_named(&result.id, &result.expression))
            .collect::<Result<Vec<_>, _>>()?;
        validate_dependency_dag(&derived, "derived result")?;

        Ok(Self {
            definition,
            helpers,
            equations,
            constraints,
            derived,
            unknown_symbols,
        })
    }

    pub fn definition(&self) -> &FitModelDefinition {
        &self.definition
    }
    pub fn unknown_symbols(&self) -> &[String] {
        &self.unknown_symbols
    }

    /// Evaluates all explicit outputs at one observation.
    pub fn evaluate_explicit(
        &self,
        values: &BTreeMap<String, f64>,
    ) -> Result<BTreeMap<String, f64>, EvaluationError> {
        if !matches!(self.definition.kind, FitModelKind::Explicit { .. }) {
            return Err(EvaluationError::WrongModelKind);
        }
        self.evaluate_named(values, &self.equations)
    }

    /// Evaluates implicit equations as `left - right`; a solution has zero residuals.
    pub fn evaluate_implicit(
        &self,
        values: &BTreeMap<String, f64>,
    ) -> Result<Vec<f64>, EvaluationError> {
        if !matches!(self.definition.kind, FitModelKind::Implicit { .. }) {
            return Err(EvaluationError::WrongModelKind);
        }
        Ok(self
            .evaluate_named(values, &self.equations)?
            .into_values()
            .collect())
    }

    /// Evaluates the derivative assigned to each ODE state.
    pub fn evaluate_derivatives(
        &self,
        values: &BTreeMap<String, f64>,
    ) -> Result<BTreeMap<String, f64>, EvaluationError> {
        if !matches!(self.definition.kind, FitModelKind::OdeSystem { .. }) {
            return Err(EvaluationError::WrongModelKind);
        }
        self.evaluate_named(values, &self.equations)
    }

    pub fn apply_constraints(
        &self,
        values: &mut BTreeMap<String, f64>,
    ) -> Result<(), EvaluationError> {
        evaluate_ordered(&self.constraints, values)
    }

    pub fn evaluate_derived(
        &self,
        values: &BTreeMap<String, f64>,
    ) -> Result<BTreeMap<String, f64>, EvaluationError> {
        let mut values = values.clone();
        evaluate_ordered(&self.derived, &mut values)?;
        Ok(self
            .derived
            .iter()
            .filter_map(|item| {
                values
                    .get(&item.name)
                    .map(|value| (item.name.clone(), *value))
            })
            .collect())
    }

    fn evaluate_named(
        &self,
        values: &BTreeMap<String, f64>,
        expressions: &[NamedExpression],
    ) -> Result<BTreeMap<String, f64>, EvaluationError> {
        if let Some(name) = self
            .unknown_symbols
            .iter()
            .find(|name| !values.contains_key(*name))
        {
            return Err(EvaluationError::MissingSymbol(name.clone()));
        }
        let mut environment = values.clone();
        evaluate_ordered(&self.helpers, &mut environment)?;
        let mut output = BTreeMap::new();
        for item in expressions {
            let value = item.expression.evaluate(&environment)?;
            if !value.is_finite() {
                return Err(EvaluationError::NonFinite {
                    output: item.name.clone(),
                });
            }
            output.insert(item.name.clone(), value);
        }
        Ok(output)
    }
}

fn evaluate_ordered(
    items: &[NamedExpression],
    values: &mut BTreeMap<String, f64>,
) -> Result<(), EvaluationError> {
    let mut remaining: Vec<&NamedExpression> = items.iter().collect();
    while !remaining.is_empty() {
        let mut progressed = false;
        let mut index = 0;
        while index < remaining.len() {
            let mut dependencies = BTreeSet::new();
            remaining[index]
                .expression
                .collect_symbols(&mut dependencies);
            if dependencies.iter().all(|name| values.contains_key(name)) {
                let item = remaining.remove(index);
                let value = item.expression.evaluate(values)?;
                if !value.is_finite() {
                    return Err(EvaluationError::NonFinite {
                        output: item.name.clone(),
                    });
                }
                values.insert(item.name.clone(), value);
                progressed = true;
            } else {
                index += 1;
            }
        }
        if !progressed {
            return Err(EvaluationError::MissingSymbol(remaining[0].name.clone()));
        }
    }
    Ok(())
}

fn compile_source(
    definition: &FitModelDefinition,
) -> Result<(Vec<NamedExpression>, Vec<NamedExpression>), ModelValidationError> {
    if definition.kind.source().len() > 64 * 1024 {
        return Err(ModelValidationError::new(
            "model source exceeds the 64 KiB resource limit",
        ));
    }
    let mut helpers = Vec::new();
    let mut equations = Vec::new();
    let mut statement_count = 0;
    for (line_index, raw) in definition.kind.source().lines().enumerate() {
        // Strip the comment before splitting statements so a ';' inside a
        // comment does not start a bogus statement.
        let code = raw.split('#').next().unwrap_or_default();
        for raw_statement in code.split(';') {
            let statement = raw_statement.trim();
            if statement.is_empty() {
                continue;
            }
            statement_count += 1;
            if statement_count > 1024 {
                return Err(ModelValidationError::new(
                    "model source exceeds the 1024-statement resource limit",
                ));
            }
            let (helper, statement) = if let Some(rest) = statement.strip_prefix("let ") {
                (true, rest.trim())
            } else {
                (false, statement)
            };
            let Some(equal) = assignment_equal(statement) else {
                return Err(ModelValidationError::at(
                    "expected '=' in model statement",
                    SourcePosition {
                        line: line_index + 1,
                        column: 1,
                    },
                ));
            };
            let left = statement[..equal].trim();
            let right = statement[equal + 1..].trim();
            let (name, expression_source) =
                if helper || matches!(definition.kind, FitModelKind::Explicit { .. }) {
                    (left.to_owned(), right.to_owned())
                } else if matches!(definition.kind, FitModelKind::Implicit { .. }) {
                    (
                        format!("equation_{}", equations.len() + 1),
                        format!("({left}) - ({right})"),
                    )
                } else {
                    let (state, independent) = parse_derivative(left).ok_or_else(|| {
                        ModelValidationError::at(
                            "ODE equations must use d(state)/d(independent) = expression",
                            SourcePosition {
                                line: line_index + 1,
                                column: 1,
                            },
                        )
                    })?;
                    if let FitModelKind::OdeSystem {
                        independent: expected,
                        ..
                    } = &definition.kind
                        && independent != *expected
                    {
                        return Err(ModelValidationError::new(format!(
                            "ODE derivative uses '{independent}', expected '{expected}'"
                        )));
                    }
                    (state, right.to_owned())
                };
            validate_identifier(&name).map_err(|message| {
                ModelValidationError::at(
                    message,
                    SourcePosition {
                        line: line_index + 1,
                        column: 1,
                    },
                )
            })?;
            let mut expression = parse_expression(&expression_source).map_err(|mut error| {
                error.position.line += line_index;
                ModelValidationError::at(error.message, error.position)
            })?;
            shift_line(&mut expression, line_index);
            let item = NamedExpression { name, expression };
            if helper {
                helpers.push(item);
            } else {
                equations.push(item);
            }
        }
    }
    validate_dependency_dag(&helpers, "let helper")?;
    if equations.is_empty() {
        return Err(ModelValidationError::new(
            "model must define at least one equation",
        ));
    }
    Ok((helpers, equations))
}

fn parse_named(name: &str, source: &str) -> Result<NamedExpression, ModelValidationError> {
    Ok(NamedExpression {
        name: name.to_owned(),
        expression: parse_expression(source)
            .map_err(|error| ModelValidationError::at(error.message, error.position))?,
    })
}

fn validate_metadata(definition: &FitModelDefinition) -> Result<(), ModelValidationError> {
    if definition.schema_version != super::definition::FIT_MODEL_SCHEMA_VERSION {
        return Err(ModelValidationError::new(format!(
            "unsupported fit-model schema version {}",
            definition.schema_version
        )));
    }
    if !looks_like_uuid(&definition.id) {
        return Err(ModelValidationError::new("model id must be a UUID"));
    }
    if definition.revision == 0 {
        return Err(ModelValidationError::new(
            "model revision must be at least 1",
        ));
    }
    if definition.name.trim().is_empty() {
        return Err(ModelValidationError::new("model name cannot be empty"));
    }
    Ok(())
}

fn declared_roles(definition: &FitModelDefinition) -> Result<BTreeSet<&str>, ModelValidationError> {
    let mut roles = BTreeSet::new();
    for id in definition
        .independent_variables
        .iter()
        .map(|value| value.id.as_str())
        .chain(definition.responses.iter().map(|value| value.id.as_str()))
        .chain(definition.constants.iter().map(|value| value.id.as_str()))
        .chain(definition.parameters.iter().map(|value| value.id.as_str()))
    {
        validate_identifier(id).map_err(ModelValidationError::new)?;
        if !roles.insert(id) {
            return Err(ModelValidationError::new(format!(
                "symbol '{id}' has more than one declared role"
            )));
        }
    }
    Ok(roles)
}

fn validate_equations(
    definition: &FitModelDefinition,
    equations: &[NamedExpression],
) -> Result<(), ModelValidationError> {
    let expected: BTreeSet<&str> = match &definition.kind {
        FitModelKind::Explicit { .. } | FitModelKind::Implicit { .. } => definition
            .responses
            .iter()
            .map(|value| value.id.as_str())
            .collect(),
        FitModelKind::OdeSystem {
            initial_conditions, ..
        } => initial_conditions
            .iter()
            .map(|value| value.state.as_str())
            .collect(),
    };
    if !matches!(definition.kind, FitModelKind::Implicit { .. }) {
        let actual: BTreeSet<&str> = equations.iter().map(|value| value.name.as_str()).collect();
        if expected != actual {
            return Err(ModelValidationError::new(format!(
                "equation outputs {actual:?} do not match declared outputs {expected:?}"
            )));
        }
    } else if equations.len() != definition.responses.len() {
        return Err(ModelValidationError::new(
            "an implicit model needs one equation per response",
        ));
    }
    Ok(())
}

fn validate_dependency_dag(
    items: &[NamedExpression],
    label: &str,
) -> Result<(), ModelValidationError> {
    let names: BTreeSet<&str> = items.iter().map(|item| item.name.as_str()).collect();
    let mut pending: BTreeMap<&str, BTreeSet<String>> = items
        .iter()
        .map(|item| {
            let mut dependencies = BTreeSet::new();
            item.expression.collect_symbols(&mut dependencies);
            dependencies.retain(|name| names.contains(name.as_str()));
            (item.name.as_str(), dependencies)
        })
        .collect();
    while !pending.is_empty() {
        let ready: Vec<&str> = pending
            .iter()
            .filter(|(_, dependencies)| dependencies.is_empty())
            .map(|(&name, _)| name)
            .collect();
        if ready.is_empty() {
            return Err(ModelValidationError::new(format!(
                "cyclic {label} dependency"
            )));
        }
        for name in ready {
            pending.remove(name);
            for dependencies in pending.values_mut() {
                dependencies.remove(name);
            }
        }
    }
    Ok(())
}

fn assignment_equal(statement: &str) -> Option<usize> {
    let bytes = statement.as_bytes();
    bytes.iter().enumerate().find_map(|(index, byte)| {
        (*byte == b'='
            && bytes.get(index.wrapping_sub(1)).copied() != Some(b'<')
            && bytes.get(index.wrapping_sub(1)).copied() != Some(b'>')
            && bytes.get(index.wrapping_sub(1)).copied() != Some(b'!')
            && bytes.get(index + 1).copied() != Some(b'='))
        .then_some(index)
    })
}

fn parse_derivative(left: &str) -> Option<(String, String)> {
    let (numerator, denominator) = left.split_once('/')?;
    let state = numerator
        .trim()
        .strip_prefix("d(")?
        .strip_suffix(')')?
        .trim();
    let independent = denominator
        .trim()
        .strip_prefix("d(")?
        .strip_suffix(')')?
        .trim();
    validate_identifier(state).ok()?;
    validate_identifier(independent).ok()?;
    Some((state.to_owned(), independent.to_owned()))
}

fn validate_identifier(id: &str) -> Result<(), String> {
    let mut bytes = id.bytes();
    if !bytes
        .next()
        .is_some_and(|byte| byte.is_ascii_alphabetic() || byte == b'_')
        || !bytes.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
    {
        return Err(format!("'{id}' is not a valid ASCII identifier"));
    }
    if matches!(id, "pi" | "e" | "let" | "if") {
        return Err(format!("'{id}' is reserved"));
    }
    Ok(())
}

fn looks_like_uuid(value: &str) -> bool {
    value.len() == 36
        && value.char_indices().all(|(index, c)| {
            if matches!(index, 8 | 13 | 18 | 23) {
                c == '-'
            } else {
                c.is_ascii_hexdigit()
            }
        })
}

fn shift_line(expression: &mut Expression, amount: usize) {
    expression.position.line += amount;
    match &mut expression.kind {
        super::dsl::ExprKind::Unary(_, value) => shift_line(value, amount),
        super::dsl::ExprKind::Binary(_, left, right) => {
            shift_line(left, amount);
            shift_line(right, amount);
        }
        super::dsl::ExprKind::Call(_, args) => {
            for arg in args {
                shift_line(arg, amount);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fit_model::{
        InitialValueRule, ParameterDefinition, ParameterSharing, VariableDefinition,
    };

    fn explicit(source: &str) -> FitModelDefinition {
        let mut model =
            FitModelDefinition::explicit("12345678-1234-1234-1234-123456789abc", "test", source);
        model.independent_variables = vec![VariableDefinition::new("x")];
        model.responses = vec![VariableDefinition::new("y")];
        model.parameters = vec![ParameterDefinition {
            id: "a".into(),
            display_name: "a".into(),
            unit: String::new(),
            description: String::new(),
            initial: InitialValueRule::Value(2.0),
            mode: ParameterMode::Free,
            lower_bound: None,
            upper_bound: None,
            sharing: ParameterSharing::PerDataset,
        }];
        model
    }

    #[test]
    fn compiles_helpers_and_evaluates_multiple_lines() {
        let compiled = CompiledModel::compile(explicit("let z = a*x\ny = z + 1")).unwrap();
        let values = BTreeMap::from([("a".into(), 2.0), ("x".into(), 3.0)]);
        assert_eq!(compiled.evaluate_explicit(&values).unwrap()["y"], 7.0);
    }

    #[test]
    fn discovers_unclassified_symbols_for_role_confirmation() {
        let compiled = CompiledModel::compile(explicit("y = a*x + offset")).unwrap();
        assert_eq!(compiled.unknown_symbols(), ["offset"]);
    }

    #[test]
    fn rejects_constraint_cycles() {
        let mut model = explicit("y = a*x");
        model.parameters[0].mode = ParameterMode::Constrained("b + 1".into());
        model.parameters.push(ParameterDefinition {
            id: "b".into(),
            mode: ParameterMode::Constrained("a + 1".into()),
            ..ParameterDefinition::free("b", 1.0)
        });
        assert!(
            CompiledModel::compile(model)
                .unwrap_err()
                .message
                .contains("cyclic")
        );
    }
}
