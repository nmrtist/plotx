//! Interactive 1D peak/integral workers: the copy-on-write mutators behind the
//! Integrate and Pick-peak tools.

use super::*;

impl PlotxApp {
    /// Recompute true-2D volumes after a committed processing change and surface
    /// any failure without dropping the stored rectangles.
    pub fn recompute_integrals_2d_after_processing(&mut self, dataset: usize) {
        let error = self
            .doc
            .datasets
            .get_mut(dataset)
            .and_then(Dataset::as_nmr2d_mut)
            .and_then(|nmr2d| nmr2d.recompute_integrals().err());
        if let Some(error) = error {
            self.session.status = format!("Could not recompute 2D integrals: {error}");
        }
    }

    /// Worker behind `SetIntegrals2D`; snapshots already contain computed values.
    pub fn set_integrals_2d(&mut self, dataset: usize, integrals: &[crate::Integral2D]) {
        if let Some(n) = self
            .doc
            .datasets
            .get_mut(dataset)
            .and_then(Dataset::as_nmr2d_mut)
        {
            n.integrals = integrals.to_vec();
            n.renormalize_integrals();
        }
    }

    /// Copy-on-write mutation for true-2D integrals. Geometry and metadata edits
    /// recompute only at commit, then the complete result is stored for undo/redo.
    pub fn edit_integrals_2d(
        &mut self,
        dataset: usize,
        edit: impl FnOnce(&mut Vec<crate::Integral2D>, &mut u64),
    ) {
        let Some(n) = self.doc.datasets.get(dataset).and_then(Dataset::as_nmr2d) else {
            return;
        };
        let dataset_id = n.resource_id;
        let before = n.integrals.clone();
        let mut after = before.clone();
        let mut next_id = n.next_integral_id;
        edit(&mut after, &mut next_id);
        let needs_recompute = after.iter().any(|candidate| {
            before
                .iter()
                .find(|original| original.id == candidate.id)
                .is_none_or(|original| {
                    original.f2 != candidate.f2
                        || original.f1 != candidate.f1
                        || original.baseline != candidate.baseline
                        || original.method != candidate.method
                })
        });
        if let Some(n) = self
            .doc
            .datasets
            .get_mut(dataset)
            .and_then(Dataset::as_nmr2d_mut)
        {
            n.next_integral_id = next_id;
            n.integrals = after;
            if needs_recompute {
                if let Err(error) = n.recompute_integrals() {
                    self.session.status = format!("Could not recompute 2D integrals: {error}");
                }
            } else {
                n.renormalize_integrals();
            }
            after = n.integrals.clone();
        }
        self.execute_action(Action::set_integrals_2d(dataset_id, before, after));
    }

    pub fn set_integral_2d_reference(&mut self, dataset: usize, id: u64, value: f64) {
        if !value.is_finite() {
            return;
        }
        self.edit_integrals_2d(dataset, |integrals, _| {
            for integral in integrals {
                integral.reference_value = (integral.id == id).then_some(value);
            }
        });
    }

    pub fn delete_integral_2d(&mut self, dataset: usize, id: u64) {
        self.edit_integrals_2d(dataset, |integrals, _| {
            integrals.retain(|integral| integral.id != id);
        });
    }

    /// Worker behind `SetIntegrals`: install the interactive integral bands. The
    /// snapshot already carries recomputed areas, so apply/undo just assign.
    pub fn set_integrals(&mut self, dataset: usize, integrals: &[crate::IntegralResult]) {
        if let Some(n) = self.doc.datasets[dataset].as_nmr_mut() {
            n.integrals = integrals.to_vec();
        }
        self.sync_integral_curves_for(dataset);
    }

    /// Refresh only the persistent integral description layer on plots whose
    /// primary dataset is `dataset`. Overlay-only datasets never contribute
    /// integral curves.
    pub fn sync_integral_curves_for(&mut self, dataset: usize) {
        // Resolve the dataset once: the index can be stale (e.g. a cancelled
        // integral drag after an import was undone), so a miss must return, not
        // index-panic on the next line.
        let Some(ds) = self.doc.datasets.get(dataset) else {
            return;
        };
        let dataset_id = ds.resource_id();
        let curves = ds
            .as_nmr()
            .map(NmrDataset::integral_curves)
            .unwrap_or_default();
        for canvas in &mut self.doc.canvases {
            for object in &mut canvas.objects {
                let Some(plot) = object.plot_mut() else {
                    continue;
                };
                if plot.binding.primary_dataset() == Some(dataset_id)
                    && plot.binding.primary_visible()
                {
                    plot.figure.integral_curves.clone_from(&curves);
                } else if plot.binding.primary_dataset() == Some(dataset_id) {
                    plot.figure.integral_curves.clear();
                }
            }
        }
    }

