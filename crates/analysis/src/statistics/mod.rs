//! Classical descriptive and inferential statistics for finite `f64` samples.
//!
//! The API keeps assumptions explicit: independent t tests select their
//! variance model, every hypothesis test selects an [`Alternative`], and
//! two-way ANOVA reports whether interaction is separately estimable. Missing
//! and non-finite observations are rejected rather than silently dropped.
//!
//! # Example
//!
//! ```
//! use plotx_analysis::statistics::{
//!     Alternative, VarianceAssumption, describe, independent_t_test,
//!     one_way_anova, shapiro_wilk, tukey_hsd,
//! };
//!
//! let control = [4.2, 4.5, 4.1, 4.4];
//! let treated = [5.1, 5.4, 5.0, 5.3];
//! let summary = describe(&treated)?;
//! let normality = shapiro_wilk(&treated)?;
//! let difference = independent_t_test(
//!     &treated,
//!     &control,
//!     0.0,
//!     VarianceAssumption::Unequal,
//!     Alternative::TwoSided,
//!     0.95,
//! )?;
//! let groups: [&[f64]; 2] = [&control, &treated];
//! let omnibus = one_way_anova(&groups)?;
//! let post_hoc = tukey_hsd(&groups, 0.95)?;
//!
//! assert_eq!(summary.count, 4);
//! assert!(normality.p_value > 0.05);
//! assert!(difference.estimate > 0.0);
//! assert!(omnibus.factor.p_value.is_some());
//! assert_eq!(post_hoc.comparisons.len(), 1);
//! # Ok::<(), plotx_analysis::statistics::StatisticsError>(())
//! ```

mod anova;
mod correlation;
mod descriptive;
mod histogram;
mod kde;
mod normality;
mod post_hoc;
mod t_test;

pub use anova::{
    AnovaRow, FactorialObservation, OneWayAnova, TwoWayAnova, TwoWayDesign, one_way_anova,
    two_way_anova,
};
pub use correlation::{CorrelationMethod, CorrelationResult, pearson, spearman};
pub use descriptive::{DescriptiveStatistics, describe};
pub use histogram::{BinRule, Histogram, histogram};
pub use kde::{KdeCurve, gaussian_kde};
pub use normality::{NormalityResult, shapiro_wilk};
pub use post_hoc::{TukeyComparison, TukeyHsd, tukey_hsd};
pub use t_test::{
    Alternative, ConfidenceInterval, TTestResult, VarianceAssumption, independent_t_test,
    one_sample_t_test, paired_t_test,
};

#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum StatisticsError {
    #[error("{sample} is empty")]
    EmptySample { sample: String },
    #[error("{sample} needs at least {minimum} observations, but has {actual}")]
    InsufficientObservations {
        sample: String,
        minimum: usize,
        actual: usize,
    },
    #[error("{sample} supports at most {maximum} observations, but has {actual}")]
    TooManyObservations {
        sample: String,
        maximum: usize,
        actual: usize,
    },
    #[error("{sample} contains a non-finite value at index {index}")]
    NonFiniteValue { sample: String, index: usize },
    #[error("sample lengths differ: left has {left}, right has {right}")]
    LengthMismatch { left: usize, right: usize },
    #[error("{sample} has zero variance")]
    ZeroVariance { sample: String },
    #[error("confidence level must be finite and strictly between 0 and 1")]
    InvalidConfidenceLevel,
    #[error("the hypothesized value must be finite")]
    InvalidNullValue,
    #[error("at least {minimum} groups are required, but {actual} were provided")]
    TooFewGroups { minimum: usize, actual: usize },
    #[error("factor level counts must both be at least two")]
    TooFewFactorLevels,
    #[error(
        "observation {index} refers to factor level ({factor_a}, {factor_b}) outside ({levels_a}, {levels_b})"
    )]
    FactorLevelOutOfRange {
        index: usize,
        factor_a: usize,
        factor_b: usize,
        levels_a: usize,
        levels_b: usize,
    },
    #[error("factorial cell ({factor_a}, {factor_b}) has no observations")]
    EmptyFactorialCell { factor_a: usize, factor_b: usize },
    #[error("the factorial design has no residual degrees of freedom")]
    NoResidualDegreesOfFreedom,
    #[error("the factorial design matrix is singular")]
    SingularDesign,
    #[error("Tukey HSD needs at least {minimum} residual degrees of freedom, but has {actual}")]
    InsufficientResidualDegreesOfFreedom { minimum: usize, actual: usize },
    #[error("the studentized-range quantile did not converge")]
    QuantileDidNotConverge,
    #[error("Tukey HSD confidence level {actual} exceeds the numerical limit {maximum}")]
    UnsupportedTukeyConfidence { maximum: f64, actual: f64 },
}

