use super::*;
use std::f64::consts::{PI, TAU};

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
        fir_filter_taps: 127,
        ..CraftParams::default()
    };
    let invocation = CraftInvocation::acquisition(&input, params);
    let result = process_craft_cancellable(&input, &invocation, &|| false).unwrap();
    assert_eq!(result.components.len(), 2, "{:?}", result.components);
    assert!((result.components[0].frequency_hz + 75.0).abs() < 0.05);
    assert!((result.components[1].frequency_hz - 120.0).abs() < 0.05);
    assert!((result.components[0].amplitude_t0 - 8.0).abs() / 8.0 < 0.01);
    assert!((result.components[1].amplitude_t0 - 4.0).abs() / 4.0 < 0.01);
    assert!(
        result.diagnostics.normalized_residual < 0.01,
        "components={:?} diagnostics={:?}",
        result.components,
        result.diagnostics
    );
}

#[test]
fn full_band_modeling_treats_empty_windows_as_valid_no_signal_results() {
    let input = data(
        &[(-300.0, 4.0, 0.1, 2.0), (300.0, 2.0, -0.2, 2.0)],
        4096,
        2_000.0,
    );
    let params = CraftParams {
        fir_filter_taps: 63,
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
    assert_eq!(result.diagnostics.modeling_windows.len(), 2);
}

#[test]
fn ssfp_skip_extrapolates_amplitude_to_time_zero() {
    let mut input = data(&[(100.0, 5.0, 0.4, 3.0)], 1000, 20_000.0);
    input.points[..10].fill(Complex64::new(50.0, -20.0));
    let params = CraftParams {
        profile: CraftProfile::Ssfp,
        skip_duration_s: 10.0 / input.spectral_width_hz,
        reconstruction_duration_s: Some(0.1),
        fir_filter_taps: 63,
        ..CraftParams::default()
    };
    let invocation = CraftInvocation::acquisition(&input, params);
    let result = process_craft_cancellable(&input, &invocation, &|| false).unwrap();
    assert_eq!(result.components.len(), 1, "{:?}", result.components);
    assert!(
        (result.components[0].amplitude_t0 - 5.0).abs() < 0.05,
        "components={:?} diagnostics={:?}",
        result.components,
        result.diagnostics
    );
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
fn group_delay_uses_the_physical_fid_time_origin() {
    let sw = 2_000.0;
    let delay = 67.985_885_620_117_2;
    let expected_amplitude = 5.0;
    let expected_phase = 0.4;
    let mut input = data(&[], 4096, sw);
    input.group_delay = delay;
    input.points = (0..input.points.len())
        .map(|index| {
            let time = (index as f64 - delay) / sw;
            Complex64::from_polar(
                expected_amplitude * (-PI * 2.0 * time).exp(),
                expected_phase + TAU * 100.0 * time,
            )
        })
        .collect();
    let params = CraftParams {
        regions: vec![CraftRegion::new(CraftRegionId(1), 0.18, 0.22)],
        fir_filter_taps: 127,
        ..CraftParams::default()
    };

    let result = process_craft_cancellable(
        &input,
        &CraftInvocation::acquisition(&input, params),
        &|| false,
    )
    .unwrap();

    assert_eq!(result.components.len(), 1, "{:?}", result.components);
    let component = &result.components[0];
    assert!((component.amplitude_t0 - expected_amplitude).abs() < 0.02);
    let phase_error = (component.phase_rad - expected_phase).sin().abs();
    assert!(phase_error < 0.01, "phase error was {phase_error}");
}

#[test]
fn default_model_limit_resolves_non_lorentzian_multiplet_quantitation() {
    let mut components = vec![
        (-108.0, 0.75, 0.3, 0.5),
        (-100.0, 1.50, 0.3, 0.6),
        (-92.0, 0.75, 0.3, 0.7),
    ];
    for (frequency, amplitude) in [(80.0, 0.25), (88.0, 0.75), (96.0, 0.75), (104.0, 0.25)] {
        components.push((frequency, amplitude * 0.6, 0.3, 1.0));
        components.push((frequency + 0.8, amplitude * 0.4, 0.3, 1.8));
    }
    let input = data(&components, 4096, 2_000.0);
    let params = CraftParams {
        regions: vec![
            CraftRegion::new(CraftRegionId(0), -0.25, -0.15),
            CraftRegion::new(CraftRegionId(1), 0.12, 0.25),
        ],
        fir_filter_taps: 127,
        ..CraftParams::default()
    };

    let result = process_craft_cancellable(
        &input,
        &CraftInvocation::acquisition(&input, params),
        &|| false,
    )
    .unwrap();

    assert_eq!(result.region_summaries[1].component_count, 8);
    assert!(
        !result
            .diagnostics
            .warnings
            .iter()
            .any(|warning| warning.kind == CraftWarningKind::InputAssessment
                && warning.message.contains("peak density"))
    );
    let ratio = result.region_ratio.unwrap().value;
    assert!((ratio - 1.5).abs() < 0.03, "ratio was {ratio}");
    assert!(
        result.diagnostics.stability.passed,
        "{:?}",
        result.diagnostics.stability
    );
    assert!(
        result
            .diagnostics
            .stability
            .ratio
            .is_some_and(|metric| metric.relative_dispersion < 0.01)
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
        ..CraftParams::default()
    };

    assert!(matches!(
        build_modeling_windows(&input, &params, CraftReference::acquisition(&input), &[],),
        Err(CraftError::InvalidParameters)
    ));
}

#[test]
fn reference_maps_displayed_regions_and_reported_shifts_without_changing_frequency() {
    let input = data(&[(120.0, 5.0, 0.2, 2.0)], 4096, 2_000.0);
    let reference = CraftReference::new(input.carrier_ppm, 0.15);
    let params = CraftParams {
        regions: vec![CraftRegion::new(CraftRegionId(7), 0.38, 0.40)],
        fir_filter_taps: 127,
        ..CraftParams::default()
    };

    let clear_signals = preflight::detect_clear_signals(&input, reference, 0);
    let regions = build_modeling_windows(&input, &params, reference, &clear_signals).unwrap();
    assert_eq!(regions.len(), 1);
    assert!(
        ((regions[0].retention_band_hz.0 + regions[0].retention_band_hz.1) * 0.5 - 120.0).abs()
            < 0.25,
        "signals={clear_signals:?} window={:?}",
        regions[0]
    );
    assert!(
        regions
            .iter()
            .all(|window| window.retention_band_hz.1 - window.retention_band_hz.0 <= 500.0)
    );

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
fn modeling_window_is_independent_of_signal_phase() {
    let params = CraftParams {
        regions: vec![CraftRegion::new(CraftRegionId(1), 0.18, 0.22)],
        fir_filter_taps: 127,
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
fn modeling_windows_are_independent_while_components_preserve_region_identity() {
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
        maximum_model_order: 3,
        fir_filter_taps: 63,
        ..CraftParams::default()
    };
    let reference = CraftReference::acquisition(&input);

    let clear_signals = preflight::detect_clear_signals(&input, reference, 0);
    let windows = build_modeling_windows(&input, &params, reference, &clear_signals).unwrap();
    assert_eq!(windows.len(), 2);
    assert!(
        windows
            .iter()
            .all(|window| window.retention_band_hz.1 - window.retention_band_hz.0 <= 500.0)
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
fn overlapping_modeling_bands_retain_each_component_once() {
    let input = data(
        &[
            (-100.0, 10.0, 0.1, 2.0),
            (0.0, 2.0, 0.1, 2.0),
            (110.0, 8.0, 0.1, 2.0),
        ],
        4096,
        2_000.0,
    );
    let params = CraftParams {
        maximum_model_order: 4,
        fir_filter_taps: 127,
        ..CraftParams::default()
    };

    let result = process_craft_cancellable(
        &input,
        &CraftInvocation::acquisition(&input, params),
        &|| false,
    )
    .unwrap();

    assert_eq!(result.diagnostics.modeling_windows.len(), 2);
    assert_eq!(
        result
            .diagnostics
            .modeling_windows
            .iter()
            .map(|window| window.selected_model_order)
            .sum::<usize>(),
        4,
        "{:?}",
        result.diagnostics.modeling_windows
    );
    assert_eq!(result.components.len(), 3, "{:?}", result.components);
    assert_eq!(
        result
            .components
            .iter()
            .filter(|component| component.frequency_hz.abs() < 1.0)
            .count(),
        1,
        "{:?}",
        result.components
    );
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
    prior_params.minimum_amplitude_to_noise = 8.0;
    prior_params.fir_filter_taps = 127;
    let prior = CraftInvocation::acquisition(&input, prior_params);
    let overrides = CraftParamOverrides {
        minimum_amplitude_to_noise: Some(5.0),
        ..CraftParamOverrides::default()
    };

    let resolved = resolve_craft_invocation(
        &input,
        CraftReference::acquisition(&input),
        &overrides,
        Some(&prior),
    );

    assert_eq!(resolved.params.minimum_amplitude_to_noise, 5.0);
    assert_eq!(
        resolved.sources.minimum_amplitude_to_noise,
        CraftParamSource::ExplicitInput
    );
    assert_eq!(resolved.params.fir_filter_taps, 127);
    assert_eq!(
        resolved.sources.fir_filter_taps,
        CraftParamSource::ResultProvenance
    );
    assert_eq!(resolved.params.profile, CraftProfile::Ssfp);
}

#[test]
fn selecting_profile_clears_profile_owned_overrides_but_keeps_regions() {
    let region = CraftRegion::new(CraftRegionId(9), -0.2, 0.2);
    let mut overrides = CraftParamOverrides {
        regions: Some(vec![region]),
        minimum_amplitude_to_noise: Some(9.0),
        ..CraftParamOverrides::default()
    };

    overrides.select_profile(CraftProfile::Ssfp);

    assert_eq!(overrides.profile, Some(CraftProfile::Ssfp));
    assert_eq!(overrides.regions, Some(vec![region]));
    assert_eq!(overrides.minimum_amplitude_to_noise, None);

    let input = data(&[(80.0, 10.0, 0.0, 2.0)], 1024, 1_000.0);
    let previous = CraftInvocation::acquisition(&input, CraftParams::conventional());
    let resolved = resolve_craft_invocation(
        &input,
        CraftReference::acquisition(&input),
        &overrides,
        Some(&previous),
    );
    assert_eq!(resolved.params.profile, CraftProfile::Ssfp);
    assert_eq!(resolved.modeling_policy.modeling_bandwidth_hz, 2_000.0);
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
fn derived_plan_matches_actual_modeling_window_diagnostics() {
    let input = data(&[(100.0, 10.0, 0.1, 2.0)], 2048, 1_000.0);
    let invocation = resolve_craft_invocation(
        &input,
        CraftReference::acquisition(&input),
        &CraftParamOverrides::default(),
        None,
    );
    let result = process_craft_cancellable(&input, &invocation, &|| false).unwrap();

    assert_eq!(
        result.diagnostics.modeling_windows.len(),
        invocation.derived_plan.modeling_windows.len()
    );
    for (actual, planned) in result
        .diagnostics
        .modeling_windows
        .iter()
        .zip(&invocation.derived_plan.modeling_windows)
    {
        assert_eq!(actual.retention_band_hz, planned.retention_band_hz);
        assert_eq!(actual.modeling_band_hz, planned.modeling_band_hz);
        assert_eq!(actual.decimation_factor, planned.planned_decimation_factor);
    }
}

#[test]
fn user_boundaries_do_not_change_fixed_modeling_protocol() {
    let input = data(
        &[(-100.0, 6.0, 0.2, 2.0), (100.0, 4.0, 0.2, 2.0)],
        4096,
        2_000.0,
    );
    let invocation = |regions| {
        let params = CraftParams {
            regions,
            fir_filter_taps: 127,
            ..CraftParams::default()
        };
        CraftInvocation::acquisition(&input, params)
    };
    let narrow = invocation(vec![CraftRegion::new(CraftRegionId(1), -0.24, -0.16)]);
    let wide = invocation(vec![CraftRegion::new(CraftRegionId(1), -0.30, -0.10)]);

    assert_eq!(narrow.modeling_policy, wide.modeling_policy);
    assert_eq!(narrow.modeling_policy.modeling_bandwidth_hz, 250.0);
    assert_eq!(narrow.modeling_policy.modeling_duration_s, 1.0);
    assert_eq!(
        narrow.derived_plan.modeling_windows.len(),
        wide.derived_plan.modeling_windows.len()
    );
    for (left, right) in narrow
        .derived_plan
        .modeling_windows
        .iter()
        .zip(&wide.derived_plan.modeling_windows)
    {
        assert_eq!(left.retention_band_hz, right.retention_band_hz);
        assert_eq!(left.modeling_band_hz, right.modeling_band_hz);
        assert_eq!(
            left.planned_decimation_factor,
            right.planned_decimation_factor
        );
        assert_eq!(
            left.planned_modeled_sample_count,
            right.planned_modeled_sample_count
        );
        assert_eq!(
            left.planned_modeled_duration_s,
            right.planned_modeled_duration_s
        );
    }
}

#[test]
fn boundary_instability_marks_run_partial_but_keeps_components() {
    let input = data(&[(100.0, 5.0, 0.2, 2.0)], 4096, 2_000.0);
    let params = CraftParams {
        regions: vec![CraftRegion::new(CraftRegionId(7), 0.195, 0.30)],
        fir_filter_taps: 127,
        ..CraftParams::default()
    };

    let result = process_craft_cancellable(
        &input,
        &CraftInvocation::acquisition(&input, params),
        &|| false,
    )
    .unwrap();

    assert_eq!(result.components.len(), 1);
    assert_eq!(result.diagnostics.status, CraftRunStatus::Partial);
    assert!(!result.diagnostics.stability.passed);
    assert!(
        result
            .diagnostics
            .warnings
            .iter()
            .any(|warning| { warning.kind == CraftWarningKind::StabilityFailure })
    );
}

#[test]
fn global_zero_order_phase_does_not_change_coherent_amplitude() {
    let input = data(
        &[(-20.0, 3.0, 0.1, 1.5), (20.0, 2.0, 0.4, 1.8)],
        4096,
        2_000.0,
    );
    let params = CraftParams {
        regions: vec![CraftRegion::new(CraftRegionId(3), -0.10, 0.10)],
        fir_filter_taps: 127,
        ..CraftParams::default()
    };
    let fit = |input: &NmrData| {
        process_craft_cancellable(
            input,
            &CraftInvocation::acquisition(input, params.clone()),
            &|| false,
        )
        .unwrap()
        .region_summaries[0]
            .coherent_amplitude_t0
    };
    let expected = fit(&input);
    let rotation = Complex64::from_polar(1.0, 1.1);
    let mut rotated = input.clone();
    for point in &mut rotated.points {
        *point *= rotation;
    }

    let actual = fit(&rotated);
    assert!(
        (actual - expected).abs() / expected < 1e-6,
        "expected={expected} actual={actual}"
    );
}

#[test]
fn no_clear_signal_allows_exploration_but_requires_review() {
    let input = data(&[], 4096, 2_000.0);
    let invocation = CraftInvocation::acquisition(
        &input,
        CraftParams {
            fir_filter_taps: 63,
            ..CraftParams::default()
        },
    );

    assert!(invocation.assessment.can_run());
    assert!(invocation.assessment.issues.iter().any(|issue| {
        issue.code == CraftIssueCode::NoClearSignal && issue.severity == CraftIssueSeverity::Warning
    }));
    let result = process_craft_cancellable(&input, &invocation, &|| false).unwrap();

    assert_eq!(result.diagnostics.status, CraftRunStatus::Partial);
    assert!(result.components.is_empty());
    assert!(!result.diagnostics.stability.passed);
}

#[test]
fn validation_selection_keeps_model_order_when_only_the_unused_tail_changes() {
    let components = [(-30.0, 5.0, 0.2, 1.5), (42.0, 3.0, -0.3, 2.0)];
    let selected_orders = [4096, 4112].map(|count| {
        let input = data(&components, count, 2_000.0);
        let invocation = CraftInvocation::acquisition(
            &input,
            CraftParams {
                fir_filter_taps: 127,
                ..CraftParams::default()
            },
        );
        process_craft_cancellable(&input, &invocation, &|| false)
            .unwrap()
            .diagnostics
            .modeling_windows[0]
            .selected_model_order
    });

    assert_eq!(selected_orders[0], 2);
    assert_eq!(selected_orders[1], selected_orders[0]);
}
