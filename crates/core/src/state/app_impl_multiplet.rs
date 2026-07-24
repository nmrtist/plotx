//! Multiplet analysis orchestration: classify fitted components (or peak
//! marks) within a ppm window through `plotx_analysis::multiplet`, store the
//! result on the dataset and materialize a summary table, all as one undoable
//! step.

use super::*;
use plotx_analysis::multiplet::{MultipletPeak, analyze};

const MAX_J_HZ: f64 = 20.0;

impl PlotxApp {
    /// Worker behind `SetMultiplets`: install the stored multiplets.
    pub fn set_multiplets(&mut self, dataset: usize, multiplets: &[StoredMultiplet]) {
        if let Some(stored) = self
            .doc
            .datasets
            .get_mut(dataset)
            .and_then(Dataset::multiplets_mut)
        {
            *stored = multiplets.to_vec();
        }
        self.rebuild_canvases_for(dataset);
    }

    /// Classify the peaks inside `[lo, hi]` ppm into multiplets, preferring
    /// fitted lineshape components over raw peak marks. Pure analysis: nothing
    /// is stored until `apply_multiplet_analysis`.
    pub fn analyze_multiplets(
        &self,
        dataset: usize,
        lo: f64,
        hi: f64,
    ) -> Result<Vec<StoredMultiplet>, String> {
        let (lo, hi) = if lo <= hi { (lo, hi) } else { (hi, lo) };
        let Some(n) = self.doc.datasets.get(dataset).and_then(Dataset::as_nmr) else {
            return Err("Multiplet analysis needs a 1D NMR dataset.".to_owned());
        };
        let obs = n.data.observe_freq_mhz;

        let mut peaks: Vec<MultipletPeak> = Vec::new();
        let mut areas: Vec<f64> = Vec::new();
        for fit in self.doc.datasets[dataset].line_fits() {
            for p in &fit.peaks {
                if p.position >= lo && p.position <= hi {
                    peaks.push(MultipletPeak {
                        position_hz: p.position * obs,
                        intensity: p.height,
                        position_sigma_hz: p.position_sigma.map(|s| s * obs),
                        fwhm_hz: p.fwhm * obs,
                    });
                    areas.push(p.area);
                }
            }
        }
        if peaks.is_empty() {
            let marks = self.doc.datasets[dataset]
                .peaks()
                .map(|p| p.resolve())
                .unwrap_or_default();
            for m in marks.iter().filter(|m| m.x >= lo && m.x <= hi) {
                peaks.push(MultipletPeak {
                    position_hz: m.x * obs,
                    intensity: m.y,
                    position_sigma_hz: None,
                    // 1.0 Hz: typical 1H linewidth stand-in for unfitted marks.
                    fwhm_hz: 1.0,
                });
                // Mark heights are not areas; report 0.0 and let the
                // descriptor stay authoritative.
                areas.push(0.0);
            }
        }
        if peaks.is_empty() {
            return Err(
                "No fitted peaks or peak marks in the range; fit or mark peaks first.".to_owned(),
            );
        }

        let stored = analyze(&peaks, MAX_J_HZ)
            .into_iter()
            .enumerate()
            .map(|(id, r)| {
                let mut peak_ppm: Vec<f64> = r
                    .peak_indices
                    .iter()
                    .map(|&i| peaks[i].position_hz / obs)
                    .collect();
                peak_ppm.sort_by(|a, b| b.total_cmp(a));
                StoredMultiplet {
                    id: id as u64,
                    lo: peak_ppm.last().copied().unwrap_or(lo),
                    hi: peak_ppm.first().copied().unwrap_or(hi),
                    center_ppm: r.center_hz / obs,
                    pattern: r.pattern.into(),
                    j_values: r.j_values.iter().map(Into::into).collect(),
                    area: r.peak_indices.iter().map(|&i| areas[i]).sum(),
                    peak_ppm,
                }
            })
            .collect();
        Ok(stored)
    }

