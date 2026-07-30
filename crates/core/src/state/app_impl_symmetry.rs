//! Homonuclear 2D symmetry-audit orchestration and undoable peak edits.

use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

use plotx_analysis::symmetry::{
    ArtifactLikelihood, CandidateKey, PartnerStatus, SymmetryEntry, SymmetryParams, audit_symmetry,
};

use super::*;

impl Nmr2DDataset {
    pub fn symmetry_unavailable_reason(&self) -> Option<&'static str> {
        if !self.is_true_2d() {
            return Some("Symmetry review needs a true-2D contour spectrum.");
        }
        if !self.preset.homonuclear() {
            return Some("Choose COSY, TOCSY, or NOESY / ROESY as the experiment.");
        }
        let Processed2D::Ft(spectrum) = &self.processed else {
            return Some("Symmetry review needs a true-2D contour spectrum.");
        };
        if spectrum.f2_domain != plotx_io::Domain::Frequency
            || spectrum.f1_domain != plotx_io::Domain::Frequency
        {
            return Some("Symmetry review needs two frequency-domain axes.");
        }
        if spectrum.direct.nucleus != spectrum.indirect.nucleus {
            return Some("F1 and F2 must describe the same nucleus.");
        }
        let (f2_lo, f2_hi) = ordered(spectrum.f2_bounds());
        let (f1_lo, f1_hi) = ordered(spectrum.f1_bounds());
        if f2_hi < f1_lo || f1_hi < f2_lo {
            return Some("F1 and F2 chemical-shift ranges do not overlap.");
        }
        None
    }

    pub fn supports_symmetry_review(&self) -> bool {
        self.symmetry_unavailable_reason().is_none()
    }
}

