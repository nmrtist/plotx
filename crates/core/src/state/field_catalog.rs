use super::{FieldAlgorithmProvenance, FieldId, FieldProvenance};
use plotx_data::{
    TraceCollectionCatalog, TraceCollectionId, TraceItemDescriptor, TraceItemId,
    TraceItemParameter, TraceParameterValue,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

/// Dataset-owned allocator and persisted lookup table for field child resources.
/// The key is supplied by the provider and identifies the actual channel/plane;
/// the numeric `FieldId` is only an owner-local reference, never an array index.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FieldCatalog {
    next_id: u64,
    key_to_id: BTreeMap<String, FieldId>,
    /// Identities of removed derived fields. Kept outside the live catalog so
    /// undo/redo can restore the same field identity without exposing a field
    /// that no longer exists.
    retired_key_to_id: BTreeMap<String, FieldId>,
    /// Persisted source and algorithm provenance for each stable child field.
    /// Runtime `FieldVersion` deliberately does not live here.
    provenance: BTreeMap<FieldId, FieldProvenance>,
    trace_collections: BTreeMap<FieldId, TraceCollectionCatalog>,
}

impl FieldCatalog {
    pub fn for_keys(keys: impl IntoIterator<Item = String>) -> Self {
        let mut catalog = Self {
            next_id: 0,
            key_to_id: BTreeMap::new(),
            retired_key_to_id: BTreeMap::new(),
            provenance: BTreeMap::new(),
            trace_collections: BTreeMap::new(),
        };
        for key in keys {
            catalog.activate_key(key);
        }
        catalog
    }

    pub fn id_for_key(&self, key: &str) -> Option<FieldId> {
        self.key_to_id.get(key).copied()
    }

    pub fn trace_collection(&self, field: FieldId) -> Option<&TraceCollectionCatalog> {
        self.trace_collections.get(&field)
    }

    pub(crate) fn trace_collection_mut(
        &mut self,
        field: FieldId,
    ) -> Option<&mut TraceCollectionCatalog> {
        self.trace_collections.get_mut(&field)
    }

    pub(crate) fn set_trace_collection(
        &mut self,
        field: FieldId,
        collection: TraceCollectionCatalog,
    ) {
        self.trace_collections.insert(field, collection);
    }

    pub(crate) fn provenance_for(&self, id: FieldId) -> Option<&FieldProvenance> {
        self.provenance.get(&id)
    }

    /// Record source identity and implementation identity independently of the
    /// session-only runtime version. The catalog itself is already persisted in
    /// each project's `plotx.fields` data extension.
    pub(crate) fn attach_provenance(
        &mut self,
        source: &str,
        algorithm: Option<FieldAlgorithmProvenance>,
    ) {
        for id in self.key_to_id.values().copied() {
            self.provenance
                .insert(id, Self::make_provenance(source, id, algorithm.clone()));
        }
    }

    pub(crate) fn make_provenance(
        source: &str,
        id: FieldId,
        algorithm: Option<FieldAlgorithmProvenance>,
    ) -> FieldProvenance {
        let mut digest = Sha256::new();
        digest.update(source.as_bytes());
        digest.update(id.get().to_le_bytes());
        FieldProvenance {
            source_fingerprint: Some(format!("{:x}", digest.finalize())),
            algorithm,
            metadata: BTreeMap::new(),
        }
    }

    /// Activate a key by the same rule used for planning and reconciliation:
    /// live first, then a retired identity, then a new allocation.
    fn activate_key(&mut self, key: String) -> FieldId {
        if let Some(id) = self.id_for_key(&key) {
            return id;
        }
        if let Some(id) = self.retired_key_to_id.remove(&key) {
            self.key_to_id.insert(key, id);
            return id;
        }
        let id = FieldId::new(self.next_id);
        self.next_id = self
            .next_id
            .checked_add(1)
            .expect("dataset field identity allocator overflow");
        self.key_to_id.insert(key, id);
        id
    }

    pub(crate) fn ensure_key(&mut self, key: String) -> FieldId {
        self.activate_key(key)
    }

