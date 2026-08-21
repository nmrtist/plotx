use super::*;

fn data(components: &[(f64, f64, f64, f64)], count: usize, sw: f64) -> NmrData {
    let points = (0..count)
        .map(|index| {
            let time = index as f64 / sw;
            components.iter().fold(
                Complex64::new(0.0, 0.0),
                |sum, &(frequency, amplitude, phase, linewidth)| {
                    sum + Complex64::from_polar(
                        amplitude * (-PI * linewidth * time).exp(),
                        phase + TAU * frequency * time,
                    )
                },
            )
        })
        .collect();
    NmrData {
        points,
        domain: Domain::Time,
        spectral_width_hz: sw,
        observe_freq_mhz: 500.0,
        carrier_ppm: 0.0,
        nucleus: "1H".to_owned(),
        source: "synthetic".to_owned(),
        group_delay: 0.0,
    }
}

#[test]
fn complete_reduction_recovers_table_and_residual() {
    let input = data(
        &[(-75.0, 8.0, 0.3, 1.5), (120.0, 4.0, -0.2, 2.0)],
        4096,
        2000.0,
    );
    let params = CraftParams {
        max_fit_window_width_hz: 2_000.0,
        filter_taps: 127,
        ..CraftParams::default()
    };
    let invocation = CraftInvocation::acquisition(&input, params);
    let result = process_craft_cancellable(&input, &invocation, &|| false).unwrap();
    assert_eq!(result.components.len(), 2, "{:?}", result.components);
    assert!((result.components[0].frequency_hz + 75.0).abs() < 0.05);
    assert!((result.components[1].frequency_hz - 120.0).abs() < 0.05);
    assert!(result.diagnostics.normalized_residual < 1e-4);
}

#[test]
fn full_band_fit_treats_empty_internal_windows_as_valid_no_signal_results() {
    let input = data(
        &[(-300.0, 4.0, 0.1, 2.0), (300.0, 2.0, -0.2, 2.0)],
        4096,
        2_000.0,
    );
    let params = CraftParams {
        filter_taps: 63,
        ..CraftParams::default()
    };

    let result = process_craft_cancellable(
        &input,
        &CraftInvocation::acquisition(&input, params),
        &|| false,
    )
    .unwrap();

    assert_eq!(result.region_summaries.len(), 1);
    assert_eq!(result.region_summaries[0].region, CraftRegionId(0));
    assert!(result.region_summaries[0].component_count >= 2);
    assert_eq!(result.diagnostics.fit_windows.len(), 4);
}

#[test]
fn ssfp_skip_extrapolates_amplitude_to_time_zero() {
    let mut input = data(&[(100.0, 5.0, 0.4, 3.0)], 1000, 20_000.0);
    input.points[..10].fill(Complex64::new(50.0, -20.0));
    let params = CraftParams {
        profile: CraftProfile::Ssfp,
        skip_duration_s: 10.0 / input.spectral_width_hz,
        reconstruction_duration_s: Some(0.1),
        max_fit_window_width_hz: 20_000.0,
        filter_taps: 63,
        ..CraftParams::default()
    };
    let invocation = CraftInvocation::acquisition(&input, params);
    let result = process_craft_cancellable(&input, &invocation, &|| false).unwrap();
    assert_eq!(result.components.len(), 1);
    assert!((result.components[0].amplitude_t0 - 5.0).abs() < 0.05);
    assert_eq!(result.synthetic_fid.len(), 2000);
    assert!(
        result
            .diagnostics
            .warnings
            .iter()
            .any(|warning| warning.message.contains("absolute qNMR"))
    );
}

#[test]
fn rejects_frequency_domain_input() {
    let mut input = data(&[], 128, 1000.0);
    input.domain = Domain::Frequency;
    assert!(matches!(
        process_craft_cancellable(
            &input,
            &CraftInvocation::acquisition(&input, CraftParams::default()),
            &|| false,
        ),
        Err(CraftError::Preflight(_))
    ));
}

