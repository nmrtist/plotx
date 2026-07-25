use super::*;
use crate::state::{ComputeService, DataBinding, Dataset, Nmr2DDataset, PlotxApp};
use num_complex::Complex64;
use plotx_figure::{
    Color, ColorSource, ContourBasePolicy, ContourLevelSpec, ContourSpec, ContourStyle,
    EstimatorSelection, PositiveFiniteF32, PositiveFiniteF64, SeriesEncoding,
};
use plotx_io::{Dim, Domain, NmrData2D, QuadMode};
use plotx_processing::Processed2D;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

pub(super) fn finite(value: f64) -> FiniteF64 {
    FiniteF64::new(value).expect("test literals are finite")
}

pub(super) fn source(resource: u128, field: u64, version: u64) -> VersionedFieldRef {
    VersionedFieldRef {
        field: FieldRef {
            resource: DatasetId::from_uuid(uuid::Uuid::from_u128(resource)),
            field: FieldId::new(field),
        },
        version: FieldVersion(version),
    }
}

fn levels(positive: &[f64], negative: &[f64]) -> ResolvedContourLevels {
    ResolvedContourLevels {
        positive: Arc::from(positive.iter().copied().map(finite).collect::<Vec<_>>()),
        negative: Arc::from(negative.iter().copied().map(finite).collect::<Vec<_>>()),
    }
}

/// A signed field whose peak magnitude is exactly 10 in both directions.
pub(super) fn summary() -> FieldSummary {
    FieldSummary {
        min: finite(-10.0),
        max: finite(10.0),
    }
}

fn noisy_grid() -> Arc<ScalarGrid2D> {
    Arc::new(ScalarGrid2D {
        values: Arc::from(vec![
            -4.0, -1.0, 3.0, 1.0, 2.0, -3.0, 4.0, -2.0, 1.0, 5.0, -5.0, 3.0, -2.0, 4.0, 0.0, 6.0,
        ]),
        rows: 4,
        cols: 4,
        x: AxisSampling::Linear {
            start: 0.0,
            end: 3.0,
        },
        y: AxisSampling::Linear {
            start: 0.0,
            end: 3.0,
        },
    })
}

fn signed_dataset(label: &str) -> Dataset {
    grid_dataset(label, &noisy_grid().values)
}

/// A 4×4 true-2D NMR dataset whose real plane is exactly `values`.
pub(super) fn grid_dataset(label: &str, values: &[f32]) -> Dataset {
    let dimension = |nucleus: &str| Dim {
        spectral_width_hz: 4_000.0,
        observe_freq_mhz: 400.0,
        carrier_ppm: 0.0,
        nucleus: nucleus.to_owned(),
        group_delay: 0.0,
    };
    let values = values
        .iter()
        .copied()
        .map(|value| Complex64::new(f64::from(value), 0.0))
        .collect();
    Dataset::Nmr2D(Box::new(Nmr2DDataset::load(NmrData2D {
        data: values,
        rows: 4,
        cols: 4,
        domain: Domain::Frequency,
        direct: dimension("1H"),
        indirect: dimension("13C"),
        quad: QuadMode::Complex,
        indirect_conjugate: false,
        experiment: None,
        pseudo_axis: None,
        diffusion: None,
        nus: None,
        source: label.to_owned(),
    })))
}

fn absolute_signed_contour() -> ContourSpec {
    let level = ContourLevelSpec {
        base: ContourBasePolicy::Absolute(PositiveFiniteF64::new(1.0).unwrap()),
        count: 3,
        ratio: PositiveFiniteF64::new(1.5).unwrap(),
    };
    ContourSpec {
        positive: level.clone(),
        negative: Some(level),
        style: ContourStyle::default(),
    }
}

pub(super) fn wait_for_app_compute(app: &mut PlotxApp) {
    let deadline = Instant::now() + Duration::from_secs(2);
    while app.compute_busy() && Instant::now() < deadline {
        app.poll_compute();
        thread::sleep(Duration::from_millis(5));
    }
    app.poll_compute();
    assert!(
        !app.compute_busy(),
        "field job did not settle before deadline"
    );
}

