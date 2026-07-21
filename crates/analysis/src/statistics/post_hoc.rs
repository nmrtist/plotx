use super::{StatisticsError, degenerate_ratio, one_way_anova, validate_confidence_level};

const MAX_CONFIDENCE_LEVEL: f64 = 0.999_99;

#[derive(Clone, Debug, PartialEq)]
pub struct TukeyComparison {
    pub group_a: usize,
    pub group_b: usize,
    /// Signed difference `mean(group_a) - mean(group_b)`.
    pub mean_difference: f64,
    pub standard_error: f64,
    pub q_statistic: f64,
    /// Family-wise-error-rate adjusted p-value.
    pub p_value: f64,
    pub confidence_lower: f64,
    pub confidence_upper: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TukeyHsd {
    pub confidence_level: f64,
    pub error_degrees_of_freedom: usize,
    pub mean_square_error: f64,
    pub critical_q: f64,
    pub comparisons: Vec<TukeyComparison>,
}

/// Tukey–Kramer honestly significant difference post-hoc comparisons.
///
/// Unequal group sizes use the Tukey–Kramer standard error. Returned p-values
/// and confidence intervals control family-wise error across all pairwise
/// comparisons under the one-way ANOVA equal-variance model.
/// The studentized-range quadrature is accurate to approximately five
/// significant figures for the supported residual degrees of freedom (`df >= 2`).
pub fn tukey_hsd(groups: &[&[f64]], confidence_level: f64) -> Result<TukeyHsd, StatisticsError> {
    validate_confidence_level(confidence_level)?;
    let anova = one_way_anova(groups)?;
    let group_count = groups.len();
    let df = anova.residual.degrees_of_freedom;
    if df < 2 {
        return Err(StatisticsError::InsufficientResidualDegreesOfFreedom {
            minimum: 2,
            actual: df,
        });
    }
    let mse = anova.residual.mean_square;
    let critical_q = studentized_range_quantile(confidence_level, group_count, df)?;
    let mut comparisons = Vec::with_capacity(group_count * (group_count - 1) / 2);
    for group_a in 0..group_count {
        for group_b in (group_a + 1)..group_count {
            let difference = anova.group_means[group_a] - anova.group_means[group_b];
            let standard_error = (mse / 2.0
                * (1.0 / anova.group_sizes[group_a] as f64
                    + 1.0 / anova.group_sizes[group_b] as f64))
                .sqrt();
            let q_statistic = degenerate_ratio(difference.abs(), standard_error);
            let p_value = if q_statistic.is_infinite() {
                0.0
            } else {
                (1.0 - tukey_test::ptukey_cdf(q_statistic, group_count, df)).clamp(0.0, 1.0)
            };
            let margin = critical_q * standard_error;
            comparisons.push(TukeyComparison {
                group_a,
                group_b,
                mean_difference: difference,
                standard_error,
                q_statistic,
                p_value,
                confidence_lower: difference - margin,
                confidence_upper: difference + margin,
            });
        }
    }
    Ok(TukeyHsd {
        confidence_level,
        error_degrees_of_freedom: df,
        mean_square_error: mse,
        critical_q,
        comparisons,
    })
}

fn studentized_range_quantile(
    probability: f64,
    groups: usize,
    df: usize,
) -> Result<f64, StatisticsError> {
    // The upstream quadrature promises about five significant figures. A
    // smaller upper-tail probability could make numerical saturation look
    // like a valid bracket, so fail explicitly instead of claiming coverage.
    if probability > MAX_CONFIDENCE_LEVEL {
        return Err(StatisticsError::UnsupportedTukeyConfidence {
            maximum: MAX_CONFIDENCE_LEVEL,
            actual: probability,
        });
    }
    let mut lower = 0.0;
    let mut upper = 16.0;
    let mut upper_probability = tukey_test::ptukey_cdf(upper, groups, df);
    while upper_probability < probability {
        if upper > f64::MAX / 2.0 {
            return Err(StatisticsError::QuantileDidNotConverge);
        }
        upper *= 2.0;
        upper_probability = tukey_test::ptukey_cdf(upper, groups, df);
        if !upper_probability.is_finite() {
            return Err(StatisticsError::QuantileDidNotConverge);
        }
    }
    for _ in 0..128 {
        let middle = (lower + upper) / 2.0;
        if tukey_test::ptukey_cdf(middle, groups, df) < probability {
            lower = middle;
        } else {
            upper = middle;
        }
    }
    let quantile = (lower + upper) / 2.0;
    if quantile.is_finite() {
        Ok(quantile)
    } else {
        Err(StatisticsError::QuantileDidNotConverge)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tukey_kramer_matches_hand_computed_statistics() {
        let a = [4.0, 5.0, 6.0];
        let b = [8.0, 9.0, 10.0];
        let c = [5.0, 6.0, 7.0];
        let result = tukey_hsd(&[&a, &b, &c], 0.95).unwrap();
        assert!((result.mean_square_error - 1.0).abs() < 1e-12);
        assert!((result.critical_q - 4.339).abs() < 0.02);
        assert!((result.comparisons[0].q_statistic - 6.928_203_230_275_51).abs() < 1e-12);
        assert!(result.comparisons[0].p_value < 0.01);
        assert!(result.comparisons[1].confidence_lower < 0.0);
        assert!(result.comparisons[1].confidence_upper > 0.0);
    }

    #[test]
    fn tukey_supports_arbitrary_confidence_levels() {
        let a = [1.0, 2.0, 3.0];
        let b = [2.0, 3.0, 4.0];
        let c = [4.0, 5.0, 6.0];
        let at_90 = tukey_hsd(&[&a, &b, &c], 0.90).unwrap();
        let at_99 = tukey_hsd(&[&a, &b, &c], 0.99).unwrap();
        assert!(at_99.critical_q > at_90.critical_q);
    }

    #[test]
    fn rejects_confidence_boundary_and_unsupported_residual_df() {
        let a = [1.0];
        let b = [2.0];
        let c = [1.5, 2.5];
        assert_eq!(
            tukey_hsd(&[&a, &b, &c], 0.0),
            Err(StatisticsError::InvalidConfidenceLevel)
        );
        assert_eq!(
            tukey_hsd(&[&a, &b, &c], 0.95),
            Err(StatisticsError::InsufficientResidualDegreesOfFreedom {
                minimum: 2,
                actual: 1,
            })
        );
    }

    #[test]
    fn extreme_quantile_is_not_silently_truncated() {
        assert_eq!(
            studentized_range_quantile(1.0 - 1e-9, 3, 1),
            Err(StatisticsError::UnsupportedTukeyConfidence {
                maximum: MAX_CONFIDENCE_LEVEL,
                actual: 1.0 - 1e-9,
            })
        );
    }
}
