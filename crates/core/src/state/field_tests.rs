use super::*;
use crate::state::{AfmDataset, Dataset, ElectrophysiologyDataset, Nmr2DDataset};

#[test]
fn colored_raster_cannot_supply_scalar_statistics() {
    let payload = FieldPayload::ColoredRaster2D(ColoredRaster2D {
        pixels: Arc::from(vec![255, 0, 0]),
        rows: 1,
        cols: 1,
        format: RasterFormat::Rgb8,
    });
    assert!(payload.scalar_grid().is_none());
    assert!(payload.summary().is_none());
}

#[test]
fn regular_capability_selects_contour_without_domain_knowledge() {
    let capabilities =
        FieldCapabilities::new([CapabilityId::new(CAP_FIELD_SCALAR_GRID_2D_REGULAR)]);
    let encoding = default_encoding(
        &capabilities,
        &FieldMetadata::default(),
        RequestedChart::Contour,
        &PresentationProfile::default(),
    );
    assert!(matches!(encoding, SeriesEncoding::Contour(_)));
}

#[test]
fn unregistered_regular_grid_provider_gets_scalar_encodings() {
    let provider_grid = ScalarGrid2D {
        values: Arc::from(vec![0.0, 1.0, 2.0, 3.0]),
        rows: 2,
        cols: 2,
        x: AxisSampling::Linear {
            start: 0.0,
            end: 1.0,
        },
        y: AxisSampling::Linear {
            start: 0.0,
            end: 1.0,
        },
    };
    let capabilities = FieldPayload::ScalarGrid2D(provider_grid).intrinsic_capabilities();
    let ids = crate::state::encoding_descriptors_for(&capabilities)
        .into_iter()
        .map(|descriptor| descriptor.id)
        .collect::<Vec<_>>();
    assert_eq!(ids, ["contour", "heatmap"]);
}

#[test]
fn colored_raster_auto_materializes_an_image_encoding() {
    let capabilities = FieldCapabilities::new([CapabilityId::new(CAP_FIELD_COLORED_RASTER_2D)]);
    let encoding = default_encoding(
        &capabilities,
        &FieldMetadata::default(),
        RequestedChart::Auto,
        &PresentationProfile::default(),
    );
    assert!(matches!(encoding, SeriesEncoding::Image(_)));
}

#[test]
fn explicitly_sampled_grid_does_not_claim_the_regular_grid_capability() {
    let payload = FieldPayload::ScalarGrid2D(ScalarGrid2D {
        values: Arc::from(vec![0.0, 1.0, 2.0, 3.0]),
        rows: 2,
        cols: 2,
        x: AxisSampling::Explicit(Arc::from(vec![0.0, 0.5])),
        y: AxisSampling::Linear {
            start: 0.0,
            end: 1.0,
        },
    });
    assert!(
        !payload
            .intrinsic_capabilities()
            .contains(CAP_FIELD_SCALAR_GRID_2D_REGULAR)
    );
    let ids = crate::state::encoding_descriptors_for(&payload.intrinsic_capabilities())
        .into_iter()
        .map(|descriptor| descriptor.id)
        .collect::<Vec<_>>();
    assert!(
        !ids.contains(&"contour"),
        "a new provider with explicit sampling must not need a registry edit to be excluded"
    );
}

#[test]
fn raster_falls_back_to_image_instead_of_a_scalar_or_line_encoding() {
    let capabilities = FieldCapabilities::new([CapabilityId::new(CAP_FIELD_COLORED_RASTER_2D)]);
    let encoding = default_encoding(
        &capabilities,
        &FieldMetadata::default(),
        RequestedChart::Contour,
        &PresentationProfile::default(),
    );
    assert!(matches!(encoding, SeriesEncoding::Image(_)));
}

#[test]
fn afm_channel_ids_follow_persisted_keys_after_channel_reordering() {
    let channel = |name: &str, raw: Vec<i32>| plotx_io::AfmImageChannel {
        name: name.to_owned(),
        width: 2,
        height: 2,
        scan_size_x: 1.0,
        scan_size_y: 1.0,
        lateral_unit: "nm".to_owned(),
        scale: plotx_io::AfmScale {
            multiplier: 1.0,
            offset: 0.0,
            unit: "nm".to_owned(),
        },
        raw: Arc::from(raw),
        frame_direction: plotx_io::AfmFrameDirection::Trace,
    };
    let first = channel("Height", vec![1, 2, 3, 4]);
    let second = channel("Phase", vec![5, 6, 7, 8]);
    let data = plotx_io::AfmData {
        images: vec![first.clone(), second.clone()],
        forces: None,
        source: "test".to_owned(),
        import_warnings: Vec::new(),
    };
    let catalog = crate::state::afm_field_catalog(&data);
    let height = catalog
        .id_for_key(&crate::state::afm_channel_key(&first))
        .unwrap();
    let phase = catalog
        .id_for_key(&crate::state::afm_channel_key(&second))
        .unwrap();

    let mut reordered = AfmDataset::load(plotx_io::AfmData {
        images: vec![second, first],
        ..data
    });
    reordered.field_catalog = catalog;
    let dataset = Dataset::Afm(Box::new(reordered));
    dataset.validate_field_catalog().unwrap();
    let fields = dataset.field_descriptors();
    assert_eq!(
        fields
            .iter()
            .find(|field| field.name == "Height")
            .unwrap()
            .id,
        height
    );
    assert_eq!(
        fields
            .iter()
            .find(|field| field.name == "Phase")
            .unwrap()
            .id,
        phase
    );
}