    /// Snapshot the integrals, let `edit` mutate a working copy (handing out fresh
    /// ids), refresh areas/normalization against the live spectrum, then commit one
    /// undoable step.
    pub fn edit_integrals(
        &mut self,
        dataset: usize,
        edit: impl FnOnce(&mut Vec<crate::IntegralResult>, &mut u64),
    ) {
        let Some(n) = self.doc.datasets.get(dataset).and_then(Dataset::as_nmr) else {
            return;
        };
        let dataset_id = n.resource_id;
        let before = n.integrals.clone();
        let mut after = before.clone();
        let mut next_id = n.next_integral_id;
        edit(&mut after, &mut next_id);
        if let Some(n) = self
            .doc
            .datasets
            .get_mut(dataset)
            .and_then(Dataset::as_nmr_mut)
        {
            n.next_integral_id = next_id;
            n.integrals = after;
            n.recompute_integrals();
            after = n.integrals.clone();
        }
        self.execute_action(Action::set_integrals(dataset_id, before, after));
    }

    /// Use one integral as the normalization reference at a user-selected value.
    pub fn set_integral_reference(&mut self, dataset: usize, id: u64, value: f64) {
        if !value.is_finite() {
            return;
        }
        self.edit_integrals(dataset, |integrals, _| {
            for integ in integrals.iter_mut() {
                integ.reference_value = (integ.id == id).then_some(value);
            }
        });
    }

    pub fn delete_integral(&mut self, dataset: usize, id: u64) {
        self.edit_integrals(dataset, |integrals, _| {
            integrals.retain(|integ| integ.id != id)
        });
        if self.session.ui.selected_integral == Some(id) {
            self.session.ui.selected_integral = None;
        }
    }

    /// Worker behind `SetPeaks`: install the peak set and rebuild so labels repaint.
    pub fn set_peaks(&mut self, dataset: usize, peaks: &PeakSet) {
        if let Some(p) = self
            .doc
            .datasets
            .get_mut(dataset)
            .and_then(Dataset::peaks_mut)
        {
            *p = peaks.clone();
        }
        self.rebuild_canvases_for(dataset);
    }

    /// Snapshot the peak set, let `edit` mutate a working copy, then commit one
    /// undoable step. No-ops on domains without a peak set.
    pub fn edit_peaks(&mut self, dataset: usize, edit: impl FnOnce(&mut PeakSet)) {
        let Some((dataset_id, before)) = self
            .doc
            .datasets
            .get(dataset)
            .and_then(|value| Some((value.resource_id(), value.peaks().cloned()?)))
        else {
            return;
        };
        let mut after = before.clone();
        edit(&mut after);
        self.execute_action(Action::set_peaks(dataset_id, before, after));
    }

    /// Place a hand-picked peak, snapping the clicked `x` to the nearest local
    /// maximum of the displayed trace.
    pub fn add_manual_peak(
        &mut self,
        dataset: usize,
        x: f64,
        column: Option<plotx_data::ColumnId>,
    ) {
        let column_id = table_peak_column(&self.doc.datasets, dataset, column);
        let Some(trace) = self
            .doc
            .datasets
            .get(dataset)
            .and_then(|d| d.displayed_trace(column))
        else {
            self.session.status = "Peaks are available for 1D traces only.".into();
            return;
        };
        let (px, py) = trace.snap(x);
        self.edit_peaks(dataset, |peaks| {
            peaks.column = column_id;
            let id = peaks.next_id();
            peaks.marks.push(PeakMark {
                id,
                x: px,
                y: py,
                origin: PeakOrigin::Manual,
                label: None,
            });
        });
        self.session.status = format!("Placed a peak at {px:.3}.");
    }