fn settle_estimates(service: &mut ComputeService) {
    let deadline = Instant::now() + Duration::from_secs(2);
    while service.is_busy() && Instant::now() < deadline {
        for done in service.try_drain() {
            match done {
                crate::state::Done::EstimateField { key, result } => {
                    let current = service.current_field_version(key.source.field);
                    assert!(service.finish_estimate(key, result, current));
                }
                crate::state::Done::EstimateFieldFailed { message, .. } => {
                    panic!("estimate unexpectedly failed: {message}");
                }
                crate::state::Done::BuildContour { .. }
                | crate::state::Done::BuildContourFailed { .. }
                | crate::state::Done::Ilt { .. }
                | crate::state::Done::Dosy { .. }
                | crate::state::Done::Processing2D { .. }
                | crate::state::Done::Cancelled { .. }
                | crate::state::Done::Failed { .. } => {
                    panic!("unexpected non-estimate job while settling estimates");
                }
            }
        }
        thread::sleep(Duration::from_millis(5));
    }
    assert!(
        !service.is_busy(),
        "estimate job did not settle before deadline"
    );
}

#[test]
fn finite_key_values_reject_invalid_numbers_and_normalize_negative_zero() {
    assert!(FiniteF64::new(f64::NAN).is_none());
    assert!(FiniteF64::new(f64::INFINITY).is_none());
    assert_eq!(finite(-0.0), finite(0.0));

    let negative_zero = ContourGeometryCacheKey {
        source: source(1, 0, 1),
        levels: levels(&[-0.0], &[]),
    };
    let positive_zero = ContourGeometryCacheKey {
        source: source(1, 0, 1),
        levels: levels(&[0.0], &[]),
    };
    assert_eq!(negative_zero, positive_zero);
}

#[test]
fn loading_immutable_fields_allocates_runtime_versions_before_rendering() {
    let dataset = signed_dataset("loaded-version");
    let fields = dataset.field_descriptors();
    let real = fields
        .iter()
        .find(|field| field.local_id == "nmr.real")
        .expect("fixture has a real scalar field")
        .id;
    let magnitude = fields
        .iter()
        .find(|field| field.local_id == "nmr.magnitude")
        .expect("fixture has a magnitude scalar field")
        .id;
    let mut service = ComputeService::new();

    service.register_loaded_dataset_fields(&dataset).unwrap();

    let real_ref = FieldRef {
        resource: dataset.resource_id(),
        field: real,
    };
    let magnitude_ref = FieldRef {
        resource: dataset.resource_id(),
        field: magnitude,
    };
    let real_version = service
        .current_field_version(real_ref)
        .expect("loading allocated a real-field version");
    let magnitude_version = service
        .current_field_version(magnitude_ref)
        .expect("loading allocated a magnitude-field version");
    assert_ne!(real_version, magnitude_version);
    assert_eq!(
        service.field_version_for(real_ref).unwrap(),
        real_version,
        "a later render reuses the import/load version instead of bumping it"
    );
    let cached = service.cached_field_summary(VersionedFieldRef {
        field: real_ref,
        version: real_version,
    });
    assert!(
        cached.is_some(),
        "scalar fields receive their cheap summary at the load boundary"
    );
    let snapshot = dataset
        .field_snapshot(real, real_version, cached)
        .expect("loaded scalar field has an owned snapshot");
    assert_eq!(
        snapshot.summary, cached,
        "a later snapshot reuses the cached summary instead of rescanning"
    );
    assert!(snapshot.provenance.source_fingerprint.is_some());
    assert_eq!(
        snapshot.provenance.algorithm,
        Some(FieldAlgorithmProvenance {
            algorithm: "process_2d".to_owned(),
            version: 1,
        }),
        "persisted provenance is separate from the runtime field version"
    );
}

#[test]
fn a_cached_summary_replaces_the_snapshot_scan_and_never_reaches_a_raster() {
    let dataset = signed_dataset("cached-summary");
    let field = dataset
        .default_field_id()
        .expect("the fixture exposes a scalar field");
    let source = VersionedFieldRef {
        field: FieldRef {
            resource: dataset.resource_id(),
            field,
        },
        version: FieldVersion(1),
    };
    // A value the min/max scan could not possibly produce for this grid: if the
    // snapshot carries it, the scan was skipped rather than run and discarded.
    let sentinel = FieldSummary {
        min: finite(-4321.0),
        max: finite(8765.0),
    };

    let cached = dataset
        .field_snapshot(field, source.version, Some(sentinel))
        .expect("scalar field has a snapshot");
    assert_eq!(cached.summary, Some(sentinel));

    let scanned = dataset
        .field_snapshot(field, source.version, None)
        .expect("scalar field has a snapshot");
    assert_ne!(scanned.summary, Some(sentinel));
    assert!(
        scanned.summary.is_some(),
        "a scalar snapshot always has one"
    );

    // The invariant holds in the other direction too: a colored raster never
    // gains scalar statistics, however insistently a caller offers them.
    let raster = FieldSnapshot::new(
        source,
        FieldPayload::ColoredRaster2D(ColoredRaster2D {
            pixels: Arc::from(vec![255, 0, 0]),
            rows: 1,
            cols: 1,
            format: RasterFormat::Rgb8,
        }),
        FieldProvenance::default(),
        Some(sentinel),
    );
    assert!(raster.summary.is_none());
}

