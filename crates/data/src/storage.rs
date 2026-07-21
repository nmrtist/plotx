use crate::{ColumnChunk, ColumnValues, DataError, LogicalType, Result, Validity};
use arrow::{
    array::{
        Array, ArrayRef, BooleanArray, Date32Array, DurationNanosecondArray, Float64Array,
        Int64Array, NullArray, StringArray, Time64NanosecondArray, TimestampNanosecondArray,
        UInt32Array,
    },
    datatypes::{Field, Schema},
    ipc::{reader::FileReader, writer::FileWriter},
    record_batch::RecordBatch,
};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, HashMap},
    fmt, fs,
    io::{Cursor, Write},
    path::{Path, PathBuf},
    sync::{Arc, RwLock},
};

pub const ARROW_IPC_CODEC_V1: &str = "space.nmrtist.plotx.arrow-ipc.v1";

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ContentHash([u8; 32]);

impl ContentHash {
    pub fn of(bytes: &[u8]) -> Self {
        let digest = Sha256::digest(bytes);
        let mut value = [0; 32];
        value.copy_from_slice(&digest);
        Self(value)
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    pub fn from_hex(value: &str) -> Result<Self> {
        if value.len() != 64 {
            return Err(DataError::CorruptBlock("invalid hash length".into()));
        }
        let mut bytes = [0; 32];
        for (index, byte) in bytes.iter_mut().enumerate() {
            *byte = u8::from_str_radix(&value[index * 2..index * 2 + 2], 16)
                .map_err(|_| DataError::CorruptBlock("invalid hexadecimal hash".into()))?;
        }
        Ok(Self(bytes))
    }
}

impl fmt::Display for ContentHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in self.0 {
            write!(f, "{byte:02x}")?;
        }
        Ok(())
    }
}

impl fmt::Debug for ContentHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

impl Serialize for ContentHash {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for ContentHash {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::from_hex(&value).map_err(serde::de::Error::custom)
    }
}

pub trait BlockStore: Send + Sync {
    fn put(&self, bytes: Vec<u8>) -> Result<ContentHash>;
    fn get(&self, hash: ContentHash) -> Result<Vec<u8>>;
    fn contains(&self, hash: ContentHash) -> Result<bool>;
}

#[derive(Default)]
pub struct MemoryBlockStore {
    blocks: RwLock<HashMap<ContentHash, Vec<u8>>>,
}

impl MemoryBlockStore {
    pub fn block_count(&self) -> usize {
        self.blocks.read().map_or(0, |blocks| blocks.len())
    }
}

impl BlockStore for MemoryBlockStore {
    fn put(&self, bytes: Vec<u8>) -> Result<ContentHash> {
        let hash = ContentHash::of(&bytes);
        self.blocks
            .write()
            .map_err(|_| DataError::Backend("block store lock is poisoned".into()))?
            .entry(hash)
            .or_insert(bytes);
        Ok(hash)
    }

    fn get(&self, hash: ContentHash) -> Result<Vec<u8>> {
        self.blocks
            .read()
            .map_err(|_| DataError::Backend("block store lock is poisoned".into()))?
            .get(&hash)
            .cloned()
            .ok_or_else(|| DataError::MissingBlock(hash.to_string()))
    }

    fn contains(&self, hash: ContentHash) -> Result<bool> {
        Ok(self
            .blocks
            .read()
            .map_err(|_| DataError::Backend("block store lock is poisoned".into()))?
            .contains_key(&hash))
    }
}

/// Persistent content-addressed block store. Blocks are immutable and written
/// atomically; opening a block never requires loading any sibling block.
pub struct DirectoryBlockStore {
    root: PathBuf,
}

