use super::{StatisticsError, centered_sum_squares, checked_mean, validate_sample};

/// A compact summary of a finite sample.
///
/// Variance uses Bessel's correction. Quartiles use the R-7/NumPy default
/// linear interpolation rule. Skewness and excess kurtosis are the bias-
/// corrected Fisher estimators and are unavailable until their required sample
/// sizes (three and four, respectively) are reached.
#[derive(Clone, Debug, PartialEq)]
pub struct DescriptiveStatistics {
    pub count: usize,
    pub mean: f64,
    pub median: f64,
    pub variance: Option<f64>,
    pub standard_deviation: Option<f64>,
    pub standard_error: Option<f64>,
    pub minimum: f64,
    pub first_quartile: f64,
    pub third_quartile: f64,
    pub maximum: f64,
    pub interquartile_range: f64,
    pub skewness: Option<f64>,
    pub excess_kurtosis: Option<f64>,
}

pub fn describe(values: &[f64]) -> Result<DescriptiveStatistics, StatisticsError> {
    validate_sample(values, "sample", 1)?;
    let count = values.len();
    let mean = checked_mean(values);
    let mut ordered = values.to_vec();
    ordered.sort_by(f64::total_cmp);

    let minimum = ordered[0];
    let maximum = ordered[count - 1];
    let first_quartile = quantile_r7(&ordered, 0.25);
    let median = quantile_r7(&ordered, 0.5);
    let third_quartile = quantile_r7(&ordered, 0.75);
    let sum_squares = centered_sum_squares(values, mean);

    let variance = (count >= 2).then(|| sum_squares / (count - 1) as f64);
    let standard_deviation = variance.map(f64::sqrt);
    let standard_error = standard_deviation.map(|value| value / (count as f64).sqrt());
    let (skewness, excess_kurtosis) = shape_statistics(values, mean, sum_squares);

    Ok(DescriptiveStatistics {
        count,
        mean,
        median,
        variance,
        standard_deviation,
        standard_error,
        minimum,
        first_quartile,
        third_quartile,
        maximum,
        interquartile_range: third_quartile - first_quartile,
        skewness,
        excess_kurtosis,
    })
}

/// R-7 (NumPy default) linear-interpolation quantile over a pre-sorted sample.
/// Shared with the histogram (Freedman–Diaconis) and KDE (Silverman) rules.
pub(super) fn quantile_r7(ordered: &[f64], probability: f64) -> f64 {
    if ordered.len() == 1 {
        return ordered[0];
    }
    let position = probability * (ordered.len() - 1) as f64;
    let lower = position.floor() as usize;
    let fraction = position - lower as f64;
    if fraction == 0.0 {
        ordered[lower]
    } else {
        ordered[lower] + fraction * (ordered[lower + 1] - ordered[lower])
    }
}

fn shape_statistics(values: &[f64], mean: f64, sum_squares: f64) -> (Option<f64>, Option<f64>) {
    if sum_squares <= 0.0 {
        return (None, None);
    }
    let n = values.len() as f64;
    let sum_cubes = values
        .iter()
        .map(|value| (value - mean).powi(3))
        .sum::<f64>();
    let skewness = (values.len() >= 3).then(|| {
        let g1 = n.sqrt() * sum_cubes / sum_squares.powf(1.5);
        (n * (n - 1.0)).sqrt() / (n - 2.0) * g1
    });
    let excess_kurtosis = (values.len() >= 4).then(|| {
        let sum_fourths = values
            .iter()
            .map(|value| (value - mean).powi(4))
            .sum::<f64>();
        let g2 = n * sum_fourths / sum_squares.powi(2) - 3.0;
        (n - 1.0) / ((n - 2.0) * (n - 3.0)) * ((n + 1.0) * g2 + 6.0)
    });
    (skewness, excess_kurtosis)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn describes_sample_with_documented_estimators() {
        let result = describe(&[1.0, 2.0, 3.0, 4.0, 5.0]).unwrap();
        assert_eq!(result.count, 5);
        assert_eq!(result.mean, 3.0);
        assert_eq!(result.median, 3.0);
        assert_eq!(result.variance, Some(2.5));
        assert_eq!(result.first_quartile, 2.0);
        assert_eq!(result.third_quartile, 4.0);
        assert_eq!(result.skewness, Some(0.0));
        assert!((result.excess_kurtosis.unwrap() + 1.2).abs() < 1e-12);
    }

    #[test]
    fn singleton_has_location_but_not_sample_moments() {
        let result = describe(&[7.0]).unwrap();
        assert_eq!(result.median, 7.0);
        assert_eq!(result.variance, None);
        assert_eq!(result.skewness, None);
        assert_eq!(result.excess_kurtosis, None);
    }
}