#[test]
fn equivalent_absolute_levels_share_the_same_geometry_key() {
    let field = source(2, 7, 11);
    let noise_spec = ContourSpec {
        positive: ContourLevelSpec {
            base: ContourBasePolicy::NoiseFloor {
                multiplier: PositiveFiniteF64::new(5.0).unwrap(),
                peak_fraction: plotx_figure::UnitInterval::new(0.0).expect("a zero floor is valid"),
                estimator: EstimatorSelection::Frozen {
                    estimator: plotx_analysis::robust::ROBUST_DIFFERENCE_MAD_ID.to_owned(),
                    version: plotx_analysis::robust::ROBUST_DIFFERENCE_MAD_VERSION,
                },
            },
            count: 3,
            ratio: PositiveFiniteF64::new(1.5).unwrap(),
        },
        negative: None,
        style: ContourStyle::default(),
    };
    let absolute_spec = ContourSpec {
        positive: ContourLevelSpec {
            base: ContourBasePolicy::Absolute(PositiveFiniteF64::new(5.0).unwrap()),
            count: 3,
            ratio: PositiveFiniteF64::new(1.5).unwrap(),
        },
        negative: None,
        style: ContourStyle {
            positive_color: ColorSource::Explicit(Color::rgb(0x11, 0x22, 0x33)),
            ..ContourStyle::default()
        },
    };
    let estimate = EstimateResult::Scale(ScaleEstimate {
        scale: EstimatedScale::Positive(PositiveFiniteF64::new(1.0).unwrap()),
        provenance: EstimateProvenance {
            estimator: plotx_analysis::robust::ROBUST_DIFFERENCE_MAD_ID.to_owned(),
            version: plotx_analysis::robust::ROBUST_DIFFERENCE_MAD_VERSION,
        },
    });
    let ContourResolution::Ready {
        levels: noise_levels,
        ..
    } = resolve_contour_levels(field, &noise_spec, summary(), |_| Some(estimate.clone()))
    else {
        panic!("noise estimate should resolve a concrete level ladder");
    };
    let ContourResolution::Ready {
        levels: absolute_levels,
        ..
    } = resolve_contour_levels(field, &absolute_spec, summary(), |_| None)
    else {
        panic!("absolute levels do not need an estimate");
    };
    assert_eq!(noise_levels, absolute_levels);
    assert_eq!(
        ContourGeometryCacheKey {
            source: field,
            levels: noise_levels,
        },
        ContourGeometryCacheKey {
            source: field,
            levels: absolute_levels,
        },
        "policy, estimate provenance, ratio source, and style are not geometry inputs"
    );
}

#[test]
fn out_of_range_absolute_levels_are_truncated_not_rewritten() {
    let level = ContourLevelSpec {
        base: ContourBasePolicy::Absolute(PositiveFiniteF64::new(20.0).unwrap()),
        count: 3,
        ratio: PositiveFiniteF64::new(1.5).unwrap(),
    };
    let spec = ContourSpec {
        positive: level.clone(),
        negative: Some(level),
        style: ContourStyle::default(),
    };
    let ContourResolution::Ready { levels, .. } =
        resolve_contour_levels(source(9, 3, 1), &spec, summary(), |_| None)
    else {
        panic!("an absolute contour needs no estimate");
    };
    // An explicit threshold is the strongest term of the value-resolution order.
    // A field whose peak is 10 draws nothing at 20; rewriting it to a computed
    // base would put contours at levels the user never asked for.
    assert!(levels.positive.is_empty());
    assert!(levels.negative.is_empty());
}

