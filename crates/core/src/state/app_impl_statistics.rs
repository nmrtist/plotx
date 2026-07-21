//! Statistics orchestration for a data table: turn the configuration card's
//! draft into finite samples (handling missing values explicitly), run the
//! requested test from `plotx_analysis::statistics`, and store the labelled
//! result on the source table as one undoable step. Mirrors the line-fit flow.

use super::statistics_prepare::{
    build_selection_snapshot, extract_column, extract_factorial, extract_pair, factor_label,
    friendly, friendly_factorial, friendly_groups, friendly_two, join_names, missing_cells_note,
    missing_rows_note, variance_test_name,
};
use super::table_numeric::NumericAnalysisTable;
use super::*;
use plotx_analysis::statistics::{
    describe, independent_t_test, one_sample_t_test, one_way_anova, paired_t_test, pearson,
    shapiro_wilk, spearman, tukey_hsd, two_way_anova,
};
use statistics_report::{fmt_level, fmt_num, headline, outcome_table};

/// A factor column whose distinct value count is at or above this looks more
/// like a continuous measurement than a small set of categories, so the card
/// warns before running a two-way ANOVA against it.
const CONTINUOUS_LEVEL_HINT: usize = 12;

impl PlotxApp {
    /// Worker behind `SetTableStatistics`: replace the stored analyses. Results
    /// do not affect the figure, so no canvas rebuild is needed.
    pub fn set_table_statistics(&mut self, dataset: usize, analyses: &[StatAnalysis]) {
        if let Some(table) = self
            .doc
            .datasets
            .get_mut(dataset)
            .and_then(Dataset::as_table_mut)
        {
            table.statistics = analyses.to_vec();
        }
    }

    /// Check a draft before the Run button is shown: report role errors that
    /// block the run, missing-value handling that must be confirmed, and
    /// non-blocking cautions. Never mutates state.
    pub fn statistics_preflight(&self, draft: &StatDraft) -> StatPreflight {
        let Some(dataset) = self
            .doc
            .datasets
            .get(draft.dataset)
            .and_then(Dataset::as_table)
        else {
            return StatPreflight {
                role_error: Some("Select a data table to run statistics.".to_owned()),
                ..StatPreflight::default()
            };
        };
        let table = match dataset.numeric_analysis_view() {
            Ok(table) => table,
            Err(error) => {
                return StatPreflight {
                    role_error: Some(error),
                    ..StatPreflight::default()
                };
            }
        };
        let draft = match table.resolve_draft(draft) {
            Ok(draft) => draft,
            Err(error) => {
                return StatPreflight {
                    role_error: Some(error),
                    ..StatPreflight::default()
                };
            }
        };
        match draft.question {
            StatQuestion::Summarize | StatQuestion::Normality => {
                self.preflight_columns(&table, &draft)
            }
            StatQuestion::CompareToValue => self.preflight_one_sample(&table, &draft),
            StatQuestion::CompareTwoGroups => self.preflight_two_columns(&table, &draft, false),
            StatQuestion::ComparePaired => self.preflight_two_columns(&table, &draft, true),
            StatQuestion::Relationship => self.preflight_two_columns(&table, &draft, true),
            StatQuestion::CompareManyGroups => self.preflight_groups(&table, &draft),
            StatQuestion::TwoFactors => self.preflight_two_way(&table, &draft),
        }
    }

    /// The factor levels a two-way design would detect from the current draft,
    /// so the card can show the user which levels its numeric factor columns
    /// resolved to and warn if a continuous column was chosen by mistake.
    pub fn factor_levels_preview(&self, draft: &StatDraft) -> Option<(Vec<String>, Vec<String>)> {
        let dataset = self
            .doc
            .datasets
            .get(draft.dataset)
            .and_then(Dataset::as_table)?;
        let table = dataset.numeric_analysis_view().ok()?;
        let draft = table.resolve_draft(draft).ok()?;
        let prepared = extract_factorial(&table, &draft);
        Some((prepared.levels_a, prepared.levels_b))
    }

