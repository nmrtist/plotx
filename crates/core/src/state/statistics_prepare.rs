//! Turning a table into finite samples for statistics, and phrasing backend
//! errors for the user. Split from `app_impl_statistics` to keep both files
//! within the source-size limit; the two are one logical unit.
//!
//! Every extractor is explicit about missing data: it never silently drops a
//! value without counting it, so the caller can report the exact number of
//! excluded cells or rows and the sample sizes actually used.

use super::statistics::{
    ResolvedStatDraft, StatCellExclusion, StatExcludedRow, StatExclusionReason, StatQuestion,
    StatRowSelection, StatSelectionSnapshot, VarianceModel,
};
use super::statistics_report::fmt_num;
use super::table_numeric::{NumericAnalysisColumn, NumericAnalysisTable};
use plotx_analysis::statistics::{FactorialObservation, StatisticsError};

/// A finite sample extracted from one table column.
pub(super) struct ColumnSample {
    pub name: String,
    pub values: Vec<f64>,
    pub skipped: usize,
}

/// Finite values of one column, counting how many cells were dropped as missing
/// or non-finite. A column shorter than the x ruler simply contributes fewer
/// values; only present non-finite cells count as skipped.
pub(super) fn extract_column(table: &NumericAnalysisTable, column: usize) -> ColumnSample {
    let Some(col) = table.columns.get(column) else {
        return ColumnSample {
            name: format!("column {}", column + 1),
            values: Vec::new(),
            skipped: 0,
        };
    };
    let mut values = Vec::with_capacity(col.values.len());
    let mut skipped = 0;
    for value in &col.values {
        match value {
            Some(value) if value.is_finite() => values.push(*value),
            _ => skipped += 1,
        }
    }
    ColumnSample {
        name: col.name.clone(),
        values,
        skipped,
    }
}

pub(super) struct PairedSample {
    pub left_name: String,
    pub right_name: String,
    pub left: Vec<f64>,
    pub right: Vec<f64>,
    pub dropped: usize,
}

/// Row-aligned finite values of two columns: a row contributes only when both
/// cells are present and finite, so paired tests and correlation never mix
/// unrelated rows. Rows past the shorter column are treated as missing pairs.
pub(super) fn extract_pair(table: &NumericAnalysisTable, a: usize, b: usize) -> PairedSample {
    let left_col = table.columns.get(a);
    let right_col = table.columns.get(b);
    let left_name = left_col
        .map(|c| c.name.clone())
        .unwrap_or_else(|| format!("column {}", a + 1));
    let right_name = right_col
        .map(|c| c.name.clone())
        .unwrap_or_else(|| format!("column {}", b + 1));
    let rows = table.row_ids.len();
    let mut left = Vec::new();
    let mut right = Vec::new();
    let mut dropped = 0;
    for row in 0..rows {
        let lv = finite_cell(left_col, row);
        let rv = finite_cell(right_col, row);
        match (lv, rv) {
            (Some(l), Some(r)) => {
                left.push(l);
                right.push(r);
            }
            // A row with no usable cell in either column (absent or blank in
            // both) loses no data; only a partially present row counts as a
            // dropped pair.
            (None, None) => {}
            _ => dropped += 1,
        }
    }
    PairedSample {
        left_name,
        right_name,
        left,
        right,
        dropped,
    }
}

pub(super) struct FactorialPrep {
    pub observations: Vec<FactorialObservation>,
    pub levels_a: Vec<String>,
    pub levels_b: Vec<String>,
    pub dropped: usize,
}