    /// Reconcile a provider's current keys without renumbering surviving fields.
    /// Derived results are added and removed through actions, so their child
    /// identities must remain stable across catalog rebuilds and project reloads.
    pub(crate) fn reconcile_keys(
        &mut self,
        keys: impl IntoIterator<Item = String>,
        source: &str,
        algorithm: Option<FieldAlgorithmProvenance>,
    ) {
        let expected = keys.into_iter().collect::<BTreeSet<_>>();
        let removed = self
            .key_to_id
            .iter()
            .filter(|(key, _)| !expected.contains(*key))
            .map(|(key, id)| (key.clone(), *id))
            .collect::<Vec<_>>();
        for (key, id) in removed {
            self.key_to_id.remove(&key);
            self.retired_key_to_id.insert(key, id);
        }
        let live = self.key_to_id.values().copied().collect::<BTreeSet<_>>();
        self.provenance.retain(|id, _| live.contains(id));
        for key in expected {
            self.activate_key(key);
        }
        self.attach_provenance(source, algorithm);
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
        let retired = self
            .retired_key_to_id
            .values()
            .copied()
            .collect::<BTreeSet<_>>();
        if retired.len() != self.retired_key_to_id.len() || !ids.is_disjoint(&retired) {
            return Err("field catalog contains conflicting retired identities".to_owned());
        }
        let minimum_next = ids
            .last()
            .into_iter()
            .chain(retired.last())
            .max()
            .map_or(Some(0), |id| id.get().checked_add(1))
            .ok_or_else(|| "field catalog identity allocator overflow".to_owned())?;
        if self.next_id < minimum_next {
            return Err("field catalog allocator would reuse an existing identity".to_owned());
        }
        if self.provenance.len() != self.key_to_id.len()
            || !ids.iter().all(|id| self.provenance.contains_key(id))
        {
            return Err("field catalog does not carry provenance for every field".to_owned());
        }
        let mut collection_ids = BTreeSet::new();
        for (field, collection) in &self.trace_collections {
            if !ids.contains(field) {
                return Err(format!("trace collection references unknown field {field}"));
            }
            if !collection_ids.insert(collection.id) {
                return Err(format!(
                    "field catalog contains duplicate trace collection id {}",
                    collection.id
                ));
            }
            collection.validate()?;
        }
        Ok(())
    }
}

pub(crate) fn pseudo_axis_display_scale(unit: &str) -> f64 {
    match unit {
        "ms" => 1e3,
        "us" | "µs" => 1e6,
        "ns" => 1e9,
        "mT/m" => 1e3,
        "G/cm" => 1e2,
        "G/mm" => 10.0,
        _ => 1.0,
    }
}

pub(crate) fn attach_pseudo_trace_collection(
    catalog: &mut FieldCatalog,
    data: &plotx_io::NmrData2D,
) {
    let Some(field) = catalog.id_for_key("nmr.stack") else {
        return;
    };
    let count = data.rows;
    let collection = TraceCollectionId::derived(data.source.as_bytes(), b"nmr.stack");
    let (quantity, unit) = data.pseudo_axis.as_ref().map_or(("Increment", ""), |axis| {
        (
            match axis.kind {
                plotx_io::PseudoKind::Gradient => "Gradient strength",
                plotx_io::PseudoKind::Delay => "Relaxation delay",
                plotx_io::PseudoKind::Generic => axis.name.as_str(),
            },
            axis.unit.as_str(),
        )
    });
    let scale = pseudo_axis_display_scale(unit);
    let items = (0..count)
        .map(|index| {
            let value = data
                .pseudo_axis
                .as_ref()
                .and_then(|axis| axis.values.get(index))
                .copied()
                .unwrap_or((index + 1) as f64)
                * scale;
            TraceItemDescriptor {
                id: TraceItemId::derived(collection, &(index as u64).to_le_bytes()),
                parameters: vec![TraceItemParameter {
                    key: "axis_value".into(),
                    name: quantity.into(),
                    value: TraceParameterValue::Number {
                        value,
                        unit: unit.into(),
                    },
                }],
                primary_label_parameter: "axis_value".into(),
                label_override: None,
            }
        })
        .collect();
    catalog.set_trace_collection(
        field,
        TraceCollectionCatalog {
            id: collection,
            axis_quantity: quantity.into(),
            axis_unit: unit.into(),
            items,
        },
    );
}

