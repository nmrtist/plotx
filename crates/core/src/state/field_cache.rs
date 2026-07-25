//! Bounded runtime caches for versioned field artifacts.
//!
//! These caches are session state: they hold no document meaning, are never
//! persisted, and never enter undo. Eviction therefore affects hit rate only —
//! an evicted key simply misses on the next resolve and is re-queued.
//!
//! There is deliberately **no** version-driven sweep. A promoted `FieldVersion`
//! makes every older key unreachable by construction, and reclaiming those
//! entries lazily is what keeps derived data free of invalidation fan-out.

use super::{
    ContourGeometry, ContourGeometryCacheKey, ContourSegment, EstimateKey, EstimateResult,
    FieldRef, FieldSummary, FieldVersion, VersionedFieldRef,
};
use std::collections::{HashMap, HashSet};
use std::hash::Hash;
use std::sync::Arc;

/// Contour geometry is bounded by bytes *and* by entry count on purpose.
/// A single entry ranges from zero segments to millions, so a count cap alone
/// cannot bound memory; conversely a byte budget alone would let millions of
/// tiny entries accumulate and make the eviction scan unbounded. Dragging a
/// contour threshold mints a fresh `ResolvedContourLevels` — hence a fresh key —
/// on every frame, so this cache is the one that actually grows with use.
const GEOMETRY_BYTE_BUDGET: usize = 64 * 1024 * 1024;
const GEOMETRY_ENTRY_LIMIT: usize = 256;
/// Summaries and estimates are fixed-size values, so an entry count is an
/// accurate memory bound. They still need one: every reprocessing run promotes
/// a new `FieldVersion` and therefore a new key.
const SUMMARY_ENTRY_LIMIT: usize = 1024;
const ESTIMATE_ENTRY_LIMIT: usize = 512;

struct LruEntry<V> {
    value: V,
    cost: usize,
    used: u64,
}

/// A least-recently-used map bounded by both an aggregate cost and an entry
/// count. `cost` is bytes for caches whose values vary wildly in size and `1`
/// for caches of uniformly small values.
struct LruMap<K, V> {
    entries: HashMap<K, LruEntry<V>>,
    clock: u64,
    cost: usize,
    cost_budget: usize,
    entry_budget: usize,
}

impl<K: Eq + Hash + Clone, V> LruMap<K, V> {
    fn new(cost_budget: usize, entry_budget: usize) -> Self {
        Self {
            entries: HashMap::new(),
            clock: 0,
            cost: 0,
            cost_budget,
            entry_budget,
        }
    }

    fn get(&mut self, key: &K) -> Option<&V> {
        self.clock = self.clock.wrapping_add(1);
        let clock = self.clock;
        let entry = self.entries.get_mut(key)?;
        entry.used = clock;
        Some(&entry.value)
    }

    /// Insert `key`, then evict least-recently-used entries until both budgets
    /// hold. `retain` protects keys that must never be dropped — in practice
    /// the in-flight sets, whose completion would otherwise write into a slot
    /// that was just evicted or re-queue work that is already running.
    fn insert(&mut self, key: K, value: V, cost: usize, retain: impl Fn(&K) -> bool) {
        self.clock = self.clock.wrapping_add(1);
        let entry = LruEntry {
            value,
            cost,
            used: self.clock,
        };
        let fresh = key.clone();
        if let Some(previous) = self.entries.insert(key, entry) {
            self.cost = self.cost.saturating_sub(previous.cost);
        }
        self.cost = self.cost.saturating_add(cost);
        while self.cost > self.cost_budget || self.entries.len() > self.entry_budget {
            // The entry budget keeps this scan bounded by a small constant.
            // The entry just written is protected as well: a single value larger
            // than the whole budget must be kept rather than dropped on arrival,
            // which would rebuild it on every frame forever.
            let Some(victim) = self
                .entries
                .iter()
                .filter(|(key, _)| **key != fresh && !retain(key))
                .min_by_key(|(_, entry)| entry.used)
                .map(|(key, _)| key.clone())
            else {
                // Everything left is protected; exceeding the budget is
                // preferable to dropping work that is already running.
                break;
            };
            if let Some(entry) = self.entries.remove(&victim) {
                self.cost = self.cost.saturating_sub(entry.cost);
            }
        }
    }
}

fn geometry_cost(geometry: &ContourGeometry) -> usize {
    geometry
        .positive
        .len()
        .saturating_add(geometry.negative.len())
        .saturating_mul(size_of::<ContourSegment>())
        .saturating_add(size_of::<ContourGeometry>())
}

/// Runtime owner for monotonic versions and content-addressed field caches.
pub(crate) struct FieldRuntime {
    next_version: u64,
    current: HashMap<FieldRef, FieldVersion>,
    summaries: LruMap<VersionedFieldRef, FieldSummary>,
    estimates: LruMap<EstimateKey, EstimateResult>,
    geometry: LruMap<ContourGeometryCacheKey, Arc<ContourGeometry>>,
    estimates_in_flight: HashSet<EstimateKey>,
    geometry_in_flight: HashSet<ContourGeometryCacheKey>,
}

