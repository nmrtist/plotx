use super::FieldId;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

/// Dataset-owned allocator and persisted lookup table for field child resources.
/// The key is supplied by the provider and identifies the actual channel/plane;
/// the numeric `FieldId` is only an owner-local reference, never an array index.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FieldCatalog {
    next_id: u64,
    key_to_id: BTreeMap<String, FieldId>,
}

impl FieldCatalog {
    pub fn for_keys(keys: impl IntoIterator<Item = String>) -> Self {
        let mut catalog = Self {
            next_id: 0,
            key_to_id: BTreeMap::new(),
        };
        for key in keys {
            catalog.allocate(key);
        }
        catalog
    }

    pub fn id_for_key(&self, key: &str) -> Option<FieldId> {
        self.key_to_id.get(key).copied()
    }

    fn allocate(&mut self, key: String) {
        if self.key_to_id.contains_key(&key) {
            return;
        }
        let id = FieldId::new(self.next_id);
        self.next_id = self
            .next_id
            .checked_add(1)
            .expect("dataset field identity allocator overflow");
        self.key_to_id.insert(key, id);
    }

    pub(crate) fn validate_for_keys(&self, keys: Vec<String>) -> Result<(), String> {
        let supplied_len = keys.len();
        let expected = keys.into_iter().collect::<BTreeSet<_>>();
        if expected.len() != supplied_len
            || expected.len() != self.key_to_id.len()
            || !expected.iter().all(|key| self.key_to_id.contains_key(key))
        {
            return Err("field catalog does not match this dataset's stable field keys".to_owned());
        }
        let ids = self.key_to_id.values().copied().collect::<BTreeSet<_>>();
        if ids.len() != self.key_to_id.len() {
            return Err("field catalog contains duplicate field identities".to_owned());
        }
        let minimum_next = ids
            .last()
            .map_or(Some(0), |id| id.get().checked_add(1))
            .ok_or_else(|| "field catalog identity allocator overflow".to_owned())?;
        if self.next_id < minimum_next {
            return Err("field catalog allocator would reuse an existing identity".to_owned());
        }
        Ok(())
    }
}

pub(crate) fn nmr_field_catalog() -> FieldCatalog {
    FieldCatalog::for_keys(["nmr.real".to_owned()])
}

pub(crate) fn nmr2d_field_catalog() -> FieldCatalog {
    FieldCatalog::for_keys([
        "nmr.real".to_owned(),
        "nmr.magnitude".to_owned(),
        "nmr.stack".to_owned(),
    ])
}

pub(crate) fn table_field_catalog() -> FieldCatalog {
    FieldCatalog::for_keys(["table.default_series".to_owned()])
}

#[cfg(test)]
pub(crate) fn afm_field_catalog(data: &plotx_io::AfmData) -> FieldCatalog {
    let image_keys = afm_channel_keys(data);
    afm_field_catalog_for_keys(data, &image_keys)
}

pub(crate) fn afm_channel_keys(data: &plotx_io::AfmData) -> Arc<[String]> {
    data.images.iter().map(afm_channel_key).collect()
}

pub(crate) fn afm_field_catalog_for_keys(
    data: &plotx_io::AfmData,
    image_keys: &[String],
) -> FieldCatalog {
    FieldCatalog::for_keys(
        data.forces
            .as_ref()
            .map(|_| "afm.force_curve".to_owned())
            .into_iter()
            .chain(image_keys.iter().cloned()),
    )
}

pub(crate) fn electrophysiology_channel_keys(
    data: &plotx_io::ElectrophysiologyData,
) -> Arc<[Option<String>]> {
    (0..data.channels.len())
        .map(|index| electrophysiology_data_channel_key(data, index))
        .collect()
}

pub(crate) fn electrophysiology_field_catalog_for_keys(
    channel_keys: &[Option<String>],
) -> FieldCatalog {
    FieldCatalog::for_keys(channel_keys.iter().filter_map(|key| key.clone()))
}

pub(crate) fn afm_channel_key(channel: &plotx_io::AfmImageChannel) -> String {
    #[cfg(test)]
    count_afm_channel_key_computation();

    let mut hash = StableFieldHasher::new();
    hash.write_str(&channel.name);
    hash.write_usize(channel.width);
    hash.write_usize(channel.height);
    hash.write(&channel.scan_size_x.to_le_bytes());
    hash.write(&channel.scan_size_y.to_le_bytes());
    hash.write_str(&channel.lateral_unit);
    hash.write(&channel.scale.multiplier.to_le_bytes());
    hash.write(&channel.scale.offset.to_le_bytes());
    hash.write_str(&channel.scale.unit);
    hash.write_afm_frame_direction(channel.frame_direction);
    for value in channel.raw.iter() {
        hash.write(&value.to_le_bytes());
    }
    format!("afm.channel.{:016x}", hash.finish())
}

pub(crate) fn electrophysiology_channel_key(
    recording: &super::ElectrophysiologyDataset,
    channel: usize,
) -> Option<String> {
    recording.field_key(channel).map(str::to_owned)
}

fn electrophysiology_data_channel_key(
    data: &plotx_io::ElectrophysiologyData,
    channel: usize,
) -> Option<String> {
    let metadata = data.channels.get(channel)?;
    let mut hash = StableFieldHasher::new();
    hash.write_str(&metadata.name);
    hash.write_str(&metadata.unit.symbol);
    hash.write_electrical_quantity(metadata.unit.quantity);
    for sweep in &data.sweeps {
        for value in sweep.channels.get(channel)? {
            hash.write(&value.to_le_bytes());
        }
    }
    Some(format!("electrophysiology.channel.{:016x}", hash.finish()))
}

struct StableFieldHasher(u64);

impl StableFieldHasher {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;

    fn new() -> Self {
        Self(Self::OFFSET)
    }

    fn write(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.0 ^= u64::from(*byte);
            self.0 = self.0.wrapping_mul(Self::PRIME);
        }
    }

    fn write_str(&mut self, value: &str) {
        self.write(value.as_bytes());
        self.write(&[0]);
    }

    fn write_usize(&mut self, value: usize) {
        self.write(&(value as u64).to_le_bytes());
    }

    fn write_afm_frame_direction(&mut self, value: plotx_io::AfmFrameDirection) {
        let value = match value {
            plotx_io::AfmFrameDirection::Trace => 0,
            plotx_io::AfmFrameDirection::Retrace => 1,
            plotx_io::AfmFrameDirection::Unknown => 2,
        };
        self.write(&[value]);
    }

    fn write_electrical_quantity(&mut self, value: plotx_io::ElectricalQuantity) {
        let value = match value {
            plotx_io::ElectricalQuantity::Voltage => 0,
            plotx_io::ElectricalQuantity::Current => 1,
            plotx_io::ElectricalQuantity::Unknown => 2,
        };
        self.write(&[value]);
    }

    fn finish(self) -> u64 {
        self.0
    }
}

#[cfg(test)]
thread_local! {
    static AFM_CHANNEL_KEY_COMPUTATIONS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
fn count_afm_channel_key_computation() {
    AFM_CHANNEL_KEY_COMPUTATIONS.with(|count| count.set(count.get() + 1));
}

#[cfg(test)]
pub(crate) fn reset_afm_channel_key_computations() {
    AFM_CHANNEL_KEY_COMPUTATIONS.with(|count| count.set(0));
}

#[cfg(test)]
pub(crate) fn afm_channel_key_computations() -> usize {
    AFM_CHANNEL_KEY_COMPUTATIONS.with(std::cell::Cell::get)
}
