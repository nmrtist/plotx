use egui::{Button, Ui};
use egui_phosphor::regular as icon;
use plotx_analysis::symmetry::{ArtifactLikelihood, CandidateKey, PartnerStatus, SymmetryEntry};
use plotx_core::state::{
    Dataset, Peak2DId, Peak2DReview, Peak2DSelection, PlotxApp, SymmetryAuditFilter, Tool,
};

pub(super) fn symmetry_group(app: &mut PlotxApp, dataset: usize, ui: &mut Ui) {
    ui.separator();
    ui.strong("Symmetry review");

    let Some(nmr) = app.doc.datasets.get(dataset).and_then(Dataset::as_nmr2d) else {
        return;
    };
    if let Some(reason) = nmr.symmetry_unavailable_reason() {
        ui.small(reason);
        return;
    }

    if app.session.tool == Tool::Symmetry {
        ui.small(
            "Move to compare A with A' · hold Shift to snap · click to pin · click a stored mark \
             to select it · Delete removes the selected mark.",
        );
    }
    ui.checkbox(&mut app.session.ui.symmetry_snap, "Snap automatically")
        .on_hover_text("Snap A and A' to detected local extrema without holding Shift.");

    audit_controls(app, dataset, ui);
    pinned_controls(app, dataset, ui);
    peak_controls(app, dataset, ui);
}

fn audit_controls(app: &mut PlotxApp, dataset: usize, ui: &mut Ui) {
    if let Some((_, elapsed)) = app.symmetry_audit_progress() {
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label(format!("Checking symmetry… {:.1}s", elapsed.as_secs_f32()));
        });
        return;
    }

    if app.current_symmetry_audit(dataset).is_none() {
        if ui
            .button("Run symmetry audit")
            .on_hover_text("Detect cross peaks and compare their cross-diagonal partners.")
            .clicked()
            && let Err(error) = app.retry_symmetry_audit(dataset)
        {
            app.session.status = error;
        }
        return;
    }

    let counts = app
        .current_symmetry_audit(dataset)
        .expect("checked above")
        .result
        .counts();
    ui.small(format!(
        "{} paired · {} unpaired · {} ambiguous · {} review suggestions",
        counts.matched, counts.missing, counts.ambiguous, counts.high_likelihood
    ));
    ui.horizontal_wrapped(|ui| {
        if ui
            .add_enabled(counts.matched > 0, Button::new("Pick paired"))
            .on_disabled_hover_text("No matched cross-diagonal pairs")
            .clicked()
            && let Err(error) = app.accept_all_matched_symmetry_pairs(dataset)
        {
            app.session.status = error;
        }
        if ui
            .add_enabled(counts.high_likelihood > 0, Button::new("Mark suggestions"))
            .on_hover_text(
                "Add the high-likelihood candidates as review marks, not definitive artifacts.",
            )
            .on_disabled_hover_text("No high-likelihood suggestions")
            .clicked()
            && let Err(error) = app.mark_high_likelihood_artifacts(dataset)
        {
            app.session.status = error;
        }
        if ui
            .small_button(icon::ARROW_CLOCKWISE)
            .on_hover_text("Run again")
            .clicked()
            && let Err(error) = app.retry_symmetry_audit(dataset)
        {
            app.session.status = error;
        }
    });

    let mut filter = app.session.ui.symmetry_filter;
    egui::ComboBox::from_label("Show")
        .selected_text(filter.label())
        .show_ui(ui, |ui| {
            for choice in [
                SymmetryAuditFilter::All,
                SymmetryAuditFilter::Matched,
                SymmetryAuditFilter::Unpaired,
                SymmetryAuditFilter::Ambiguous,
                SymmetryAuditFilter::Suggestions,
            ] {
                ui.selectable_value(&mut filter, choice, choice.label());
            }
        });
    app.session.ui.symmetry_filter = filter;

    let rows = audit_rows(app, dataset, filter);
    let pinned_key = app
        .session
        .ui
        .symmetry_pin
        .as_ref()
        .and_then(|pin| pin.current_key);
    let mut pin = None;
    egui::ScrollArea::vertical()
        .id_salt(("symmetry_audit_rows", dataset))
        .max_height(210.0)
        .show(ui, |ui| {
            for row in &rows {
                let response = ui.selectable_label(
                    pinned_key == Some(row.key),
                    format!(
                        "{}  {:.3}, {:.3}  · S/N {:.1}",
                        row.status, row.f2, row.f1, row.snr
                    ),
                );
                let response = if row.reasons.is_empty() {
                    response
                } else {
                    response.on_hover_text(row.reasons.join(" · "))
                };
                if response.clicked() {
                    pin = Some(row.key);
                }
            }
        });
    if let Some(key) = pin {
        app.pin_symmetry_entry(dataset, key);
    }
}

