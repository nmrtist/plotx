//! Numerical extraction of representative spectra from LC–MS scan windows.

use plotx_io::MassSpectrum;
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpectrumAggregation {
    NearestScan,
    HighestTic,
    Mean,
    Sum,
}

/// Extract an ion-current trace by summing every point in an inclusive m/z
/// interval for each source scan. This is O(total source points), which is the
/// appropriate clear baseline before profile-data indexing is justified.
pub fn extract_ion_chromatogram(
    scans: &[MassSpectrum],
    mz_min: f64,
    mz_max: f64,
) -> Result<Vec<f64>, String> {
    if !mz_min.is_finite() || !mz_max.is_finite() || mz_min < 0.0 || mz_max < 0.0 {
        return Err("The m/z interval must contain finite, non-negative bounds.".to_owned());
    }
    if mz_min >= mz_max {
        return Err("The m/z interval must have distinct ordered bounds.".to_owned());
    }
    let mut values = Vec::with_capacity(scans.len());
    for scan in scans {
        let mut sum = 0.0;
        for (&mz, &intensity) in scan.mz.iter().zip(&scan.intensity) {
            if !mz.is_finite() || !intensity.is_finite() {
                return Err("A source spectrum contains a non-finite point.".to_owned());
            }
            if mz >= mz_min && mz <= mz_max {
                sum += intensity;
                if !sum.is_finite() {
                    return Err("Extracted-ion intensity overflowed or is non-finite.".to_owned());
                }
            }
        }
        values.push(sum);
    }
    Ok(values)
}

pub fn extract_spectrum(
    scans: &[MassSpectrum],
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

#[cfg(test)]
mod tests {
    use super::*;
    use plotx_io::{MassSpectrum, Polarity, SpectrumId, SpectrumRepresentation};

    fn scan(id: u64, time: f64, mz: &[f64], intensity: &[f64]) -> MassSpectrum {
        MassSpectrum {
            id: SpectrumId::new(id),
            source_native_id: None,
            retention_time_min: time,
            ms_level: 1,
            polarity: Polarity::Positive,
            representation: SpectrumRepresentation::Profile,
            mz: mz.to_vec(),
            intensity: intensity.to_vec(),
            tic: 0.0,
            base_peak_mz: None,
            base_peak_intensity: None,
            precursor: None,
        }
    }

    #[test]
    fn xic_sums_inclusively_across_unsorted_profile_points() {
        let scans = [
            scan(1, 1.0, &[101.0, 99.0, 100.0, 102.0], &[2.0, 3.0, 5.0, 7.0]),
            scan(2, 2.0, &[98.0], &[11.0]),
        ];
        assert_eq!(
            extract_ion_chromatogram(&scans, 99.0, 101.0).unwrap(),
            [10.0, 0.0]
        );
        assert_eq!(
            extract_ion_chromatogram(&scans, 101.0, 99.0).unwrap_err(),
            "The m/z interval must have distinct ordered bounds."
        );
    }

    #[test]
    fn xic_rejects_invalid_or_overflowed_values() {
        let scans = [scan(1, 1.0, &[100.0, 100.5], &[f64::MAX, f64::MAX])];
        for (a, b) in [
            (f64::NAN, 101.0),
            (f64::INFINITY, 101.0),
            (-1.0, 1.0),
            (1.0, 1.0),
        ] {
            assert!(extract_ion_chromatogram(&scans, a, b).is_err());
        }
        assert!(extract_ion_chromatogram(&scans, 99.0, 101.0).is_err());
    }

    #[test]
    fn xic_linear_baseline_handles_a_moderate_profile_run() {
        let mz = (0..1_000).map(|value| value as f64).collect::<Vec<_>>();
        let intensity = vec![1.0; mz.len()];
        let scans = (0..100)
            .map(|index| scan(index, index as f64 / 10.0, &mz, &intensity))
            .collect::<Vec<_>>();
        let values = extract_ion_chromatogram(&scans, 250.0, 749.0).unwrap();
        assert_eq!(values, vec![500.0; 100]);
    }
}