#[test]
fn overlapping_requested_regions_are_rejected_as_ambiguous() {
    let input = data(&[], 128, 2_000.0);
    let params = CraftParams {
        regions: vec![
            CraftRegion::new(CraftRegionId(10), -1.0, 1.0),
            CraftRegion::new(CraftRegionId(20), 0.5, 2.0),
        ],
        max_fit_window_width_hz: 500.0,
        ..CraftParams::default()
    };

    assert!(matches!(
        build_regions(&input, &params, CraftReference::acquisition(&input)),
        Err(CraftError::InvalidParameters)
    ));
}

#[test]
fn reference_maps_displayed_regions_and_reported_shifts_without_changing_frequency() {
    let input = data(&[(120.0, 5.0, 0.2, 2.0)], 4096, 2_000.0);
    let reference = CraftReference::new(input.carrier_ppm, 0.15);
    let params = CraftParams {
        regions: vec![CraftRegion::new(CraftRegionId(7), 0.38, 0.40)],
        max_fit_window_width_hz: 500.0,
        filter_taps: 127,
        ..CraftParams::default()
    };

    let regions = build_regions(&input, &params, reference).unwrap();
    assert!((regions[0].core.0 - 115.0).abs() < 1e-9);
    assert!((regions[0].core.1 - 125.0).abs() < 1e-9);

    let invocation = resolve_craft_invocation(
        &input,
        reference,
        &CraftParamOverrides::from_params(params),
        None,
    );
    let result = process_craft_cancellable(&input, &invocation, &|| false).unwrap();
    assert_eq!(result.components.len(), 1, "{:?}", result.components);
    assert_eq!(result.components[0].region, CraftRegionId(7));
    assert_eq!(result.region_summaries[0].region, CraftRegionId(7));
    assert!((result.components[0].frequency_hz - 120.0).abs() < 0.05);
    assert!((result.components[0].chemical_shift_ppm - 0.39).abs() < 1e-4);
}

#[test]
fn narrow_window_fit_is_independent_of_signal_phase() {
    let params = CraftParams {
        regions: vec![CraftRegion::new(CraftRegionId(1), 0.18, 0.22)],
        max_fit_window_width_hz: 500.0,
        filter_taps: 127,
        ..CraftParams::default()
    };
    let fitted_frequency = |phase| {
        let input = data(&[(100.0, 8.0, phase, 1.5)], 4096, 2_000.0);
        let invocation = CraftInvocation::acquisition(&input, params.clone());
        let result = process_craft_cancellable(&input, &invocation, &|| false).unwrap();
        assert_eq!(result.components.len(), 1, "phase {phase}: {result:?}");
        result.components[0].frequency_hz
    };

    let positive = fitted_frequency(0.0);
    let negative = fitted_frequency(PI);
    assert!((positive - 100.0).abs() < 0.1);
    assert!((negative - positive).abs() < 0.1);
}

#[test]
fn fit_windows_preserve_user_region_identity_and_one_region_ratio() {
    let input = data(
        &[(-300.0, 4.0, 0.1, 2.0), (300.0, 2.0, 0.1, 2.0)],
        4096,
        2_000.0,
    );
    let params = CraftParams {
        regions: vec![
            CraftRegion::new(CraftRegionId(22), 0.1, 1.0),
            CraftRegion::new(CraftRegionId(11), -1.0, -0.1),
        ],
        max_fit_window_width_hz: 150.0,
        max_components_per_fit_window: 3,
        filter_taps: 63,
        ..CraftParams::default()
    };
    let reference = CraftReference::acquisition(&input);

    let windows = build_regions(&input, &params, reference).unwrap();
    assert_eq!(windows.len(), 6);
    assert!(
        windows[..3]
            .iter()
            .all(|window| window.selection.id == CraftRegionId(11))
    );
    assert!(
        windows[3..]
            .iter()
            .all(|window| window.selection.id == CraftRegionId(22))
    );

    let invocation = resolve_craft_invocation(
        &input,
        reference,
        &CraftParamOverrides::from_params(params),
        None,
    );
    let result = process_craft_cancellable(&input, &invocation, &|| false).unwrap();
    assert_eq!(result.region_summaries.len(), 2);
    assert_eq!(result.region_summaries[0].region, CraftRegionId(22));
    assert_eq!(result.region_summaries[1].region, CraftRegionId(11));
    assert!(
        result
            .components
            .iter()
            .all(|component| { matches!(component.region, CraftRegionId(11) | CraftRegionId(22)) })
    );
    assert!((result.region_ratio.unwrap().value - 0.5).abs() < 0.05);
}