    /// Pick every peak inside an x-window and add them as hand-placed marks, using
    /// noise estimated from the window so a noisy baseline outside it is ignored.
    pub fn add_peaks_in_range(
        &mut self,
        dataset: usize,
        x_a: f64,
        x_b: f64,
        column: Option<plotx_data::ColumnId>,
    ) {
        let column_id = table_peak_column(&self.doc.datasets, dataset, column);
        let Some(trace) = self
            .doc
            .datasets
            .get(dataset)
            .and_then(|d| d.displayed_trace(column))
        else {
            return;
        };
        let found = PeakSet::pick_in_range(&trace, x_a, x_b);
        if found.is_empty() {
            self.session.status = "No peaks found in that range.".into();
            return;
        }
        let tol = trace.tol();
        let mut added = 0;
        self.edit_peaks(dataset, |peaks| {
            peaks.column = column_id;
            for (x, y) in found {
                if peaks.marks.iter().any(|m| (m.x - x).abs() <= tol) {
                    continue;
                }
                let id = peaks.next_id();
                peaks.marks.push(PeakMark {
                    id,
                    x,
                    y,
                    origin: PeakOrigin::Manual,
                    label: None,
                });
                added += 1;
            }
        });
        self.session.status = format!("Picked {added} peak(s) in range.");
    }

    pub fn remove_peak(&mut self, dataset: usize, id: u64) {
        self.edit_peaks(dataset, |peaks| peaks.remove_mark(id));
        if self.session.ui.selected_peak == Some(id) {
            self.session.ui.selected_peak = None;
        }
    }

    /// Run detection at `threshold` (storing it), replacing the detected marks and
    /// keeping the hand-placed ones. The one-shot behind the Detect button and the
    /// threshold-line release.
    pub fn run_detection(
        &mut self,
        dataset: usize,
        threshold: Option<f64>,
        column: Option<plotx_data::ColumnId>,
    ) {
        let column_id = table_peak_column(&self.doc.datasets, dataset, column);
        let Some(trace) = self
            .doc
            .datasets
            .get(dataset)
            .and_then(|d| d.displayed_trace(column))
        else {
            return;
        };
        self.edit_peaks(dataset, |peaks| {
            peaks.column = column_id;
            peaks.detector.threshold = threshold;
            peaks.redetect(&trace);
        });
        let count = self
            .doc
            .datasets
            .get(dataset)
            .and_then(Dataset::peaks)
            .map(|p| p.marks.len())
            .unwrap_or(0);
        self.session.status = format!("Detected peaks — {count} total.");
    }

    pub fn set_peak_max_count(
        &mut self,
        dataset: usize,
        max_count: Option<usize>,
        column: Option<plotx_data::ColumnId>,
    ) {
        let column_id = table_peak_column(&self.doc.datasets, dataset, column);
        let Some(trace) = self
            .doc
            .datasets
            .get(dataset)
            .and_then(|d| d.displayed_trace(column))
        else {
            return;
        };
        self.edit_peaks(dataset, |peaks| {
            peaks.column = column_id;
            peaks.detector.max_count = max_count;
            peaks.redetect(&trace);
        });
    }

    pub fn relabel_peak(&mut self, dataset: usize, id: u64, label: Option<String>) {
        self.edit_peaks(dataset, |peaks| {
            if let Some(mark) = peaks.marks.iter_mut().find(|m| m.id == id) {
                mark.label = label;
            }
        });
    }

    pub fn clear_peaks(&mut self, dataset: usize) {
        self.edit_peaks(dataset, |peaks| peaks.marks.clear());
        self.session.ui.selected_peak = None;
        self.session.status = "Cleared peaks.".into();
    }
}

fn table_peak_column(
    datasets: &[Dataset],
    dataset: usize,
    column: Option<plotx_data::ColumnId>,
) -> Option<plotx_data::ColumnId> {
    datasets
        .get(dataset)
        .and_then(Dataset::as_table)
        .and_then(|table| {
            column
                .filter(|column| {
                    table
                        .series_bindings
                        .iter()
                        .any(|binding| binding.value_column == *column)
                })
                .or_else(|| {
                    table
                        .series_bindings
                        .first()
                        .map(|binding| binding.value_column)
                })
        })
}