    /// Run the configured analysis, store the labelled result, and set a status
    /// line. Returns a user-facing error when the data cannot support the test;
    /// the caller surfaces it (the card shows it, the command path sets status).
    pub fn run_statistics(&mut self, draft: &StatDraft) -> Result<(), String> {
        let (title, configuration, data_note, outcome, selection) = {
            let dataset = self
                .doc
                .datasets
                .get(draft.dataset)
                .and_then(Dataset::as_table)
                .ok_or("Select a data table to run statistics.")?;
            let table = dataset.numeric_analysis_view()?;
            let resolved = table.resolve_draft(draft)?;
            let built = match draft.question {
                StatQuestion::Summarize => Self::build_descriptive(&table, &resolved)?,
                StatQuestion::Normality => Self::build_normality(&table, &resolved)?,
                StatQuestion::CompareToValue => Self::build_one_sample(&table, &resolved)?,
                StatQuestion::CompareTwoGroups => Self::build_independent(&table, &resolved)?,
                StatQuestion::ComparePaired => Self::build_paired(&table, &resolved)?,
                StatQuestion::Relationship => Self::build_correlation(&table, &resolved)?,
                StatQuestion::CompareManyGroups => Self::build_one_way(&table, &resolved)?,
                StatQuestion::TwoFactors => Self::build_two_way(&table, &resolved)?,
            };
            let selection = build_selection_snapshot(&table, &resolved)?;
            (built.0, built.1, built.2, built.3, selection)
        };
        let id = self.next_statistics_id(draft.dataset);
        let analysis = StatAnalysis {
            id,
            question: draft.question,
            title,
            configuration,
            data_note,
            selection,
            outcome,
        };
        let status = headline(&analysis);
        let before = self.stored_statistics(draft.dataset);
        let mut after = before.clone();
        after.push(analysis);
        self.execute_action(Action::SetTableStatistics {
            dataset: draft.dataset,
            before,
            after,
        });
        self.session.status = status;
        Ok(())
    }

    /// Delete one stored analysis as an undoable step.
    pub fn remove_statistics(&mut self, dataset: usize, id: u64) {
        let before = self.stored_statistics(dataset);
        if before.is_empty() {
            return;
        }
        let after: Vec<StatAnalysis> = before.iter().filter(|a| a.id != id).cloned().collect();
        self.execute_action(Action::SetTableStatistics {
            dataset,
            before,
            after,
        });
    }

    /// Materialise one stored analysis as a derived table and canvas on request.
    pub fn add_statistics_result_to_board(
        &mut self,
        dataset: usize,
        id: u64,
    ) -> Result<(), String> {
        let source = self
            .doc
            .datasets
            .get(dataset)
            .and_then(Dataset::as_table)
            .ok_or("The source table is no longer available.")?;
        let analysis = source
            .statistics
            .iter()
            .find(|analysis| analysis.id == id)
            .cloned()
            .ok_or("The analysis result is no longer available.")?;
        let source_name = source
            .name
            .clone()
            .unwrap_or_else(|| "Data table".to_owned());
        let data = outcome_table(&analysis)
            .ok_or("This result is best copied as text rather than added as a table.")?;
        let mut table = data;
        table.lineage = Some(DatasetLineage::new(
            DerivationKind::StatisticsTable,
            [dataset],
        ));
        table.name = Some(format!("{source_name} — {}", analysis.title));
        table.board_pos = super::app_impl_analysis::next_sheet_pos_after_new_canvas(self);
        let action = Action::insert_dataset_with_default_canvas(
            self,
            Dataset::Table(Box::new(table)),
            format!("Canvas {} — statistics", self.doc.canvases.len() + 1),
            DEFAULT_CANVAS_SIZE_MM,
        );
        self.execute_action(action);
        self.session.status = "Added the statistics result to the board.".to_owned();
        Ok(())
    }

    fn stored_statistics(&self, dataset: usize) -> Vec<StatAnalysis> {
        self.doc
            .datasets
            .get(dataset)
            .and_then(Dataset::as_table)
            .map(|table| table.statistics.clone())
            .unwrap_or_default()
    }

    fn next_statistics_id(&mut self, dataset: usize) -> u64 {
        let Some(table) = self
            .doc
            .datasets
            .get_mut(dataset)
            .and_then(Dataset::as_table_mut)
        else {
            return 0;
        };
        let id = table.next_stat_id;
        table.next_stat_id = id + 1;
        id
    }
}

// ----- preflight helpers -------------------------------------------------

impl PlotxApp {
    fn preflight_columns(
        &self,
        table: &NumericAnalysisTable,
        draft: &ResolvedStatDraft,
    ) -> StatPreflight {
        let mut preflight = StatPreflight::default();
        if draft.columns.is_empty() {
            preflight.role_error = Some("Choose at least one column to analyse.".to_owned());
            return preflight;
        }
        let minimum = if draft.question == StatQuestion::Normality {
            3
        } else {
            1
        };
        let mut skipped = 0;
        for &col in &draft.columns {
            let sample = extract_column(table, col);
            if sample.values.len() < minimum {
                preflight.role_error = Some(format!(
                    "{} has {} usable value(s); this test needs at least {minimum}.",
                    sample.name,
                    sample.values.len()
                ));
                return preflight;
            }
            skipped += sample.skipped;
        }
        preflight.missing_note = missing_cells_note(skipped);
        preflight
    }