    /// Commit analyzed multiplets: store them and materialize their summary
    /// table as one undoable step. The undo "before" snapshot is the dataset's
    /// current list, so a result landing after mid-flight edits undoes cleanly.
    pub fn apply_multiplet_analysis(
        &mut self,
        dataset: usize,
        mut multiplets: Vec<StoredMultiplet>,
    ) {
        if multiplets.is_empty() || self.doc.datasets.get(dataset).is_none() {
            return;
        }
        let Some(next_id) = self.doc.datasets[dataset].next_multiplet_id_mut() else {
            return;
        };
        for m in &mut multiplets {
            m.id = *next_id;
            *next_id += 1;
        }
        let before = self.doc.datasets[dataset].multiplets().to_vec();
        let mut after = before.clone();
        after.extend(multiplets.iter().cloned());

        let mut tds = multiplet_summary_table(&multiplets);
        tds.lineage = Some(DatasetLineage::new(
            DerivationKind::MultipletTable,
            [self.doc.datasets[dataset].resource_id()],
        ));
        tds.name = Some(format!(
            "{} — multiplets",
            self.doc.datasets[dataset].display_name()
        ));
        tds.board_pos = super::app_impl_analysis::next_sheet_pos_after_new_canvas(self);
        let insert = Action::insert_dataset_with_default_canvas(
            self,
            Dataset::Table(Box::new(tds)),
            format!("Canvas {} — Multiplets", self.doc.canvases.len() + 1),
            DEFAULT_CANVAS_SIZE_MM,
        );
        self.execute_action(Action::Composite(vec![
            Action::set_multiplets(dataset, before, after),
            insert,
        ]));
        self.session.status = format!("Classified {} multiplet(s).", multiplets.len());
    }

    /// Delete one stored multiplet as an undoable step.
    pub fn remove_multiplet(&mut self, dataset: usize, id: u64) {
        let Some(before) = self
            .doc
            .datasets
            .get(dataset)
            .map(|d| d.multiplets().to_vec())
        else {
            return;
        };
        let after: Vec<StoredMultiplet> = before.iter().filter(|m| m.id != id).cloned().collect();
        self.execute_action(Action::set_multiplets(dataset, before, after));
    }
}

/// One row per multiplet. Missing J values and uncertainties stay null; the
/// descriptor remains the authoritative text form.
fn multiplet_summary_table(ms: &[StoredMultiplet]) -> TableDataset {
    let n = ms.len();
    let column = |name: &str,
                  unit: &str,
                  values: Vec<Option<f64>>,
                  uncertainty: Option<Vec<Option<f64>>>| FloatSeries {
        name: name.to_owned(),
        unit: unit.to_owned(),
        values,
        uncertainty,
        fit: None,
    };
    let j = |k: usize| ms.iter().map(move |m| m.j_values.get(k));
    let series = vec![
        column(
            "center (ppm)",
            "ppm",
            ms.iter().map(|m| Some(m.center_ppm)).collect(),
            None,
        ),
        column(
            "J1 (Hz)",
            "Hz",
            j(0).map(|value| value.map(|j| j.hz)).collect(),
            Some(j(0).map(|value| value.and_then(|j| j.sigma_hz)).collect()),
        ),
        column(
            "J2 (Hz)",
            "Hz",
            j(1).map(|value| value.map(|j| j.hz)).collect(),
            Some(j(1).map(|value| value.and_then(|j| j.sigma_hz)).collect()),
        ),
        column("area", "", ms.iter().map(|m| Some(m.area)).collect(), None),
        column(
            "peaks",
            "1",
            ms.iter().map(|m| Some(m.peak_ppm.len() as f64)).collect(),
            None,
        ),
    ];
    materialized_float_series_table(
        (
            "multiplet".into(),
            "".into(),
            (1..=n).map(|i| Some(i as f64)).collect(),
        ),
        series,
        "plotx.analysis.multiplet-table.v1",
    )
    .expect("stored multiplet values form aligned typed columns")
}