impl DirectoryBlockStore {
    pub fn open(root: impl Into<PathBuf>) -> Result<Self> {
        let root = root.into();
        if root.as_os_str().is_empty() {
            return Err(DataError::Backend("block-store path is empty".into()));
        }
        fs::create_dir_all(&root).map_err(|error| DataError::Backend(error.to_string()))?;
        let root = root
            .canonicalize()
            .map_err(|error| DataError::Backend(error.to_string()))?;
        Ok(Self { root })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn path(&self, hash: ContentHash) -> PathBuf {
        let hash = hash.to_string();
        self.root.join(&hash[..2]).join(hash)
    }
}

impl BlockStore for DirectoryBlockStore {
    fn put(&self, bytes: Vec<u8>) -> Result<ContentHash> {
        let hash = ContentHash::of(&bytes);
        let path = self.path(hash);
        if path.exists() {
            return Ok(hash);
        }
        let parent = path
            .parent()
            .ok_or_else(|| DataError::Backend("block path has no parent".into()))?;
        fs::create_dir_all(parent).map_err(|error| DataError::Backend(error.to_string()))?;
        let temporary = parent.join(format!(".{}.{}.tmp", hash, uuid::Uuid::new_v4()));
        let result = (|| {
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temporary)
                .map_err(|error| DataError::Backend(error.to_string()))?;
            file.write_all(&bytes)
                .and_then(|()| file.sync_all())
                .map_err(|error| DataError::Backend(error.to_string()))?;
            match fs::rename(&temporary, &path) {
                Ok(()) => Ok(()),
                Err(_) if path.exists() => {
                    fs::remove_file(&temporary)
                        .map_err(|error| DataError::Backend(error.to_string()))?;
                    Ok(())
                }
                Err(error) => Err(DataError::Backend(error.to_string())),
            }
        })();
        if result.is_err() && temporary.exists() {
            let _ = fs::remove_file(&temporary);
        }
        result?;
        Ok(hash)
    }

    fn get(&self, hash: ContentHash) -> Result<Vec<u8>> {
        let bytes = fs::read(self.path(hash)).map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                DataError::MissingBlock(hash.to_string())
            } else {
                DataError::Backend(error.to_string())
            }
        })?;
        if ContentHash::of(&bytes) != hash {
            return Err(DataError::CorruptBlock(format!(
                "block {hash} does not match its content hash"
            )));
        }
        Ok(bytes)
    }

    fn contains(&self, hash: ContentHash) -> Result<bool> {
        Ok(self.path(hash).is_file())
    }
}

pub trait ChunkCodec: Send + Sync {
    fn id(&self) -> &'static str;
    fn encode(&self, logical_type: &LogicalType, chunk: &ColumnChunk) -> Result<Vec<u8>>;
    fn decode(&self, logical_type: &LogicalType, bytes: &[u8]) -> Result<ColumnChunk>;
}

#[derive(Default)]
pub struct CodecRegistry {
    codecs: BTreeMap<String, Arc<dyn ChunkCodec>>,
}

impl CodecRegistry {
    pub fn with_arrow_ipc() -> Self {
        let mut registry = Self::default();
        registry.register(Arc::new(ArrowIpcCodec));
        registry
    }

    pub fn register(&mut self, codec: Arc<dyn ChunkCodec>) {
        self.codecs.insert(codec.id().into(), codec);
    }

    pub fn get(&self, id: &str) -> Result<&dyn ChunkCodec> {
        self.codecs
            .get(id)
            .map(AsRef::as_ref)
            .ok_or_else(|| DataError::UnknownCodec(id.into()))
    }
}

pub struct ArrowIpcCodec;

impl ChunkCodec for ArrowIpcCodec {
    fn id(&self) -> &'static str {
        ARROW_IPC_CODEC_V1
    }

    fn encode(&self, logical_type: &LogicalType, chunk: &ColumnChunk) -> Result<Vec<u8>> {
        chunk.validate_type(logical_type)?;
        let array = to_arrow(chunk.values(), chunk.validity())?;
        let mut metadata = HashMap::from([
            ("plotx.codec".into(), ARROW_IPC_CODEC_V1.into()),
            (
                "plotx.logical_type".into(),
                logical_type_name(logical_type).into(),
            ),
        ]);
        if let LogicalType::Extension(extension) = logical_type {
            metadata.insert("plotx.extension_id".into(), extension.id.clone());
        }
        let field = Field::new("value", array.data_type().clone(), true).with_metadata(metadata);
        let schema = Arc::new(Schema::new(vec![field]));
        let batch = RecordBatch::try_new(schema.clone(), vec![array])
            .map_err(|error| DataError::Backend(error.to_string()))?;
        let mut bytes = Vec::new();
        {
            let mut writer = FileWriter::try_new(&mut bytes, &schema)
                .map_err(|error| DataError::Backend(error.to_string()))?;
            writer
                .write(&batch)
                .map_err(|error| DataError::Backend(error.to_string()))?;
            writer
                .finish()
                .map_err(|error| DataError::Backend(error.to_string()))?;
        }
        Ok(bytes)
    }

    fn decode(&self, logical_type: &LogicalType, bytes: &[u8]) -> Result<ColumnChunk> {
        let mut reader = FileReader::try_new(Cursor::new(bytes), None)
            .map_err(|error| DataError::CorruptBlock(error.to_string()))?;
        let batch = reader
            .next()
            .transpose()
            .map_err(|error| DataError::CorruptBlock(error.to_string()))?
            .ok_or_else(|| DataError::CorruptBlock("Arrow IPC file contains no batch".into()))?;
        if reader.next().is_some() || batch.num_columns() != 1 {
            return Err(DataError::CorruptBlock(
                "a PlotX column block must contain exactly one batch and one column".into(),
            ));
        }
        from_arrow(logical_type, batch.column(0).as_ref())
    }
}

