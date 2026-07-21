use statrs::distribution::{ContinuousCDF, StudentsT};

use super::{
    StatisticsError, centered_sum_squares, checked_mean, validate_confidence_level, validate_sample,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Alternative {
    TwoSided,
    Less,
    Greater,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VarianceAssumption {
    /// Classical Student test with a pooled variance estimate.
    Equal,
    /// Welch test with Satterthwaite degrees of freedom.
    Unequal,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ConfidenceInterval {
    pub level: f64,
    pub lower: f64,
    pub upper: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TTestResult {
    pub statistic: f64,
    pub degrees_of_freedom: f64,
    pub p_value: f64,
    pub estimate: f64,
    pub null_value: f64,
    pub standard_error: f64,
    pub confidence_interval: ConfidenceInterval,
    /// Cohen's d. One-sample and paired tests standardize the distance from the
    /// null; independent tests standardize the raw difference between means.
    pub cohens_d: f64,
    pub alternative: Alternative,
}

pub fn one_sample_t_test(
    sample: &[f64],
    hypothesized_mean: f64,
    alternative: Alternative,
    confidence_level: f64,
) -> Result<TTestResult, StatisticsError> {
    one_sample_t_test_impl(
        sample,
        "sample",
        hypothesized_mean,
        alternative,
        confidence_level,
    )
}

fn one_sample_t_test_impl(
    sample: &[f64],
    sample_name: &str,
    hypothesized_mean: f64,
    alternative: Alternative,
    confidence_level: f64,
) -> Result<TTestResult, StatisticsError> {
    validate_common(sample, sample_name, hypothesized_mean, confidence_level)?;
    let mean = checked_mean(sample);
    let variance = centered_sum_squares(sample, mean) / (sample.len() - 1) as f64;
    if variance <= 0.0 {
        return Err(StatisticsError::ZeroVariance {
            sample: sample_name.to_owned(),
        });
    }
    let standard_deviation = variance.sqrt();
    let standard_error = standard_deviation / (sample.len() as f64).sqrt();
    finish_t_test(
        mean,
        hypothesized_mean,
        standard_error,
        (sample.len() - 1) as f64,
        (mean - hypothesized_mean) / standard_deviation,
        alternative,
        confidence_level,
    )
}

/// Paired t test over element-wise `left - right` differences.
pub fn paired_t_test(
    left: &[f64],
    right: &[f64],
    hypothesized_difference: f64,
    alternative: Alternative,
    confidence_level: f64,
) -> Result<TTestResult, StatisticsError> {
    if left.len() != right.len() {
        return Err(StatisticsError::LengthMismatch {
            left: left.len(),
            right: right.len(),
        });
    }
    validate_sample(left, "left sample", 2)?;
    validate_sample(right, "right sample", 2)?;
    let differences: Vec<f64> = left.iter().zip(right).map(|(&a, &b)| a - b).collect();
    one_sample_t_test_impl(
        &differences,
        "paired differences",
        hypothesized_difference,
        alternative,
        confidence_level,
    )
}

pub fn independent_t_test(
    left: &[f64],
    right: &[f64],
    hypothesized_difference: f64,
    variance_assumption: VarianceAssumption,
    alternative: Alternative,
    confidence_level: f64,
) -> Result<TTestResult, StatisticsError> {
    validate_common(
        left,
        "left sample",
        hypothesized_difference,
        confidence_level,
    )?;
    validate_sample(right, "right sample", 2)?;
    let left_mean = checked_mean(left);
    let right_mean = checked_mean(right);
    let left_variance = centered_sum_squares(left, left_mean) / (left.len() - 1) as f64;
    let right_variance = centered_sum_squares(right, right_mean) / (right.len() - 1) as f64;
    if left_variance <= 0.0 {
        return Err(StatisticsError::ZeroVariance {
            sample: "left sample".to_owned(),
        });
    }
    if right_variance <= 0.0 {
        return Err(StatisticsError::ZeroVariance {
            sample: "right sample".to_owned(),
        });
    }
    let n_left = left.len() as f64;
    let n_right = right.len() as f64;
    let pooled_variance = ((n_left - 1.0) * left_variance + (n_right - 1.0) * right_variance)
        / (n_left + n_right - 2.0);
    let effect_scale = pooled_variance.sqrt();
    let (standard_error, degrees_of_freedom) = match variance_assumption {
        VarianceAssumption::Equal => (
            (pooled_variance * (1.0 / n_left + 1.0 / n_right)).sqrt(),
            n_left + n_right - 2.0,
        ),
        VarianceAssumption::Unequal => {
            let left_term = left_variance / n_left;
            let right_term = right_variance / n_right;
            let se_squared = left_term + right_term;
            let df = se_squared.powi(2)
                / (left_term.powi(2) / (n_left - 1.0) + right_term.powi(2) / (n_right - 1.0));
            (se_squared.sqrt(), df)
        }
    };
    let estimate = left_mean - right_mean;
    finish_t_test(
        estimate,
        hypothesized_difference,
        standard_error,
        degrees_of_freedom,
        estimate / effect_scale,
        alternative,
        confidence_level,
    )
}

fn validate_common(
    sample: &[f64],
    name: &str,
    null_value: f64,
    confidence_level: f64,
) -> Result<(), StatisticsError> {
    validate_sample(sample, name, 2)?;
    if !null_value.is_finite() {
        return Err(StatisticsError::InvalidNullValue);
    }
    validate_confidence_level(confidence_level)
}

#[allow(clippy::too_many_arguments)]
fn finish_t_test(
    estimate: f64,
    null_value: f64,
    standard_error: f64,
    degrees_of_freedom: f64,
    cohens_d: f64,
    alternative: Alternative,
    confidence_level: f64,
) -> Result<TTestResult, StatisticsError> {
    let distribution = StudentsT::new(0.0, 1.0, degrees_of_freedom)
        .expect("validated sample sizes guarantee positive finite degrees of freedom");
    let statistic = (estimate - null_value) / standard_error;
    let p_value = match alternative {
        Alternative::TwoSided => 2.0 * distribution.sf(statistic.abs()),
        Alternative::Less => distribution.cdf(statistic),
        Alternative::Greater => distribution.sf(statistic),
    }
    .clamp(0.0, 1.0);
    let alpha = 1.0 - confidence_level;
    let (lower, upper) = match alternative {
        Alternative::TwoSided => {
            let critical = distribution.inverse_cdf(1.0 - alpha / 2.0);
            (
                estimate - critical * standard_error,
                estimate + critical * standard_error,
            )
        }
        Alternative::Less => {
            let critical = distribution.inverse_cdf(confidence_level);
            (f64::NEG_INFINITY, estimate + critical * standard_error)
        }
        Alternative::Greater => {
            let critical = distribution.inverse_cdf(confidence_level);
            (estimate - critical * standard_error, f64::INFINITY)
        }
    };
    Ok(TTestResult {
        statistic,
        degrees_of_freedom,
        p_value,
        estimate,
        null_value,
        standard_error,
        confidence_interval: ConfidenceInterval {
            level: confidence_level,
            lower,
            upper,
        },
        cohens_d,
        alternative,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_sample_matches_scipy_reference() {
        let result = one_sample_t_test(
            &[2.0, 4.0, 6.0, 8.0, 10.0],
            5.0,
            Alternative::TwoSided,
            0.95,
        )
        .unwrap();
        assert!((result.statistic - 0.707_106_781_186_547_5).abs() < 1e-14);
        assert!((result.p_value - 0.518_518_518_518_518_3).abs() < 1e-13);
        assert!(result.confidence_interval.lower < 5.0);
        assert!(result.confidence_interval.upper > 5.0);
    }

    #[test]
    fn welch_and_student_use_their_respective_degrees_of_freedom() {
        let left = [14.2, 15.3, 14.7, 16.1, 15.8];
        let right = [12.1, 11.9, 13.0, 12.5, 12.7, 11.8];
        let student = independent_t_test(
            &left,
            &right,
            0.0,
            VarianceAssumption::Equal,
            Alternative::TwoSided,
            0.95,
        )
        .unwrap();
        let welch = independent_t_test(
            &left,
            &right,
            0.0,
            VarianceAssumption::Unequal,
            Alternative::TwoSided,
            0.95,
        )
        .unwrap();
        assert_eq!(student.degrees_of_freedom, 9.0);
        assert!((welch.degrees_of_freedom - 6.382_383_409_022_324).abs() < 1e-12);
        assert!(student.p_value < 0.001 && welch.p_value < 0.001);
    }

    #[test]
    fn paired_test_uses_left_minus_right() {
        let result = paired_t_test(
            &[5.0, 7.0, 9.0, 10.0],
            &[4.0, 5.0, 8.0, 8.0],
            0.0,
            Alternative::Greater,
            0.95,
        )
        .unwrap();
        assert_eq!(result.estimate, 1.5);
        assert!(result.statistic > 0.0);
        assert_eq!(result.confidence_interval.upper, f64::INFINITY);
    }

    #[test]
    fn zero_confidence_level_is_rejected() {
        assert_eq!(
            one_sample_t_test(&[1.0, 2.0], 0.0, Alternative::Less, 0.0),
            Err(StatisticsError::InvalidConfidenceLevel)
        );
    }

    #[test]
    fn independent_effect_size_does_not_depend_on_null_difference() {
        let result = independent_t_test(
            &[10.0, 11.0, 12.0],
            &[1.0, 2.0, 3.0],
            20.0,
            VarianceAssumption::Equal,
            Alternative::TwoSided,
            0.95,
        )
        .unwrap();
        assert_eq!(result.estimate, 9.0);
        assert_eq!(result.cohens_d, 9.0);
        assert!(result.cohens_d.is_sign_positive());
    }

    #[test]
    fn paired_difference_errors_name_the_derived_sample() {
        assert_eq!(
            paired_t_test(
                &[1e308, 2.0],
                &[-1e308, 1.0],
                0.0,
                Alternative::TwoSided,
                0.95,
            ),
            Err(StatisticsError::NonFiniteValue {
                sample: "paired differences".to_owned(),
                index: 0,
            })
        );
        assert_eq!(
            paired_t_test(&[1.0, 2.0], &[1.0, 2.0], 0.0, Alternative::TwoSided, 0.95,),
            Err(StatisticsError::ZeroVariance {
                sample: "paired differences".to_owned(),
            })
        );
    }
}