    fn preflight_one_sample(
        &self,
        table: &NumericAnalysisTable,
        draft: &ResolvedStatDraft,
    ) -> StatPreflight {
        let mut preflight = StatPreflight::default();
        if !draft.reference_value.is_finite() {
            preflight.role_error =
                Some("Enter a finite reference value to compare against.".to_owned());
            return preflight;
        }
        let sample = extract_column(table, draft.column_a);
        if sample.values.len() < 2 {
            preflight.role_error = Some(format!(
                "{} has {} usable value(s); a one-sample t test needs at least 2.",
                sample.name,
                sample.values.len()
            ));
            return preflight;
        }
        preflight.missing_note = missing_cells_note(sample.skipped);
        preflight
    }

    fn preflight_two_columns(
        &self,
        table: &NumericAnalysisTable,
        draft: &ResolvedStatDraft,
        paired: bool,
    ) -> StatPreflight {
        let mut preflight = StatPreflight::default();
        if draft.column_a == draft.column_b {
            preflight.role_error = Some("Choose two different columns.".to_owned());
            return preflight;
        }
        if draft.column_a >= table.columns.len() || draft.column_b >= table.columns.len() {
            preflight.role_error = Some("Choose two columns from this table.".to_owned());
            return preflight;
        }
        if paired {
            let pair = extract_pair(table, draft.column_a, draft.column_b);
            if pair.left.len() < 3 {
                preflight.role_error = Some(format!(
                    "Only {} complete row pair(s) remain; this test needs at least 3.",
                    pair.left.len()
                ));
                return preflight;
            }
            preflight.missing_note = missing_rows_note(pair.dropped);
        } else {
            let left = extract_column(table, draft.column_a);
            let right = extract_column(table, draft.column_b);
            for sample in [&left, &right] {
                if sample.values.len() < 2 {
                    preflight.role_error = Some(format!(
                        "{} has {} usable value(s); this test needs at least 2 per group.",
                        sample.name,
                        sample.values.len()
                    ));
                    return preflight;
                }
            }
            preflight.missing_note = missing_cells_note(left.skipped + right.skipped);
        }
        preflight
    }

    fn preflight_groups(
        &self,
        table: &NumericAnalysisTable,
        draft: &ResolvedStatDraft,
    ) -> StatPreflight {
        let mut preflight = StatPreflight::default();
        if draft.group_columns.len() < 2 {
            preflight.role_error = Some("Choose at least two group columns to compare.".to_owned());
            return preflight;
        }
        let mut skipped = 0;
        for &col in &draft.group_columns {
            let sample = extract_column(table, col);
            if sample.values.len() < 2 {
                preflight.role_error = Some(format!(
                    "{} has {} usable value(s); each group needs at least 2.",
                    sample.name,
                    sample.values.len()
                ));
                return preflight;
            }
            skipped += sample.skipped;
        }
        preflight.missing_note = missing_cells_note(skipped);
        preflight
    }

    fn preflight_two_way(
        &self,
        table: &NumericAnalysisTable,
        draft: &ResolvedStatDraft,
    ) -> StatPreflight {
        let mut preflight = StatPreflight::default();
        let columns = [
            draft.value_column,
            draft.factor_a_column,
            draft.factor_b_column,
        ];
        if columns.iter().any(|&col| col >= table.columns.len()) {
            preflight.role_error =
                Some("Choose the value column and both factor columns.".to_owned());
            return preflight;
        }
        if draft.factor_a_column == draft.factor_b_column
            || draft.value_column == draft.factor_a_column
            || draft.value_column == draft.factor_b_column
        {
            preflight.role_error = Some(
                "The value column and the two factors must be three different columns.".to_owned(),
            );
            return preflight;
        }
        let prepared = extract_factorial(table, draft);
        if prepared.observations.is_empty() {
            preflight.role_error =
                Some("No rows have a value and both factor levels present.".to_owned());
            return preflight;
        }
        for (label, levels) in [
            (
                &table.columns[draft.factor_a_column].name,
                &prepared.levels_a,
            ),
            (
                &table.columns[draft.factor_b_column].name,
                &prepared.levels_b,
            ),
        ] {
            if levels.len() < 2 {
                preflight.role_error = Some(format!(
                    "Factor {label} has only {} level; a two-way ANOVA needs at least 2.",
                    levels.len()
                ));
                return preflight;
            }
            if levels.len() >= CONTINUOUS_LEVEL_HINT {
                preflight.warnings.push(format!(
                    "{label} has {} distinct values and may be a continuous measurement rather \
                     than a grouping factor.",
                    levels.len()
                ));
            }
        }
        preflight.missing_note = missing_rows_note(prepared.dropped);
        preflight
    }
}