#[test]
fn force_curve_id_is_not_derived_from_the_image_count() {
    let data = plotx_io::AfmData {
        images: Vec::new(),
        forces: Some(plotx_io::AfmForceSet {
            grid_width: 1,
            grid_height: 1,
            samples_per_curve: 1,
            raw: Arc::from(vec![0]),
            signal_scale: plotx_io::AfmScale {
                multiplier: 1.0,
                offset: 0.0,
                unit: "V".to_owned(),
            },
            sample_period_s: None,
            z_positions: None,
            display_order: Arc::from(vec![0]),
            approach_samples: 1,
            deflection_sensitivity_m_per_v: None,
            spring_constant_n_per_m: None,
        }),
        source: "test".to_owned(),
        import_warnings: Vec::new(),
    };
    let catalog = crate::state::afm_field_catalog(&data);
    assert_eq!(catalog.id_for_key("afm.force_curve"), Some(FieldId::new(0)));
    let with_image = plotx_io::AfmData {
        images: vec![plotx_io::AfmImageChannel {
            name: "Height".to_owned(),
            width: 1,
            height: 1,
            scan_size_x: 1.0,
            scan_size_y: 1.0,
            lateral_unit: "nm".to_owned(),
            scale: plotx_io::AfmScale {
                multiplier: 1.0,
                offset: 0.0,
                unit: "nm".to_owned(),
            },
            raw: Arc::from(vec![1]),
            frame_direction: plotx_io::AfmFrameDirection::Trace,
        }],
        ..data
    };
    let catalog = crate::state::afm_field_catalog(&with_image);
    assert_eq!(catalog.id_for_key("afm.force_curve"), Some(FieldId::new(0)));
}

#[test]
fn afm_channel_key_is_stable_and_matches_the_loaded_key_cache() {
    let channel = afm_channel("Height", 1.0, 0.0, 2.0, 3.0);
    let first = crate::state::afm_channel_key(&channel);
    let second = crate::state::afm_channel_key(&channel);
    assert_eq!(first, second);

    let dataset = Dataset::Afm(Box::new(AfmDataset::load(plotx_io::AfmData {
        images: vec![channel],
        forces: None,
        source: "key cache test".to_owned(),
        import_warnings: Vec::new(),
    })));
    assert_eq!(dataset.field_descriptors()[0].local_id, first);
}

#[test]
fn afm_field_keys_are_hashed_once_then_reused_during_rendering() {
    crate::state::reset_afm_channel_key_computations();
    let dataset = Dataset::Afm(Box::new(AfmDataset::load(plotx_io::AfmData {
        images: vec![afm_channel("Height", 1.0, 0.0, 2.0, 3.0)],
        forces: None,
        source: "key cache render test".to_owned(),
        import_warnings: Vec::new(),
    })));
    assert_eq!(crate::state::afm_channel_key_computations(), 1);

    let field = dataset.default_field_id().unwrap();
    assert!(dataset.has_field(field));
    assert!(dataset.supports_encoding(field, &SeriesEncoding::Heatmap(HeatmapSpec::default())));
    assert!(
        dataset
            .encoded_field_figure(field, &SeriesEncoding::Heatmap(HeatmapSpec::default()))
            .is_some()
    );
    assert_eq!(
        crate::state::afm_channel_key_computations(),
        1,
        "descriptor lookup and rendering must reuse the key calculated while loading"
    );
}