impl PlotxApp {
    pub fn start_symmetry_audit(&mut self, dataset: usize) -> Result<(), String> {
        if self.session.symmetry_audit_job.is_some() {
            return Err("A symmetry audit is already running.".to_owned());
        }
        let nmr = self
            .doc
            .datasets
            .get(dataset)
            .and_then(Dataset::as_nmr2d)
            .ok_or_else(|| "Symmetry review needs a 2D NMR dataset.".to_owned())?;
        if let Some(reason) = nmr.symmetry_unavailable_reason() {
            return Err(reason.to_owned());
        }
        let Processed2D::Ft(spectrum) = &nmr.processed else {
            return Err("Symmetry review needs a true-2D contour spectrum.".to_owned());
        };
        let spectrum = Arc::clone(spectrum);
        let mode = nmr.display_mode();
        let dataset_id = nmr.resource_id;
        let worker_spectrum = Arc::clone(&spectrum);
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let values = worker_spectrum.grid(mode);
            let result = audit_symmetry(
                &worker_spectrum.f2_ppm,
                &worker_spectrum.f1_ppm,
                &values,
                worker_spectrum.f1_size,
                worker_spectrum.f2_size,
                SymmetryParams::default(),
            )
            .map_err(|error| error.to_string());
            // The receiver may have been dropped because the dataset changed.
            let _ = tx.send(result);
        });
        self.session.ui.symmetry_audit = None;
        self.session.ui.symmetry_attempted_spectrum = Some(Arc::clone(&spectrum));
        self.session.symmetry_audit_job = Some(SymmetryAuditJob {
            dataset: dataset_id,
            spectrum,
            mode,
            started_at: Instant::now(),
            rx,
        });
        self.session.status = "Checking cross-diagonal symmetry…".to_owned();
        Ok(())
    }

    pub fn symmetry_audit_progress(&self) -> Option<(DatasetId, Duration)> {
        self.session
            .symmetry_audit_job
            .as_ref()
            .map(|job| (job.dataset, job.started_at.elapsed()))
    }

    /// Drain the background audit without blocking. The exact processed
    /// spectrum `Arc` is the provenance guard: a result computed before phase or
    /// processing changed can never attach to the newer display.
    pub fn poll_symmetry_audit(&mut self) -> bool {
        let Some(job) = &self.session.symmetry_audit_job else {
            return false;
        };
        let result = match job.rx.try_recv() {
            Err(mpsc::TryRecvError::Empty) => return true,
            Err(mpsc::TryRecvError::Disconnected) => {
                Err("The symmetry worker stopped before returning a result.".to_owned())
            }
            Ok(result) => result,
        };
        let job = self
            .session
            .symmetry_audit_job
            .take()
            .expect("job checked above");
        match result {
            Err(message) => {
                self.session.status = format!("Symmetry audit failed: {message}");
            }
            Ok(result) => {
                let current = self
                    .doc
                    .dataset_index(job.dataset)
                    .and_then(|index| self.doc.datasets.get(index))
                    .and_then(Dataset::as_nmr2d)
                    .and_then(|dataset| match &dataset.processed {
                        Processed2D::Ft(spectrum) => Some((dataset, spectrum)),
                        Processed2D::Stack(_) => None,
                    });
                if current.is_some_and(|(dataset, spectrum)| {
                    dataset.supports_symmetry_review()
                        && dataset.display_mode() == job.mode
                        && Arc::ptr_eq(spectrum, &job.spectrum)
                }) {
                    let counts = result.counts();
                    self.session.status = format!(
                        "Symmetry audit: {} paired, {} unpaired, {} ambiguous.",
                        counts.matched, counts.missing, counts.ambiguous
                    );
                    self.session.ui.symmetry_audit = Some(SymmetryAuditState {
                        dataset: job.dataset,
                        spectrum: job.spectrum,
                        mode: job.mode,
                        result,
                    });
                } else {
                    self.session.status =
                        "The spectrum changed during symmetry analysis; the stale result was discarded."
                            .to_owned();
                }
            }
        }
        true
    }

    pub fn current_symmetry_audit(&self, dataset: usize) -> Option<&SymmetryAuditState> {
        let nmr = self.doc.datasets.get(dataset)?.as_nmr2d()?;
        let Processed2D::Ft(spectrum) = &nmr.processed else {
            return None;
        };
        self.session.ui.symmetry_audit.as_ref().filter(|audit| {
            audit.dataset == nmr.resource_id
                && audit.mode == nmr.display_mode()
                && Arc::ptr_eq(&audit.spectrum, spectrum)
        })
    }

    pub fn symmetry_audit_needs_start(&self, dataset: usize) -> bool {
        let Some(nmr) = self.doc.datasets.get(dataset).and_then(Dataset::as_nmr2d) else {
            return false;
        };
        let Processed2D::Ft(spectrum) = &nmr.processed else {
            return false;
        };
        nmr.supports_symmetry_review()
            && self.session.symmetry_audit_job.is_none()
            && self
                .session
                .ui
                .symmetry_attempted_spectrum
                .as_ref()
                .is_none_or(|attempted| !Arc::ptr_eq(attempted, spectrum))
    }

    pub fn retry_symmetry_audit(&mut self, dataset: usize) -> Result<(), String> {
        self.session.ui.symmetry_attempted_spectrum = None;
        self.start_symmetry_audit(dataset)
    }

    pub fn symmetry_reading(
        &self,
        dataset: usize,
        f2: f64,
        f1: f64,
        snap: bool,
    ) -> Option<SymmetryCursorReading> {
        let nmr = self.doc.datasets.get(dataset)?.as_nmr2d()?;
        if !nmr.supports_symmetry_review() {
            return None;
        }
        let Processed2D::Ft(spectrum) = &nmr.processed else {
            return None;
        };
        let mode = nmr.display_mode();
        let audit = self.current_symmetry_audit(dataset);
        let evidence_candidate = audit.and_then(|audit| audit.result.nearest_candidate(f2, f1));
        let current_candidate = snap.then_some(evidence_candidate).flatten();
        let current =
            current_candidate.map_or_else(|| sample_point(spectrum, mode, f2, f1), candidate_point);
        let partner_target = [current.f1, current.f2];

        let entry = evidence_candidate
            .and_then(|candidate| audit?.result.entry_for(candidate.key))
            .cloned();
        let current_key = evidence_candidate.map(|candidate| candidate.key);
        let counterpart_key = entry
            .as_ref()
            .and_then(|entry| counterpart_for(entry, current_key?));
        let partner_candidate = counterpart_key
            .and_then(|key| audit?.result.candidate(key))
            .or_else(|| {
                audit?
                    .result
                    .nearest_candidate(partner_target[0], partner_target[1])
            });
        let partner = partner_candidate.map(candidate_point);
        let on_diagonal = audit.is_some_and(|audit| {
            (current.f2 - current.f1).abs() <= audit.result.diagonal_tolerance
        });

        Some(SymmetryCursorReading {
            dataset: nmr.resource_id,
            current,
            partner_target,
            partner,
            current_key,
            partner_key: partner_candidate.map(|candidate| candidate.key),
            alternatives: entry.as_ref().map_or(0, |entry| entry.alternatives.len()),
            status: entry.as_ref().map(|entry| entry.status),
            likelihood: entry.as_ref().map(|entry| entry.likelihood),
            reasons: entry
                .as_ref()
                .map(|entry| entry.reasons.clone())
                .unwrap_or_default(),
            current_signal_to_noise: evidence_candidate.map(|candidate| candidate.signal_to_noise),
            partner_signal_to_noise: partner_candidate.map(|candidate| candidate.signal_to_noise),
            on_diagonal,
        })
    }

    pub fn pin_symmetry_entry(&mut self, dataset: usize, key: CandidateKey) -> bool {
        let reading = {
            let Some(audit) = self.current_symmetry_audit(dataset) else {
                return false;
            };
            let Some(candidate) = audit.result.candidate(key) else {
                return false;
            };
            self.symmetry_reading(dataset, candidate.f2, candidate.f1, true)
        };
        self.session.ui.symmetry_pin = reading;
        true
    }

    pub fn accept_symmetry_pair(
        &mut self,
        dataset: usize,
        reading: &SymmetryCursorReading,
    ) -> Result<(), String> {
        let partner = reading
            .partner
            .ok_or_else(|| "No detected partner is available to pick.".to_owned())?;
        let (dataset_id, before, tolerances) = self.peaks_2d_edit_input(dataset)?;
        if reading.dataset != dataset_id {
            return Err("The pinned comparison belongs to another dataset.".to_owned());
        }
        let mut after = before.clone();
        let ids = after.add_pair(reading.current, partner, tolerances, Peak2DOrigin::Manual)?;
        for id in ids {
            after.set_review(id, Peak2DReview::Confirmed);
        }
        self.execute_action(Action::set_peaks_2d(dataset_id, before, after));
        self.session.ui.selected_peak_2d = Some(Peak2DSelection::new(dataset_id, ids[0]));
        self.session.status = "Picked the symmetry-related peak pair.".to_owned();
        Ok(())
    }

    pub fn accept_all_matched_symmetry_pairs(&mut self, dataset: usize) -> Result<usize, String> {
        let pairs = {
            let audit = self
                .current_symmetry_audit(dataset)
                .ok_or_else(|| "Run the symmetry audit first.".to_owned())?;
            audit
                .result
                .entries
                .iter()
                .filter(|entry| entry.status == PartnerStatus::Matched)
                .filter_map(|entry| {
                    let first = audit.result.candidate(entry.primary)?;
                    let second = audit.result.candidate(entry.partner?)?;
                    Some((candidate_point(first), candidate_point(second)))
                })
                .collect::<Vec<_>>()
        };
        let (dataset_id, before, tolerances) = self.peaks_2d_edit_input(dataset)?;
        let mut after = before.clone();
        let mut accepted = 0;
        for (first, second) in pairs {
            let ids = after.add_pair(first, second, tolerances, Peak2DOrigin::SymmetryAudit)?;
            for id in ids {
                after.set_review(id, Peak2DReview::Confirmed);
            }
            accepted += 1;
        }
        self.execute_action(Action::set_peaks_2d(dataset_id, before, after));
        self.session.status = format!("Accepted {accepted} symmetry-related peak pairs.");
        Ok(accepted)
    }

    pub fn mark_high_likelihood_artifacts(&mut self, dataset: usize) -> Result<usize, String> {
        let points = {
            let audit = self
                .current_symmetry_audit(dataset)
                .ok_or_else(|| "Run the symmetry audit first.".to_owned())?;
            audit
                .result
                .entries
                .iter()
                .filter(|entry| entry.likelihood == ArtifactLikelihood::High)
                .flat_map(|entry| {
                    std::iter::once(entry.primary)
                        .chain(entry.partner)
                        .filter_map(|key| audit.result.candidate(key))
                        .map(candidate_point)
                        .collect::<Vec<_>>()
                })
                .collect::<Vec<_>>()
        };
        let (dataset_id, before, tolerances) = self.peaks_2d_edit_input(dataset)?;
        let mut after = before.clone();
        let mut marked = 0;
        for point in points {
            after.add_single(
                point,
                tolerances,
                Peak2DOrigin::SymmetryAudit,
                Peak2DReview::PossibleArtifact,
            )?;
            marked += 1;
        }
        self.execute_action(Action::set_peaks_2d(dataset_id, before, after));
        self.session.status = format!("Marked {marked} peaks as possible artifacts for review.");
        Ok(marked)
    }

    pub fn mark_pinned_possible_artifact(
        &mut self,
        dataset: usize,
        reading: &SymmetryCursorReading,
    ) -> Result<(), String> {
        let (dataset_id, before, tolerances) = self.peaks_2d_edit_input(dataset)?;
        if reading.dataset != dataset_id {
            return Err("The pinned comparison belongs to another dataset.".to_owned());
        }
        let mut after = before.clone();
        let id = after.add_single(
            reading.current,
            tolerances,
            Peak2DOrigin::Manual,
            Peak2DReview::PossibleArtifact,
        )?;
        self.execute_action(Action::set_peaks_2d(dataset_id, before, after));
        self.session.ui.selected_peak_2d = Some(Peak2DSelection::new(dataset_id, id));
        self.session.status = "Marked the peak as a possible artifact.".to_owned();
        Ok(())
    }

    pub fn review_peak_2d(&mut self, dataset: usize, id: Peak2DId, review: Peak2DReview) {
        let Some(nmr) = self.doc.datasets.get(dataset).and_then(Dataset::as_nmr2d) else {
            return;
        };
        let dataset_id = nmr.resource_id;
        let before = nmr.peaks.clone();
        let mut after = before.clone();
        if !after.set_review(id, review) {
            return;
        }
        self.execute_action(Action::set_peaks_2d(dataset_id, before, after));
        self.session.status = format!("2D peak marked {}.", review.label().to_lowercase());
    }

    pub fn remove_peak_2d(&mut self, dataset: usize, id: Peak2DId) {
        let Some(nmr) = self.doc.datasets.get(dataset).and_then(Dataset::as_nmr2d) else {
            return;
        };
        let dataset_id = nmr.resource_id;
        let before = nmr.peaks.clone();
        let mut after = before.clone();
        if !after.remove(id) {
            return;
        }
        self.execute_action(Action::set_peaks_2d(dataset_id, before, after));
        self.session.ui.selected_peak_2d = None;
        self.session.status = "Removed the 2D peak mark.".to_owned();
    }

    pub fn clear_peaks_2d(&mut self, dataset: usize) {
        let Some(nmr) = self.doc.datasets.get(dataset).and_then(Dataset::as_nmr2d) else {
            return;
        };
        if nmr.peaks.marks.is_empty() {
            return;
        }
        let dataset_id = nmr.resource_id;
        let before = nmr.peaks.clone();
        self.execute_action(Action::set_peaks_2d(
            dataset_id,
            before,
            Peak2DSet::default(),
        ));
        self.session.ui.selected_peak_2d = None;
        self.session.status = "Cleared the 2D peak list.".to_owned();
    }

    /// Worker behind `SetPeaks2D`.
    pub fn set_peaks_2d(&mut self, dataset: usize, peaks: &Peak2DSet) {
        let Some(nmr) = self
            .doc
            .datasets
            .get_mut(dataset)
            .and_then(Dataset::as_nmr2d_mut)
        else {
            return;
        };
        nmr.peaks = peaks.clone();
        if self.session.ui.selected_peak_2d.is_some_and(|selection| {
            selection.dataset == nmr.resource_id && nmr.peaks.mark(selection.peak).is_none()
        }) {
            self.session.ui.selected_peak_2d = None;
        }
    }

    fn peaks_2d_edit_input(
        &self,
        dataset: usize,
    ) -> Result<(DatasetId, Peak2DSet, [f64; 2]), String> {
        let nmr = self
            .doc
            .datasets
            .get(dataset)
            .and_then(Dataset::as_nmr2d)
            .ok_or_else(|| "2D peak picking needs a true-2D NMR dataset.".to_owned())?;
        if let Some(reason) = nmr.symmetry_unavailable_reason() {
            return Err(reason.to_owned());
        }
        let tolerances = self
            .current_symmetry_audit(dataset)
            .map(|audit| [audit.result.f2_tolerance, audit.result.f1_tolerance])
            .unwrap_or([0.01, 0.01]);
        Ok((nmr.resource_id, nmr.peaks.clone(), tolerances))
    }
}

