//! Domain-neutral peak model: a labelled `(x, y)` apex on any 1D trace, not an
//! NMR-specific annotation.

use plotx_analysis::peaks::{DetectParams, detect_peaks, estimate_noise};
use serde::{Deserialize, Serialize};

/// A reduced 1D trace any data domain can expose for peak work: the x samples, the
/// scalar y the peaks sit on, and whether the domain draws x high-to-low.
#[derive(Debug, Clone)]
pub struct Trace1d {
    pub xs: Vec<f64>,
    pub ys: Vec<f64>,
    pub x_reversed: bool,
}

impl Trace1d {
    /// A small x distance (a fraction of the span) used to treat two peaks as the
    /// same location — for suppression and de-duplicating picks.
    pub fn tol(&self) -> f64 {
        x_tolerance(self)
    }

    /// Snap `x` to the tallest local maximum within a small window, so a click near
    /// a peak lands on its apex. Falls back to the nearest sample.
    pub fn snap(&self, x: f64) -> (f64, f64) {
        let n = self.xs.len();
        let window = x_tolerance(self) * 15.0;
        let mut best: Option<(f64, f64)> = None;
        for i in 1..n.saturating_sub(1) {
            let px = self.xs[i];
            if (px - x).abs() > window {
                continue;
            }
            let v = self.ys[i];
            if v >= self.ys[i - 1] && v > self.ys[i + 1] && best.is_none_or(|(_, bv)| v > bv) {
                best = Some((px, v));
            }
        }
        best.unwrap_or_else(|| {
            self.xs
                .iter()
                .zip(&self.ys)
                .min_by(|a, b| (a.0 - x).abs().partial_cmp(&(b.0 - x).abs()).unwrap())
                .map(|(&px, &py)| (px, py))
                .unwrap_or((x, 0.0))
        })
    }
}

/// Provenance only — both kinds are ordinary, individually editable marks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum PeakOrigin {
    Detected,
    #[default]
    Manual,
}

/// One peak: a labelled `(x, y)` apex. `label` overrides the shift-formatted
/// default when set.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PeakMark {
    pub id: u64,
    pub x: f64,
    pub y: f64,
    #[serde(default)]
    pub origin: PeakOrigin,
    #[serde(default)]
    pub label: Option<String>,
}