#[test]
fn style_edits_reuse_cached_geometry_without_sync_marching_squares() {
    let mut app = PlotxApp::new_with_settings(crate::settings::Settings::default());
    app.doc.datasets.push(signed_dataset("style-cache"));
    let mut binding = DataBinding::single(&app.doc.datasets[0]);
    binding.series[0].encoding = SeriesEncoding::Contour(absolute_signed_contour());

    crate::contour_probe::reset();
    let first = app.build_binding_figure(
        &binding,
        &crate::state::ChartSpec::default_for(crate::state::DataDomain::Nmr2d),
        &crate::state::StackSpec::default(),
        [120.0, 80.0],
    );
    assert!(first.contours.is_empty());
    assert_eq!(crate::contour_probe::queued_contour_builds(), 1);
    assert_eq!(crate::contour_probe::marching_squares_on_this_thread(), 0);

    wait_for_app_compute(&mut app);
    let materialized = crate::contour_probe::field_payload_materializations();
    let cached = app.build_binding_figure(
        &binding,
        &crate::state::ChartSpec::default_for(crate::state::DataDomain::Nmr2d),
        &crate::state::StackSpec::default(),
        [120.0, 80.0],
    );
    assert!(!cached.contours.is_empty());
    assert_eq!(
        crate::contour_probe::field_payload_materializations(),
        materialized,
        "a warm geometry cache must not allocate an O(rows x cols) buffer: \
         capabilities come from the cheap representation query and the summary \
         from the runtime cache"
    );

    let changed_negative = Color::rgb(0x2a, 0xa1, 0x55);
    let mut styled = binding.clone();
    let SeriesEncoding::Contour(contour) = &mut styled.series[0].encoding else {
        panic!("test binding has a contour encoding");
    };
    contour.style.negative_color = ColorSource::Explicit(changed_negative);
    contour.style.width = PositiveFiniteF32::new(2.0).unwrap();
    let restyled = app.build_binding_figure(
        &styled,
        &crate::state::ChartSpec::default_for(crate::state::DataDomain::Nmr2d),
        &crate::state::StackSpec::default(),
        [120.0, 80.0],
    );
    assert_eq!(crate::contour_probe::queued_contour_builds(), 1);
    assert_eq!(crate::contour_probe::marching_squares_on_this_thread(), 0);
    assert_eq!(
        crate::contour_probe::field_payload_materializations(),
        materialized,
        "a style-only edit re-resolves the same key and stays on the warm path"
    );
    assert!(restyled.contours.iter().all(|contour| contour.width == 2.0));
    assert!(
        restyled
            .contours
            .iter()
            .any(|contour| contour.color == changed_negative),
        "negative colour is applied only while assembling the Figure"
    );
}

#[test]
fn new_field_versions_naturally_miss_old_geometry_and_reject_stale_done() {
    let mut service = ComputeService::new();
    let field = FieldRef {
        resource: DatasetId::from_uuid(uuid::Uuid::from_u128(3)),
        field: FieldId::new(4),
    };
    let first_version = service.field_version_for(field).unwrap();
    let first_source = VersionedFieldRef {
        field,
        version: first_version,
    };
    service.promote_field_version(first_source, Some(summary()));
    let first_key = ContourGeometryCacheKey {
        source: first_source,
        levels: levels(&[1.0, 1.5], &[-1.0, -1.5]),
    };
    assert!(service.finish_contour(
        first_key.clone(),
        ContourGeometry::empty(),
        service.current_field_version(field),
    ));
    assert!(service.geometry_for(&first_key).is_some());

    let second_source = VersionedFieldRef {
        field,
        version: service.reserve_field_version().unwrap(),
    };
    service.promote_field_version(second_source, Some(summary()));
    let second_key = ContourGeometryCacheKey {
        source: second_source,
        levels: first_key.levels.clone(),
    };
    assert!(service.geometry_for(&second_key).is_none());
    let current = service.current_field_version(field);
    assert!(
        !service.finish_contour(first_key, ContourGeometry::empty(), current),
        "a late BuildContour Done cannot overwrite the newer field version"
    );
}

