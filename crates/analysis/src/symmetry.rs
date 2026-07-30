//! Symmetry-partner evidence for homonuclear true-2D spectra.
//!
//! The analysis deliberately reports evidence, not ground truth. A missing
//! cross-diagonal partner can be caused by an artifact, but also by overlap,
//! suppression, spectral-window coverage, or a weak signal below the selected
//! floor. Callers should therefore present [`ArtifactLikelihood`] as a review
//! suggestion rather than an automatic classification.

use std::collections::{HashMap, HashSet};

use crate::robust::robust_difference_mad;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct CandidateKey {
    pub row: usize,
    pub col: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PeakPolarity {
    Positive,
    Negative,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PartnerStatus {
    Matched,
    Ambiguous,
    Missing,
    OutsideRange,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ArtifactLikelihood {
    Low,
    Medium,
    High,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ArtifactReason {
    MissingPartner,
    MultiplePartners,
    AxialElongation,
    LowSignalToNoise,
    MissingDiagonalSupport,
}

impl ArtifactReason {
    pub fn label(self) -> &'static str {
        match self {
            Self::MissingPartner => "no symmetry partner",
            Self::MultiplePartners => "multiple nearby partners",
            Self::AxialElongation => "axis-aligned shape",
            Self::LowSignalToNoise => "low signal-to-noise",
            Self::MissingDiagonalSupport => "no diagonal support",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct SymmetryCandidate {
    pub key: CandidateKey,
    pub f2: f64,
    pub f1: f64,
    pub intensity: f64,
    pub signal_to_noise: f64,
    pub polarity: PeakPolarity,
    /// Ratio of the larger to the smaller local second moment.
    pub axial_ratio: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SymmetryEntry {
    pub primary: CandidateKey,
    pub partner: Option<CandidateKey>,
    pub alternatives: Vec<CandidateKey>,
    pub status: PartnerStatus,
    pub likelihood: ArtifactLikelihood,
    pub score: f64,
    pub reasons: Vec<ArtifactReason>,
    pub diagonal_support: [bool; 2],
}

#[derive(Clone, Debug, PartialEq)]
pub struct SymmetryAudit {
    pub noise: f64,
    pub f2_tolerance: f64,
    pub f1_tolerance: f64,
    pub diagonal_tolerance: f64,
    pub candidates: Vec<SymmetryCandidate>,
    pub entries: Vec<SymmetryEntry>,
}

impl SymmetryAudit {
    pub fn candidate(&self, key: CandidateKey) -> Option<&SymmetryCandidate> {
        self.candidates
            .iter()
            .find(|candidate| candidate.key == key)
    }

    pub fn entry_for(&self, key: CandidateKey) -> Option<&SymmetryEntry> {
        self.entries.iter().find(|entry| {
            entry.primary == key || entry.partner == Some(key) || entry.alternatives.contains(&key)
        })
    }

    /// Nearest detected candidate inside the same local window used for partner
    /// matching. The normalized metric keeps unequal F1/F2 digitization from
    /// preferring the more finely sampled dimension.
    pub fn nearest_candidate(&self, f2: f64, f1: f64) -> Option<&SymmetryCandidate> {
        self.candidates
            .iter()
            .filter_map(|candidate| {
                let dx = (candidate.f2 - f2).abs() / self.f2_tolerance;
                let dy = (candidate.f1 - f1).abs() / self.f1_tolerance;
                (dx <= 1.0 && dy <= 1.0).then_some((dx * dx + dy * dy, candidate))
            })
            .min_by(|left, right| left.0.total_cmp(&right.0))
            .map(|(_, candidate)| candidate)
    }

    pub fn counts(&self) -> AuditCounts {
        let mut counts = AuditCounts::default();
        for entry in &self.entries {
            match entry.status {
                PartnerStatus::Matched => counts.matched += 1,
                PartnerStatus::Ambiguous => counts.ambiguous += 1,
                PartnerStatus::Missing => counts.missing += 1,
                PartnerStatus::OutsideRange => counts.outside_range += 1,
            }
            if entry.likelihood == ArtifactLikelihood::High {
                counts.high_likelihood += 1;
            }
        }
        counts
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AuditCounts {
    pub matched: usize,
    pub ambiguous: usize,
    pub missing: usize,
    pub outside_range: usize,
    pub high_likelihood: usize,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SymmetryParams {
    pub min_signal_to_noise: f64,
    pub search_radius_points: usize,
    pub axial_ratio_threshold: f64,
    pub max_candidates: usize,
}

impl Default for SymmetryParams {
    fn default() -> Self {
        Self {
            min_signal_to_noise: 4.0,
            search_radius_points: 3,
            axial_ratio_threshold: 6.0,
            max_candidates: 512,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum SymmetryError {
    #[error("F2 axis has {actual} values, but the grid has {expected} columns")]
    F2Shape { actual: usize, expected: usize },
    #[error("F1 axis has {actual} values, but the grid has {expected} rows")]
    F1Shape { actual: usize, expected: usize },
    #[error("2D grid has {actual} values, but its shape requires {expected}")]
    GridShape { actual: usize, expected: usize },
    #[error("symmetry analysis needs at least a 3 × 3 grid")]
    TooSmall,
    #[error("symmetry analysis parameters must be finite and positive")]
    InvalidParameters,
}

/// Detect local extrema, associate cross-diagonal partners, and attach
/// conservative artifact-review evidence.
pub fn audit_symmetry(
    f2_axis: &[f64],
    f1_axis: &[f64],
    values: &[f32],
    rows: usize,
    cols: usize,
    params: SymmetryParams,
) -> Result<SymmetryAudit, SymmetryError> {
    validate(f2_axis, f1_axis, values, rows, cols, params)?;

    let max_abs = values
        .iter()
        .copied()
        .filter(|value| value.is_finite())
        .map(f32::abs)
        .fold(0.0_f32, f32::max) as f64;
    let noise = robust_difference_mad(values, rows, cols).max(max_abs * 1e-12);
    let threshold = params.min_signal_to_noise * noise;
    let f2_spacing = representative_spacing(f2_axis);
    let f1_spacing = representative_spacing(f1_axis);
    let f2_tolerance = (f2_spacing * params.search_radius_points as f64).max(f64::MIN_POSITIVE);
    let f1_tolerance = (f1_spacing * params.search_radius_points as f64).max(f64::MIN_POSITIVE);
    let diagonal_tolerance = (f2_spacing.max(f1_spacing) * 2.0).max(f64::MIN_POSITIVE);

    let mut candidates = detect_candidates(values, rows, cols, threshold, noise);
    candidates.sort_by(|left, right| {
        right
            .signal_to_noise
            .total_cmp(&left.signal_to_noise)
            .then_with(|| left.key.cmp(&right.key))
    });
    let retained: HashSet<CandidateKey> = candidates
        .iter()
        .take(params.max_candidates)
        .map(|candidate| candidate.key)
        .collect();
    for candidate in &mut candidates {
        candidate.f2 = f2_axis[candidate.key.col];
        candidate.f1 = f1_axis[candidate.key.row];
    }
    candidates.sort_by_key(|candidate| candidate.key);

    let lookup: HashMap<CandidateKey, usize> = candidates
        .iter()
        .enumerate()
        .map(|(index, candidate)| (candidate.key, index))
        .collect();
    let relationships: HashMap<CandidateKey, Relationship> = candidates
        .iter()
        .filter(|candidate| (candidate.f2 - candidate.f1).abs() > diagonal_tolerance)
        .map(|candidate| {
            (
                candidate.key,
                relationship(
                    candidate,
                    &candidates,
                    f2_tolerance,
                    f1_tolerance,
                    f2_axis,
                    f1_axis,
                ),
            )
        })
        .collect();

    let mut visited = HashSet::new();
    let mut entries = Vec::new();
    for candidate in &candidates {
        if !retained.contains(&candidate.key)
            || (candidate.f2 - candidate.f1).abs() <= diagonal_tolerance
            || visited.contains(&candidate.key)
        {
            continue;
        }
        let relation = relationships
            .get(&candidate.key)
            .cloned()
            .unwrap_or(Relationship::Missing);
        let (status, partner, alternatives) = match relation {
            Relationship::Matched(key)
                if relationships.get(&key) == Some(&Relationship::Matched(candidate.key)) =>
            {
                visited.insert(key);
                (PartnerStatus::Matched, Some(key), Vec::new())
            }
            Relationship::Matched(key) => (PartnerStatus::Ambiguous, None, vec![key]),
            Relationship::Ambiguous(keys) => (PartnerStatus::Ambiguous, None, keys),
            Relationship::Missing => (PartnerStatus::Missing, None, Vec::new()),
            Relationship::OutsideRange => (PartnerStatus::OutsideRange, None, Vec::new()),
        };
        visited.insert(candidate.key);

        let diagonal_support = [
            has_candidate_near(
                &candidates,
                candidate.f2,
                candidate.f2,
                f2_tolerance,
                f1_tolerance,
            ),
            has_candidate_near(
                &candidates,
                candidate.f1,
                candidate.f1,
                f2_tolerance,
                f1_tolerance,
            ),
        ];
        let paired = partner.and_then(|key| lookup.get(&key).map(|&index| &candidates[index]));
        let evidence = artifact_evidence(
            candidate,
            paired,
            status,
            diagonal_support,
            params.axial_ratio_threshold,
            params.min_signal_to_noise,
        );
        entries.push(SymmetryEntry {
            primary: candidate.key,
            partner,
            alternatives,
            status,
            likelihood: evidence.likelihood,
            score: evidence.score,
            reasons: evidence.reasons,
            diagonal_support,
        });
    }
    entries.sort_by(|left, right| {
        right
            .likelihood
            .cmp(&left.likelihood)
            .then_with(|| right.score.total_cmp(&left.score))
            .then_with(|| left.primary.cmp(&right.primary))
    });

    // Keep the configured number of strongest primaries, plus any lower-ranked
    // candidates their full-set relationships reference. Dropping those
    // dependencies would turn a real reciprocal partner into a false missing
    // suggestion merely because it fell below the presentation cap.
    let mut emitted = retained;
    for entry in &entries {
        emitted.insert(entry.primary);
        emitted.extend(entry.partner);
        emitted.extend(entry.alternatives.iter().copied());
    }
    candidates.retain(|candidate| emitted.contains(&candidate.key));

    Ok(SymmetryAudit {
        noise,
        f2_tolerance,
        f1_tolerance,
        diagonal_tolerance,
        candidates,
        entries,
    })
}

fn validate(
    f2_axis: &[f64],
    f1_axis: &[f64],
    values: &[f32],
    rows: usize,
    cols: usize,
    params: SymmetryParams,
) -> Result<(), SymmetryError> {
    if rows < 3 || cols < 3 {
        return Err(SymmetryError::TooSmall);
    }
    if f2_axis.len() != cols {
        return Err(SymmetryError::F2Shape {
            actual: f2_axis.len(),
            expected: cols,
        });
    }
    if f1_axis.len() != rows {
        return Err(SymmetryError::F1Shape {
            actual: f1_axis.len(),
            expected: rows,
        });
    }
    let expected = rows.saturating_mul(cols);
    if values.len() != expected {
        return Err(SymmetryError::GridShape {
            actual: values.len(),
            expected,
        });
    }
    if !params.min_signal_to_noise.is_finite()
        || params.min_signal_to_noise <= 0.0
        || !params.axial_ratio_threshold.is_finite()
        || params.axial_ratio_threshold <= 1.0
        || params.search_radius_points == 0
        || params.max_candidates == 0
    {
        return Err(SymmetryError::InvalidParameters);
    }
    Ok(())
}

fn detect_candidates(
    values: &[f32],
    rows: usize,
    cols: usize,
    threshold: f64,
    noise: f64,
) -> Vec<SymmetryCandidate> {
    let mut candidates = Vec::new();
    for row in 1..rows - 1 {
        for col in 1..cols - 1 {
            let index = row * cols + col;
            let value = f64::from(values[index]);
            if !value.is_finite() || value.abs() < threshold || value == 0.0 {
                continue;
            }
            let absolute = value.abs();
            let mut greater_than_one = false;
            let mut local = true;
            for near_row in row - 1..=row + 1 {
                for near_col in col - 1..=col + 1 {
                    if near_row == row && near_col == col {
                        continue;
                    }
                    let near = f64::from(values[near_row * cols + near_col]).abs();
                    if near > absolute {
                        local = false;
                        break;
                    }
                    greater_than_one |= absolute > near;
                }
                if !local {
                    break;
                }
            }
            if !local || !greater_than_one {
                continue;
            }
            candidates.push(SymmetryCandidate {
                key: CandidateKey { row, col },
                f2: 0.0,
                f1: 0.0,
                intensity: value,
                signal_to_noise: absolute / noise.max(f64::MIN_POSITIVE),
                polarity: if value >= 0.0 {
                    PeakPolarity::Positive
                } else {
                    PeakPolarity::Negative
                },
                axial_ratio: local_axial_ratio(values, rows, cols, row, col),
            });
        }
    }
    candidates.sort_by(|left, right| {
        right
            .signal_to_noise
            .total_cmp(&left.signal_to_noise)
            .then_with(|| left.key.cmp(&right.key))
    });
    suppress_adjacent(candidates)
}

fn suppress_adjacent(candidates: Vec<SymmetryCandidate>) -> Vec<SymmetryCandidate> {
    let mut kept: Vec<SymmetryCandidate> = Vec::new();
    for candidate in candidates {
        if kept.iter().any(|other| {
            other.key.row.abs_diff(candidate.key.row) <= 1
                && other.key.col.abs_diff(candidate.key.col) <= 1
        }) {
            continue;
        }
        kept.push(candidate);
    }
    kept
}

fn local_axial_ratio(values: &[f32], rows: usize, cols: usize, row: usize, col: usize) -> f64 {
    let radius = 3;
    let row_lo = row.saturating_sub(radius);
    let row_hi = (row + radius).min(rows - 1);
    let col_lo = col.saturating_sub(radius);
    let col_hi = (col + radius).min(cols - 1);
    let center = f64::from(values[row * cols + col]).abs();
    let floor = center * 0.1;
    let mut weight = 0.0;
    let mut row_moment = 0.0;
    let mut col_moment = 0.0;
    for near_row in row_lo..=row_hi {
        for near_col in col_lo..=col_hi {
            let sample = f64::from(values[near_row * cols + near_col]).abs();
            let w = (sample - floor).max(0.0);
            weight += w;
            row_moment += w * near_row.abs_diff(row).pow(2) as f64;
            col_moment += w * near_col.abs_diff(col).pow(2) as f64;
        }
    }
    if weight <= f64::MIN_POSITIVE {
        return 1.0;
    }
    let small = row_moment.min(col_moment) / weight;
    let large = row_moment.max(col_moment) / weight;
    if small <= 1e-9 {
        if large <= 1e-9 { 1.0 } else { f64::INFINITY }
    } else {
        large / small
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Relationship {
    Matched(CandidateKey),
    Ambiguous(Vec<CandidateKey>),
    Missing,
    OutsideRange,
}

fn relationship(
    candidate: &SymmetryCandidate,
    candidates: &[SymmetryCandidate],
    f2_tolerance: f64,
    f1_tolerance: f64,
    f2_axis: &[f64],
    f1_axis: &[f64],
) -> Relationship {
    let target_f2 = candidate.f1;
    let target_f1 = candidate.f2;
    if !in_bounds(f2_axis, target_f2) || !in_bounds(f1_axis, target_f1) {
        return Relationship::OutsideRange;
    }
    let mut matches: Vec<(f64, CandidateKey)> = candidates
        .iter()
        .filter(|other| other.key != candidate.key)
        .filter_map(|other| {
            let dx = (other.f2 - target_f2).abs() / f2_tolerance;
            let dy = (other.f1 - target_f1).abs() / f1_tolerance;
            (dx <= 1.0 && dy <= 1.0).then_some((dx * dx + dy * dy, other.key))
        })
        .collect();
    matches.sort_by(|left, right| left.0.total_cmp(&right.0));
    match matches.as_slice() {
        [] => Relationship::Missing,
        [(_, key)] => Relationship::Matched(*key),
        [
            (first_distance, first),
            (second_distance, second),
            rest @ ..,
        ] if second_distance - first_distance <= 0.35 => {
            let mut keys = vec![*first, *second];
            keys.extend(rest.iter().map(|(_, key)| *key));
            Relationship::Ambiguous(keys)
        }
        [(_, key), ..] => Relationship::Matched(*key),
    }
}

fn in_bounds(axis: &[f64], value: f64) -> bool {
    let (lo, hi) = axis
        .iter()
        .copied()
        .filter(|sample| sample.is_finite())
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), sample| {
            (lo.min(sample), hi.max(sample))
        });
    lo <= value && value <= hi
}

fn has_candidate_near(
    candidates: &[SymmetryCandidate],
    f2: f64,
    f1: f64,
    f2_tolerance: f64,
    f1_tolerance: f64,
) -> bool {
    candidates.iter().any(|candidate| {
        (candidate.f2 - f2).abs() <= f2_tolerance && (candidate.f1 - f1).abs() <= f1_tolerance
    })
}

struct ArtifactEvidence {
    likelihood: ArtifactLikelihood,
    score: f64,
    reasons: Vec<ArtifactReason>,
}

fn artifact_evidence(
    primary: &SymmetryCandidate,
    partner: Option<&SymmetryCandidate>,
    status: PartnerStatus,
    diagonal_support: [bool; 2],
    axial_threshold: f64,
    min_snr: f64,
) -> ArtifactEvidence {
    let mut score: f64 = 0.05;
    let mut reasons = Vec::new();
    match status {
        PartnerStatus::Matched => {}
        PartnerStatus::Ambiguous => {
            score += 0.24;
            reasons.push(ArtifactReason::MultiplePartners);
        }
        PartnerStatus::Missing => {
            score += 0.58;
            reasons.push(ArtifactReason::MissingPartner);
        }
        // Missing coverage is not evidence that the observed peak is an artifact.
        PartnerStatus::OutsideRange => {}
    }
    if primary.axial_ratio >= axial_threshold
        || partner.is_some_and(|candidate| candidate.axial_ratio >= axial_threshold)
    {
        score += 0.24;
        reasons.push(ArtifactReason::AxialElongation);
    }
    if primary.signal_to_noise < min_snr * 1.5
        || partner.is_some_and(|candidate| candidate.signal_to_noise < min_snr * 1.5)
    {
        score += 0.10;
        reasons.push(ArtifactReason::LowSignalToNoise);
    }
    if !diagonal_support[0] && !diagonal_support[1] {
        score += 0.06;
        reasons.push(ArtifactReason::MissingDiagonalSupport);
    }
    let score = score.clamp(0.0, 1.0);
    let likelihood = if score >= 0.65 {
        ArtifactLikelihood::High
    } else if score >= 0.35 {
        ArtifactLikelihood::Medium
    } else {
        ArtifactLikelihood::Low
    };
    ArtifactEvidence {
        likelihood,
        score,
        reasons,
    }
}

fn representative_spacing(axis: &[f64]) -> f64 {
    let mut differences: Vec<f64> = axis
        .windows(2)
        .filter_map(|pair| {
            let difference = (pair[1] - pair[0]).abs();
            (difference.is_finite() && difference > 0.0).then_some(difference)
        })
        .collect();
    if differences.is_empty() {
        return 1.0;
    }
    let middle = differences.len() / 2;
    differences.select_nth_unstable_by(middle, f64::total_cmp);
    differences[middle]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid(peaks: &[(usize, usize, f32)]) -> (Vec<f64>, Vec<f32>) {
        let axis = (0..9).map(|value| value as f64).collect::<Vec<_>>();
        let mut values = vec![0.0; 81];
        for &(row, col, height) in peaks {
            values[row * 9 + col] = height;
            for (dr, dc, scale) in [
                (0_isize, -1_isize, 0.35_f32),
                (0, 1, 0.35),
                (-1, 0, 0.35),
                (1, 0, 0.35),
            ] {
                let r = row.checked_add_signed(dr).unwrap();
                let c = col.checked_add_signed(dc).unwrap();
                values[r * 9 + c] = height * scale;
            }
        }
        (axis, values)
    }

    #[test]
    fn reciprocal_cross_peaks_form_one_matched_entry() {
        let (axis, values) = grid(&[(2, 6, 10.0), (6, 2, 8.0), (2, 2, 12.0), (6, 6, 11.0)]);
        let audit = audit_symmetry(&axis, &axis, &values, 9, 9, SymmetryParams::default()).unwrap();
        let matched: Vec<_> = audit
            .entries
            .iter()
            .filter(|entry| entry.status == PartnerStatus::Matched)
            .collect();
        assert_eq!(matched.len(), 1);
        assert_eq!(matched[0].likelihood, ArtifactLikelihood::Low);
        assert_eq!(audit.counts().matched, 1);
    }

    #[test]
    fn unpaired_peak_is_a_high_likelihood_review_suggestion() {
        let (axis, values) = grid(&[(2, 6, 10.0)]);
        let audit = audit_symmetry(&axis, &axis, &values, 9, 9, SymmetryParams::default()).unwrap();
        assert_eq!(audit.entries.len(), 1);
        assert_eq!(audit.entries[0].status, PartnerStatus::Missing);
        assert_eq!(audit.entries[0].likelihood, ArtifactLikelihood::High);
        assert!(
            audit.entries[0]
                .reasons
                .contains(&ArtifactReason::MissingPartner)
        );
    }

    #[test]
    fn candidate_cap_does_not_hide_a_real_reciprocal_partner() {
        let (axis, values) = grid(&[(2, 6, 10.0), (6, 2, 8.0)]);
        let params = SymmetryParams {
            max_candidates: 1,
            ..SymmetryParams::default()
        };
        let audit = audit_symmetry(&axis, &axis, &values, 9, 9, params).unwrap();

        assert_eq!(audit.entries.len(), 1);
        assert_eq!(audit.entries[0].status, PartnerStatus::Matched);
        assert_eq!(audit.entries[0].likelihood, ArtifactLikelihood::Low);
        assert!(audit.entries[0].partner.is_some());
        assert!(audit.candidate(audit.entries[0].partner.unwrap()).is_some());
    }

    #[test]
    fn unavailable_transposed_coordinate_is_not_called_an_artifact() {
        let f2 = (0..9).map(|value| value as f64).collect::<Vec<_>>();
        let f1 = (0..9).map(|value| value as f64 + 10.0).collect::<Vec<_>>();
        let (_, values) = grid(&[(2, 6, 10.0)]);
        let audit = audit_symmetry(&f2, &f1, &values, 9, 9, SymmetryParams::default()).unwrap();
        assert_eq!(audit.entries[0].status, PartnerStatus::OutsideRange);
        assert_ne!(audit.entries[0].likelihood, ArtifactLikelihood::High);
    }

    #[test]
    fn malformed_shape_is_rejected() {
        let error = audit_symmetry(
            &[0.0, 1.0, 2.0],
            &[0.0, 1.0, 2.0],
            &[0.0; 8],
            3,
            3,
            SymmetryParams::default(),
        )
        .unwrap_err();
        assert!(matches!(error, SymmetryError::GridShape { .. }));
    }
}
