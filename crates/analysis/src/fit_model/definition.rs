use serde::{Deserialize, Serialize};

pub const FIT_MODEL_SCHEMA_VERSION: u32 = 1;

/// A portable model definition. `id` is a UUID string; revisions are monotonic
/// within that identity and results embed the complete definition they used.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FitModelDefinition {
    pub schema_version: u32,
    pub id: String,
    pub revision: u32,
    pub name: String,
    #[serde(default)]
    pub category: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub references: Vec<String>,
    pub kind: FitModelKind,
    #[serde(default)]
    pub independent_variables: Vec<VariableDefinition>,
    #[serde(default)]
    pub responses: Vec<VariableDefinition>,
    #[serde(default)]
    pub constants: Vec<ConstantDefinition>,
    #[serde(default)]
    pub parameters: Vec<ParameterDefinition>,
    #[serde(default)]
    pub derived_results: Vec<DerivedResultDefinition>,
}

impl FitModelDefinition {
    pub fn explicit(
        id: impl Into<String>,
        name: impl Into<String>,
        source: impl Into<String>,
    ) -> Self {
        Self {
            schema_version: FIT_MODEL_SCHEMA_VERSION,
            id: id.into(),
            revision: 1,
            name: name.into(),
            category: String::new(),
            summary: String::new(),
            description: String::new(),
            references: Vec::new(),
            kind: FitModelKind::Explicit {
                source: source.into(),
            },
            independent_variables: Vec::new(),
            responses: Vec::new(),
            constants: Vec::new(),
            parameters: Vec::new(),
            derived_results: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum FitModelKind {
    Explicit {
        source: String,
    },
    Implicit {
        source: String,
    },
    OdeSystem {
        source: String,
        independent: String,
        #[serde(default)]
        initial_conditions: Vec<InitialConditionDefinition>,
    },
}

impl FitModelKind {
    pub fn source(&self) -> &str {
        match self {
            Self::Explicit { source }
            | Self::Implicit { source }
            | Self::OdeSystem { source, .. } => source,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VariableDefinition {
    pub id: String,
    #[serde(default)]
    pub display_name: String,
    #[serde(default)]
    pub unit: String,
    #[serde(default)]
    pub description: String,
}

impl VariableDefinition {
    pub fn new(id: impl Into<String>) -> Self {
        let id = id.into();
        Self {
            display_name: id.clone(),
            id,
            unit: String::new(),
            description: String::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConstantDefinition {
    pub id: String,
    #[serde(default)]
    pub display_name: String,
    #[serde(default)]
    pub unit: String,
    #[serde(default)]
    pub description: String,
    /// A portable fallback. `None` makes the constant mandatory for every
    /// dataset so acquisition-specific values are never guessed silently.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_value: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParameterDefinition {
    pub id: String,
    #[serde(default)]
    pub display_name: String,
    #[serde(default)]
    pub unit: String,
    #[serde(default)]
    pub description: String,
    pub initial: InitialValueRule,
    #[serde(default)]
    pub mode: ParameterMode,
    #[serde(default)]
    pub lower_bound: Option<f64>,
    #[serde(default)]
    pub upper_bound: Option<f64>,
    #[serde(default)]
    pub sharing: ParameterSharing,
}

impl ParameterDefinition {
    pub fn free(id: impl Into<String>, initial: f64) -> Self {
        let id = id.into();
        Self {
            display_name: id.clone(),
            id,
            unit: String::new(),
            description: String::new(),
            initial: InitialValueRule::Value(initial),
            mode: ParameterMode::Free,
            lower_bound: None,
            upper_bound: None,
            sharing: ParameterSharing::PerDataset,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum InitialValueRule {
    Value(f64),
    DataExpression(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(tag = "type", content = "expression", rename_all = "snake_case")]
pub enum ParameterMode {
    #[default]
    Free,
    Fixed,
    Constrained(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ParameterSharing {
    Shared,
    #[default]
    PerDataset,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DerivedResultDefinition {
    pub id: String,
    #[serde(default)]
    pub display_name: String,
    #[serde(default)]
    pub unit: String,
    #[serde(default)]
    pub description: String,
    pub expression: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InitialConditionDefinition {
    pub state: String,
    pub at: String,
    pub value: String,
}