pub(crate) fn validate_sample(
    values: &[f64],
    name: impl Into<String>,
    minimum: usize,
) -> Result<(), StatisticsError> {
    let name = name.into();
    if values.len() < minimum {
        return Err(if values.is_empty() {
            StatisticsError::EmptySample { sample: name }
        } else {
            StatisticsError::InsufficientObservations {
                sample: name,
                minimum,
                actual: values.len(),
            }
        });
    }
    if let Some((index, _)) = values
        .iter()
        .enumerate()
        .find(|(_, value)| !value.is_finite())
    {
        return Err(StatisticsError::NonFiniteValue {
            sample: name,
            index,
        });
    }
    Ok(())
}

pub(crate) fn checked_mean(values: &[f64]) -> f64 {
    checked_sum(values) / values.len() as f64
}

pub(crate) fn checked_sum(values: &[f64]) -> f64 {
    // Neumaier summation preserves small contributions when samples have a
    // large offset, which is common for instrument timestamps and intensities.
    let mut sum = 0.0;
    let mut correction = 0.0;
    for &value in values {
        let next = sum + value;
        if sum.abs() >= value.abs() {
            correction += (sum - next) + value;
        } else {
            correction += (value - next) + sum;
        }
        sum = next;
    }
    sum + correction
}

pub(crate) fn centered_sum_squares(values: &[f64], mean: f64) -> f64 {
    values.iter().map(|value| (value - mean).powi(2)).sum()
}

pub(crate) fn validate_confidence_level(level: f64) -> Result<(), StatisticsError> {
    if level.is_finite() && level > 0.0 && level < 1.0 {
        Ok(())
    } else {
        Err(StatisticsError::InvalidConfidenceLevel)
    }
}

/// Divide a statistic numerator by a non-negative scale while preserving the
/// conventional limiting values for an exact zero scale.
pub(crate) fn degenerate_ratio(numerator: f64, denominator: f64) -> f64 {
    debug_assert!(denominator >= 0.0);
    if denominator == 0.0 {
        if numerator == 0.0 {
            0.0
        } else {
            numerator.signum() * f64::INFINITY
        }
    } else {
        numerator / denominator
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_entry_points_reject_invalid_data_explicitly() {
        assert_eq!(
            describe(&[1.0, f64::NAN]),
            Err(StatisticsError::NonFiniteValue {
                sample: "sample".to_owned(),
                index: 1,
            })
        );
        assert_eq!(
            pearson(&[1.0, 2.0, 3.0], &[1.0, 2.0]),
            Err(StatisticsError::LengthMismatch { left: 3, right: 2 })
        );
        assert_eq!(
            one_sample_t_test(&[1.0, 2.0], 0.0, Alternative::TwoSided, 1.0),
            Err(StatisticsError::InvalidConfidenceLevel)
        );
        assert_eq!(
            one_way_anova(&[&[1.0, 2.0], &[]]),
            Err(StatisticsError::EmptySample {
                sample: "group 1".to_owned(),
            })
        );
        assert!(matches!(
            shapiro_wilk(&vec![1.0; 5001]),
            Err(StatisticsError::TooManyObservations { maximum: 5000, .. })
        ));
    }
}
