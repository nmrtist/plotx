//! Explicit import inputs; nmr validates them against the vendor acquisition.

use crate::{IoError, LoadResult, nmr_bridge};
use serde::{Deserialize, Serialize};
use std::{io::Read, path::Path, sync::Arc};

/// Invocation data only. The checked declaration is persisted inside snapshot v1.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SamplingDeclaration {
    pub assertion_id: String,
    pub source: String,
    pub grid_shape: Vec<usize>,
    pub coordinates: Vec<Vec<usize>>,
    pub index_base: IndexBase,
    pub component_counts: Vec<usize>,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum IndexBase {
    Zero,
    One,
}

impl SamplingDeclaration {
    pub fn into_native(self) -> Result<nmr::SamplingDeclaration, IoError> {
        let id = nmr::raw::AssertionId::try_new(self.assertion_id)
            .map_err(|error| IoError::NmrConversion(error.to_string()))?;
        Ok(nmr::SamplingDeclaration::new(
            id,
            self.source,
            self.grid_shape,
            self.coordinates,
            match self.index_base {
                IndexBase::Zero => nmr::SamplingIndexBase::Zero,
                IndexBase::One => nmr::SamplingIndexBase::One,
            },
            self.component_counts,
        ))
    }
}

/// Bound external declaration text before decoding its nested coordinate lists.
pub fn read_declaration(path: &Path) -> Result<SamplingDeclaration, IoError> {
    const MAX_BYTES: u64 = 8 * 1024 * 1024;
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err(IoError::NmrConversion(
            "sampling declaration exceeds 8 MiB".into(),
        ));
    }
    serde_json::from_slice(&bytes)
        .map_err(|error| IoError::NmrConversion(format!("invalid sampling declaration: {error}")))
}

pub fn read(
    path: &Path,
    declaration: SamplingDeclaration,
    context: &mut nmr::ExecutionContext<'_>,
) -> Result<Arc<nmr::Dataset>, IoError> {
    nmr_bridge::read_options()
        .sampling_declaration(declaration.into_native()?)
        .read_with_context(path, context)
        .map(Arc::new)
        .map_err(|error| IoError::Nmr(Box::new(error)))
}

pub fn load(path: &Path, declaration: SamplingDeclaration) -> Result<LoadResult, IoError> {
    nmr_bridge::loaded(read(
        path,
        declaration,
        &mut nmr::ExecutionContext::default(),
    )?)
}

pub fn load_with_declaration_file(path: &Path, declaration: &Path) -> Result<LoadResult, IoError> {
    load(path, read_declaration(declaration)?)
}
