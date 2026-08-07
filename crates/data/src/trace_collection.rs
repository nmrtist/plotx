use crate::{TraceCollectionId, TraceItemId};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TraceParameterValue {
    Number { value: f64, unit: String },
    Text { value: String },
}

impl TraceParameterValue {
    pub fn formatted(&self) -> String {
        match self {
            Self::Number { value, unit } => {
                let value = format!("{value:.6}")
                    .trim_end_matches('0')
                    .trim_end_matches('.')
                    .to_owned();
                if unit.is_empty() {
                    value
                } else {
                    format!("{value} {unit}")
                }
            }
            Self::Text { value } => value.clone(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TraceItemParameter {
    pub key: String,
    pub name: String,
    pub value: TraceParameterValue,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TraceItemDescriptor {
    pub id: TraceItemId,
    pub parameters: Vec<TraceItemParameter>,
    pub primary_label_parameter: String,
    pub label_override: Option<String>,
}

impl TraceItemDescriptor {
    pub fn automatic_label(&self) -> Option<String> {
        self.label_override.clone().or_else(|| {
            self.parameters
                .iter()
                .find(|parameter| parameter.key == self.primary_label_parameter)
                .map(|parameter| parameter.value.formatted())
        })
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TraceCollectionCatalog {
    pub id: TraceCollectionId,
    pub axis_quantity: String,
    pub axis_unit: String,
    pub items: Vec<TraceItemDescriptor>,
}

impl TraceCollectionCatalog {
    pub fn validate(&self) -> Result<(), String> {
        if self.axis_quantity.trim().is_empty() {
            return Err("trace collection axis quantity is empty".to_owned());
        }
        let mut ids = BTreeSet::new();
        for item in &self.items {
            if !ids.insert(item.id) {
                return Err(format!(
                    "trace collection contains duplicate item id {}",
                    item.id
                ));
            }
            let mut keys = BTreeSet::new();
            for parameter in &item.parameters {
                if parameter.key.trim().is_empty() || !keys.insert(parameter.key.as_str()) {
                    return Err(format!(
                        "trace item {} has empty or duplicate parameter keys",
                        item.id
                    ));
                }
                if let TraceParameterValue::Number { value, .. } = parameter.value
                    && !value.is_finite()
                {
                    return Err(format!("trace item {} has a non-finite parameter", item.id));
                }
            }
            if !keys.contains(item.primary_label_parameter.as_str()) {
                return Err(format!(
                    "trace item {} has an unknown primary label parameter",
                    item.id
                ));
            }
        }
        Ok(())
    }

    pub fn item(&self, id: TraceItemId) -> Option<&TraceItemDescriptor> {
        self.items.iter().find(|item| item.id == id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(id: TraceItemId) -> TraceItemDescriptor {
        TraceItemDescriptor {
            id,
            parameters: vec![TraceItemParameter {
                key: "level".into(),
                name: "Level".into(),
                value: TraceParameterValue::Number {
                    value: -60.0,
                    unit: "mV".into(),
                },
            }],
            primary_label_parameter: "level".into(),
            label_override: None,
        }
    }

    #[test]
    fn validation_rejects_duplicate_items_and_unknown_primary_parameters() {
        let collection_id = TraceCollectionId::new();
        let item_id = TraceItemId::derived(collection_id, b"one");
        let mut collection = TraceCollectionCatalog {
            id: collection_id,
            axis_quantity: "Sweep".into(),
            axis_unit: String::new(),
            items: vec![item(item_id), item(item_id)],
        };
        assert!(
            collection
                .validate()
                .unwrap_err()
                .contains("duplicate item")
        );
        collection.items.truncate(1);
        collection.items[0].primary_label_parameter = "missing".into();
        assert!(
            collection
                .validate()
                .unwrap_err()
                .contains("unknown primary")
        );
    }

    #[test]
    fn explicit_label_override_wins_over_the_primary_parameter() {
        let collection = TraceCollectionId::new();
        let mut descriptor = item(TraceItemId::derived(collection, b"one"));
        assert_eq!(descriptor.automatic_label().as_deref(), Some("-60 mV"));
        descriptor.label_override = Some("Threshold".into());
        assert_eq!(descriptor.automatic_label().as_deref(), Some("Threshold"));
    }
}