/// Build long-format factorial observations from a value column and two numeric
/// factor columns. Distinct finite factor codes become levels (ascending), so
/// the detected levels can be shown to the user. Rows missing any of the three
/// cells are dropped and counted.
pub(super) fn extract_factorial(
    table: &NumericAnalysisTable,
    draft: &ResolvedStatDraft,
) -> FactorialPrep {
    let value_col = table.columns.get(draft.value_column);
    let factor_a_col = table.columns.get(draft.factor_a_column);
    let factor_b_col = table.columns.get(draft.factor_b_column);
    let rows = table.row_ids.len();

    let mut codes_a = Vec::new();
    let mut codes_b = Vec::new();
    let mut triples = Vec::new();
    let mut dropped = 0;
    for row in 0..rows {
        let value = finite_cell(value_col, row);
        let code_a = finite_cell(factor_a_col, row);
        let code_b = finite_cell(factor_b_col, row);
        match (value, code_a, code_b) {
            (Some(v), Some(a), Some(b)) => {
                push_level(&mut codes_a, a);
                push_level(&mut codes_b, b);
                triples.push((v, a, b));
            }
            (None, None, None) => {}
            _ => dropped += 1,
        }
    }
    codes_a.sort_by(|x, y| x.partial_cmp(y).unwrap_or(std::cmp::Ordering::Equal));
    codes_b.sort_by(|x, y| x.partial_cmp(y).unwrap_or(std::cmp::Ordering::Equal));

    let label_a = factor_label(table, draft.factor_a_column);
    let label_b = factor_label(table, draft.factor_b_column);
    let observations = triples
        .into_iter()
        .map(|(value, a, b)| FactorialObservation {
            factor_a: level_index(&codes_a, a),
            factor_b: level_index(&codes_b, b),
            value,
        })
        .collect();
    FactorialPrep {
        observations,
        levels_a: codes_a
            .iter()
            .map(|&code| level_label(&label_a, code))
            .collect(),
        levels_b: codes_b
            .iter()
            .map(|&code| level_label(&label_b, code))
            .collect(),
        dropped,
    }
}

/// A cell that is present and finite, treating blank (non-finite) cells the
/// same as cells past the column's end.
fn finite_cell(column: Option<&NumericAnalysisColumn>, row: usize) -> Option<f64> {
    column
        .and_then(|c| c.values.get(row))
        .copied()
        .flatten()
        .filter(|value| value.is_finite())
}

pub(super) fn factor_label(table: &NumericAnalysisTable, column: usize) -> String {
    table
        .columns
        .get(column)
        .map(|c| c.name.clone())
        .unwrap_or_else(|| format!("column {}", column + 1))
}