#[test]
fn estimates_are_demand_driven_and_coexist_per_estimator_selection() {
    let mut service = ComputeService::new();
    let field = FieldRef {
        resource: DatasetId::from_uuid(uuid::Uuid::from_u128(4)),
        field: FieldId::new(0),
    };
    let source = VersionedFieldRef {
        field,
        version: service.field_version_for(field).unwrap(),
    };
    let frozen_noise = EstimateKey {
        source,
        kind: EstimateKind::Noise,
        estimator: EstimatorSelection::Frozen {
            estimator: plotx_analysis::robust::ROBUST_DIFFERENCE_MAD_ID.to_owned(),
            version: plotx_analysis::robust::ROBUST_DIFFERENCE_MAD_VERSION,
        },
    };
    let background = EstimateKey {
        source,
        kind: EstimateKind::Background,
        estimator: EstimatorSelection::Frozen {
            estimator: plotx_analysis::robust::DEPLANED_LOCATION_SCALE_ID.to_owned(),
            version: plotx_analysis::robust::DEPLANED_LOCATION_SCALE_VERSION,
        },
    };
    let noise_only = ContourSpec {
        positive: ContourLevelSpec {
            base: ContourBasePolicy::NoiseFloor {
                multiplier: PositiveFiniteF64::new(5.0).unwrap(),
                peak_fraction: plotx_figure::UnitInterval::new(0.0).expect("a zero floor is valid"),
                estimator: frozen_noise.estimator.clone(),
            },
            count: 3,
            ratio: PositiveFiniteF64::new(1.5).unwrap(),
        },
        negative: None,
        style: ContourStyle::default(),
    };
    assert_eq!(
        resolve_contour_levels(source, &noise_only, summary(), |_| None),
        ContourResolution::Pending(vec![frozen_noise.clone()]),
        "a NoiseFloor contour does not request an unrelated background fit"
    );
    let background_only = ContourSpec {
        positive: ContourLevelSpec {
            base: ContourBasePolicy::BackgroundScale {
                multiplier: PositiveFiniteF64::new(5.0).unwrap(),
                estimator: background.estimator.clone(),
            },
            count: 3,
            ratio: PositiveFiniteF64::new(1.5).unwrap(),
        },
        negative: None,
        style: ContourStyle::default(),
    };
    assert_eq!(
        resolve_contour_levels(source, &background_only, summary(), |_| None),
        ContourResolution::Pending(vec![background.clone()]),
        "the expensive background fit is requested only by BackgroundScale"
    );
    assert!(
        service
            .enqueue_estimate(frozen_noise.clone(), noisy_grid())
            .unwrap()
    );
    settle_estimates(&mut service);
    let Some(EstimateResult::Scale(noise)) = service.estimate_for(&frozen_noise) else {
        panic!("expected the frozen noise estimate");
    };
    assert_eq!(
        noise.provenance,
        EstimateProvenance {
            estimator: plotx_analysis::robust::ROBUST_DIFFERENCE_MAD_ID.to_owned(),
            version: plotx_analysis::robust::ROBUST_DIFFERENCE_MAD_VERSION,
        }
    );
    assert!(
        service.estimate_for(&background).is_none(),
        "a NoiseFloor contour never schedules an unrelated background fit"
    );

    let latest_noise = EstimateKey {
        source,
        kind: EstimateKind::Noise,
        estimator: EstimatorSelection::FollowLatest,
    };
    assert!(
        service
            .enqueue_estimate(latest_noise.clone(), noisy_grid())
            .unwrap()
    );
    settle_estimates(&mut service);
    assert!(service.estimate_for(&frozen_noise).is_some());
    assert!(service.estimate_for(&latest_noise).is_some());

    assert!(
        service
            .enqueue_estimate(background.clone(), noisy_grid())
            .unwrap()
    );
    settle_estimates(&mut service);
    let Some(EstimateResult::LocationScale(background_result)) = service.estimate_for(&background)
    else {
        panic!("expected the background location/scale estimate");
    };
    assert_eq!(
        background_result.provenance,
        EstimateProvenance {
            estimator: plotx_analysis::robust::DEPLANED_LOCATION_SCALE_ID.to_owned(),
            version: plotx_analysis::robust::DEPLANED_LOCATION_SCALE_VERSION,
        }
    );
}

#[test]
fn descriptor_capabilities_follow_actual_sampling_not_a_domain_heuristic() {
    let mut dataset = signed_dataset("explicit-axis");
    let Dataset::Nmr2D(nmr) = &mut dataset else {
        panic!("test dataset is NMR 2D");
    };
    let Processed2D::Ft(spectrum) = &mut nmr.processed else {
        panic!("frequency-domain input produces a scalar grid");
    };
    Arc::make_mut(spectrum).f1_ppm[2] += 0.25;

    let real = dataset
        .field_descriptors()
        .into_iter()
        .find(|field| field.local_id == "nmr.real")
        .unwrap();
    assert!(
        !real
            .capabilities
            .contains(crate::automation::CAP_FIELD_SCALAR_GRID_2D_REGULAR)
    );
    assert!(
        !dataset.supports_encoding(real.id, &SeriesEncoding::Contour(absolute_signed_contour()))
    );
    assert!(matches!(
        dataset.field_payload(real.id),
        Some(FieldPayload::ScalarGrid2D(ScalarGrid2D {
            y: AxisSampling::Explicit(_),
            ..
        }))
    ));
}