impl Default for FieldRuntime {
    fn default() -> Self {
        Self {
            next_version: 0,
            current: HashMap::new(),
            summaries: LruMap::new(SUMMARY_ENTRY_LIMIT, SUMMARY_ENTRY_LIMIT),
            estimates: LruMap::new(ESTIMATE_ENTRY_LIMIT, ESTIMATE_ENTRY_LIMIT),
            geometry: LruMap::new(GEOMETRY_BYTE_BUDGET, GEOMETRY_ENTRY_LIMIT),
            estimates_in_flight: HashSet::new(),
            geometry_in_flight: HashSet::new(),
        }
    }
}

impl FieldRuntime {
    pub(crate) fn version_for(&mut self, field: FieldRef) -> Option<FieldVersion> {
        if let Some(version) = self.current.get(&field) {
            return Some(*version);
        }
        let version = self.reserve_version()?;
        self.current.insert(field, version);
        Some(version)
    }

    pub(crate) fn reserve_version(&mut self) -> Option<FieldVersion> {
        let next = self.next_version.checked_add(1)?;
        self.next_version = next;
        Some(FieldVersion(next))
    }

    pub(crate) fn current_version(&self, field: FieldRef) -> Option<FieldVersion> {
        self.current.get(&field).copied()
    }

    pub(crate) fn promote(&mut self, source: VersionedFieldRef, summary: Option<FieldSummary>) {
        self.current.insert(source.field, source.version);
        if let Some(summary) = summary {
            self.remember_summary(source, summary);
        }
    }

    /// The cached summary for exactly this `(field, version)`. Callers consult
    /// it *before* materializing a payload: a hit skips the full min/max scan
    /// outright rather than discarding one that already ran.
    pub(crate) fn summary(&mut self, source: VersionedFieldRef) -> Option<FieldSummary> {
        self.summaries.get(&source).copied()
    }

    pub(crate) fn remember_summary(&mut self, source: VersionedFieldRef, summary: FieldSummary) {
        self.summaries.insert(source, summary, 1, |_| false);
    }

    pub(crate) fn estimate(&mut self, key: &EstimateKey) -> Option<&EstimateResult> {
        self.estimates.get(key)
    }

    pub(crate) fn geometry(
        &mut self,
        key: &ContourGeometryCacheKey,
    ) -> Option<Arc<ContourGeometry>> {
        self.geometry.get(key).cloned()
    }

    pub(crate) fn begin_estimate(&mut self, key: EstimateKey) -> bool {
        self.estimates_in_flight.insert(key)
    }

    pub(crate) fn begin_geometry(&mut self, key: ContourGeometryCacheKey) -> bool {
        self.geometry_in_flight.insert(key)
    }

    pub(crate) fn finish_estimate_request(&mut self, key: &EstimateKey) {
        self.estimates_in_flight.remove(key);
    }

    pub(crate) fn finish_geometry_request(&mut self, key: &ContourGeometryCacheKey) {
        self.geometry_in_flight.remove(key);
    }

    pub(crate) fn install_estimate(
        &mut self,
        key: EstimateKey,
        value: EstimateResult,
        current: Option<FieldVersion>,
    ) -> bool {
        if current != Some(key.source.version) {
            return false;
        }
        let in_flight = &self.estimates_in_flight;
        self.estimates
            .insert(key, value, 1, |candidate| in_flight.contains(candidate));
        true
    }

    pub(crate) fn install_geometry(
        &mut self,
        key: ContourGeometryCacheKey,
        value: ContourGeometry,
        current: Option<FieldVersion>,
    ) -> bool {
        if current != Some(key.source.version) {
            return false;
        }
        let cost = geometry_cost(&value);
        let in_flight = &self.geometry_in_flight;
        self.geometry
            .insert(key, Arc::new(value), cost, |candidate| {
                in_flight.contains(candidate)
            });
        true
    }

    pub(crate) fn has_in_flight(&self) -> bool {
        !self.estimates_in_flight.is_empty() || !self.geometry_in_flight.is_empty()
    }

    #[cfg(test)]
    pub(crate) fn geometry_entry_limit() -> usize {
        GEOMETRY_ENTRY_LIMIT
    }

    #[cfg(test)]
    pub(crate) fn estimate_entry_limit() -> usize {
        ESTIMATE_ENTRY_LIMIT
    }

    #[cfg(test)]
    pub(crate) fn cached_geometry_count(&self) -> usize {
        self.geometry.entries.len()
    }

    #[cfg(test)]
    pub(crate) fn cached_estimate_count(&self) -> usize {
        self.estimates.entries.len()
    }
}

#[cfg(test)]
#[path = "field_cache_tests.rs"]
mod tests;
