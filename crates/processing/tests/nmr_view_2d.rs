use nmr::axis::{AxisCoordinates, AxisDomain, AxisRole, AxisUnit};
use nmr::processed::{
    ComponentBasis, ProcessedAxis, ProcessedData, ProcessedDataset, ProcessedDescriptor,
    ProcessedOrigin, ProcessedProvenance,
};
use nmr::{Complex64, ExecutionContext};
use plotx_io::nmr_view::NmrSource;
use plotx_processing::{Layout2D, Processed2D, nmr_execution::view_2d};
use std::sync::Arc;

#[test]
fn streamed_view_preserves_every_cartesian_component_and_scalar_axis() {
    for indirect in [ComponentBasis::Scalar, ComponentBasis::Cartesian] {
        for direct in [ComponentBasis::Scalar, ComponentBasis::Cartesian] {
            let axes = [(3, indirect.clone()), (5, direct)]
                .into_iter()
                .map(|(points, basis)| {
                    ProcessedAxis::new(
                        AxisRole::Signal,
                        AxisDomain::Frequency,
                        Some(AxisUnit::Hertz),
                        points,
                        AxisCoordinates::Uniform {
                            start: 9.0,
                            step: -0.5,
                        },
                        basis,
                    )
                    .unwrap()
                })
                .collect();
            let descriptor = ProcessedDescriptor::new(axes).unwrap();
            let counts = descriptor.component_counts();
            let size = 15 * counts.iter().product::<usize>();
            let samples = (0..size).map(|i| (i as f64 - 20.5) / 3.0).collect();
            let data = ProcessedData::from_descriptor(&descriptor, samples).unwrap();
            let source = NmrSource::new(Arc::new(
                ProcessedDataset::new(
                    descriptor,
                    data,
                    ProcessedProvenance::new(ProcessedOrigin::Unknown, vec![]).unwrap(),
                )
                .unwrap()
                .into(),
            ))
            .unwrap();
            let Processed2D::Ft(view) =
                view_2d(&source, Layout2D::Ft, &mut ExecutionContext::default()).unwrap()
            else {
                panic!("expected a plane");
            };
            let native = source.dataset().as_processed().unwrap().data();
            for row in 0..3 {
                for col in 0..5 {
                    let re = native.get(&[row, col], &[0, 0]).unwrap();
                    let im = if counts[1] == 2 {
                        native.get(&[row, col], &[0, 1]).unwrap()
                    } else {
                        0.0
                    };
                    assert_eq!(view.at(row, col), Complex64::new(re, im));
                    let mut magnitude = 0.0_f64;
                    for a in 0..counts[0] {
                        for b in 0..counts[1] {
                            magnitude = magnitude.hypot(native.get(&[row, col], &[a, b]).unwrap());
                        }
                    }
                    assert_eq!(view.magnitude_at(row * 5 + col), Some(magnitude));
                }
            }
            let Processed2D::Stack(stack) =
                view_2d(&source, Layout2D::Stack, &mut ExecutionContext::default()).unwrap()
            else {
                panic!("expected traces");
            };
            assert_eq!(
                stack.traces.iter().flatten().copied().collect::<Vec<_>>(),
                view.data
            );
        }
    }
}

#[test]
fn cancelled_view_is_not_returned_as_a_success() {
    // Reuse the public source constructor exercised by the integration fixtures.
    let dim = plotx_io::Dim {
        spectral_width_hz: 10.0,
        observe_freq_mhz: 100.0,
        carrier_ppm: 5.0,
        nucleus: "1H".into(),
        group_delay: 0.0,
    };
    let input = plotx_io::nmr_series::NmrSeriesSource::try_from(plotx_io::NmrData2D {
        data: vec![Complex64::new(1.0, 2.0); 16],
        rows: 4,
        cols: 4,
        domain: plotx_io::Domain::Frequency,
        direct: dim.clone(),
        indirect: dim,
        quad: plotx_io::QuadMode::Complex,
        indirect_conjugate: false,
        experiment: None,
        pseudo_axis: None,
        diffusion: None,
        nus: None,
        source: "cancel".into(),
    })
    .unwrap();
    let token = nmr::CancellationToken::new();
    token.cancel();
    let mut ledger = plotx_processing::nmr_execution::processing_2d_work_ledger();
    let result = view_2d(
        input.source_dataset(),
        Layout2D::Ft,
        &mut ExecutionContext::new(&mut ledger).with_cancellation(token),
    );
    assert!(result.unwrap_err().is_cancelled());
}