fn optional_values<T: Copy>(values: &[T], validity: &Validity) -> Vec<Option<T>> {
    values
        .iter()
        .enumerate()
        .map(|(index, value)| validity.is_valid(index).unwrap_or(false).then_some(*value))
        .collect()
}

#[doc(hidden)]
pub fn to_arrow(values: &ColumnValues, validity: &Validity) -> Result<ArrayRef> {
    let array: ArrayRef = match values {
        ColumnValues::Null(len) => Arc::new(NullArray::new(*len)),
        ColumnValues::Boolean(values) => Arc::new(BooleanArray::from(
            values
                .iter()
                .enumerate()
                .map(|(index, value)| validity.is_valid(index).unwrap_or(false).then_some(*value))
                .collect::<Vec<_>>(),
        )),
        ColumnValues::Int64(values) => {
            Arc::new(Int64Array::from(optional_values(values, validity)))
        }
        ColumnValues::Float64(values) => {
            Arc::new(Float64Array::from(optional_values(values, validity)))
        }
        ColumnValues::Utf8(values) => Arc::new(StringArray::from(
            values
                .iter()
                .enumerate()
                .map(|(index, value)| {
                    validity
                        .is_valid(index)
                        .unwrap_or(false)
                        .then_some(value.as_str())
                })
                .collect::<Vec<_>>(),
        )),
        ColumnValues::Categorical(values) => {
            Arc::new(UInt32Array::from(optional_values(values, validity)))
        }
        ColumnValues::Date(values) => {
            Arc::new(Date32Array::from(optional_values(values, validity)))
        }
        ColumnValues::Time(values) => Arc::new(Time64NanosecondArray::from(optional_values(
            values, validity,
        ))),
        ColumnValues::Timestamp(values) => Arc::new(
            TimestampNanosecondArray::from(optional_values(values, validity)).with_timezone("UTC"),
        ),
        ColumnValues::Duration(values) => Arc::new(DurationNanosecondArray::from(optional_values(
            values, validity,
        ))),
        ColumnValues::Extension { storage, .. } => to_arrow(storage, validity)?,
    };
    Ok(array)
}

#[doc(hidden)]
pub fn from_arrow(logical_type: &LogicalType, array: &dyn Array) -> Result<ColumnChunk> {
    if let LogicalType::Extension(extension) = logical_type {
        let storage = from_arrow(&extension.storage, array)?;
        return ColumnChunk::new(
            ColumnValues::Extension {
                type_id: extension.id.clone(),
                storage: Box::new(storage.values().clone()),
            },
            storage.validity().clone(),
        );
    }
    // Arrow's NullArray has no validity buffer and its generic `is_valid`
    // implementation therefore cannot be used to reconstruct PlotX's explicit
    // all-null bitmap.
    let validity = if matches!(logical_type, LogicalType::Null) {
        Validity::all_null(array.len())
    } else {
        Validity::from_valid((0..array.len()).map(|index| array.is_valid(index)))
    };
    let values = match logical_type {
        LogicalType::Null => ColumnValues::Null(array.len()),
        LogicalType::Boolean => {
            ColumnValues::Boolean(downcast::<BooleanArray>(array)?.values().iter().collect())
        }
        LogicalType::Int64 => ColumnValues::Int64(downcast::<Int64Array>(array)?.values().to_vec()),
        LogicalType::Float64 => {
            ColumnValues::Float64(downcast::<Float64Array>(array)?.values().to_vec())
        }
        LogicalType::Utf8 => ColumnValues::Utf8(
            downcast::<StringArray>(array)?
                .iter()
                .map(|value| value.unwrap_or_default().into())
                .collect(),
        ),
        LogicalType::Categorical { .. } => {
            ColumnValues::Categorical(downcast::<UInt32Array>(array)?.values().to_vec())
        }
        LogicalType::Date => ColumnValues::Date(downcast::<Date32Array>(array)?.values().to_vec()),
        LogicalType::Time => {
            ColumnValues::Time(downcast::<Time64NanosecondArray>(array)?.values().to_vec())
        }
        LogicalType::Timestamp { .. } => ColumnValues::Timestamp(
            downcast::<TimestampNanosecondArray>(array)?
                .values()
                .to_vec(),
        ),
        LogicalType::Duration => ColumnValues::Duration(
            downcast::<DurationNanosecondArray>(array)?
                .values()
                .to_vec(),
        ),
        LogicalType::Extension(_) => unreachable!(),
    };
    ColumnChunk::new(values, validity)
}

