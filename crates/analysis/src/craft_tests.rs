use super::*;

fn synthetic(
    components: &[(f64, f64, f64, f64)],
    count: usize,
    sw: f64,
) -> (Vec<f64>, Vec<Complex64>) {
    let times: Vec<f64> = (0..count).map(|index| index as f64 / sw).collect();
    let samples = times
        .iter()
        .map(|&time| {
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
    (times, samples)
}

#[test]
fn backward_prediction_restores_filtered_record_leading_points() {
    let components = [
        (13.0, 4.0, 0.3, 0.8),
        (-21.0, 2.5, -0.4, 1.7),
        (37.0, 1.2, 1.1, 2.4),
    ];
    let (_, expected) = synthetic(&components, 300, 500.0);
    let mut samples = expected.clone();
    samples[..5].fill(Complex64::new(100.0, -50.0));

    backward_linear_predict(&mut samples, 5, 256, 32).unwrap();

    for index in 0..5 {
        assert!(
            (samples[index] - expected[index]).norm() < 1e-7,
            "index={index} predicted={:?} expected={:?}",
            samples[index],
            expected[index]
        );
    }
}

#[test]
fn backward_prediction_restores_a_short_single_exponential() {
    let (_, expected) = synthetic(&[(0.0, 3.0, 0.4, 5.0)], 192, 4_000.0);
    let mut samples = expected.clone();
    samples[..5].fill(Complex64::new(100.0, -50.0));

    backward_linear_predict(&mut samples, 5, 187, 16).unwrap();

    for index in 0..5 {
        assert!(
            (samples[index] - expected[index]).norm() < 1e-7,
            "index={index} predicted={:?} expected={:?}",
            samples[index],
            expected[index]
        );
    }
}

#[test]
fn backward_prediction_rejects_an_underspecified_fit() {
    let mut samples = vec![Complex64::new(1.0, 0.0); 12];
    assert_eq!(
        backward_linear_predict(&mut samples, 5, 7, 7),
        Err(CraftFitError::InvalidInput)
    );
}

#[test]
fn recovers_single_damped_sinusoid() {
    let (times, samples) = synthetic(&[(123.4, 7.5, 0.37, 2.2)], 2048, 2000.0);
    let fit = fit_damped_sinusoids_cancellable(
        &samples,
        &times,
        &[123.0],
        CraftFitBounds {
            frequency_hz: (100.0, 150.0),
            linewidth_hz: (0.05, 20.0),
        },
        CraftFitOptions::default(),
        &|| false,
    )
    .unwrap();
    let component = &fit.components[0];
    assert!((component.frequency_hz - 123.4).abs() < 0.01);
    assert!((component.amplitude - 7.5).abs() < 0.01);
    assert!((component.phase_rad - 0.37).abs() < 0.01);
    assert!((component.linewidth_hz - 2.2).abs() < 0.02);
    assert!(fit.rss < 1e-8, "rss={}", fit.rss);
}

#[test]
fn recovers_two_overlapping_components() {
    let (times, samples) = synthetic(
        &[(35.0, 4.0, 0.2, 1.4), (38.0, 2.5, -0.5, 2.0)],
        4096,
        2000.0,
    );
    let fit = fit_damped_sinusoids_cancellable(
        &samples,
        &times,
        &[34.8, 38.2],
        CraftFitBounds {
            frequency_hz: (30.0, 45.0),
            linewidth_hz: (0.05, 20.0),
        },
        CraftFitOptions::default(),
        &|| false,
    )
    .unwrap();
    assert_eq!(fit.components.len(), 2);
    assert!((fit.components[0].frequency_hz - 35.0).abs() < 0.03);
    assert!((fit.components[1].frequency_hz - 38.0).abs() < 0.03);
    assert!(fit.rss < 1e-7, "rss={}", fit.rss);
}

#[test]
fn matrix_pencil_initialization_recovers_poles_and_fixed_amplitudes() {
    let (times, samples) = synthetic(&[(35.0, 4.0, 0.2, 1.4), (38.0, 2.5, -0.5, 2.0)], 128, 200.0);
    let bounds = CraftFitBounds {
        frequency_hz: (30.0, 45.0),
        linewidth_hz: (0.05, 20.0),
    };
    let estimate = matrix_pencil_estimates(&samples, 1.0 / 200.0, 2, bounds).unwrap();
    let fit = evaluate_damped_sinusoids_cancellable(
        &samples,
        &times,
        &estimate.components,
        bounds,
        &|| false,
    )
    .unwrap();

    assert_eq!(fit.components.len(), 2);
    assert!((fit.components[0].frequency_hz - 35.0).abs() < 1e-6);
    assert!((fit.components[1].frequency_hz - 38.0).abs() < 1e-6);
    assert!((fit.components[0].amplitude - 4.0).abs() < 1e-6);
    assert!((fit.components[1].amplitude - 2.5).abs() < 1e-6);
    assert!(
        fit.components
            .iter()
            .all(|component| component.amplitude_std.is_some())
    );
}

#[test]
fn matrix_pencil_rejects_an_order_with_no_poles_inside_the_fit_bounds() {
    let (_, samples) = synthetic(&[(35.0, 4.0, 0.2, 1.4)], 128, 200.0);
    let result = matrix_pencil_estimates(
        &samples,
        1.0 / 200.0,
        1,
        CraftFitBounds {
            frequency_hz: (100.0, 120.0),
            linewidth_hz: (0.05, 20.0),
        },
    );

    assert_eq!(result.unwrap_err(), CraftFitError::Singular);
}

#[test]
fn reports_cancellation() {
    let (times, samples) = synthetic(&[(10.0, 1.0, 0.0, 1.0)], 128, 1000.0);
    let error = fit_damped_sinusoids_cancellable(
        &samples,
        &times,
        &[10.0],
        CraftFitBounds {
            frequency_hz: (0.0, 20.0),
            linewidth_hz: (0.05, 20.0),
        },
        CraftFitOptions::default(),
        &|| true,
    )
    .unwrap_err();
    assert_eq!(error, CraftFitError::Cancelled);
}

#[test]
fn rank_deficient_design_has_infinite_condition_number() {
    let times = [0.0, 0.001, 0.002, 0.003];
    let component = DecodedComponent {
        coefficient: Complex64::new(1.0, 0.0),
        frequency_hz: 10.0,
        decay_rate: 2.0,
        frequency_scale: 1.0,
        linewidth_scale: 1.0,
    };

    let condition = design_condition_number(&times, &[component, component]);

    assert!(condition.is_infinite());
}