#[test]
fn field_identity_survives_reorder_and_deleted_sources_drop_worker_results() {
    let mut app = PlotxApp::new_with_settings(crate::settings::Settings::default());
    app.doc.datasets.push(signed_dataset("first"));
    app.doc.datasets.push(signed_dataset("second"));
    let field = app.doc.datasets[0].default_field_id().unwrap();
    let resource = app.doc.datasets[0].resource_id();
    let source = FieldRef { resource, field };
    let version = app.session.compute.field_version_for(source).unwrap();
    let snapshot = app.doc.datasets[0]
        .field_snapshot(field, version, None)
        .unwrap();
    let grid = Arc::new(snapshot.payload.scalar_grid().unwrap().clone());
    let key = ContourGeometryCacheKey {
        source: snapshot.source,
        levels: levels(&[1.0], &[]),
    };
    assert!(
        app.session
            .compute
            .enqueue_contour(key.clone(), grid)
            .unwrap()
    );

    app.doc.datasets.swap(0, 1);
    assert_eq!(app.doc.dataset_index(resource), Some(1));
    let other_field = FieldRef {
        resource,
        field: FieldId::new(field.get() + 1),
    };
    assert_ne!(
        ContourGeometryCacheKey {
            source: VersionedFieldRef {
                field: source,
                version,
            },
            levels: key.levels.clone(),
        },
        ContourGeometryCacheKey {
            source: VersionedFieldRef {
                field: other_field,
                version,
            },
            levels: key.levels.clone(),
        }
    );

    app.doc
        .datasets
        .retain(|dataset| dataset.resource_id() != resource);
    wait_for_app_compute(&mut app);
    assert!(
        app.session.compute.geometry_for(&key).is_none(),
        "a result whose dataset has been deleted is discarded instead of cached"
    );
}

#[test]
fn colored_rasters_have_import_versions_but_no_scalar_summary_or_contour_path() {
    let mut service = ComputeService::new();
    let field = FieldRef {
        resource: DatasetId::from_uuid(uuid::Uuid::from_u128(5)),
        field: FieldId::new(9),
    };
    let snapshot = service
        .register_imported_field(
            field,
            FieldPayload::ColoredRaster2D(ColoredRaster2D {
                pixels: Arc::from(vec![255, 0, 0, 0, 255, 0]),
                rows: 1,
                cols: 2,
                format: RasterFormat::Rgb8,
            }),
            FieldProvenance {
                source_fingerprint: Some("import-fingerprint".to_owned()),
                algorithm: None,
                metadata: Default::default(),
            },
        )
        .expect("a pipeline-free raster receives a runtime version at import");
    assert_eq!(
        service.current_field_version(field),
        Some(snapshot.source.version)
    );
    assert!(snapshot.summary.is_none());
    assert!(snapshot.payload.scalar_grid().is_none());
    let capabilities = snapshot.payload.intrinsic_capabilities();
    assert!(capabilities.contains(crate::automation::CAP_FIELD_COLORED_RASTER_2D));
    assert!(!capabilities.contains(crate::automation::CAP_FIELD_SCALAR_GRID_2D_REGULAR));
    assert!(
        serde_json::to_value(&snapshot.provenance)
            .unwrap()
            .get("source_fingerprint")
            .is_some(),
        "persistable provenance deliberately carries no FieldVersion"
    );
}

#[test]
fn imported_grayscale_fields_keep_scalar_summary_and_regular_capability() {
    let mut service = ComputeService::new();
    let field = FieldRef {
        resource: DatasetId::from_uuid(uuid::Uuid::from_u128(6)),
        field: FieldId::new(1),
    };
    let snapshot = service
        .register_imported_field(
            field,
            FieldPayload::ScalarGrid2D((*noisy_grid()).clone()),
            FieldProvenance {
                source_fingerprint: Some("gray-import-fingerprint".to_owned()),
                algorithm: None,
                metadata: Default::default(),
            },
        )
        .expect("a grayscale import receives a runtime version");
    assert!(snapshot.summary.is_some());
    assert!(snapshot.payload.scalar_grid().is_some());
    assert!(
        snapshot
            .payload
            .intrinsic_capabilities()
            .contains(crate::automation::CAP_FIELD_SCALAR_GRID_2D_REGULAR)
    );
}