#[test]
fn electrophysiology_keys_include_the_channel_quantity() {
    let data = plotx_io::ElectrophysiologyData {
        abf_version: "2.0".to_owned(),
        sample_rate_hz: 10_000.0,
        channels: vec![
            plotx_io::RecordedChannel {
                name: "Response".to_owned(),
                unit: plotx_io::ElectricalUnit {
                    symbol: "mV".to_owned(),
                    quantity: plotx_io::ElectricalQuantity::Voltage,
                },
            },
            plotx_io::RecordedChannel {
                name: "Response".to_owned(),
                unit: plotx_io::ElectricalUnit {
                    symbol: "mV".to_owned(),
                    quantity: plotx_io::ElectricalQuantity::Current,
                },
            },
        ],
        sweeps: vec![plotx_io::Sweep {
            start_time_s: 0.0,
            channels: vec![vec![1.0, 2.0], vec![1.0, 2.0]],
            commands: Vec::new(),
        }],
        protocol: None,
        source: "quantity test".to_owned(),
        import_warnings: Vec::new(),
    };
    let dataset = Dataset::Electrophysiology(Box::new(ElectrophysiologyDataset::load(data)));
    let fields = dataset.field_descriptors();
    assert_eq!(fields.len(), 2);
    assert_ne!(fields[0].local_id, fields[1].local_id);
    dataset.validate_field_catalog().unwrap();
}

fn afm_channel(
    name: &str,
    multiplier: f64,
    offset: f64,
    scan_size_x: f64,
    scan_size_y: f64,
) -> plotx_io::AfmImageChannel {
    plotx_io::AfmImageChannel {
        name: name.to_owned(),
        width: 2,
        height: 2,
        scan_size_x,
        scan_size_y,
        lateral_unit: "nm".to_owned(),
        scale: plotx_io::AfmScale {
            multiplier,
            offset,
            unit: "nm".to_owned(),
        },
        raw: Arc::from(vec![1, 2, 3, 4]),
        frame_direction: plotx_io::AfmFrameDirection::Trace,
    }
}

#[test]
fn magnitude_field_renders_magnitude_instead_of_falling_back_to_real() {
    let dimension = |nucleus: &str| plotx_io::Dim {
        spectral_width_hz: 4_000.0,
        observe_freq_mhz: 400.0,
        carrier_ppm: 0.0,
        nucleus: nucleus.to_owned(),
        group_delay: 0.0,
    };
    let dataset = Dataset::Nmr2D(Box::new(Nmr2DDataset::load(plotx_io::NmrData2D {
        data: vec![
            num_complex::Complex64::new(-3.0, 4.0),
            num_complex::Complex64::new(5.0, 12.0),
            num_complex::Complex64::new(8.0, 15.0),
            num_complex::Complex64::new(-7.0, 24.0),
        ],
        rows: 2,
        cols: 2,
        domain: plotx_io::Domain::Frequency,
        direct: dimension("1H"),
        indirect: dimension("13C"),
        quad: plotx_io::QuadMode::Complex,
        indirect_conjugate: false,
        experiment: None,
        pseudo_axis: None,
        diffusion: None,
        nus: None,
        source: "magnitude test".to_owned(),
    })));
    let magnitude = dataset
        .field_descriptors()
        .into_iter()
        .find(|field| field.local_id == "nmr.magnitude")
        .unwrap();
    let figure = dataset
        .encoded_field_figure(
            magnitude.id,
            &SeriesEncoding::Heatmap(HeatmapSpec::default()),
        )
        .unwrap();
    let heatmap = figure.heatmap.unwrap();
    assert_eq!(heatmap.values, vec![5.0, 13.0, 17.0, 25.0]);
    assert_ne!(heatmap.values, vec![-3.0, 5.0, 8.0, -7.0]);
}

#[test]
fn default_nmr_contour_uses_the_processed_figure_cache() {
    let dimension = |nucleus: &str| plotx_io::Dim {
        spectral_width_hz: 4_000.0,
        observe_freq_mhz: 400.0,
        carrier_ppm: 0.0,
        nucleus: nucleus.to_owned(),
        group_delay: 0.0,
    };
    let dataset = Dataset::Nmr2D(Box::new(Nmr2DDataset::load(plotx_io::NmrData2D {
        data: vec![num_complex::Complex64::new(1.0, 0.0); 16],
        rows: 4,
        cols: 4,
        domain: plotx_io::Domain::Frequency,
        direct: dimension("1H"),
        indirect: dimension("13C"),
        quad: plotx_io::QuadMode::Complex,
        indirect_conjugate: false,
        experiment: None,
        pseudo_axis: None,
        diffusion: None,
        nus: None,
        source: "cache test".to_owned(),
    })));
    let real = dataset
        .field_descriptors()
        .into_iter()
        .find(|field| field.local_id == "nmr.real")
        .unwrap();
    crate::state::datasets_2d_figure::reset_synchronous_contour_builds();
    dataset
        .encoded_field_figure(
            real.id,
            &default_encoding(
                &real.capabilities,
                &real.metadata,
                RequestedChart::Contour,
                &PresentationProfile::default(),
            ),
        )
        .unwrap();
    assert_eq!(
        crate::state::datasets_2d_figure::synchronous_contour_builds(),
        0,
        "the default contour must clone the background-built processed figure"
    );
}
