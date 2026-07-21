use crate::{BlockStore, ContentHash, DataError, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Immutable original input embedded by content hash. Project archives apply
/// compression to this block; the logical object records the exact source
/// bytes, including clipboard text.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RawInputObject {
    pub byte_hash: ContentHash,
    pub byte_len: u64,
    pub media_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default)]
    pub metadata: BTreeMap<String, serde_json::Value>,
}

impl RawInputObject {
    pub fn embed(
        bytes: Vec<u8>,
        media_type: impl Into<String>,
        name: Option<String>,
        store: &dyn BlockStore,
    ) -> Result<Self> {
        let byte_len = u64::try_from(bytes.len())
            .map_err(|_| DataError::Backend("raw input is too large".into()))?;
        let byte_hash = store.put(bytes)?;
        let object = Self {
            byte_hash,
            byte_len,
            media_type: media_type.into(),
            name,
            metadata: BTreeMap::new(),
        };
        object.validate()?;
        Ok(object)
    }

    pub fn read(&self, store: &dyn BlockStore) -> Result<Vec<u8>> {
        self.validate()?;
        let bytes = store.get(self.byte_hash)?;
        if bytes.len() as u64 != self.byte_len || ContentHash::of(&bytes) != self.byte_hash {
            return Err(DataError::CorruptBlock(format!(
                "raw input {} failed length or hash validation",
                self.byte_hash
            )));
        }
        Ok(bytes)
    }

    pub fn validate(&self) -> Result<()> {
        if self.media_type.trim().is_empty() {
            return Err(DataError::InvalidSchema(
                "raw input media type is empty".into(),
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MemoryBlockStore;

    #[test]
    fn raw_inputs_are_content_addressed_and_deduplicated() {
        let store = MemoryBlockStore::default();
        let first = RawInputObject::embed(
            b"sample,value\na,1\n".to_vec(),
            "text/csv",
            Some("input.csv".into()),
            &store,
        )
        .unwrap();
        let second =
            RawInputObject::embed(b"sample,value\na,1\n".to_vec(), "text/csv", None, &store)
                .unwrap();
        assert_eq!(first.byte_hash, second.byte_hash);
        assert_eq!(store.block_count(), 1);
        assert_eq!(first.read(&store).unwrap(), b"sample,value\na,1\n");
    }
}