pub(crate) fn attach_electrophysiology_trace_collections(
    catalog: &mut FieldCatalog,
    data: &plotx_io::ElectrophysiologyData,
    stimulus: Option<&super::StimulusDefinition>,
) {
    let abf_stimulus = super::resolve_abf_stimulus(data);
    let fields = (0..data.channels.len())
        .filter_map(|channel| {
            let key = electrophysiology_data_channel_key(data, channel)?;
            Some((key.clone(), catalog.id_for_key(&key)?))
        })
        .collect::<Vec<_>>();
    for (key, field) in fields {
        let collection = TraceCollectionId::derived(data.source.as_bytes(), key.as_bytes());
        let items = data
            .sweeps
            .iter()
            .enumerate()
            .map(|(index, _sweep)| {
                let mut parameters = vec![TraceItemParameter {
                    key: "sweep".into(),
                    name: "Sweep".into(),
                    value: TraceParameterValue::Text {
                        value: format!("Sweep {}", index + 1),
                    },
                }];
                let resolved_level = abf_stimulus
                    .as_ref()
                    .and_then(|resolved| {
                        resolved
                            .values
                            .get(index)
                            .map(|level| (resolved.name.clone(), resolved.unit.clone(), *level))
                    })
                    .or_else(|| {
                        _sweep.commands.first().map(|command| {
                            (
                                command.name.clone(),
                                command.unit.symbol.clone(),
                                super::command_level(command),
                            )
                        })
                    });
                let primary = if let Some((name, unit, level)) = resolved_level {
                    parameters.push(TraceItemParameter {
                        key: "abf_stimulus".into(),
                        name,
                        value: TraceParameterValue::Number { value: level, unit },
                    });
                    "abf_stimulus"
                } else if let Some((value, unit)) = stimulus
                    .filter(|definition| definition.confirmed)
                    .and_then(|definition| match definition.protocol {
                        super::StimulusProtocol::VoltageStep {
                            start_mv, step_mv, ..
                        } => Some((start_mv + index as f64 * step_mv, "mV")),
                        super::StimulusProtocol::CurrentStep {
                            start_pa, step_pa, ..
                        } => Some((start_pa + index as f64 * step_pa, "pA")),
                        _ => None,
                    })
                {
                    parameters.push(TraceItemParameter {
                        key: "stimulus_template".into(),
                        name: "Command".into(),
                        value: TraceParameterValue::Number {
                            value,
                            unit: unit.into(),
                        },
                    });
                    "stimulus_template"
                } else {
                    "sweep"
                };
                TraceItemDescriptor {
                    id: TraceItemId::derived(collection, &(index as u64).to_le_bytes()),
                    parameters,
                    primary_label_parameter: primary.into(),
                    label_override: None,
                }
            })
            .collect();
        catalog.set_trace_collection(
            field,
            TraceCollectionCatalog {
                id: collection,
                axis_quantity: "Sweep".into(),
                axis_unit: String::new(),
                items,
            },
        );
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
        "nmr.dosy_map".to_owned(),
        "nmr.ilt_map".to_owned(),
    ])
}

pub(crate) fn table_field_catalog() -> FieldCatalog {
    FieldCatalog::for_keys(["table.default_series".to_owned()])
}

#[cfg(test)]
pub(crate) fn afm_field_catalog(data: &plotx_io::AfmData) -> FieldCatalog {
    let image_keys = afm_channel_keys(data);
    let mut catalog = afm_field_catalog_for_keys(data, &image_keys);
    catalog.attach_provenance(&data.source, None);
    catalog
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn field_provenance_round_trips_with_the_persisted_catalog() {
        let mut catalog = FieldCatalog::for_keys(["grid".to_owned()]);
        catalog.attach_provenance(
            "source.raw",
            Some(FieldAlgorithmProvenance {
                algorithm: "process_2d".to_owned(),
                version: 1,
            }),
        );
        let encoded = serde_json::to_value(&catalog).unwrap();
        let decoded: FieldCatalog = serde_json::from_value(encoded).unwrap();
        let provenance = decoded
            .provenance_for(FieldId::new(0))
            .expect("a catalog field keeps provenance");
        assert!(provenance.source_fingerprint.is_some());
        assert_eq!(
            provenance.algorithm,
            Some(FieldAlgorithmProvenance {
                algorithm: "process_2d".to_owned(),
                version: 1,
            })
        );
    }

    #[test]
    fn activating_a_retired_key_restores_only_its_own_identity() {
        let mut catalog = FieldCatalog::for_keys(["first".to_owned()]);
        catalog.attach_provenance("source.raw", None);
        let first = catalog.id_for_key("first").unwrap();
        catalog.reconcile_keys(Vec::new(), "source.raw", None);

        assert_eq!(catalog.ensure_key("first".to_owned()), first);
        let second = catalog.ensure_key("second".to_owned());
        assert_ne!(second, first);
        assert_eq!(catalog.id_for_key("first"), Some(first));
        assert_eq!(catalog.id_for_key("second"), Some(second));
        assert!(catalog.retired_key_to_id.is_empty());
    }

    #[test]
    fn every_supported_pseudo_axis_unit_converts_from_si() {
        let cases = [
            ("s", 1.0),
            ("ms", 1e3),
            ("us", 1e6),
            ("µs", 1e6),
            ("ns", 1e9),
            ("T/m", 1.0),
            ("mT/m", 1e3),
            ("G/cm", 1e2),
            ("G/mm", 10.0),
        ];
        for (unit, expected) in cases {
            assert_eq!(super::pseudo_axis_display_scale(unit), expected, "{unit}");
        }
    }
}
