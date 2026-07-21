//! JEOL Delta digital-filter (oversampling FIR decimation) group delay.

use super::Params;

// Group delay in final sample points, present on the direct axis whenever
// `DIGITAL_FILTER = TRUE`. The filter is a cascade of symmetric FIR stages:
// `orders` is the stage count followed by each stage's tap count (`"2 41 74"` →
// two stages of 41 and 74 taps) and `factors` the matching per-stage decimation
// (`"6  2"`). A symmetric FIR of `M` taps delays by `(M-1)/2` samples at its own
// input rate; referred to the fully-decimated output rate that is scaled by the
// decimation accumulated before the stage, so the total is
// `Σ (M_k-1)/2 · D_{k-1} / D_total`. Returns 0.0 when the filter is off or the
// parameters are missing/unparsable, leaving the FID untouched.
pub(super) fn group_delay(params: &Params) -> f64 {
    let enabled = params
        .string_ci("DIGITAL_FILTER")
        .map(|s| s.trim().eq_ignore_ascii_case("true"))
        .unwrap_or(false);
    if !enabled {
        return 0.0;
    }
    let ints = |name: &str| -> Vec<f64> {
        params
            .string_ci(name)
            .map(|s| {
                s.split_whitespace()
                    .filter_map(|t| t.parse().ok())
                    .collect()
            })
            .unwrap_or_default()
    };
    let orders = ints("orders");
    let factors = ints("factors");
    let taps = orders.split_first().map(|(_, rest)| rest).unwrap_or(&[]);
    let stages = taps.len().min(factors.len());
    if stages == 0 {
        return 0.0;
    }
    let total_decim: f64 = factors[..stages].iter().product();
    if total_decim <= 0.0 {
        return 0.0;
    }
    let mut delay = 0.0;
    let mut cumulative = 1.0; // decimation accumulated before the current stage
    for k in 0..stages {
        delay += (taps[k] - 1.0) / 2.0 * cumulative;
        cumulative *= factors[k];
    }
    let g = delay / total_decim;
    if g.is_finite() && g >= 0.0 { g } else { 0.0 }
}
