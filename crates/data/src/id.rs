use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{fmt, str::FromStr};

macro_rules! uuid_id {
    ($name:ident) => {
        #[derive(
            Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
        )]
        #[serde(transparent)]
        pub struct $name(uuid::Uuid);

        impl $name {
            pub fn new() -> Self {
                Self(uuid::Uuid::new_v4())
            }

            pub fn from_bytes(bytes: [u8; 16]) -> Self {
                Self(uuid::Uuid::from_bytes(bytes))
            }

            pub fn as_bytes(&self) -> &[u8; 16] {
                self.0.as_bytes()
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(f)
            }
        }

        impl FromStr for $name {
            type Err = uuid::Error;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                uuid::Uuid::parse_str(value).map(Self)
            }
        }
    };
}

uuid_id!(TableId);
uuid_id!(RevisionId);
uuid_id!(RowId);
uuid_id!(ColumnId);
uuid_id!(OperationId);

impl RowId {
    pub fn derived(operation: OperationId, inputs: &[RowId], discriminator: &[u8]) -> Self {
        let mut deriver = RowIdDeriver::new(operation);
        inputs.iter().for_each(|input| deriver.push(*input));
        deriver.finish(discriminator)
    }

    pub fn namespaced(source: TableId, row: RowId) -> Self {
        Self(deterministic_uuid(
            b"plotx.union-row.v1",
            &[source.as_bytes(), row.as_bytes()],
        ))
    }
}

/// Incremental equivalent of [`RowId::derived`] for large groups. It retains
/// one SHA-256 state per output group rather than every source RowId.
#[doc(hidden)]
pub struct RowIdDeriver {
    hash: Sha256,
}

impl RowIdDeriver {
    #[doc(hidden)]
    pub fn new(operation: OperationId) -> Self {
        let mut hash = Sha256::new();
        hash.update(b"plotx.row.v1");
        update_part(&mut hash, operation.as_bytes());
        Self { hash }
    }

    #[doc(hidden)]
    pub fn push(&mut self, input: RowId) {
        update_part(&mut self.hash, input.as_bytes());
    }

    #[doc(hidden)]
    pub fn finish(mut self, discriminator: &[u8]) -> RowId {
        update_part(&mut self.hash, discriminator);
        RowId(uuid_from_digest(self.hash.finalize()))
    }
}

impl ColumnId {
    pub fn derived(operation: OperationId, name: &[u8]) -> Self {
        Self(deterministic_uuid(
            b"plotx.column.v1",
            &[operation.as_bytes(), name],
        ))
    }

    pub fn derived_from(source: ColumnId, name: &[u8]) -> Self {
        Self(deterministic_uuid(
            b"plotx.derived-column.v1",
            &[source.as_bytes(), name],
        ))
    }
}

fn deterministic_uuid(domain: &[u8], parts: &[&[u8]]) -> uuid::Uuid {
    let mut hash = Sha256::new();
    hash.update(domain);
    for part in parts {
        update_part(&mut hash, part);
    }
    uuid_from_digest(hash.finalize())
}

fn update_part(hash: &mut Sha256, part: &[u8]) {
    hash.update((part.len() as u64).to_le_bytes());
    hash.update(part);
}

fn uuid_from_digest(digest: impl AsRef<[u8]>) -> uuid::Uuid {
    let digest = digest.as_ref();
    let mut bytes = [0_u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    // RFC 9562-compatible variant with a reserved, deterministic version
    // nibble. The digest remains the identity source.
    bytes[6] = (bytes[6] & 0x0f) | 0x80;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    uuid::Uuid::from_bytes(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derived_row_ids_are_stable_and_domain_separated() {
        let operation = OperationId::from_bytes([1; 16]);
        let input = RowId::from_bytes([2; 16]);
        assert_eq!(
            RowId::derived(operation, &[input], b"group-a"),
            RowId::derived(operation, &[input], b"group-a")
        );
        assert_ne!(
            RowId::derived(operation, &[input], b"group-a"),
            RowId::derived(operation, &[input], b"group-b")
        );
    }
}