#[test]
fn rejects_non_finite_reference() {
    let input = data(&[], 128, 1_000.0);
    assert!(matches!(
        process_craft_cancellable(
            &input,
            &resolve_craft_invocation(
                &input,
                CraftReference::new(input.carrier_ppm, f64::NAN),
                &CraftParamOverrides::default(),
                None,
            ),
            &|| false,
        ),
        Err(CraftError::InvalidReference)
    ));
}

#[test]
fn rejects_reference_for_a_different_acquisition_carrier() {
    let input = data(&[], 128, 1_000.0);
    assert!(matches!(
        process_craft_cancellable(
            &input,
            &resolve_craft_invocation(
                &input,
                CraftReference::new(input.carrier_ppm + 0.1, 0.0),
                &CraftParamOverrides::default(),
                None,
            ),
            &|| false,
        ),
        Err(CraftError::InvalidReference)
    ));
}

#[test]
fn selected_sample_synthesis_matches_full_reconstruction() {
    let component = CraftComponent {
        id: CraftComponentId(0),
        region: CraftRegionId(0),
        frequency_hz: 25.0,
        chemical_shift_ppm: 0.05,
        amplitude_t0: 2.0,
        phase_rad: 0.3,
        decay_rate_s_inv: 4.0,
        linewidth_hz: 4.0 / PI,
        amplitude_to_noise: 10.0,
        amplitude_std: None,
        frequency_std_hz: None,
        linewidth_std_hz: None,
        phase_std_rad: None,
    };
    let indices = [0, 3, 17, 99];
    let full = synthesize_craft_fid(std::slice::from_ref(&component), 100, 1_000.0);

    let selected = synthesize_craft_samples(&[component], &indices, 1_000.0);

    assert_eq!(selected, indices.map(|index| full[index]).to_vec());
}

#[test]
fn zero_overrides_resolve_complete_bandwidth_and_stable_sources() {
    let input = data(&[(120.0, 10.0, 0.0, 2.0)], 4096, 2_000.0);
    let invocation = resolve_craft_invocation(
        &input,
        CraftReference::acquisition(&input),
        &CraftParamOverrides::default(),
        None,
    );

    assert_eq!(invocation.params.profile, CraftProfile::Conventional);
    assert_eq!(invocation.sources.profile, CraftParamSource::StableDefault);
    assert_eq!(invocation.sources.regions, CraftParamSource::InputDerived);
    assert_eq!(invocation.params.regions.len(), 1);
    assert!(invocation.assessment.can_run());
    assert!(!invocation.assessment.clear_signals.is_empty());
    assert_eq!(invocation.derived_plan.available_points, input.points.len());
}

