use statrs::distribution::{ContinuousCDF, FisherSnedecor};

use super::{
    StatisticsError, centered_sum_squares, checked_mean, checked_sum, degenerate_ratio,
    validate_sample,
};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AnovaRow {
    pub sum_squares: f64,
    pub degrees_of_freedom: usize,
    pub mean_square: f64,
    pub f_statistic: Option<f64>,
    pub p_value: Option<f64>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct OneWayAnova {
    pub factor: AnovaRow,
    pub residual: AnovaRow,
    pub total: AnovaRow,
    pub grand_mean: f64,
    pub group_means: Vec<f64>,
    pub group_sizes: Vec<usize>,
    pub eta_squared: f64,
    pub omega_squared: f64,
}

/// One-way fixed-effects ANOVA with a pooled within-group error term.
pub fn one_way_anova(groups: &[&[f64]]) -> Result<OneWayAnova, StatisticsError> {
    if groups.len() < 2 {
        return Err(StatisticsError::TooFewGroups {
            minimum: 2,
            actual: groups.len(),
        });
    }
    let mut group_means = Vec::with_capacity(groups.len());
    let mut group_sizes = Vec::with_capacity(groups.len());
    let mut group_sums = Vec::with_capacity(groups.len());
    let mut total_count = 0;
    for (index, group) in groups.iter().enumerate() {
        validate_sample(group, format!("group {index}"), 1)?;
        let sum = checked_sum(group);
        group_sums.push(sum);
        group_means.push(sum / group.len() as f64);
        group_sizes.push(group.len());
        total_count += group.len();
    }
    let residual_df = total_count.saturating_sub(groups.len());
    if residual_df == 0 {
        return Err(StatisticsError::NoResidualDegreesOfFreedom);
    }
    let grand_mean = checked_sum(&group_sums) / total_count as f64;
    let factor_ss = group_means
        .iter()
        .zip(&group_sizes)
        .map(|(&mean, &size)| size as f64 * (mean - grand_mean).powi(2))
        .sum::<f64>();
    let residual_ss = groups
        .iter()
        .zip(&group_means)
        .map(|(group, &mean)| centered_sum_squares(group, mean))
        .sum::<f64>();
    // This identity avoids allocating and copying a concatenated O(N) sample.
    let total_ss = factor_ss + residual_ss;
    if total_ss <= 0.0 {
        return Err(StatisticsError::ZeroVariance {
            sample: "all groups".to_owned(),
        });
    }
    let factor_df = groups.len() - 1;
    let factor = tested_row(
        factor_ss,
        factor_df,
        residual_ss / residual_df as f64,
        residual_df,
    );
    let residual = untested_row(residual_ss, residual_df);
    let total = untested_row(total_ss, total_count - 1);
    let eta_squared = factor_ss / total_ss;
    let omega_squared = ((factor_ss - factor_df as f64 * residual.mean_square)
        / (total_ss + residual.mean_square))
        .max(0.0);
    Ok(OneWayAnova {
        factor,
        residual,
        total,
        grand_mean,
        group_means,
        group_sizes,
        eta_squared,
        omega_squared,
    })
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FactorialObservation {
    pub factor_a: usize,
    pub factor_b: usize,
    pub value: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TwoWayDesign {
    /// Every cell has one observation. The residual term contains interaction,
    /// so a separate interaction test is impossible.
    WithoutReplication,
    /// At least one cell is replicated. Main-effect sums of squares are Type II
    /// and interaction is tested against within-cell error.
    WithReplication,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TwoWayAnova {
    pub factor_a: AnovaRow,
    pub factor_b: AnovaRow,
    pub interaction: Option<AnovaRow>,
    pub residual: AnovaRow,
    pub total: AnovaRow,
    pub design: TwoWayDesign,
    pub levels_a: usize,
    pub levels_b: usize,
    pub observations: usize,
}

/// Two-way fixed-effects ANOVA over a complete factorial grid.
///
/// Unbalanced replication uses hierarchical Type II sums of squares: each main
/// effect is added to a model containing the other main effect, and interaction
/// is added to the additive model. Empty cells are rejected because the full
/// interaction model would otherwise be non-estimable without an explicit
/// contrast convention.
pub fn two_way_anova(
    observations: &[FactorialObservation],
    levels_a: usize,
    levels_b: usize,
) -> Result<TwoWayAnova, StatisticsError> {
    if levels_a < 2 || levels_b < 2 {
        return Err(StatisticsError::TooFewFactorLevels);
    }
    validate_factorial_observations(observations, levels_a, levels_b)?;
    let cell_count = levels_a
        .checked_mul(levels_b)
        .ok_or(StatisticsError::SingularDesign)?;
    let mut cells = vec![Vec::new(); cell_count];
    for observation in observations {
        cells[observation.factor_a * levels_b + observation.factor_b].push(observation.value);
    }
    for factor_a in 0..levels_a {
        for factor_b in 0..levels_b {
            if cells[factor_a * levels_b + factor_b].is_empty() {
                return Err(StatisticsError::EmptyFactorialCell { factor_a, factor_b });
            }
        }
    }

    let values: Vec<f64> = observations
        .iter()
        .map(|observation| observation.value)
        .collect();
    let grand_mean = checked_mean(&values);
    let total_ss = centered_sum_squares(&values, grand_mean);
    if total_ss <= 0.0 {
        return Err(StatisticsError::ZeroVariance {
            sample: "factorial observations".to_owned(),
        });
    }
    let sse_a_only = grouped_sse(observations, levels_a, |item| item.factor_a);
    let sse_b_only = grouped_sse(observations, levels_b, |item| item.factor_b);
    let additive_sse = additive_model_sse(observations, levels_a, levels_b)?;
    let ss_a = (sse_b_only - additive_sse).max(0.0);
    let ss_b = (sse_a_only - additive_sse).max(0.0);

    let replicated = observations.len() > cell_count;
    let (design, interaction, residual_ss, residual_df) = if replicated {
        let within_ss = cells
            .iter()
            .map(|cell| centered_sum_squares(cell, checked_mean(cell)))
            .sum::<f64>();
        let within_df = observations.len() - cell_count;
        let error_ms = within_ss / within_df as f64;
        let interaction_df = (levels_a - 1) * (levels_b - 1);
        let interaction_ss = (additive_sse - within_ss).max(0.0);
        (
            TwoWayDesign::WithReplication,
            Some(tested_row(
                interaction_ss,
                interaction_df,
                error_ms,
                within_df,
            )),
            within_ss,
            within_df,
        )
    } else {
        let df = (levels_a - 1) * (levels_b - 1);
        (TwoWayDesign::WithoutReplication, None, additive_sse, df)
    };
    if residual_df == 0 {
        return Err(StatisticsError::NoResidualDegreesOfFreedom);
    }
    let error_ms = residual_ss / residual_df as f64;
    Ok(TwoWayAnova {
        factor_a: tested_row(ss_a, levels_a - 1, error_ms, residual_df),
        factor_b: tested_row(ss_b, levels_b - 1, error_ms, residual_df),
        interaction,
        residual: untested_row(residual_ss, residual_df),
        total: untested_row(total_ss, observations.len() - 1),
        design,
        levels_a,
        levels_b,
        observations: observations.len(),
    })
}

fn validate_factorial_observations(
    observations: &[FactorialObservation],
    levels_a: usize,
    levels_b: usize,
) -> Result<(), StatisticsError> {
    if observations.is_empty() {
        return Err(StatisticsError::EmptySample {
            sample: "factorial observations".to_owned(),
        });
    }
    for (index, observation) in observations.iter().enumerate() {
        if observation.factor_a >= levels_a || observation.factor_b >= levels_b {
            return Err(StatisticsError::FactorLevelOutOfRange {
                index,
                factor_a: observation.factor_a,
                factor_b: observation.factor_b,
                levels_a,
                levels_b,
            });
        }
        if !observation.value.is_finite() {
            return Err(StatisticsError::NonFiniteValue {
                sample: "factorial observations".to_owned(),
                index,
            });
        }
    }
    Ok(())
}

fn grouped_sse(
    observations: &[FactorialObservation],
    levels: usize,
    level: impl Fn(&FactorialObservation) -> usize,
) -> f64 {
    let mut sums = vec![0.0; levels];
    let mut counts = vec![0; levels];
    for observation in observations {
        let index = level(observation);
        sums[index] += observation.value;
        counts[index] += 1;
    }
    let means: Vec<f64> = sums
        .iter()
        .zip(&counts)
        .map(|(&sum, &count)| sum / count as f64)
        .collect();
    observations
        .iter()
        .map(|observation| (observation.value - means[level(observation)]).powi(2))
        .sum()
}

fn additive_model_sse(
    observations: &[FactorialObservation],
    levels_a: usize,
    levels_b: usize,
) -> Result<f64, StatisticsError> {
    let columns = 1 + levels_a - 1 + levels_b - 1;
    let mut normal = vec![vec![0.0; columns]; columns];
    let mut rhs = vec![0.0; columns];
    for observation in observations {
        let row = design_row(observation, levels_a, levels_b);
        for i in 0..columns {
            rhs[i] += row[i] * observation.value;
            for j in i..columns {
                normal[i][j] += row[i] * row[j];
            }
        }
    }
    crate::fit::mirror_upper(&mut normal);
    let coefficients =
        crate::fit::solve_linear(&normal, &rhs).ok_or(StatisticsError::SingularDesign)?;
    Ok(observations
        .iter()
        .map(|observation| {
            let fitted = design_row(observation, levels_a, levels_b)
                .iter()
                .zip(&coefficients)
                .map(|(x, beta)| x * beta)
                .sum::<f64>();
            (observation.value - fitted).powi(2)
        })
        .sum())
}

fn design_row(observation: &FactorialObservation, levels_a: usize, levels_b: usize) -> Vec<f64> {
    let mut row = vec![0.0; 1 + levels_a - 1 + levels_b - 1];
    row[0] = 1.0;
    if observation.factor_a > 0 {
        row[observation.factor_a] = 1.0;
    }
    if observation.factor_b > 0 {
        row[levels_a + observation.factor_b - 1] = 1.0;
    }
    row
}

fn tested_row(sum_squares: f64, df: usize, error_ms: f64, error_df: usize) -> AnovaRow {
    let mean_square = sum_squares / df as f64;
    let f_statistic = degenerate_ratio(mean_square, error_ms);
    let p_value = if f_statistic.is_infinite() {
        0.0
    } else {
        let distribution = FisherSnedecor::new(df as f64, error_df as f64)
            .expect("ANOVA rows always have positive degrees of freedom");
        distribution.sf(f_statistic).clamp(0.0, 1.0)
    };
    AnovaRow {
        sum_squares,
        degrees_of_freedom: df,
        mean_square,
        f_statistic: Some(f_statistic),
        p_value: Some(p_value),
    }
}

fn untested_row(sum_squares: f64, df: usize) -> AnovaRow {
    AnovaRow {
        sum_squares,
        degrees_of_freedom: df,
        mean_square: sum_squares / df as f64,
        f_statistic: None,
        p_value: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_way_matches_standard_reference() {
        let a = [6.0, 8.0, 4.0, 5.0, 3.0, 4.0];
        let b = [8.0, 12.0, 9.0, 11.0, 6.0, 8.0];
        let c = [13.0, 9.0, 11.0, 8.0, 12.0, 14.0];
        let result = one_way_anova(&[&a, &b, &c]).unwrap();
        assert!((result.factor.sum_squares - 117.444_444_444_444_41).abs() < 1e-12);
        assert!((result.residual.sum_squares - 66.833_333_333_333_34).abs() < 1e-12);
        assert!((result.factor.f_statistic.unwrap() - 13.179_551_122_194_509).abs() < 1e-12);
        assert!((result.factor.p_value.unwrap() - 0.000_497_051_248_100_737_7).abs() < 1e-12);
    }

    #[test]
    fn balanced_two_way_with_replication_separates_interaction() {
        let values = [[[8.0, 10.0], [12.0, 14.0]], [[18.0, 20.0], [26.0, 28.0]]];
        let observations: Vec<_> = values
            .iter()
            .enumerate()
            .flat_map(|(a, rows)| {
                rows.iter().enumerate().flat_map(move |(b, cell)| {
                    cell.iter().map(move |&value| FactorialObservation {
                        factor_a: a,
                        factor_b: b,
                        value,
                    })
                })
            })
            .collect();
        let result = two_way_anova(&observations, 2, 2).unwrap();
        assert_eq!(result.design, TwoWayDesign::WithReplication);
        assert!(result.factor_a.p_value.unwrap() < 0.001);
        assert!(result.factor_b.p_value.unwrap() < 0.01);
        assert_eq!(result.interaction.unwrap().degrees_of_freedom, 1);
        assert_eq!(result.residual.degrees_of_freedom, 4);
    }

    #[test]
    fn two_way_without_replication_uses_interaction_as_error() {
        let observations = [
            FactorialObservation {
                factor_a: 0,
                factor_b: 0,
                value: 8.0,
            },
            FactorialObservation {
                factor_a: 0,
                factor_b: 1,
                value: 12.0,
            },
            FactorialObservation {
                factor_a: 1,
                factor_b: 0,
                value: 18.0,
            },
            FactorialObservation {
                factor_a: 1,
                factor_b: 1,
                value: 26.0,
            },
        ];
        let result = two_way_anova(&observations, 2, 2).unwrap();
        assert_eq!(result.design, TwoWayDesign::WithoutReplication);
        assert_eq!(result.interaction, None);
        assert_eq!(result.residual.degrees_of_freedom, 1);
    }

    #[test]
    fn incomplete_factor_grid_is_rejected() {
        let observations = [
            FactorialObservation {
                factor_a: 0,
                factor_b: 0,
                value: 1.0,
            },
            FactorialObservation {
                factor_a: 1,
                factor_b: 1,
                value: 2.0,
            },
        ];
        assert_eq!(
            two_way_anova(&observations, 2, 2),
            Err(StatisticsError::EmptyFactorialCell {
                factor_a: 0,
                factor_b: 1
            })
        );
    }
}
