use statrs::distribution::{ContinuousCDF, Normal};

use super::{StatisticsError, centered_sum_squares, checked_mean, validate_sample};

const MAX_SAMPLE_SIZE: usize = 5000;
const C1: [f64; 6] = [0.0, 0.221_157, -0.147_981, -2.071_19, 4.434_685, -2.706_056];
const C2: [f64; 6] = [
    0.0, 0.042_981, -0.293_762, -1.752_461, 5.682_633, -3.582_633,
];
const C3: [f64; 4] = [0.544, -0.399_78, 0.025_054, -0.000_671_4];
const C4: [f64; 4] = [1.382_2, -0.778_57, 0.062_767, -0.002_032_2];
const C5: [f64; 4] = [-1.586_1, -0.310_82, -0.083_751, 0.003_891_5];
const C6: [f64; 3] = [-0.480_3, -0.082_676, 0.003_030_2];

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NormalityResult {
    pub observations: usize,
    /// Shapiro–Wilk W statistic in `[0, 1]`; values near one are more normal.
    pub statistic: f64,
    /// Royston's AS R94 approximation (exact transform when `n = 3`).
    pub p_value: f64,
}

/// Shapiro–Wilk normality test for 3 through 5000 finite observations.
///
/// The W coefficients and p-value transform follow Royston's AS R94 algorithm.
/// Samples with no spread are rejected because W is undefined.
pub fn shapiro_wilk(values: &[f64]) -> Result<NormalityResult, StatisticsError> {
    validate_sample(values, "sample", 3)?;
    if values.len() > MAX_SAMPLE_SIZE {
        return Err(StatisticsError::TooManyObservations {
            sample: "sample".to_owned(),
            maximum: MAX_SAMPLE_SIZE,
            actual: values.len(),
        });
    }
    let mut ordered = values.to_vec();
    ordered.sort_by(f64::total_cmp);
    let mean = checked_mean(&ordered);
    let denominator = centered_sum_squares(&ordered, mean);
    if denominator <= 0.0 {
        return Err(StatisticsError::ZeroVariance {
            sample: "sample".to_owned(),
        });
    }
    let weights = shapiro_weights(values.len());
    let numerator = weights
        .iter()
        .enumerate()
        .map(|(index, weight)| weight * (ordered[ordered.len() - 1 - index] - ordered[index]))
        .sum::<f64>();
    let statistic = (numerator * numerator / denominator).clamp(0.0, 1.0);
    let p_value = shapiro_p_value(statistic, values.len());
    Ok(NormalityResult {
        observations: values.len(),
        statistic,
        p_value,
    })
}

fn shapiro_weights(n: usize) -> Vec<f64> {
    let half = n / 2;
    if n == 3 {
        return vec![std::f64::consts::FRAC_1_SQRT_2];
    }
    let normal = Normal::standard();
    let mut raw: Vec<f64> = (0..half)
        .map(|index| {
            let probability = (index as f64 + 1.0 - 0.375) / (n as f64 + 0.25);
            normal.inverse_cdf(probability)
        })
        .collect();
    let sum_squares = 2.0 * raw.iter().map(|value| value * value).sum::<f64>();
    let normalization = sum_squares.sqrt();
    let inverse_root_n = 1.0 / (n as f64).sqrt();
    let first = polynomial(&C1, inverse_root_n) - raw[0] / normalization;
    let start = if n > 5 {
        let second = polynomial(&C2, inverse_root_n) - raw[1] / normalization;
        let scale = ((sum_squares - 2.0 * raw[0].powi(2) - 2.0 * raw[1].powi(2))
            / (1.0 - 2.0 * first.powi(2) - 2.0 * second.powi(2)))
        .sqrt();
        raw[0] = first;
        raw[1] = second;
        for value in &mut raw[2..] {
            *value /= -scale;
        }
        2
    } else {
        let scale = ((sum_squares - 2.0 * raw[0].powi(2)) / (1.0 - 2.0 * first.powi(2))).sqrt();
        raw[0] = first;
        for value in &mut raw[1..] {
            *value /= -scale;
        }
        1
    };
    debug_assert!(raw[..start].iter().all(|value| *value > 0.0));
    raw
}