// ----- result builders ---------------------------------------------------
//
// Associated functions rather than methods: they read only the table and the
// draft, which lets `run_statistics` borrow the table in place of cloning it.

type BuiltAnalysis = (String, String, Option<String>, StatOutcome);

impl PlotxApp {
    fn build_descriptive(
        table: &NumericAnalysisTable,
        draft: &ResolvedStatDraft,
    ) -> Result<BuiltAnalysis, String> {
        let mut records = Vec::new();
        let mut skipped = 0;
        for &col in &draft.columns {
            let sample = extract_column(table, col);
            skipped += sample.skipped;
            let result =
                describe(&sample.values).map_err(|error| friendly(&error, &sample.name))?;
            records.push(DescriptiveRecord::from_result(sample.name, &result));
        }
        let title = if records.len() == 1 {
            format!("Descriptive statistics: {}", records[0].column)
        } else {
            format!("Descriptive statistics: {} columns", records.len())
        };
        Ok((
            title,
            format!("Columns: {}", join_names(table, &draft.columns)),
            missing_cells_note(skipped),
            StatOutcome::Descriptive(records),
        ))
    }

    fn build_normality(
        table: &NumericAnalysisTable,
        draft: &ResolvedStatDraft,
    ) -> Result<BuiltAnalysis, String> {
        let mut records = Vec::new();
        let mut skipped = 0;
        for &col in &draft.columns {
            let sample = extract_column(table, col);
            skipped += sample.skipped;
            let result =
                shapiro_wilk(&sample.values).map_err(|error| friendly(&error, &sample.name))?;
            records.push(NormalityRecord::from_result(sample.name, &result));
        }
        Ok((
            "Shapiro–Wilk normality test".to_owned(),
            format!("Columns: {}", join_names(table, &draft.columns)),
            missing_cells_note(skipped),
            StatOutcome::Normality(records),
        ))
    }

    fn build_one_sample(
        table: &NumericAnalysisTable,
        draft: &ResolvedStatDraft,
    ) -> Result<BuiltAnalysis, String> {
        let sample = extract_column(table, draft.column_a);
        let result = one_sample_t_test(
            &sample.values,
            draft.reference_value,
            draft.direction.into(),
            draft.confidence,
        )
        .map_err(|error| friendly(&error, &sample.name))?;
        let count = sample.values.len();
        let name = sample.name.clone();
        let outcome = TTestOutcome::from_result(
            TTestKind::OneSample,
            name.clone(),
            None,
            count,
            None,
            &result,
        );
        Ok((
            format!("One-sample t test: {name}"),
            format!(
                "{name} vs {} · {} · {} confidence",
                fmt_num(draft.reference_value),
                draft.direction.label(),
                fmt_level(draft.confidence)
            ),
            missing_cells_note(sample.skipped),
            StatOutcome::TTest(outcome),
        ))
    }

    fn build_independent(
        table: &NumericAnalysisTable,
        draft: &ResolvedStatDraft,
    ) -> Result<BuiltAnalysis, String> {
        let left = extract_column(table, draft.column_a);
        let right = extract_column(table, draft.column_b);
        let result = independent_t_test(
            &left.values,
            &right.values,
            0.0,
            draft.variance.into(),
            draft.direction.into(),
            draft.confidence,
        )
        .map_err(|error| friendly_two(&error, &left.name, &right.name))?;
        let outcome = TTestOutcome::from_result(
            TTestKind::Independent(draft.variance),
            left.name.clone(),
            Some(right.name.clone()),
            left.values.len(),
            Some(right.values.len()),
            &result,
        );
        Ok((
            format!(
                "{}: {} vs {}",
                variance_test_name(draft.variance),
                left.name,
                right.name
            ),
            format!(
                "{} − {} · {} · {} · {} confidence",
                left.name,
                right.name,
                draft.variance.label(),
                draft.direction.label(),
                fmt_level(draft.confidence)
            ),
            missing_cells_note(left.skipped + right.skipped),
            StatOutcome::TTest(outcome),
        ))
    }

