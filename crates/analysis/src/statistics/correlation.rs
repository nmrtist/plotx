use statrs::distribution::{ContinuousCDF, StudentsT};

use super::{
    StatisticsError, centered_sum_squares, checked_mean, degenerate_ratio, validate_sample,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CorrelationMethod {
    Pearson,
    Spearman,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CorrelationResult {
    pub method: CorrelationMethod,
    pub observations: usize,
    pub coefficient: f64,
    /// Student t approximation used to calculate `p_value`.
    pub statistic: f64,
    pub degrees_of_freedom: usize,
    pub p_value: f64,
}

/// Pearson product-moment correlation with a two-sided t-test of `rho = 0`.
pub fn pearson(left: &[f64], right: &[f64]) -> Result<CorrelationResult, StatisticsError> {
    validate_pair(left, right)?;
    correlation_core(left, right, CorrelationMethod::Pearson)
}

/// Spearman rank correlation with average ranks for ties.
///
/// The two-sided p-value uses the conventional t approximation. This remains
/// deterministic in the presence of ties and is intended for `n >= 3`; it is
/// not an exact small-sample permutation p-value.
pub fn spearman(left: &[f64], right: &[f64]) -> Result<CorrelationResult, StatisticsError> {
    validate_pair(left, right)?;
    let left_ranks = average_ranks(left);
    let right_ranks = average_ranks(right);
    correlation_core(&left_ranks, &right_ranks, CorrelationMethod::Spearman)
}

fn validate_pair(left: &[f64], right: &[f64]) -> Result<(), StatisticsError> {
    if left.len() != right.len() {
        return Err(StatisticsError::LengthMismatch {
            left: left.len(),
            right: right.len(),
        });
    }
    validate_sample(left, "left sample", 3)?;
    validate_sample(right, "right sample", 3)
}

fn correlation_core(
    left: &[f64],
    right: &[f64],
    method: CorrelationMethod,
) -> Result<CorrelationResult, StatisticsError> {
    let left_mean = checked_mean(left);
    let right_mean = checked_mean(right);
    let left_ss = centered_sum_squares(left, left_mean);
    let right_ss = centered_sum_squares(right, right_mean);
    if left_ss <= 0.0 {
        return Err(StatisticsError::ZeroVariance {
            sample: "left sample".to_owned(),
        });
    }
    if right_ss <= 0.0 {
        return Err(StatisticsError::ZeroVariance {
            sample: "right sample".to_owned(),
        });
    }
    let cross = left
        .iter()
        .zip(right)
        .map(|(&x, &y)| (x - left_mean) * (y - right_mean))
        .sum::<f64>();
    let coefficient = (cross / (left_ss * right_ss).sqrt()).clamp(-1.0, 1.0);
    let degrees_of_freedom = left.len() - 2;
    let statistic = degenerate_ratio(
        coefficient * (degrees_of_freedom as f64).sqrt(),
        (1.0 - coefficient * coefficient).max(0.0).sqrt(),
    );
    let distribution = StudentsT::new(0.0, 1.0, degrees_of_freedom as f64)
        .expect("correlation validation guarantees positive degrees of freedom");
    let p_value = if statistic.is_infinite() {
        0.0
    } else {
        (2.0 * distribution.sf(statistic.abs())).clamp(0.0, 1.0)
    };
    Ok(CorrelationResult {
        method,
        observations: left.len(),
        coefficient,
        statistic,
        degrees_of_freedom,
        p_value,
    })
}

fn average_ranks(values: &[f64]) -> Vec<f64> {
    let mut order: Vec<usize> = (0..values.len()).collect();
    order.sort_by(|&a, &b| values[a].total_cmp(&values[b]));
    let mut ranks = vec![0.0; values.len()];
    let mut start = 0;
    while start < order.len() {
        let mut end = start + 1;
        while end < order.len() && values[order[end]] == values[order[start]] {
            end += 1;
        }
        // Ranks are one-based; all tied entries receive the middle rank.
        let rank = (start + 1 + end) as f64 / 2.0;
        for &index in &order[start..end] {
            ranks[index] = rank;
        }
        start = end;
    }
    ranks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pearson_matches_reference_value() {
        let result = pearson(
            &[43.0, 21.0, 25.0, 42.0, 57.0, 59.0],
            &[99.0, 65.0, 79.0, 75.0, 87.0, 81.0],
        )
        .unwrap();
        assert!((result.coefficient - 0.529_808_901_890_174_4).abs() < 1e-14);
        // statrs' incomplete-beta evaluation agrees with SciPy to < 1e-9.
        assert!((result.p_value - 0.279_644_657_004_871_95).abs() < 1e-9);
    }

    #[test]
    fn spearman_uses_average_tie_ranks() {
        let result = spearman(&[1.0, 2.0, 2.0, 4.0, 5.0], &[5.0, 3.0, 4.0, 2.0, 1.0]).unwrap();
        assert!((result.coefficient + 0.974_679_434_480_896_3).abs() < 1e-14);
        assert!(result.p_value < 0.01);
    }
}
