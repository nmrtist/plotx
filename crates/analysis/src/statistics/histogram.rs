use super::{StatisticsError, validate_sample};

/// How histogram bins are chosen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BinRule {
    /// Freedman–Diaconis width (robust to outliers), falling back to Sturges
    /// when the interquartile range is zero.
    Auto,
    /// A fixed bin count.
    Count(usize),
}

/// Equal-width histogram bins: `edges` has one more entry than `counts`, and
/// bin `i` covers `[edges[i], edges[i+1])` with the final bin closed on the
/// right so the sample maximum is counted.
#[derive(Clone, Debug, PartialEq)]
pub struct Histogram {
    pub edges: Vec<f64>,
    pub counts: Vec<usize>,
}

/// The number of bins any rule resolves to is clamped to this range; beyond a
/// few hundred bins a histogram stops reading as a distribution summary.
const MAX_BINS: usize = 512;

pub fn histogram(values: &[f64], rule: BinRule) -> Result<Histogram, StatisticsError> {
    validate_sample(values, "sample", 1)?;
    let mut ordered = values.to_vec();
    ordered.sort_by(f64::total_cmp);
    let (lo, hi) = (ordered[0], ordered[ordered.len() - 1]);

    if lo == hi {
        // A degenerate sample still deserves a drawable bar: one unit-wide bin.
        return Ok(Histogram {
            edges: vec![lo - 0.5, lo + 0.5],
            counts: vec![values.len()],
        });
    }

    let bins = match rule {
        BinRule::Count(count) => count.clamp(1, MAX_BINS),
        BinRule::Auto => auto_bin_count(&ordered, lo, hi),
    };
    let width = (hi - lo) / bins as f64;
    let mut counts = vec![0usize; bins];
    for &v in values {
        // The maximum lands exactly on the last edge; fold it into the last bin.
        let index = (((v - lo) / width) as usize).min(bins - 1);
        counts[index] += 1;
    }
    let edges = (0..=bins).map(|i| lo + i as f64 * width).collect();
    Ok(Histogram { edges, counts })
}

fn auto_bin_count(ordered: &[f64], lo: f64, hi: f64) -> usize {
    let n = ordered.len() as f64;
    let iqr = super::descriptive::quantile_r7(ordered, 0.75)
        - super::descriptive::quantile_r7(ordered, 0.25);
    let fd_width = 2.0 * iqr / n.cbrt();
    let bins = if fd_width > 0.0 {
        ((hi - lo) / fd_width).ceil()
    } else {
        // Sturges handles heavy-tie samples where the IQR collapses to zero.
        n.log2().ceil() + 1.0
    };
    if bins.is_finite() {
        (bins as usize).clamp(1, MAX_BINS)
    } else {
        1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_cover_every_observation_and_the_maximum() {
        let values: Vec<f64> = (0..100).map(|i| i as f64).collect();
        let h = histogram(&values, BinRule::Count(10)).unwrap();
        assert_eq!(h.counts.len(), 10);
        assert_eq!(h.edges.len(), 11);
        assert_eq!(h.counts.iter().sum::<usize>(), 100);
        // The max (99.0) sits on the final edge and must land in the last bin.
        assert_eq!(*h.counts.last().unwrap(), 10);
        assert_eq!(h.edges[0], 0.0);
        assert_eq!(*h.edges.last().unwrap(), 99.0);
    }

    #[test]
    fn auto_rule_uses_freedman_diaconis_width() {
        let values: Vec<f64> = (0..1000).map(|i| i as f64 / 10.0).collect();
        let h = histogram(&values, BinRule::Auto).unwrap();
        // IQR = 49.95, n^(1/3) = 10 → width ≈ 9.99 → 10 bins over a span of 99.9.
        assert_eq!(h.counts.len(), 10);
        assert_eq!(h.counts.iter().sum::<usize>(), 1000);
    }

    #[test]
    fn zero_iqr_falls_back_to_sturges() {
        // 90% ties: IQR = 0, but the span is still 0..10.
        let mut values = vec![5.0; 90];
        values.extend([0.0, 10.0, 5.0, 5.0, 5.0, 5.0, 5.0, 5.0, 5.0, 5.0]);
        let h = histogram(&values, BinRule::Auto).unwrap();
        assert_eq!(h.counts.len(), (100f64.log2().ceil() + 1.0) as usize);
        assert_eq!(h.counts.iter().sum::<usize>(), 100);
    }

    #[test]
    fn constant_sample_gets_one_unit_bin() {
        let h = histogram(&[3.0, 3.0, 3.0], BinRule::Auto).unwrap();
        assert_eq!(h.edges, vec![2.5, 3.5]);
        assert_eq!(h.counts, vec![3]);
    }

    #[test]
    fn rejects_empty_and_non_finite_samples() {
        assert!(histogram(&[], BinRule::Auto).is_err());
        assert!(histogram(&[1.0, f64::NAN], BinRule::Auto).is_err());
    }
}