fn pinned_controls(app: &mut PlotxApp, dataset: usize, ui: &mut Ui) {
    let dataset_id = app.doc.datasets[dataset].resource_id();
    let Some(pin) = app
        .session
        .ui
        .symmetry_pin
        .as_ref()
        .filter(|pin| pin.dataset == dataset_id)
        .cloned()
    else {
        return;
    };
    ui.separator();
    ui.small(format!(
        "Pinned A {:.3}, {:.3} -> A' {:.3}, {:.3}",
        pin.current.f2, pin.current.f1, pin.partner_target[0], pin.partner_target[1]
    ));
    ui.horizontal(|ui| {
        if ui
            .add_enabled(pin.partner.is_some(), Button::new("Pick both peaks"))
            .on_disabled_hover_text("No detected partner is available")
            .clicked()
            && let Err(error) = app.accept_symmetry_pair(dataset, &pin)
        {
            app.session.status = error;
        }
        if ui.small_button("Clear pin").clicked() {
            app.session.ui.symmetry_pin = None;
        }
    });
    if ui
        .button("Mark possible artifact")
        .on_hover_text("Record a review mark; this does not classify the signal automatically.")
        .clicked()
        && let Err(error) = app.mark_pinned_possible_artifact(dataset, &pin)
    {
        app.session.status = error;
    }
}

fn peak_controls(app: &mut PlotxApp, dataset: usize, ui: &mut Ui) {
    let Some(nmr) = app.doc.datasets.get(dataset).and_then(Dataset::as_nmr2d) else {
        return;
    };
    if nmr.peaks.marks.is_empty() {
        return;
    }
    let count = nmr.peaks.marks.len();
    let marks = nmr.peaks.marks.clone();
    let dataset_id = nmr.resource_id;
    let selected = app
        .session
        .ui
        .selected_peak_2d
        .and_then(|selection| selection.in_dataset(dataset_id));
    ui.separator();
    ui.horizontal(|ui| {
        ui.label(format!("Stored cross peaks: {count}"));
        if ui.small_button("Clear").clicked() {
            app.clear_peaks_2d(dataset);
        }
    });

    let mut select = None;
    egui::ScrollArea::vertical()
        .id_salt(("stored_cross_peaks", dataset))
        .max_height(140.0)
        .show(ui, |ui| {
            for mark in marks {
                let mate = mark
                    .partner
                    .map(|partner| format!(" ↔ {}", partner.get()))
                    .unwrap_or_default();
                if ui
                    .selectable_label(
                        selected == Some(mark.id),
                        format!(
                            "#{:02}  {:.3}, {:.3}{mate} · {}",
                            mark.id.get(),
                            mark.f2,
                            mark.f1,
                            mark.review.label()
                        ),
                    )
                    .clicked()
                {
                    select = Some(mark.id);
                }
            }
        });
    if let Some(id) = select {
        app.session.ui.selected_peak_2d = Some(Peak2DSelection::new(dataset_id, id));
        let mark = app.doc.datasets[dataset]
            .as_nmr2d()
            .and_then(|nmr| nmr.peaks.mark(id))
            .cloned();
        if let Some(mark) = mark {
            app.session.ui.symmetry_pin = app.symmetry_reading(dataset, mark.f2, mark.f1, true);
        }
    }
    if let Some(id) = app
        .session
        .ui
        .selected_peak_2d
        .and_then(|selection| selection.in_dataset(dataset_id))
    {
        review_controls(app, dataset, id, ui);
    }
}

fn review_controls(app: &mut PlotxApp, dataset: usize, id: Peak2DId, ui: &mut Ui) {
    ui.horizontal_wrapped(|ui| {
        for (label, review) in [
            ("Confirm", Peak2DReview::Confirmed),
            ("Uncertain", Peak2DReview::Uncertain),
            ("Artifact?", Peak2DReview::PossibleArtifact),
        ] {
            if ui.small_button(label).clicked() {
                app.review_peak_2d(dataset, id, review);
            }
        }
        if ui.small_button(icon::TRASH).clicked() {
            app.remove_peak_2d(dataset, id);
        }
    });
}

struct AuditRow {
    key: CandidateKey,
    status: &'static str,
    f2: f64,
    f1: f64,
    snr: f64,
    reasons: Vec<&'static str>,
}

fn audit_rows(app: &PlotxApp, dataset: usize, filter: SymmetryAuditFilter) -> Vec<AuditRow> {
    let Some(audit) = app.current_symmetry_audit(dataset) else {
        return Vec::new();
    };
    audit
        .result
        .entries
        .iter()
        .filter(|entry| filter_entry(entry, filter))
        .filter_map(|entry| {
            let candidate = audit.result.candidate(entry.primary)?;
            Some(AuditRow {
                key: entry.primary,
                status: entry_status(entry),
                f2: candidate.f2,
                f1: candidate.f1,
                snr: candidate.signal_to_noise,
                reasons: entry.reasons.iter().map(|reason| reason.label()).collect(),
            })
        })
        .collect()
}

fn filter_entry(entry: &SymmetryEntry, filter: SymmetryAuditFilter) -> bool {
    match filter {
        SymmetryAuditFilter::All => true,
        SymmetryAuditFilter::Matched => entry.status == PartnerStatus::Matched,
        SymmetryAuditFilter::Unpaired => {
            matches!(
                entry.status,
                PartnerStatus::Missing | PartnerStatus::OutsideRange
            )
        }
        SymmetryAuditFilter::Ambiguous => entry.status == PartnerStatus::Ambiguous,
        SymmetryAuditFilter::Suggestions => entry.likelihood == ArtifactLikelihood::High,
    }
}

fn entry_status(entry: &SymmetryEntry) -> &'static str {
    match (entry.status, entry.likelihood) {
        (_, ArtifactLikelihood::High) => "Review",
        (PartnerStatus::Matched, _) => "Paired",
        (PartnerStatus::Ambiguous, _) => "Ambiguous",
        (PartnerStatus::Missing, _) => "Unpaired",
        (PartnerStatus::OutsideRange, _) => "Out of range",
    }
}
