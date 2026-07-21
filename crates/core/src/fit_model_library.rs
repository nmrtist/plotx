//! Versioned global library for user-created declarative fit models.

use plotx_analysis::fit_model::{CompiledModel, FitModelDefinition};
use serde::{Deserialize, Serialize};
use std::io;
use std::path::{Path, PathBuf};

pub const MODEL_LIBRARY_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FitModelLibrary {
    pub schema_version: u32,
    #[serde(default)]
    pub models: Vec<FitModelDefinition>,
}

impl Default for FitModelLibrary {
    fn default() -> Self {
        Self {
            schema_version: MODEL_LIBRARY_SCHEMA_VERSION,
            models: Vec::new(),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ModelLibraryError {
    #[error("model '{0}' was not found")]
    NotFound(String),
    #[error("model id '{0}' already exists")]
    Duplicate(String),
    #[error("model is invalid: {0}")]
    Invalid(String),
    #[error("unsupported fit-model library schema version {0}")]
    UnsupportedSchema(u32),
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

impl FitModelLibrary {
    pub fn load() -> Result<Self, ModelLibraryError> {
        let Some(path) = library_file() else {
            return Ok(Self::default());
        };
        Self::load_from_path(&path)
    }

    pub fn load_from_path(path: &Path) -> Result<Self, ModelLibraryError> {
        let data = match std::fs::read(path) {
            Ok(data) => data,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(error) => return Err(error.into()),
        };
        let library: Self = match serde_json::from_slice(&data) {
            Ok(value) => value,
            Err(error) => {
                quarantine_corrupt(path);
                return Err(error.into());
            }
        };
        if library.schema_version != MODEL_LIBRARY_SCHEMA_VERSION {
            return Err(ModelLibraryError::UnsupportedSchema(library.schema_version));
        }
        for model in &library.models {
            validate(model)?;
        }
        Ok(library)
    }

    pub fn save(&self) -> Result<(), ModelLibraryError> {
        let Some(path) = library_file() else {
            return Ok(());
        };
        self.save_to_path(&path)
    }

    pub fn save_to_path(&self, path: &Path) -> Result<(), ModelLibraryError> {
        for model in &self.models {
            validate(model)?;
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut persisted = self.clone();
        persisted.schema_version = MODEL_LIBRARY_SCHEMA_VERSION;
        // Write-then-rename replaces the target in one step on both Unix and
        // Windows, so a crash never leaves the library missing or truncated.
        let temporary = sibling(path, "tmp");
        std::fs::write(&temporary, serde_json::to_vec_pretty(&persisted)?)?;
        std::fs::rename(temporary, path)?;
        Ok(())
    }

    pub fn add(&mut self, model: FitModelDefinition) -> Result<(), ModelLibraryError> {
        validate(&model)?;
        if self.models.iter().any(|existing| existing.id == model.id) {
            return Err(ModelLibraryError::Duplicate(model.id));
        }
        self.models.push(model);
        Ok(())
    }

    pub fn update_as_new_revision(
        &mut self,
        mut model: FitModelDefinition,
    ) -> Result<(), ModelLibraryError> {
        validate(&model)?;
        let existing = self
            .models
            .iter_mut()
            .find(|existing| existing.id == model.id)
            .ok_or_else(|| ModelLibraryError::NotFound(model.id.clone()))?;
        model.revision = existing.revision + 1;
        *existing = model;
        Ok(())
    }

    pub fn remove(&mut self, id: &str) -> Result<FitModelDefinition, ModelLibraryError> {
        let index = self
            .models
            .iter()
            .position(|model| model.id == id)
            .ok_or_else(|| ModelLibraryError::NotFound(id.into()))?;
        Ok(self.models.remove(index))
    }

    pub fn rename(&mut self, id: &str, name: impl Into<String>) -> Result<(), ModelLibraryError> {
        let model = self
            .models
            .iter_mut()
            .find(|model| model.id == id)
            .ok_or_else(|| ModelLibraryError::NotFound(id.into()))?;
        let name = name.into();
        if name.trim().is_empty() {
            return Err(ModelLibraryError::Invalid(
                "model name cannot be empty".into(),
            ));
        }
        model.name = name;
        Ok(())
    }

    pub fn move_to(&mut self, id: &str, target: usize) -> Result<(), ModelLibraryError> {
        let index = self
            .models
            .iter()
            .position(|model| model.id == id)
            .ok_or_else(|| ModelLibraryError::NotFound(id.into()))?;
        let model = self.models.remove(index);
        self.models.insert(target.min(self.models.len()), model);
        Ok(())
    }

    pub fn search(&self, query: &str) -> Vec<&FitModelDefinition> {
        let query = query.to_lowercase();
        self.models
            .iter()
            .filter(|model| {
                [&model.name, &model.category, &model.summary]
                    .iter()
                    .any(|value| value.to_lowercase().contains(&query))
            })
            .collect()
    }

    pub fn import_plotxfit(&mut self, path: &Path) -> Result<String, ModelLibraryError> {
        let model: FitModelDefinition = serde_json::from_slice(&std::fs::read(path)?)?;
        let id = model.id.clone();
        self.add(model)?;
        Ok(id)
    }

    pub fn export_plotxfit(&self, id: &str, path: &Path) -> Result<(), ModelLibraryError> {
        let model = self
            .models
            .iter()
            .find(|model| model.id == id)
            .ok_or_else(|| ModelLibraryError::NotFound(id.into()))?;
        validate(model)?;
        std::fs::write(path, serde_json::to_vec_pretty(model)?)?;
        Ok(())
    }
}

fn validate(model: &FitModelDefinition) -> Result<(), ModelLibraryError> {
    let compiled = CompiledModel::compile(model.clone())
        .map_err(|error| ModelLibraryError::Invalid(error.to_string()))?;
    if !compiled.unknown_symbols().is_empty() {
        return Err(ModelLibraryError::Invalid(format!(
            "unclassified symbols: {}",
            compiled.unknown_symbols().join(", ")
        )));
    }
    Ok(())
}

fn library_file() -> Option<PathBuf> {
    crate::settings::config_dir().map(|directory| directory.join("fit-models.json"))
}

fn sibling(path: &Path, suffix: &str) -> PathBuf {
    let mut value = path.to_owned();
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("fit-models.json");
    value.set_file_name(format!("{name}.{suffix}"));
    value
}

fn quarantine_corrupt(path: &Path) {
    for index in 1..1000 {
        let candidate = sibling(path, &format!("corrupt-{index}"));
        if !candidate.exists() {
            let _ = std::fs::rename(path, candidate);
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use plotx_analysis::fit_model::{ParameterDefinition, VariableDefinition};

    fn model(id: &str, name: &str) -> FitModelDefinition {
        let mut model = FitModelDefinition::explicit(id, name, "y = a*x");
        model.independent_variables = vec![VariableDefinition::new("x")];
        model.responses = vec![VariableDefinition::new("y")];
        model.parameters = vec![ParameterDefinition::free("a", 1.0)];
        model
    }

    fn path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("plotx-{name}-{}.json", std::process::id()))
    }

    #[test]
    fn crud_round_trip_and_revision_are_versioned() {
        let path = path("model-library");
        let mut library = FitModelLibrary::default();
        let first = model("12345678-1234-1234-1234-123456789abc", "Line");
        library.add(first.clone()).unwrap();
        library.rename(&first.id, "Renamed").unwrap();
        library.save_to_path(&path).unwrap();
        let mut loaded = FitModelLibrary::load_from_path(&path).unwrap();
        let mut next = first;
        next.name = "Next".into();
        loaded.update_as_new_revision(next).unwrap();
        assert_eq!(loaded.models[0].revision, 2);
        loaded
            .remove("12345678-1234-1234-1234-123456789abc")
            .unwrap();
        assert!(loaded.models.is_empty());
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn corrupt_library_is_isolated() {
        let path = path("corrupt-model-library");
        std::fs::write(&path, b"not json").unwrap();
        assert!(FitModelLibrary::load_from_path(&path).is_err());
        assert!(!path.exists());
        let parent = path.parent().unwrap();
        let stem = path.file_name().unwrap().to_string_lossy();
        let quarantined = std::fs::read_dir(parent)
            .unwrap()
            .flatten()
            .find(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with(&format!("{stem}.corrupt-"))
            })
            .unwrap()
            .path();
        std::fs::remove_file(quarantined).unwrap();
    }
}