fn downcast<T: 'static>(array: &dyn Array) -> Result<&T> {
    array.as_any().downcast_ref().ok_or_else(|| {
        DataError::CorruptBlock(format!("unexpected Arrow type {:?}", array.data_type()))
    })
}

fn logical_type_name(logical_type: &LogicalType) -> &'static str {
    match logical_type {
        LogicalType::Null => "null",
        LogicalType::Boolean => "boolean",
        LogicalType::Int64 => "int64",
        LogicalType::Float64 => "float64",
        LogicalType::Utf8 => "utf8",
        LogicalType::Categorical { .. } => "categorical",
        LogicalType::Date => "date",
        LogicalType::Time => "time",
        LogicalType::Timestamp { .. } => "timestamp",
        LogicalType::Duration => "duration",
        LogicalType::Extension(_) => "extension",
    }
}

pub fn logical_fingerprint(logical_type: &LogicalType, chunk: &ColumnChunk) -> Result<ContentHash> {
    let mut canonical =
        serde_json::to_vec(logical_type).map_err(|error| DataError::Backend(error.to_string()))?;
    canonical.extend_from_slice(&(chunk.len() as u64).to_le_bytes());
    canonical.extend_from_slice(chunk.validity().bytes());
    append_values(&mut canonical, chunk.values(), chunk.validity());
    Ok(ContentHash::of(&canonical))
}

