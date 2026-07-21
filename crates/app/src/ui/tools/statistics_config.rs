//! The configuration half of the Statistics card: the question picker, the data
//! roles and options for each question, and the pre-run feasibility notice.
//! Split from `statistics.rs` to keep both within the source-size limit.

use egui::Ui;
use egui_phosphor::regular as icon;
use plotx_core::data::ColumnId;
use plotx_core::state::{
    CorrelationKind, Dataset, PlotxApp, StatDraft, StatPreflight, StatQuestion, TestDirection,
    VarianceModel,
};

pub(super) type ColumnChoice = (ColumnId, String);

/// The names of the active table's columns, for the role selectors.
pub(super) fn column_names(app: &PlotxApp, di: usize) -> Vec<ColumnChoice> {
    app.doc
        .datasets
        .get(di)
        .and_then(Dataset::as_table)
        .map(|table| table.numeric_analysis_columns().into_iter().collect())
        .unwrap_or_default()
}

/// Step 1: choose the analysis by the question it answers, not its name. A
/// compact combo keeps the whole workflow — roles, options, and saved results —
/// visible in the narrow card instead of a tall list of choices.
pub(super) fn question_picker(draft: &mut StatDraft, ui: &mut Ui) {
    ui.strong("What do you want to find out?");
    egui::ComboBox::from_id_salt("stat_question")
        .width(ui.available_width())
        .selected_text(draft.question.prompt())
        .show_ui(ui, |ui| {
            for question in StatQuestion::ALL {
                ui.selectable_value(&mut draft.question, question, question.prompt());
            }
        });
    ui.small(format!("Runs the {}.", draft.question.formal_name()));
}

/// Step 2 and 3: the data roles and the options specific to the question.
pub(super) fn roles_and_options(
    app: &PlotxApp,
    draft: &mut StatDraft,
    names: &[ColumnChoice],
    ui: &mut Ui,
) {
    match draft.question {
        StatQuestion::Summarize => {
            ui.label("Columns to summarize");
            multi_column(&mut draft.columns, names, ui);
        }
        StatQuestion::Normality => {
            ui.label("Columns to check");
            multi_column(&mut draft.columns, names, ui);
        }
        StatQuestion::CompareToValue => {
            column_combo(
                ui,
                "stat_one_sample_col",
                "Column",
                names,
                &mut draft.column_a,
            );
            ui.horizontal(|ui| {
                ui.label("Reference value");
                ui.add(egui::DragValue::new(&mut draft.reference_value).speed(0.1));
            });
            direction_selector(
                draft,
                &names_get(names, draft.column_a),
                "the reference",
                ui,
            );
            confidence_selector(draft, ui);
        }
        StatQuestion::CompareTwoGroups => {
            column_combo(ui, "stat_two_a", "Group A", names, &mut draft.column_a);
            column_combo(ui, "stat_two_b", "Group B", names, &mut draft.column_b);
            variance_selector(draft, ui);
            direction_selector(
                draft,
                &names_get(names, draft.column_a),
                &names_get(names, draft.column_b),
                ui,
            );
            confidence_selector(draft, ui);
        }
        StatQuestion::ComparePaired => {
            column_combo(ui, "stat_paired_a", "Column A", names, &mut draft.column_a);
            column_combo(ui, "stat_paired_b", "Column B", names, &mut draft.column_b);
            ui.small(
                "Each row pairs the two columns; the test uses their per-row difference A − B.",
            );
            direction_selector(
                draft,
                &names_get(names, draft.column_a),
                &names_get(names, draft.column_b),
                ui,
            );
            confidence_selector(draft, ui);
        }
        StatQuestion::Relationship => {
            column_combo(ui, "stat_corr_a", "Column A", names, &mut draft.column_a);
            column_combo(ui, "stat_corr_b", "Column B", names, &mut draft.column_b);
            correlation_selector(draft, ui);
        }
        StatQuestion::CompareManyGroups => {
            ui.label("Group columns to compare");
            multi_column(&mut draft.group_columns, names, ui);
            ui.checkbox(
                &mut draft.run_tukey,
                "Also compare each pair of groups (Tukey HSD)",
            );
            ui.small("Pairwise comparisons run regardless of the overall p-value.");
            confidence_selector(draft, ui);
        }
        StatQuestion::TwoFactors => two_factor_roles(app, draft, names, ui),
    }
}

fn two_factor_roles(app: &PlotxApp, draft: &mut StatDraft, names: &[ColumnChoice], ui: &mut Ui) {
    column_combo(
        ui,
        "stat_2w_value",
        "Value column",
        names,
        &mut draft.value_column,
    );
    column_combo(
        ui,
        "stat_2w_a",
        "Factor A",
        names,
        &mut draft.factor_a_column,
    );
    column_combo(
        ui,
        "stat_2w_b",
        "Factor B",
        names,
        &mut draft.factor_b_column,
    );
    if let Some((levels_a, levels_b)) = app.factor_levels_preview(draft) {
        ui.small(format!("Factor A levels: {}", level_preview(&levels_a)));
        ui.small(format!("Factor B levels: {}", level_preview(&levels_b)));
    }
    ui.small(
        "Factors must be numeric codes for groups. With one row per factor combination the \
         interaction cannot be separated from error; add replicate rows to test it.",
    );
}

