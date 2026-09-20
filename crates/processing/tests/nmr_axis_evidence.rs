use nmr::processed::{
    ComponentBasis, ProcessedAxis, ProcessedData, ProcessedDataset, ProcessedDescriptor,
    ProcessedOrigin, ProcessedProvenance,
};
use nmr::processing::{FrequencyFrame, ProcessingOperation as Op, ProcessingPlan, ReferenceSource};
use nmr::{
    ExecutionContext,
    axis::{AxisCoordinates, AxisDomain, AxisRole, AxisUnit, FrequencyEvidence},
};
use plotx_io::{nmr_bridge, nmr_view::NmrSource};
use plotx_processing::{
    arithmetic::{SpectrumBinaryOp, combine_spectra},
    slice::{Reduction, SliceKind, extract},
};
use std::sync::Arc;

fn two_axes(indirect_reference: f64) -> NmrSource {
    let axes = [2, 3]
        .into_iter()
        .map(|points| {
            ProcessedAxis::new(
                AxisRole::Signal,
                AxisDomain::Frequency,
                Some(AxisUnit::Hertz),
                points,
                AxisCoordinates::Uniform {
                    start: 0.0,
                    step: 1.0,
                },
                ComponentBasis::Cartesian,
            )
            .unwrap()
            .with_frequency_evidence(Some(FrequencyEvidence::new(Some(500.005), None).unwrap()))
            .unwrap()
        })
        .collect();
    let descriptor = ProcessedDescriptor::new(axes).unwrap();
    let data =
        ProcessedData::from_descriptor(&descriptor, (1..=24).map(f64::from).collect()).unwrap();
    let input = ProcessedDataset::new(
        descriptor,
        data,
        ProcessedProvenance::new(ProcessedOrigin::Unknown, vec![]).unwrap(),
    )
    .unwrap();
    let ops = [indirect_reference, 400.0]
        .into_iter()
        .enumerate()
        .map(|(axis, mhz)| Op::ResolveFrequencyFrame {
            axis,
            frame: FrequencyFrame::Ppm(ReferenceSource::Explicit(
                nmr::raw::ChemicalShiftReference::user_constructed(5.0, mhz).unwrap(),
            )),
        })
        .collect();
    NmrSource::new(Arc::new(
        ProcessingPlan::new(ops)
            .unwrap()
            .apply(&input.into())
            .unwrap(),
    ))
    .unwrap()
}

#[test]
fn column_slice_reindexes_reference_and_binary_output_keeps_a_reference_offline() {
    let (a, _) = extract(&two_axes(100.0), SliceKind::Column, Reduction::Slice(1)).unwrap();
    let (b, _) = extract(&two_axes(200.0), SliceKind::Column, Reduction::Slice(1)).unwrap();
    assert_eq!(a.reference_frequency_mhz(0), Some(100.0));
    assert_eq!(a.reference_frequency_mhz(1), None);
    assert_eq!(b.reference_frequency_mhz(0), Some(200.0));
    let result = combine_spectra(&a, &b, SpectrumBinaryOp::Add, 1.0).unwrap();
    assert_eq!(result.reference_frequency_mhz(0), Some(100.0));
    assert_eq!(result.axes()[0].observe_frequency_mhz(), Some(500.005));
    assert_eq!(
        result.axes()[0].coordinate_values().unwrap(),
        a.axes()[0].coordinate_values().unwrap()
    );
    let mut bytes = Vec::new();
    nmr_bridge::snapshot::write(
        result.dataset(),
        &mut bytes,
        Default::default(),
        &mut ExecutionContext::default(),
    )
    .unwrap();
    let restored = NmrSource::new(
        nmr_bridge::snapshot::read(
            &mut bytes.as_slice(),
            Default::default(),
            &mut ExecutionContext::default(),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(restored.reference_frequency_mhz(0), Some(100.0));
    assert_eq!(restored.axes()[0].observe_frequency_mhz(), Some(500.005));
    assert_eq!(restored.trace().unwrap(), result.trace().unwrap());
}