/// The detection recipe remembered between runs: the last threshold the on-plot
/// line committed (`None` = the noise-relative auto floor) and an optional cap.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct DetectorConfig {
    pub threshold: Option<f64>,
    pub max_count: Option<usize>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PeakSet {
    /// Stable source column for table peaks. Non-table 1D domains leave this
    /// unset until they expose typed column identities.
    #[serde(default)]
    pub column: Option<plotx_data::ColumnId>,
    pub detector: DetectorConfig,
    pub marks: Vec<PeakMark>,
    #[serde(default)]
    pub next_id: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedPeak {
    pub x: f64,
    pub y: f64,
    pub label: String,
    pub origin: PeakOrigin,
    pub mark_id: Option<u64>,
}

/// Multiple of the noise sigma used for the automatic threshold and as the
/// prominence floor that rejects shoulders and baseline ripple. Conservative so
/// auto-detection starts clean; the on-plot line relaxes it when peaks are wanted.
const NOISE_SIGMA_MULT: f64 = 5.0;

/// Prominence floor for range-scoped picking, in sigma of the window's own noise.
const RANGE_SIGMA_MULT: f64 = 3.0;

pub fn default_label(x: f64) -> String {
    format!("{x:.2}")
}

impl PeakSet {
    pub fn next_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    /// The automatic y floor for a trace: its baseline (median) lifted by a few
    /// noise sigma. What the on-plot threshold line snaps to when set to Auto.
    pub fn auto_threshold(trace: &Trace1d) -> f64 {
        let sigma = estimate_noise(&trace.ys);
        baseline(&trace.ys) + NOISE_SIGMA_MULT * sigma
    }

    /// Peaks detected on the whole trace at `threshold` (`None` = auto), as apex
    /// `(x, y)` pairs. The transient set the on-plot line previews while dragging.
    pub fn detect_at(
        trace: &Trace1d,
        threshold: Option<f64>,
        max_count: Option<usize>,
    ) -> Vec<(f64, f64)> {
        if trace.xs.len() < 3 {
            return Vec::new();
        }
        let sigma = estimate_noise(&trace.ys);
        let base = baseline(&trace.ys);
        let height = threshold.unwrap_or(base + NOISE_SIGMA_MULT * sigma);
        let params = DetectParams {
            min_height: Some(height),
            // The line drives noise rejection: raising it lifts the prominence floor
            // in step, so a higher line also prunes low, broad ripple — not just short
            // peaks — while never dropping below a few sigma of point noise.
            min_prominence: (height - base)
                .max(NOISE_SIGMA_MULT * sigma)
                .max(f64::MIN_POSITIVE),
            min_spacing: None,
            max_count,
        };
        detect_peaks(&trace.xs, &trace.ys, &params)
            .into_iter()
            .map(|p| (p.x, p.y))
            .collect()
    }

    /// Re-run detection at the stored threshold: replace every detected mark with a
    /// fresh set, leaving hand-placed marks (and any detection coincident with one)
    /// untouched.
    pub fn redetect(&mut self, trace: &Trace1d) {
        self.marks.retain(|m| m.origin == PeakOrigin::Manual);
        let tol = x_tolerance(trace);
        for (x, y) in Self::detect_at(trace, self.detector.threshold, self.detector.max_count) {
            if self.marks.iter().any(|m| (m.x - x).abs() <= tol) {
                continue;
            }
            let id = self.next_id();
            self.marks.push(PeakMark {
                id,
                x,
                y,
                origin: PeakOrigin::Detected,
                label: None,
            });
        }
    }

    /// Detect peaks inside an x-window using noise estimated from that window alone,
    /// returning apex `(x, y)` pairs. Prominence-only so an elevated local baseline
    /// does not hide a real line; the scoped window keeps far-off noise out.
    pub fn pick_in_range(trace: &Trace1d, x_a: f64, x_b: f64) -> Vec<(f64, f64)> {
        let (lo, hi) = (x_a.min(x_b), x_a.max(x_b));
        let mut xs = Vec::new();
        let mut ys = Vec::new();
        for (i, &x) in trace.xs.iter().enumerate() {
            if x >= lo && x <= hi {
                xs.push(x);
                ys.push(trace.ys[i]);
            }
        }
        if xs.len() < 3 {
            return Vec::new();
        }
        let sigma = estimate_noise(&ys);
        let params = DetectParams {
            min_height: None,
            min_prominence: (RANGE_SIGMA_MULT * sigma).max(f64::MIN_POSITIVE),
            min_spacing: None,
            max_count: None,
        };
        detect_peaks(&xs, &ys, &params)
            .into_iter()
            .map(|p| (p.x, p.y))
            .collect()
    }

    pub fn resolve(&self) -> Vec<ResolvedPeak> {
        self.marks
            .iter()
            .map(|m| ResolvedPeak {
                x: m.x,
                y: m.y,
                label: m.label.clone().unwrap_or_else(|| default_label(m.x)),
                origin: m.origin,
                mark_id: Some(m.id),
            })
            .collect()
    }

    pub fn remove_mark(&mut self, id: u64) {
        self.marks.retain(|m| m.id != id);
    }
}

fn baseline(ys: &[f64]) -> f64 {
    let mut finite: Vec<f64> = ys.iter().copied().filter(|v| v.is_finite()).collect();
    if finite.is_empty() {
        return 0.0;
    }
    finite.sort_by(|a, b| a.partial_cmp(b).unwrap());
    finite[finite.len() / 2]
}

fn x_tolerance(trace: &Trace1d) -> f64 {
    let (lo, hi) = trace
        .xs
        .iter()
        .filter(|v| v.is_finite())
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), &v| {
            (lo.min(v), hi.max(v))
        });
    if lo.is_finite() && hi > lo {
        (hi - lo) * 1e-3
    } else {
        f64::MIN_POSITIVE
    }
}
