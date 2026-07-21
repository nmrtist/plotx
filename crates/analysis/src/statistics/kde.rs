use super::{StatisticsError, centered_sum_squares, checked_mean, validate_sample};

/// A Gaussian kernel density estimate evaluated on an even grid.
#[derive(Clone, Debug, PartialEq)]
pub struct KdeCurve {
    pub xs: Vec<f64>,
    pub densities: Vec<f64>,
    pub bandwidth: f64,
}

/// Gaussian KDE with Silverman's rule-of-thumb bandwidth,
/// `h = 0.9 · min(σ, IQR/1.34) · n^(-1/5)`, evaluated on `grid_points` evenly
/// spaced positions spanning the data ± 3 bandwidths (where the Gaussian tail
/// has decayed to ~1%). Needs at least two distinct observations.
pub fn gaussian_kde(values: &[f64], grid_points: usize) -> Result<KdeCurve, StatisticsError> {
    validate_sample(values, "sample", 2)?;
    let n = values.len() as f64;
    let mean = checked_mean(values);
    let sd = (centered_sum_squares(values, mean) / (n - 1.0)).sqrt();

    let mut ordered = values.to_vec();
    ordered.sort_by(f64::total_cmp);
    let iqr = super::descriptive::quantile_r7(&ordered, 0.75)
        - super::descriptive::quantile_r7(&ordered, 0.25);

    // Silverman takes the smaller of the two spread estimates, but either may
    // legitimately be zero (heavy ties); only a fully degenerate sample fails.
    let spread = match (sd > 0.0, iqr > 0.0) {
        (true, true) => sd.min(iqr / 1.34),
        (true, false) => sd,
        (false, true) => iqr / 1.34,
        (false, false) => {
            return Err(StatisticsError::ZeroVariance {
                sample: "sample".to_owned(),
            });
        }
    };
    let bandwidth = 0.9 * spread * n.powf(-0.2);

    let grid_points = grid_points.clamp(2, 2048);
    let lo = ordered[0] - 3.0 * bandwidth;
    let hi = ordered[ordered.len() - 1] + 3.0 * bandwidth;
    let step = (hi - lo) / (grid_points - 1) as f64;
    let norm = 1.0 / (n * bandwidth * (2.0 * std::f64::consts::PI).sqrt());

    let mut xs = Vec::with_capacity(grid_points);
    let mut densities = Vec::with_capacity(grid_points);
    for i in 0..grid_points {
        let x = lo + i as f64 * step;
        let sum: f64 = values
            .iter()
            .map(|&v| {
                let z = (x - v) / bandwidth;
                (-0.5 * z * z).exp()
            })
            .sum();
        xs.push(x);
        densities.push(norm * sum);
    }
    Ok(KdeCurve {
        xs,
        densities,
        bandwidth,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn trapezoid_integral(curve: &KdeCurve) -> f64 {
        curve
            .xs
            .windows(2)
            .zip(curve.densities.windows(2))
            .map(|(x, d)| (x[1] - x[0]) * 0.5 * (d[0] + d[1]))
            .sum()
    }

    #[test]
    fn density_integrates_to_one_and_peaks_at_the_center() {
        let values: Vec<f64> = (0..200).map(|i| (i as f64 / 199.0 - 0.5) * 2.0).collect();
        let curve = gaussian_kde(&values, 400).unwrap();
        assert!(curve.bandwidth > 0.0);
        let integral = trapezoid_integral(&curve);
        assert!((integral - 1.0).abs() < 0.02, "integral = {integral}");
        let peak_x = curve.xs[curve
            .densities
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.total_cmp(b.1))
            .unwrap()
            .0];
        assert!(peak_x.abs() < 0.2, "peak at {peak_x}");
    }

    #[test]
    fn silverman_bandwidth_matches_the_formula_for_a_simple_sample() {
        let values = [1.0, 2.0, 3.0, 4.0, 5.0];
        let curve = gaussian_kde(&values, 50).unwrap();
        // σ = sqrt(2.5) ≈ 1.5811, IQR/1.34 = 2/1.34 ≈ 1.4925 (smaller),
        // h = 0.9 · 1.4925 · 5^(-0.2) ≈ 0.9729.
        assert!(
            (curve.bandwidth - 0.9729).abs() < 1e-3,
            "{}",
            curve.bandwidth
        );
    }

    #[test]
    fn degenerate_and_invalid_samples_are_rejected() {
        assert!(matches!(
            gaussian_kde(&[2.0, 2.0, 2.0], 100),
            Err(StatisticsError::ZeroVariance { .. })
        ));
        assert!(gaussian_kde(&[1.0], 100).is_err());
        assert!(gaussian_kde(&[1.0, f64::NAN], 100).is_err());
    }
}