fn append_values(output: &mut Vec<u8>, values: &ColumnValues, validity: &Validity) {
    match values {
        ColumnValues::Null(_) => {}
        ColumnValues::Boolean(values) => output.extend(
            values
                .iter()
                .enumerate()
                .map(|(index, value)| u8::from(validity.is_valid(index) == Some(true) && *value)),
        ),
        ColumnValues::Int64(values)
        | ColumnValues::Time(values)
        | ColumnValues::Timestamp(values)
        | ColumnValues::Duration(values) => {
            for (index, value) in values.iter().enumerate() {
                output.extend_from_slice(
                    &validity
                        .is_valid(index)
                        .filter(|valid| *valid)
                        .map_or(0, |_| *value)
                        .to_le_bytes(),
                );
            }
        }
        ColumnValues::Float64(values) => {
            for (index, value) in values.iter().enumerate() {
                let bits = validity
                    .is_valid(index)
                    .filter(|valid| *valid)
                    .map_or(0, |_| value.to_bits());
                output.extend_from_slice(&bits.to_le_bytes());
            }
        }
        ColumnValues::Utf8(values) => {
            for (index, value) in values.iter().enumerate() {
                let value = validity
                    .is_valid(index)
                    .filter(|valid| *valid)
                    .map_or("", |_| value.as_str());
                output.extend_from_slice(&(value.len() as u64).to_le_bytes());
                output.extend_from_slice(value.as_bytes());
            }
        }
        ColumnValues::Categorical(values) => {
            for (index, value) in values.iter().enumerate() {
                let value = validity
                    .is_valid(index)
                    .filter(|valid| *valid)
                    .map_or(0, |_| *value);
                output.extend_from_slice(&value.to_le_bytes());
            }
        }
        ColumnValues::Date(values) => {
            for (index, value) in values.iter().enumerate() {
                let value = validity
                    .is_valid(index)
                    .filter(|valid| *valid)
                    .map_or(0, |_| *value);
                output.extend_from_slice(&value.to_le_bytes());
            }
        }
        ColumnValues::Extension { type_id, storage } => {
            output.extend_from_slice(type_id.as_bytes());
            append_values(output, storage, validity);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arrow_codec_round_trips_null_nan_and_infinities() {
        let input = ColumnChunk::new(
            ColumnValues::Float64(vec![f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 4.0]),
            Validity::from_valid([true, true, true, false]),
        )
        .unwrap();
        let codec = ArrowIpcCodec;
        let encoded = codec.encode(&LogicalType::Float64, &input).unwrap();
        let output = codec.decode(&LogicalType::Float64, &encoded).unwrap();
        assert!(
            matches!(output.value(0), Some(crate::ScalarValue::Float64(value)) if value.is_nan())
        );
        assert_eq!(
            output.value(1),
            Some(crate::ScalarValue::Float64(f64::INFINITY))
        );
        assert_eq!(
            output.value(2),
            Some(crate::ScalarValue::Float64(f64::NEG_INFINITY))
        );
        assert_eq!(output.value(3), Some(crate::ScalarValue::Null));
    }

    #[test]
    fn arrow_codec_round_trips_every_v1_logical_storage_type() {
        let categorical = LogicalType::Categorical {
            levels: vec![
                crate::CategoryLevel {
                    value: "control".into(),
                    label: Some("Control".into()),
                },
                crate::CategoryLevel {
                    value: "treated".into(),
                    label: None,
                },
            ],
        };
        let extension = crate::ExtensionType {
            id: "space.nmrtist.plotx.test-value".into(),
            version: 1,
            storage: Box::new(LogicalType::Int64),
            semantics_critical: false,
        };
        let cases = vec![
            (LogicalType::Null, ColumnValues::Null(2)),
            (
                LogicalType::Boolean,
                ColumnValues::Boolean(vec![true, false]),
            ),
            (LogicalType::Int64, ColumnValues::Int64(vec![-2, 7])),
            (
                LogicalType::Utf8,
                ColumnValues::Utf8(vec!["α".into(), "".into()]),
            ),
            (categorical, ColumnValues::Categorical(vec![0, 1])),
            (LogicalType::Date, ColumnValues::Date(vec![0, 20_000])),
            (
                LogicalType::Time,
                ColumnValues::Time(vec![0, 86_399_000_000_000]),
            ),
            (
                LogicalType::Timestamp {
                    display_timezone: "Asia/Singapore".into(),
                },
                ColumnValues::Timestamp(vec![0, 1_721_430_123_000_000_000]),
            ),
            (
                LogicalType::Duration,
                ColumnValues::Duration(vec![-1_000, 2_000]),
            ),
            (
                LogicalType::Extension(extension.clone()),
                ColumnValues::Extension {
                    type_id: extension.id,
                    storage: Box::new(ColumnValues::Int64(vec![3, 4])),
                },
            ),
        ];
        let codec = ArrowIpcCodec;
        for (logical_type, values) in cases {
            let validity = if matches!(logical_type, LogicalType::Null) {
                Validity::all_null(2)
            } else {
                Validity::from_valid([true, false])
            };
            let input = ColumnChunk::new(values, validity).unwrap();
            let encoded = codec.encode(&logical_type, &input).unwrap();
            let output = codec.decode(&logical_type, &encoded).unwrap();
            assert_eq!(
                output.validity(),
                input.validity(),
                "validity changed for {logical_type:?}"
            );
            for index in 0..input.len() {
                assert_eq!(
                    output.value(index),
                    input.value(index),
                    "logical value changed for {logical_type:?}"
                );
            }
            assert_eq!(
                logical_fingerprint(&logical_type, &output).unwrap(),
                logical_fingerprint(&logical_type, &input).unwrap(),
                "logical fingerprint changed for {logical_type:?}"
            );
        }
    }

    #[test]
    fn memory_store_deduplicates_identical_bytes() {
        let store = MemoryBlockStore::default();
        assert_eq!(
            store.put(vec![1, 2, 3]).unwrap(),
            store.put(vec![1, 2, 3]).unwrap()
        );
        assert_eq!(store.block_count(), 1);
    }

    #[test]
    fn directory_store_distinguishes_missing_and_corrupt_blocks() {
        let directory = std::env::temp_dir().join(format!(
            "plotx-block-store-test-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        let store = DirectoryBlockStore::open(&directory).unwrap();
        let hash = store.put(vec![1, 2, 3]).unwrap();
        let hash_text = hash.to_string();
        let path = directory.join(&hash_text[..2]).join(&hash_text);
        std::fs::write(path, [9, 9, 9]).unwrap();
        assert!(matches!(store.get(hash), Err(DataError::CorruptBlock(_))));

        let missing = ContentHash::of(b"absent");
        assert!(matches!(
            store.get(missing),
            Err(DataError::MissingBlock(_))
        ));
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn codec_registry_rejects_unknown_codec_ids() {
        let codecs = CodecRegistry::with_arrow_ipc();
        assert!(matches!(
            codecs.get("space.nmrtist.plotx.unknown.v1"),
            Err(DataError::UnknownCodec(_))
        ));
    }
}
