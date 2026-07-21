//! Domain-neutral peak detection over plain `&[f64]` traces: local maxima
//! ranked by topographic prominence, with height / prominence / spacing / count
//! filters. Free of any spectral or figure types so any 1D series can reuse it.

/// A local maximum that passed every filter.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DetectedPeak {
    /// Sample index of the apex in the input arrays.
    pub index: usize,
    /// `xs[index]`.
    pub x: f64,
    /// `ys[index]`.
    pub y: f64,
    /// Topographic prominence in y units.
    pub prominence: f64,
}

/// Filters applied to candidate maxima.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DetectParams {
    /// Apex `ys[i]` must be strictly greater than this; `None` imposes no floor.
    pub min_height: Option<f64>,
    /// Apex prominence must be at least this (y units); `0.0` disables it.
    pub min_prominence: f64,
    /// Merge maxima closer than this in `|x|`, keeping the taller; `None` is off.
    pub min_spacing: Option<f64>,
    /// Keep only the N most prominent; `None` is unlimited.
    pub max_count: Option<usize>,
}

/// Robust baseline-noise sigma estimate from the median absolute deviation of
/// the successive differences of `ys`, converted to a Gaussian-sigma equivalent.
/// Returns `0.0` for fewer than 2 samples.
pub fn estimate_noise(ys: &[f64]) -> f64 {
    if ys.len() < 2 {
        return 0.0;
    }
    let mut diffs: Vec<f64> = ys.windows(2).map(|w| w[1] - w[0]).collect();
    let med = median(&mut diffs);
    let mut dev: Vec<f64> = diffs.iter().map(|d| (d - med).abs()).collect();
    let mad = median(&mut dev);
    mad / (std::f64::consts::SQRT_2 * 0.6745)
}

/// Detect local maxima passing every filter, most-prominent first, capped at
/// `max_count`. `xs` must be monotonic (either direction) and the same length as
/// `ys`. Returns empty for `len < 3`.
pub fn detect_peaks(xs: &[f64], ys: &[f64], params: &DetectParams) -> Vec<DetectedPeak> {
    let n = ys.len();
    if n < 3 || xs.len() != n {
        return Vec::new();
    }

    let mut maxima: Vec<usize> = Vec::new();
    for i in 1..n - 1 {
        if ys[i] >= ys[i - 1] && ys[i] > ys[i + 1] {
            maxima.push(i);
        }
    }
    if maxima.is_empty() {
        return Vec::new();
    }

    let ngl = nearest_greater_left(ys);
    let ngr = nearest_greater_right(ys);
    let table = SparseMin::build(ys);
    let global_min = ys.iter().copied().fold(f64::INFINITY, f64::min);

    let prominence = |i: usize| -> f64 {
        let lo = ngl[i].map(|j| j + 1).unwrap_or(0);
        let hi = ngr[i].map(|j| j - 1).unwrap_or(n - 1);
        if ngl[i].is_none() && ngr[i].is_none() {
            return ys[i] - global_min;
        }
        let left_col = table.range_min(lo, i);
        let right_col = table.range_min(i, hi);
        ys[i] - left_col.max(right_col)
    };

    let mut peaks: Vec<DetectedPeak> = maxima
        .iter()
        .map(|&i| DetectedPeak {
            index: i,
            x: xs[i],
            y: ys[i],
            prominence: prominence(i),
        })
        .filter(|p| {
            params.min_height.is_none_or(|h| p.y > h) && p.prominence >= params.min_prominence
        })
        .collect();

    if let Some(spacing) = params.min_spacing {
        peaks.sort_by(|a, b| b.y.total_cmp(&a.y));
        let mut kept: Vec<DetectedPeak> = Vec::new();
        for p in peaks {
            if kept.iter().all(|k| (k.x - p.x).abs() >= spacing) {
                kept.push(p);
            }
        }
        peaks = kept;
    }

    peaks.sort_by(|a, b| b.prominence.total_cmp(&a.prominence));
    if let Some(max) = params.max_count {
        peaks.truncate(max);
    }
    peaks
}

fn median(v: &mut [f64]) -> f64 {
    if v.is_empty() {
        return 0.0;
    }
    v.sort_by(f64::total_cmp);
    let m = v.len() / 2;
    if v.len() % 2 == 1 {
        v[m]
    } else {
        (v[m - 1] + v[m]) / 2.0
    }
}

fn nearest_greater_left(ys: &[f64]) -> Vec<Option<usize>> {
    let mut res = vec![None; ys.len()];
    let mut stack: Vec<usize> = Vec::new();
    for i in 0..ys.len() {
        while stack.last().is_some_and(|&t| ys[t] <= ys[i]) {
            stack.pop();
        }
        res[i] = stack.last().copied();
        stack.push(i);
    }
    res
}

fn nearest_greater_right(ys: &[f64]) -> Vec<Option<usize>> {
    let mut res = vec![None; ys.len()];
    let mut stack: Vec<usize> = Vec::new();
    for i in (0..ys.len()).rev() {
        while stack.last().is_some_and(|&t| ys[t] <= ys[i]) {
            stack.pop();
        }
        res[i] = stack.last().copied();
        stack.push(i);
    }
    res
}

/// Sparse table for O(1) inclusive range-minimum queries after O(n log n) build.
struct SparseMin {
    rows: Vec<Vec<f64>>,
}

impl SparseMin {
    fn build(ys: &[f64]) -> Self {
        let n = ys.len();
        let mut rows = vec![ys.to_vec()];
        let mut k = 1;
        while (1usize << k) <= n {
            let prev = &rows[k - 1];
            let half = 1usize << (k - 1);
            let span = 1usize << k;
            let row: Vec<f64> = (0..=n - span)
                .map(|i| prev[i].min(prev[i + half]))
                .collect();
            rows.push(row);
            k += 1;
        }
        SparseMin { rows }
    }