fn counterpart_for(entry: &SymmetryEntry, current: CandidateKey) -> Option<CandidateKey> {
    if entry.primary == current {
        entry
            .partner
            .or_else(|| entry.alternatives.first().copied())
    } else if entry.partner == Some(current) || entry.alternatives.contains(&current) {
        Some(entry.primary)
    } else {
        None
    }
}

fn candidate_point(candidate: &plotx_analysis::symmetry::SymmetryCandidate) -> Peak2DPoint {
    Peak2DPoint {
        f2: candidate.f2,
        f1: candidate.f1,
        intensity: candidate.intensity,
    }
}

fn sample_point(
    spectrum: &plotx_processing::Spectrum2D,
    mode: DisplayMode,
    f2: f64,
    f1: f64,
) -> Peak2DPoint {
    let col = nearest_axis_index(&spectrum.f2_ppm, f2);
    let row = nearest_axis_index(&spectrum.f1_ppm, f1);
    Peak2DPoint {
        f2,
        f1,
        intensity: spectrum
            .data
            .get(row.saturating_mul(spectrum.f2_size).saturating_add(col))
            .map_or(0.0, |value| mode.reduce(value)),
    }
}

fn nearest_axis_index(axis: &[f64], value: f64) -> usize {
    axis.iter()
        .enumerate()
        .filter(|(_, sample)| sample.is_finite())
        .min_by(|(_, left), (_, right)| (*left - value).abs().total_cmp(&(*right - value).abs()))
        .map_or(0, |(index, _)| index)
}

fn ordered((first, second): (f64, f64)) -> (f64, f64) {
    (first.min(second), first.max(second))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_ambiguous_alternative_points_back_to_the_primary() {
        let primary = CandidateKey { row: 2, col: 6 };
        let alternatives = [
            CandidateKey { row: 6, col: 2 },
            CandidateKey { row: 6, col: 3 },
        ];
        let entry = SymmetryEntry {
            primary,
            partner: None,
            alternatives: alternatives.to_vec(),
            status: PartnerStatus::Ambiguous,
            likelihood: ArtifactLikelihood::Medium,
            score: 0.4,
            reasons: Vec::new(),
            diagonal_support: [false; 2],
        };

        assert_eq!(counterpart_for(&entry, alternatives[0]), Some(primary));
        assert_eq!(counterpart_for(&entry, alternatives[1]), Some(primary));
    }
}