/// Capture stable column and row identities for exactly the inclusion rule
/// used by the selected statistical procedure.
pub(super) fn build_selection_snapshot(
    table: &NumericAnalysisTable,
    draft: &ResolvedStatDraft,
) -> Result<StatSelectionSnapshot, String> {
    let groups: Vec<Vec<usize>> = match draft.question {
        StatQuestion::Summarize | StatQuestion::Normality => draft
            .columns
            .iter()
            .copied()
            .map(|column| vec![column])
            .collect(),
        StatQuestion::CompareToValue => vec![vec![draft.column_a]],
        StatQuestion::CompareTwoGroups => {
            vec![vec![draft.column_a], vec![draft.column_b]]
        }
        StatQuestion::CompareManyGroups => draft
            .group_columns
            .iter()
            .copied()
            .map(|column| vec![column])
            .collect(),
        StatQuestion::ComparePaired | StatQuestion::Relationship => {
            vec![vec![draft.column_a, draft.column_b]]
        }
        StatQuestion::TwoFactors => vec![vec![
            draft.value_column,
            draft.factor_a_column,
            draft.factor_b_column,
        ]],
    };
    let mut selections = Vec::with_capacity(groups.len());
    for indices in groups {
        let columns = indices
            .iter()
            .map(|&index| {
                table
                    .columns
                    .get(index)
                    .map(|column| column.id)
                    .ok_or_else(|| format!("Table column {} does not exist.", index + 1))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let row_count = table.row_ids.len();
        let role = if indices.len() == 1 {
            factor_label(table, indices[0])
        } else {
            "complete_cases".to_owned()
        };
        let mut included_rows = Vec::new();
        let mut excluded_rows = Vec::new();
        for row in 0..row_count {
            let row_id = *table
                .row_ids
                .get(row)
                .ok_or_else(|| format!("Table row {} has no stable identity.", row + 1))?;
            let mut cells = Vec::new();
            let mut any_present = false;
            for (&index, &column_id) in indices.iter().zip(&columns) {
                let value = table
                    .columns
                    .get(index)
                    .and_then(|column| column.values.get(row))
                    .copied();
                any_present |= value.is_some();
                let reason = match value {
                    Some(Some(value)) if value.is_finite() => None,
                    Some(Some(_)) => Some(StatExclusionReason::NonFinite),
                    Some(None) | None => Some(StatExclusionReason::Null),
                };
                if let Some(reason) = reason {
                    cells.push(StatCellExclusion {
                        column: column_id,
                        reason,
                    });
                }
            }
            if cells.is_empty() {
                included_rows.push(row_id);
            } else if any_present {
                excluded_rows.push(StatExcludedRow { row: row_id, cells });
            }
        }
        selections.push(StatRowSelection {
            role,
            columns,
            included_rows,
            excluded_rows,
        });
    }
    Ok(StatSelectionSnapshot {
        source_revision: table.revision_id,
        selections,
    })
}

fn push_level(levels: &mut Vec<f64>, code: f64) {
    if !levels.contains(&code) {
        levels.push(code);
    }
}

fn level_index(levels: &[f64], code: f64) -> usize {
    levels
        .iter()
        .position(|&existing| existing == code)
        .unwrap_or(0)
}

/// A factor level label: the factor name with its numeric code, formatted as an
/// integer when the code is whole so `1.0`-style codes read as `= 1`.
fn level_label(factor: &str, code: f64) -> String {
    if code.fract() == 0.0 && code.abs() < 1e15 {
        format!("{factor} = {}", code as i64)
    } else {
        format!("{factor} = {}", fmt_num(code))
    }
}

pub(super) fn join_names(table: &NumericAnalysisTable, columns: &[usize]) -> String {
    columns
        .iter()
        .map(|&col| factor_label(table, col))
        .collect::<Vec<_>>()
        .join(", ")
}

pub(super) fn variance_test_name(variance: VarianceModel) -> &'static str {
    match variance {
        VarianceModel::Equal => "Student's t test",
        VarianceModel::Welch => "Welch's t test",
    }
}

pub(super) fn missing_cells_note(skipped: usize) -> Option<String> {
    (skipped > 0).then(|| {
        format!(
            "{skipped} non-finite cell(s) were excluded; the sample sizes shown are after exclusion."
        )
    })
}

pub(super) fn missing_rows_note(dropped: usize) -> Option<String> {
    (dropped > 0).then(|| {
        format!(
            "{dropped} row(s) with a missing value were excluded; the counts shown are after exclusion."
        )
    })
}

/// Translate a backend error against one named sample into user-facing text.
pub(super) fn friendly(error: &StatisticsError, name: &str) -> String {
    match error {
        StatisticsError::ZeroVariance { .. } => {
            format!("{name} has no variation (all values are equal), so this test is undefined.")
        }
        StatisticsError::InsufficientObservations {
            minimum, actual, ..
        } => {
            format!("{name} has {actual} value(s); this test needs at least {minimum}.")
        }
        StatisticsError::TooManyObservations { maximum, .. } => {
            format!("{name} has more than {maximum} values, which this test does not support.")
        }
        StatisticsError::EmptySample { .. } => format!("{name} has no usable values."),
        StatisticsError::NonFiniteValue { .. } => {
            format!("{name} still contains a non-finite value.")
        }
        StatisticsError::InvalidConfidenceLevel => {
            "The confidence level must be between 0 and 1.".to_owned()
        }
        StatisticsError::InvalidNullValue => "The reference value must be finite.".to_owned(),
        other => other.to_string(),
    }
}

pub(super) fn friendly_two(error: &StatisticsError, left: &str, right: &str) -> String {
    match error {
        StatisticsError::LengthMismatch { .. } => {
            "The two columns have different numbers of usable rows.".to_owned()
        }
        // The backend names the degenerate sample of a paired test "paired
        // differences": neither column is constant, their difference is.
        StatisticsError::ZeroVariance { sample } if sample.contains("differences") => format!(
            "The per-row differences {left} − {right} are all identical, so this test is undefined."
        ),
        StatisticsError::ZeroVariance { sample } if sample.contains("right") => {
            format!("{right} has no variation, so this test is undefined.")
        }
        StatisticsError::ZeroVariance { .. } => {
            format!("{left} has no variation, so this test is undefined.")
        }
        _ => friendly(error, left),
    }
}

pub(super) fn friendly_groups(error: &StatisticsError, names: &[String]) -> String {
    match error {
        StatisticsError::TooFewGroups { minimum, .. } => {
            format!("At least {minimum} groups are needed.")
        }
        StatisticsError::EmptySample { sample } => {
            format!("{} has no usable values.", resolve_group(sample, names))
        }
        StatisticsError::InsufficientObservations {
            sample,
            minimum,
            actual,
        } => format!(
            "{} has {actual} value(s); each group needs at least {minimum}.",
            resolve_group(sample, names)
        ),
        // `one_way_anova` (also run inside `tukey_hsd`) reports this when every
        // value across all groups is identical, so do not mention Tukey here.
        StatisticsError::ZeroVariance { .. } => {
            "The groups have no variation (all values are identical), so this test is undefined."
                .to_owned()
        }
        StatisticsError::InsufficientResidualDegreesOfFreedom { .. } => {
            "There are too few observations beyond the group means to run Tukey HSD.".to_owned()
        }
        other => other.to_string(),
    }
}

/// Map an internal "group N" sample name to the user's column name. The
/// backend numbers groups from zero (`anova.rs` formats `group {index}` over
/// `enumerate()`), so the number is the index itself.
fn resolve_group(sample: &str, names: &[String]) -> String {
    sample
        .strip_prefix("group ")
        .and_then(|index| index.trim().parse::<usize>().ok())
        .and_then(|zero_based| names.get(zero_based))
        .cloned()
        .unwrap_or_else(|| sample.to_owned())
}

pub(super) fn friendly_factorial(error: &StatisticsError, prep: &FactorialPrep) -> String {
    match error {
        StatisticsError::EmptyFactorialCell { factor_a, factor_b } => {
            let a = prep.levels_a.get(*factor_a).cloned().unwrap_or_default();
            let b = prep.levels_b.get(*factor_b).cloned().unwrap_or_default();
            format!("No observations combine {a} with {b}; every factor combination must appear.")
        }
        StatisticsError::TooFewFactorLevels => {
            "Each factor must have at least two distinct levels.".to_owned()
        }
        StatisticsError::ZeroVariance { .. } => {
            "The value column has no variation, so the ANOVA is undefined.".to_owned()
        }
        StatisticsError::NoResidualDegreesOfFreedom => {
            "Add replicate observations so the design has error degrees of freedom.".to_owned()
        }
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn group_errors_resolve_zero_based_backend_indices_to_column_names() {
        let names = vec!["low".to_owned(), "mid".to_owned(), "high".to_owned()];
        // The backend numbers groups from zero: "group 1" is the SECOND group.
        let error = StatisticsError::EmptySample {
            sample: "group 1".to_owned(),
        };
        assert_eq!(friendly_groups(&error, &names), "mid has no usable values.");
    }

    #[test]
    fn paired_zero_variance_blames_the_differences_not_a_column() {
        let error = StatisticsError::ZeroVariance {
            sample: "paired differences".to_owned(),
        };
        let message = friendly_two(&error, "before", "after");
        assert!(message.contains("before − after"), "got: {message}");
        assert!(!message.starts_with("before has"), "got: {message}");
    }

    #[test]
    fn anova_zero_variance_does_not_mention_tukey() {
        let error = StatisticsError::ZeroVariance {
            sample: "all groups".to_owned(),
        };
        let message = friendly_groups(&error, &["a".to_owned()]);
        assert!(!message.contains("Tukey"), "got: {message}");
    }
}