fn shapiro_p_value(statistic: f64, n: usize) -> f64 {
    if n == 3 {
        return (6.0 / std::f64::consts::PI
            * (statistic.sqrt().asin() - std::f64::consts::PI / 3.0))
            .clamp(0.0, 1.0);
    }
    let mut transformed = (1.0 - statistic).max(f64::MIN_POSITIVE).ln();
    let (mean, deviation) = if n <= 11 {
        let gamma = -2.273 + 0.459 * n as f64;
        if transformed >= gamma {
            return 0.0;
        }
        transformed = -(gamma - transformed).ln();
        (polynomial(&C3, n as f64), polynomial(&C4, n as f64).exp())
    } else {
        let log_n = (n as f64).ln();
        (polynomial(&C5, log_n), polynomial(&C6, log_n).exp())
    };
    Normal::standard()
        .sf((transformed - mean) / deviation)
        .clamp(0.0, 1.0)
}

fn polynomial(coefficients: &[f64], value: f64) -> f64 {
    coefficients
        .iter()
        .rev()
        .fold(0.0, |result, coefficient| result * value + coefficient)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_scipy_reference_samples() {
        let result = shapiro_wilk(&[1.2, 0.8, 1.5, 0.9, 1.0, 1.1, 0.7, 1.3, 1.4, 0.6]).unwrap();
        assert!((result.statistic - 0.970_164_611_085_605_6).abs() < 2e-8);
        assert!((result.p_value - 0.892_367_306_190_297_8).abs() < 2e-7);

        let skewed = shapiro_wilk(&[1.0, 1.1, 1.2, 1.3, 1.4, 10.0, 12.0, 15.0]).unwrap();
        assert!((skewed.statistic - 0.739_515_434_656_978_6).abs() < 2e-8);
        assert!((skewed.p_value - 0.006_250_180_262_374_652_5).abs() < 2e-7);
    }

    #[test]
    fn three_point_transform_is_exact_and_constant_data_fails() {
        let result = shapiro_wilk(&[1.0, 2.0, 3.0]).unwrap();
        assert!((result.statistic - 1.0).abs() < 1e-14);
        assert!((result.p_value - 1.0).abs() < 1e-7);
        assert_eq!(
            shapiro_wilk(&[2.0, 2.0, 2.0]),
            Err(StatisticsError::ZeroVariance {
                sample: "sample".to_owned()
            })
        );
    }

    #[test]
    fn royston_sample_size_branches_match_scipy() {
        let references = [
            (4, 0.967_903_126_846_404_1, 0.828_458_386_493_402_1),
            (5, 0.983_757_563_069_097_1, 0.953_664_636_694_939_5),
            (6, 0.931_550_849_376_511_1, 0.592_130_572_547_563_1),
            (11, 0.900_495_470_039_652_9, 0.187_443_222_193_765_14),
            (12, 0.922_950_701_746_254_7, 0.311_309_994_265_664_47),
            (20, 0.935_724_257_323_813_6, 0.198_849_151_169_064),
            (100, 0.979_337_358_868_163_8, 0.118_021_440_869_108_6),
        ];
        for (n, expected_w, expected_p) in references {
            let values: Vec<f64> = (0..n)
                .map(|index| (index as f64 * 1.7).sin() + 0.03 * (index * index) as f64 / n as f64)
                .collect();
            let result = shapiro_wilk(&values).unwrap();
            assert!((result.statistic - expected_w).abs() < 2e-8, "n={n}");
            assert!((result.p_value - expected_p).abs() < 2e-7, "n={n}");
        }
    }
}