    fn build_paired(
        table: &NumericAnalysisTable,
        draft: &ResolvedStatDraft,
    ) -> Result<BuiltAnalysis, String> {
        let pair = extract_pair(table, draft.column_a, draft.column_b);
        let result = paired_t_test(
            &pair.left,
            &pair.right,
            0.0,
            draft.direction.into(),
            draft.confidence,
        )
        .map_err(|error| friendly_two(&error, &pair.left_name, &pair.right_name))?;
        let count = pair.left.len();
        let outcome = TTestOutcome::from_result(
            TTestKind::Paired,
            pair.left_name.clone(),
            Some(pair.right_name.clone()),
            count,
            Some(count),
            &result,
        );
        Ok((
            format!("Paired t test: {} vs {}", pair.left_name, pair.right_name),
            format!(
                "{} − {} (per row) · {} · {} confidence",
                pair.left_name,
                pair.right_name,
                draft.direction.label(),
                fmt_level(draft.confidence)
            ),
            missing_rows_note(pair.dropped),
            StatOutcome::TTest(outcome),
        ))
    }

    fn build_correlation(
        table: &NumericAnalysisTable,
        draft: &ResolvedStatDraft,
    ) -> Result<BuiltAnalysis, String> {
        let pair = extract_pair(table, draft.column_a, draft.column_b);
        let result = match draft.correlation {
            CorrelationKind::Pearson => pearson(&pair.left, &pair.right),
            CorrelationKind::Spearman => spearman(&pair.left, &pair.right),
        }
        .map_err(|error| friendly_two(&error, &pair.left_name, &pair.right_name))?;
        let outcome = CorrelationOutcome::from_result(
            pair.left_name.clone(),
            pair.right_name.clone(),
            &result,
        );
        Ok((
            format!(
                "{} correlation: {} vs {}",
                draft.correlation.label(),
                pair.left_name,
                pair.right_name
            ),
            format!(
                "{} vs {} · complete row pairs",
                pair.left_name, pair.right_name
            ),
            missing_rows_note(pair.dropped),
            StatOutcome::Correlation(outcome),
        ))
    }

    fn build_one_way(
        table: &NumericAnalysisTable,
        draft: &ResolvedStatDraft,
    ) -> Result<BuiltAnalysis, String> {
        let mut samples = Vec::new();
        let mut skipped = 0;
        for &col in &draft.group_columns {
            let sample = extract_column(table, col);
            skipped += sample.skipped;
            samples.push(sample);
        }
        let names: Vec<String> = samples.iter().map(|sample| sample.name.clone()).collect();
        let refs: Vec<&[f64]> = samples
            .iter()
            .map(|sample| sample.values.as_slice())
            .collect();
        let anova = one_way_anova(&refs).map_err(|error| friendly_groups(&error, &names))?;
        let tukey = if draft.run_tukey {
            let result = tukey_hsd(&refs, draft.confidence)
                .map_err(|error| friendly_groups(&error, &names))?;
            Some(TukeyOutcome::from_result(&names, &result))
        } else {
            None
        };
        let outcome = OneWayOutcome::from_result(&names, &anova, tukey);
        let post_hoc = if draft.run_tukey { " · Tukey HSD" } else { "" };
        Ok((
            format!("One-way ANOVA: {} groups", names.len()),
            format!("Groups: {}{post_hoc}", names.join(", ")),
            missing_cells_note(skipped),
            StatOutcome::OneWay(outcome),
        ))
    }

    fn build_two_way(
        table: &NumericAnalysisTable,
        draft: &ResolvedStatDraft,
    ) -> Result<BuiltAnalysis, String> {
        let prepared = extract_factorial(table, draft);
        // Total lookups: a stale draft index must surface as the backend's
        // empty-observations error, never as an out-of-bounds panic.
        let value_label = factor_label(table, draft.value_column);
        let factor_a_label = factor_label(table, draft.factor_a_column);
        let factor_b_label = factor_label(table, draft.factor_b_column);
        let result = two_way_anova(
            &prepared.observations,
            prepared.levels_a.len(),
            prepared.levels_b.len(),
        )
        .map_err(|error| friendly_factorial(&error, &prepared))?;
        let outcome = TwoWayOutcome::from_result(
            value_label.clone(),
            factor_a_label.clone(),
            factor_b_label.clone(),
            prepared.levels_a.clone(),
            prepared.levels_b.clone(),
            &result,
        );
        Ok((
            format!("Two-way ANOVA: {value_label} by {factor_a_label} and {factor_b_label}"),
            format!(
                "Value {value_label} · Factor A {factor_a_label} ({}) · Factor B {factor_b_label} ({})",
                prepared.levels_a.join(", "),
                prepared.levels_b.join(", ")
            ),
            missing_rows_note(prepared.dropped),
            StatOutcome::TwoWay(outcome),
        ))
    }
}
