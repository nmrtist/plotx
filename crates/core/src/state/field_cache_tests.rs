use super::*;
use crate::state::{
    DatasetId, EstimateKind, EstimateProvenance, EstimatedScale, FieldId, FiniteF64,
    ResolvedContourLevels, ScaleEstimate,
};
use plotx_figure::{EstimatorSelection, PositiveFiniteF64};

fn field(index: u128) -> FieldRef {
    FieldRef {
        resource: DatasetId::from_uuid(uuid::Uuid::from_u128(index)),
        field: FieldId::new(0),
    }
}

fn geometry_key(source: VersionedFieldRef, level: u64) -> ContourGeometryCacheKey {
    ContourGeometryCacheKey {
        source,
        levels: ResolvedContourLevels {
            positive: Arc::from([FiniteF64::new(level as f64 + 1.0).expect("finite level")]),
            negative: Arc::from([]),
        },
    }
}

fn estimate_key(source: VersionedFieldRef, version: u32) -> EstimateKey {
    EstimateKey {
        source,
        kind: EstimateKind::Noise,
        estimator: EstimatorSelection::Frozen {
            estimator: "test.estimator".to_owned(),
            version,
        },
    }
}

fn scale_estimate() -> EstimateResult {
    EstimateResult::Scale(ScaleEstimate {
        scale: EstimatedScale::Positive(PositiveFiniteF64::new(1.0).expect("positive literal")),
        provenance: EstimateProvenance {
            estimator: "test.estimator".to_owned(),
            version: 1,
        },
    })
}

fn runtime_with_field(index: u128) -> (FieldRuntime, VersionedFieldRef) {
    let mut runtime = FieldRuntime::default();
    let field = field(index);
    let version = runtime.version_for(field).expect("a fresh version");
    (runtime, VersionedFieldRef { field, version })
}

#[test]
fn cost_budget_evicts_least_recently_used_entries_first() {
    let mut cache = LruMap::<u32, u32>::new(10, usize::MAX);
    cache.insert(1, 1, 4, |_| false);
    cache.insert(2, 2, 4, |_| false);
    assert_eq!(cache.get(&1).copied(), Some(1));

    // Re-reading key 1 made key 2 the least recently used one.
    cache.insert(3, 3, 4, |_| false);
    assert_eq!(cache.get(&2), None);
    assert_eq!(cache.get(&1).copied(), Some(1));
    assert_eq!(cache.get(&3).copied(), Some(3));
}

#[test]
fn a_single_oversized_entry_does_not_wedge_the_cost_budget() {
    let mut cache = LruMap::<u32, u32>::new(10, usize::MAX);
    cache.insert(1, 1, 4, |_| false);
    cache.insert(2, 2, 64, |_| false);

    // Everything cheaper was evicted; the oversized entry stays usable rather
    // than being dropped as soon as it is written.
    assert_eq!(cache.get(&1), None);
    assert_eq!(cache.get(&2).copied(), Some(2));
}

#[test]
fn geometry_cache_is_bounded_and_an_evicted_key_is_simply_rebuilt() {
    let (mut runtime, source) = runtime_with_field(1);
    let limit = FieldRuntime::geometry_entry_limit();
    let current = Some(source.version);

    for level in 0..(limit as u64 + 8) {
        assert!(runtime.install_geometry(
            geometry_key(source, level),
            ContourGeometry::empty(),
            current,
        ));
    }

    assert!(runtime.cached_geometry_count() <= limit);
    let evicted = geometry_key(source, 0);
    assert!(
        runtime.geometry(&evicted).is_none(),
        "the least recently used geometry is dropped once the cache is full"
    );
    assert!(
        runtime
            .geometry(&geometry_key(source, limit as u64 + 7))
            .is_some(),
        "the most recent geometry survives"
    );

    // Eviction may only cost hit rate: the key re-queues and resolves again.
    assert!(runtime.begin_geometry(evicted.clone()));
    assert!(runtime.install_geometry(evicted.clone(), ContourGeometry::empty(), current));
    runtime.finish_geometry_request(&evicted);
    assert!(runtime.geometry(&evicted).is_some());
}

#[test]
fn an_in_flight_geometry_key_is_never_evicted() {
    let (mut runtime, source) = runtime_with_field(2);
    let limit = FieldRuntime::geometry_entry_limit();
    let current = Some(source.version);

    // A `BuildContour` result is installed while its request is still recorded
    // as in flight. Evicting it there would leave `enqueue_contour` with
    // neither a cached geometry nor a fresh request, i.e. a permanently empty
    // contour rather than a slower one.
    let in_flight = geometry_key(source, 0);
    assert!(runtime.begin_geometry(in_flight.clone()));
    assert!(runtime.install_geometry(in_flight.clone(), ContourGeometry::empty(), current));

    for level in 1..(limit as u64 + 8) {
        assert!(runtime.install_geometry(
            geometry_key(source, level),
            ContourGeometry::empty(),
            current,
        ));
    }

    assert!(
        runtime.geometry(&in_flight).is_some(),
        "an in-flight key must survive eviction"
    );
}

#[test]
fn estimate_cache_is_bounded_too() {
    let (mut runtime, source) = runtime_with_field(3);
    let limit = FieldRuntime::estimate_entry_limit();
    let current = Some(source.version);

    for version in 0..(limit as u32 + 8) {
        assert!(runtime.install_estimate(estimate_key(source, version), scale_estimate(), current));
    }

    assert!(runtime.cached_estimate_count() <= limit);
    assert!(runtime.estimate(&estimate_key(source, 0)).is_none());
    assert!(
        runtime
            .estimate(&estimate_key(source, limit as u32 + 7))
            .is_some()
    );
}
