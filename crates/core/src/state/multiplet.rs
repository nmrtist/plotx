//! Serde mirror of a classified multiplet, stored on the source 1D NMR
//! dataset so analyses persist in projects.

use plotx_analysis::multiplet::{JValue, Pattern};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum MultipletPatternKind {
    Singlet,
    Doublet,
    Triplet,
    Quartet,
    DoubletOfDoublets,
    Multiplet,
}

impl MultipletPatternKind {
    pub fn label(self) -> &'static str {
        Pattern::from(self).label()
    }
}

impl From<Pattern> for MultipletPatternKind {
    fn from(p: Pattern) -> Self {
        match p {
            Pattern::Singlet => Self::Singlet,
            Pattern::Doublet => Self::Doublet,
            Pattern::Triplet => Self::Triplet,
            Pattern::Quartet => Self::Quartet,
            Pattern::DoubletOfDoublets => Self::DoubletOfDoublets,
            Pattern::Multiplet => Self::Multiplet,
        }
    }
}

impl From<MultipletPatternKind> for Pattern {
    fn from(k: MultipletPatternKind) -> Self {
        match k {
            MultipletPatternKind::Singlet => Pattern::Singlet,
            MultipletPatternKind::Doublet => Pattern::Doublet,
            MultipletPatternKind::Triplet => Pattern::Triplet,
            MultipletPatternKind::Quartet => Pattern::Quartet,
            MultipletPatternKind::DoubletOfDoublets => Pattern::DoubletOfDoublets,
            MultipletPatternKind::Multiplet => Pattern::Multiplet,
        }
    }
}

#[derive(Clone, PartialEq, Serialize, Deserialize, Debug)]
pub struct StoredJValue {
    pub hz: f64,
    #[serde(default)]
    pub sigma_hz: Option<f64>,
}

impl From<&JValue> for StoredJValue {
    fn from(j: &JValue) -> Self {
        Self {
            hz: j.hz,
            sigma_hz: j.sigma_hz,
        }
    }
}

/// One classified multiplet over the ppm window `[lo, hi]` of its dataset.
#[derive(Clone, PartialEq, Serialize, Deserialize, Debug)]
pub struct StoredMultiplet {
    pub id: u64,
    pub lo: f64,
    pub hi: f64,
    pub center_ppm: f64,
    pub pattern: MultipletPatternKind,
    #[serde(default)]
    pub j_values: Vec<StoredJValue>,
    pub area: f64,
    #[serde(default)]
    pub peak_ppm: Vec<f64>,
}

impl StoredMultiplet {
    /// Journal-style report entry, e.g. `2.35 (dd, J = 12.0, 4.0 Hz)`. An
    /// unresolved multiplet reports its range, high shift first to match the
    /// reversed ppm axis.
    pub fn descriptor(&self) -> String {
        match self.pattern {
            MultipletPatternKind::Multiplet => {
                format!("{:.2}–{:.2} (m)", self.hi, self.lo)
            }
            MultipletPatternKind::Singlet => format!("{:.2} (s)", self.center_ppm),
            pattern => {
                let label = pattern.label();
                if self.j_values.is_empty() {
                    format!("{:.2} ({label})", self.center_ppm)
                } else {
                    let js: Vec<String> = self
                        .j_values
                        .iter()
                        .map(|j| format!("{:.1}", j.hz))
                        .collect();
                    format!("{:.2} ({label}, J = {} Hz)", self.center_ppm, js.join(", "))
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stored(pattern: MultipletPatternKind, js: &[f64]) -> StoredMultiplet {
        StoredMultiplet {
            id: 0,
            lo: 2.30,
            hi: 2.41,
            center_ppm: 2.354,
            pattern,
            j_values: js
                .iter()
                .map(|&hz| StoredJValue { hz, sigma_hz: None })
                .collect(),
            area: 1.0,
            peak_ppm: vec![2.41, 2.30],
        }
    }

    #[test]
    fn singlet_descriptor_is_shift_only() {
        assert_eq!(
            stored(MultipletPatternKind::Singlet, &[]).descriptor(),
            "2.35 (s)"
        );
    }

    #[test]
    fn doublet_descriptor_reports_one_j() {
        assert_eq!(
            stored(MultipletPatternKind::Doublet, &[7.2]).descriptor(),
            "2.35 (d, J = 7.2 Hz)"
        );
    }

    #[test]
    fn triplet_and_quartet_labels() {
        assert_eq!(
            stored(MultipletPatternKind::Triplet, &[7.0]).descriptor(),
            "2.35 (t, J = 7.0 Hz)"
        );
        assert_eq!(
            stored(MultipletPatternKind::Quartet, &[7.0]).descriptor(),
            "2.35 (q, J = 7.0 Hz)"
        );
    }

    #[test]
    fn dd_descriptor_joins_both_j() {
        assert_eq!(
            stored(MultipletPatternKind::DoubletOfDoublets, &[12.0, 4.0]).descriptor(),
            "2.35 (dd, J = 12.0, 4.0 Hz)"
        );
    }

    #[test]
    fn multiplet_descriptor_is_range_high_first() {
        assert_eq!(
            stored(MultipletPatternKind::Multiplet, &[]).descriptor(),
            "2.41–2.30 (m)"
        );
    }

    #[test]
    fn stored_multiplet_survives_json_round_trip() {
        let m = stored(MultipletPatternKind::DoubletOfDoublets, &[12.0, 4.0]);
        let json = serde_json::to_string(&m).unwrap();
        let back: StoredMultiplet = serde_json::from_str(&json).unwrap();
        assert_eq!(back, m);
    }

    #[test]
    fn pattern_kind_round_trips_through_analysis_pattern() {
        for kind in [
            MultipletPatternKind::Singlet,
            MultipletPatternKind::Doublet,
            MultipletPatternKind::Triplet,
            MultipletPatternKind::Quartet,
            MultipletPatternKind::DoubletOfDoublets,
            MultipletPatternKind::Multiplet,
        ] {
            assert_eq!(MultipletPatternKind::from(Pattern::from(kind)), kind);
        }
        assert_eq!(MultipletPatternKind::DoubletOfDoublets.label(), "dd");
    }
}