    fn range_min(&self, l: usize, r: usize) -> f64 {
        let len = r - l + 1;
        let k = (usize::BITS - 1 - len.leading_zeros()) as usize;
        let span = 1usize << k;
        self.rows[k][l].min(self.rows[k][r + 1 - span])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NO_FILTERS: DetectParams = DetectParams {
        min_height: None,
        min_prominence: 0.0,
        min_spacing: None,
        max_count: None,
    };

    fn ramp(n: usize) -> Vec<f64> {
        (0..n).map(|i| i as f64).collect()
    }

    #[test]
    fn single_peak_yields_one_apex() {
        let ys = vec![0.0, 1.0, 4.0, 9.0, 16.0, 9.0, 4.0, 1.0, 0.0];
        let xs = ramp(ys.len());
        let peaks = detect_peaks(&xs, &ys, &NO_FILTERS);
        assert_eq!(peaks.len(), 1);
        assert_eq!(peaks[0].index, 4);
        assert_eq!(peaks[0].x, 4.0);
        assert_eq!(peaks[0].y, 16.0);
        assert_eq!(peaks[0].prominence, 16.0);
    }

    #[test]
    fn two_separated_peaks_sorted_by_prominence() {
        let ys = vec![0.0, 50.0, 0.0, 0.0, 80.0, 0.0];
        let xs = ramp(ys.len());
        let peaks = detect_peaks(&xs, &ys, &NO_FILTERS);
        assert_eq!(peaks.len(), 2);
        assert_eq!(peaks[0].index, 4);
        assert_eq!(peaks[0].prominence, 80.0);
        assert_eq!(peaks[1].index, 1);
        assert_eq!(peaks[1].prominence, 50.0);
    }

    #[test]
    fn shoulder_suppressed_by_min_prominence() {
        let ys = vec![0.0, 10.0, 40.0, 100.0, 60.0, 65.0, 30.0, 5.0, 0.0];
        let xs = ramp(ys.len());

        let both = detect_peaks(&xs, &ys, &NO_FILTERS);
        assert_eq!(both.len(), 2);

        let params = DetectParams {
            min_prominence: 10.0,
            ..NO_FILTERS
        };
        let main_only = detect_peaks(&xs, &ys, &params);
        assert_eq!(main_only.len(), 1);
        assert_eq!(main_only[0].index, 3);
    }

    #[test]
    fn noise_only_rejected_by_height_floor() {
        let ys = vec![
            0.0, 0.2, -0.1, 0.15, -0.2, 0.1, -0.15, 0.2, -0.1, 0.05, -0.05, 0.1, -0.1, 0.0,
        ];
        let xs = ramp(ys.len());
        let sigma = estimate_noise(&ys);
        assert!(sigma > 0.0);
        let params = DetectParams {
            min_height: Some(4.0 * sigma),
            ..NO_FILTERS
        };
        let peaks = detect_peaks(&xs, &ys, &params);
        assert!(peaks.is_empty());
    }

    #[test]
    fn min_spacing_keeps_the_taller() {
        let ys = vec![0.0, 50.0, 10.0, 60.0, 0.0];
        let xs = ramp(ys.len());
        let params = DetectParams {
            min_spacing: Some(3.0),
            ..NO_FILTERS
        };
        let peaks = detect_peaks(&xs, &ys, &params);
        assert_eq!(peaks.len(), 1);
        assert_eq!(peaks[0].index, 3);
        assert_eq!(peaks[0].y, 60.0);
    }

    #[test]
    fn max_count_caps_to_most_prominent() {
        let ys = vec![0.0, 30.0, 0.0, 50.0, 0.0, 70.0, 0.0];
        let xs = ramp(ys.len());
        let params = DetectParams {
            max_count: Some(2),
            ..NO_FILTERS
        };
        let peaks = detect_peaks(&xs, &ys, &params);
        assert_eq!(peaks.len(), 2);
        assert_eq!(peaks[0].index, 5);
        assert_eq!(peaks[1].index, 3);
    }

    #[test]
    fn descending_x_axis_handled() {
        let xs = vec![5.0, 4.0, 3.0, 2.0, 1.0, 0.0];
        let ys = vec![0.0, 50.0, 0.0, 0.0, 80.0, 0.0];
        let peaks = detect_peaks(&xs, &ys, &NO_FILTERS);
        assert_eq!(peaks.len(), 2);
        assert_eq!(peaks[0].index, 4);
        assert_eq!(peaks[0].x, 1.0);
        assert_eq!(peaks[1].index, 1);
        assert_eq!(peaks[1].x, 4.0);
    }

    #[test]
    fn degenerate_inputs_are_empty() {
        assert!(detect_peaks(&[], &[], &NO_FILTERS).is_empty());
        assert!(detect_peaks(&[0.0, 1.0], &[0.0, 1.0], &NO_FILTERS).is_empty());
        let flat = vec![5.0; 8];
        assert!(detect_peaks(&ramp(8), &flat, &NO_FILTERS).is_empty());
        assert_eq!(estimate_noise(&[]), 0.0);
        assert_eq!(estimate_noise(&[1.0]), 0.0);
    }

    #[test]
    fn flat_plateau_emits_one_peak() {
        let ys = vec![0.0, 2.0, 2.0, 2.0, 0.0];
        let xs = ramp(ys.len());
        let peaks = detect_peaks(&xs, &ys, &NO_FILTERS);
        assert_eq!(peaks.len(), 1);
        assert_eq!(peaks[0].index, 3);
        assert_eq!(peaks[0].prominence, 2.0);
    }
}