fn level_preview(levels: &[String]) -> String {
    if levels.is_empty() {
        return "none detected".to_owned();
    }
    // Labels already embed the factor name (e.g. "dose = 1"); keep only the code
    // part after the last "= " for a compact preview.
    levels
        .iter()
        .map(|label| label.rsplit("= ").next().unwrap_or(label).to_owned())
        .collect::<Vec<_>>()
        .join(", ")
}

/// The pre-run notice: blocking role errors, non-blocking cautions, and the
/// explicit missing-value confirmation that gates the Run button.
pub(super) fn feasibility(draft: &mut StatDraft, preflight: &StatPreflight, ui: &mut Ui) {
    ui.add_space(4.0);
    if let Some(error) = &preflight.role_error {
        ui.colored_label(
            ui.visuals().error_fg_color,
            format!("{}  {error}", icon::WARNING),
        );
    }
    for warning in &preflight.warnings {
        ui.colored_label(ui.visuals().warn_fg_color, warning);
    }
    if let Some(note) = &preflight.missing_note {
        ui.colored_label(ui.visuals().warn_fg_color, note);
        ui.checkbox(
            &mut draft.exclusion_confirmed,
            "Exclude the affected cells or rows and continue",
        );
    } else {
        // Nothing to exclude: keep the flag from blocking a later clean run.
        draft.exclusion_confirmed = false;
    }
}

// ----- shared widgets ----------------------------------------------------

fn names_get(names: &[ColumnChoice], id: ColumnId) -> String {
    names
        .iter()
        .find(|(column, _)| *column == id)
        .map(|(_, name)| name.clone())
        .unwrap_or_else(|| format!("column {id}"))
}

fn column_combo(
    ui: &mut Ui,
    salt: &str,
    label: &str,
    names: &[ColumnChoice],
    selected: &mut ColumnId,
) {
    ui.horizontal(|ui| {
        ui.label(label);
        // A selection that went out of range (its column was deleted) shows a
        // placeholder rather than silently retargeting another column; the
        // preflight blocks the run until the user picks again.
        egui::ComboBox::from_id_salt(salt)
            .selected_text(
                names
                    .iter()
                    .find(|(column, _)| column == selected)
                    .map(|(_, name)| name.clone())
                    .unwrap_or_else(|| "Choose a column".to_owned()),
            )
            .show_ui(ui, |ui| {
                for (column, name) in names {
                    ui.selectable_value(selected, *column, name);
                }
            });
    });
}

fn multi_column(selected: &mut Vec<ColumnId>, names: &[ColumnChoice], ui: &mut Ui) {
    // Deleted columns must not linger as invisible, un-uncheckable selections.
    selected.retain(|id| names.iter().any(|(column, _)| column == id));
    for (column, name) in names {
        let mut on = selected.contains(column);
        if ui.checkbox(&mut on, name).changed() {
            if on {
                if !selected.contains(column) {
                    selected.push(*column);
                    selected.sort_unstable();
                }
            } else {
                selected.retain(|value| value != column);
            }
        }
    }
}

fn variance_selector(draft: &mut StatDraft, ui: &mut Ui) {
    ui.horizontal(|ui| {
        ui.label("Variances");
        if ui
            .selectable_label(draft.variance == VarianceModel::Welch, "Welch")
            .clicked()
        {
            draft.variance = VarianceModel::Welch;
        }
        if ui
            .selectable_label(draft.variance == VarianceModel::Equal, "Student")
            .clicked()
        {
            draft.variance = VarianceModel::Equal;
        }
    });
    ui.small(match draft.variance {
        VarianceModel::Welch => {
            "Welch does not assume the two groups have equal spread. Use it if unsure."
        }
        VarianceModel::Equal => "Student assumes both groups share the same spread and pools it.",
    });
}

fn correlation_selector(draft: &mut StatDraft, ui: &mut Ui) {
    ui.horizontal(|ui| {
        ui.label("Method");
        if ui
            .selectable_label(draft.correlation == CorrelationKind::Pearson, "Pearson")
            .clicked()
        {
            draft.correlation = CorrelationKind::Pearson;
        }
        if ui
            .selectable_label(draft.correlation == CorrelationKind::Spearman, "Spearman")
            .clicked()
        {
            draft.correlation = CorrelationKind::Spearman;
        }
    });
    ui.small(match draft.correlation {
        CorrelationKind::Pearson => "Pearson measures a straight-line relationship.",
        CorrelationKind::Spearman => {
            "Spearman ranks the values, capturing any consistent rise or fall."
        }
    });
}

/// A direction selector phrased with the actual role names, so a one-sided test
/// says exactly which way it looks (e.g. "A less than B").
fn direction_selector(draft: &mut StatDraft, left: &str, right: &str, ui: &mut Ui) {
    ui.label("Direction");
    ui.radio_value(
        &mut draft.direction,
        TestDirection::TwoSided,
        "Two-sided (either direction)",
    );
    ui.radio_value(
        &mut draft.direction,
        TestDirection::Less,
        format!("One-sided: {left} less than {right}"),
    );
    ui.radio_value(
        &mut draft.direction,
        TestDirection::Greater,
        format!("One-sided: {left} greater than {right}"),
    );
}

fn confidence_selector(draft: &mut StatDraft, ui: &mut Ui) {
    ui.horizontal(|ui| {
        ui.label("Confidence");
        let mut pct = draft.confidence * 100.0;
        if ui
            .add(
                egui::DragValue::new(&mut pct)
                    .speed(0.5)
                    .range(50.0..=99.9)
                    .suffix("%"),
            )
            .changed()
        {
            draft.confidence = pct / 100.0;
        }
    });
}