#[test]
fn resolver_applies_per_field_explicit_provenance_default_priority() {
    let input = data(&[(80.0, 10.0, 0.0, 2.0)], 1024, 1_000.0);
    let mut prior_params = CraftParams::ssfp();
    prior_params.min_amplitude_to_noise = 8.0;
    prior_params.filter_taps = 127;
    let prior = CraftInvocation::acquisition(&input, prior_params);
    let overrides = CraftParamOverrides {
        min_amplitude_to_noise: Some(5.0),
        ..CraftParamOverrides::default()
    };

    let resolved = resolve_craft_invocation(
        &input,
        CraftReference::acquisition(&input),
        &overrides,
        Some(&prior),
    );

    assert_eq!(resolved.params.min_amplitude_to_noise, 5.0);
    assert_eq!(
        resolved.sources.min_amplitude_to_noise,
        CraftParamSource::ExplicitInput
    );
    assert_eq!(resolved.params.filter_taps, 127);
    assert_eq!(
        resolved.sources.filter_taps,
        CraftParamSource::ResultProvenance
    );
    assert_eq!(resolved.params.profile, CraftProfile::Ssfp);
}

#[test]
fn selecting_profile_clears_profile_owned_overrides_but_keeps_regions() {
    let region = CraftRegion::new(CraftRegionId(9), -0.2, 0.2);
    let mut overrides = CraftParamOverrides {
        regions: Some(vec![region]),
        min_amplitude_to_noise: Some(9.0),
        ..CraftParamOverrides::default()
    };

    overrides.select_profile(CraftProfile::Ssfp);

    assert_eq!(overrides.profile, Some(CraftProfile::Ssfp));
    assert_eq!(overrides.regions, Some(vec![region]));
    assert_eq!(overrides.min_amplitude_to_noise, None);

    let input = data(&[(80.0, 10.0, 0.0, 2.0)], 1024, 1_000.0);
    let mut conventional = CraftParams::conventional();
    conventional.max_fit_window_width_hz = 125.0;
    let previous = CraftInvocation::acquisition(&input, conventional);
    let resolved = resolve_craft_invocation(
        &input,
        CraftReference::acquisition(&input),
        &overrides,
        Some(&previous),
    );
    assert_eq!(resolved.params.max_fit_window_width_hz, 2_000.0);
    assert_eq!(
        resolved.sources.max_fit_window_width_hz,
        CraftParamSource::StableDefault
    );
}

#[test]
fn short_and_invalid_inputs_are_classified_before_execution() {
    let mut short = data(&[], 32, 1_000.0);
    short.group_delay = 20.0;
    let assessment = resolve_craft_invocation(
        &short,
        CraftReference::acquisition(&short),
        &CraftParamOverrides::default(),
        None,
    )
    .assessment;
    assert!(!assessment.can_run());
    assert!(
        assessment
            .issues
            .iter()
            .any(|issue| issue.code == CraftIssueCode::TooFewEffectivePoints)
    );

    let noisy = data(&[], 128, 1_000.0);
    let assessment = resolve_craft_invocation(
        &noisy,
        CraftReference::acquisition(&noisy),
        &CraftParamOverrides::default(),
        None,
    )
    .assessment;
    assert!(!assessment.can_run());
    assert!(
        assessment
            .issues
            .iter()
            .any(|issue| issue.code == CraftIssueCode::TooFewEffectivePoints)
    );
}

#[test]
fn derived_plan_matches_actual_fit_window_diagnostics() {
    let input = data(&[(100.0, 10.0, 0.1, 2.0)], 2048, 1_000.0);
    let invocation = resolve_craft_invocation(
        &input,
        CraftReference::acquisition(&input),
        &CraftParamOverrides::default(),
        None,
    );
    let result = process_craft_cancellable(&input, &invocation, &|| false).unwrap();

    assert_eq!(
        result.diagnostics.fit_windows.len(),
        invocation.derived_plan.fit_windows.len()
    );
    for (actual, planned) in result
        .diagnostics
        .fit_windows
        .iter()
        .zip(&invocation.derived_plan.fit_windows)
    {
        assert_eq!(actual.region, planned.region);
        assert_eq!(actual.core_hz, planned.core_hz);
        assert_eq!(actual.padded_hz, planned.padded_hz);
        assert_eq!(actual.actual_decimation, planned.planned_decimation);
    }
}
