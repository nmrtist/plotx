//! Numerical extraction of representative spectra from LC–MS scan windows.

use plotx_io::MassScan;
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpectrumAggregation {
    NearestScan,
    HighestTic,
    Mean,
    Sum,
}

pub fn extract_spectrum(
    scans: &[MassScan],
    range_min: [f64; 2],
    aggregation: SpectrumAggregation,
) -> Option<Vec<[f64; 2]>> {
    let scans = scans
        .iter()
        .filter(|scan| {
            scan.retention_time_min >= range_min[0] && scan.retention_time_min <= range_min[1]
        })
        .collect::<Vec<_>>();
    let selected = match aggregation {
        SpectrumAggregation::NearestScan => {
            let center = (range_min[0] + range_min[1]) / 2.0;
            scans.iter().copied().min_by(|left, right| {
                (left.retention_time_min - center)
                    .abs()
                    .total_cmp(&(right.retention_time_min - center).abs())
                    .then_with(|| left.id.cmp(&right.id))
            })
        }
        SpectrumAggregation::HighestTic => scans.iter().copied().max_by(|left, right| {
            left.tic
                .total_cmp(&right.tic)
                .then_with(|| right.id.cmp(&left.id))
        }),
        SpectrumAggregation::Mean | SpectrumAggregation::Sum => None,
    };
    if let Some(scan) = selected {
        return Some(
            scan.mz
                .iter()
                .copied()
                .zip(scan.intensity.iter().copied())
                .map(|(x, y)| [x, y])
                .collect(),
        );
    }
    if scans.is_empty() {
        return None;
    }
    // Waters low-resolution profile coordinates are stable scan-to-scan, but
    // calibration round-off can perturb their final decimals. A 1e-4 Da key
    // joins numerical copies without introducing a peak-width assumption.
    let mut bins = BTreeMap::<i64, (f64, f64, usize)>::new();
    for scan in &scans {
        for (&mz, &intensity) in scan.mz.iter().zip(&scan.intensity) {
            let bin = bins.entry((mz * 10_000.0).round() as i64).or_default();
            bin.0 += mz;
            bin.1 += intensity;
            bin.2 += 1;
        }
    }
    let denominator = scans.len() as f64;
    Some(
        bins.into_values()
            .map(|(mz_sum, intensity_sum, count)| {
                let intensity = if aggregation == SpectrumAggregation::Mean {
                    intensity_sum / denominator
                } else {
                    intensity_sum
                };
                [mz_sum / count as f64, intensity]
            })
            .collect(),
    )
}
