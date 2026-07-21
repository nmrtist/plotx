//! Human-readable rendering of a stored statistics analysis.
//!
//! Two products share the same records: a labelled text report (copied to the
//! clipboard, and pasteable into a spreadsheet because the multi-row sections
//! are tab-delimited) and a derived typed table for outcomes whose column
//! names survive as table headers. Formatting lives here so the UI card and the
//! export path never phrase the same result two different ways.

use super::statistics::{
    CorrelationOutcome, DescriptiveRecord, NormalityRecord, OneWayOutcome, StatAnalysis,
    StatOutcome, TTestKind, TTestOutcome, TwoWayOutcome, TwoWayReplication, VarianceModel,
};
use super::{FloatSeries, TableDataset, materialized_float_series_table};

/// Format a general statistic: fixed decimals in the readable range, scientific
/// notation outside it, and an explicit marker for non-finite values so an
/// infinite t statistic from a degenerate sample is never shown as a number.
pub fn fmt_num(value: f64) -> String {
    if value.is_nan() {
        return "n/a".to_owned();
    }
    if value.is_infinite() {
        return if value > 0.0 { "+∞" } else { "−∞" }.to_owned();
    }
    let magnitude = value.abs();
    if magnitude != 0.0 && !(1e-3..1e6).contains(&magnitude) {
        format!("{value:.3e}")
    } else {
        format!("{value:.4}")
    }
}

/// Format a p-value, flooring tiny values rather than rounding them to zero,
/// which would misread as "impossible".
pub fn fmt_p(p: f64) -> String {
    if !p.is_finite() {
        return "n/a".to_owned();
    }
    if p < 0.0001 {
        "< 0.0001".to_owned()
    } else {
        format!("{p:.4}")
    }
}

/// Format a p-value with its relational operator for running prose, so a floored
/// value reads "p < 0.0001" rather than the doubled "p = < 0.0001".
pub fn fmt_p_prose(p: f64) -> String {
    if !p.is_finite() {
        "= n/a".to_owned()
    } else if p < 0.0001 {
        "< 0.0001".to_owned()
    } else {
        format!("= {p:.4}")
    }
}

/// Format a confidence level as a percentage without trailing-zero noise.
/// Rounded to two decimals first: `0.565 * 100.0` is `56.499999999999993`, and
/// printing the raw product would leak that float noise into every report.
pub fn fmt_level(level: f64) -> String {
    let pct = (level * 10_000.0).round() / 100.0;
    if pct.fract() == 0.0 {
        format!("{pct:.0}%")
    } else {
        format!("{pct}%")
    }
}

fn optional(value: Option<f64>) -> String {
    value.map(fmt_num).unwrap_or_else(|| "n/a".to_owned())
}

/// The one-line, answer-first headline for a result, reused by the card and the
/// report. It states the estimated quantity and its direction before any test
/// statistic, and never collapses a result to "significant".
pub fn headline(analysis: &StatAnalysis) -> String {
    match &analysis.outcome {
        StatOutcome::Descriptive(records) => match records.as_slice() {
            [only] => format!(
                "{}: mean {} (n = {})",
                only.column,
                fmt_num(only.mean),
                only.count
            ),
            _ => format!("Summary of {} columns", records.len()),
        },
        StatOutcome::Normality(records) => match records.as_slice() {
            [only] => format!(
                "{}: W = {}, p {}",
                only.column,
                fmt_num(only.w_statistic),
                fmt_p_prose(only.p_value)
            ),
            _ => format!("Normality of {} columns", records.len()),
        },
        StatOutcome::TTest(result) => ttest_headline(result),
        StatOutcome::Correlation(result) => format!(
            "{} vs {}: r = {} (p {})",
            result.left_label,
            result.right_label,
            fmt_num(result.coefficient),
            fmt_p_prose(result.p_value)
        ),
        StatOutcome::OneWay(result) => format!(
            "F({}, {}) = {}, p {}",
            result.factor.degrees_of_freedom,
            result.residual.degrees_of_freedom,
            optional(result.factor.f_statistic),
            result
                .factor
                .p_value
                .map(fmt_p_prose)
                .unwrap_or_else(|| "= n/a".to_owned())
        ),
        StatOutcome::TwoWay(result) => format!(
            "{} by {} and {}",
            result.value_label, result.factor_a_label, result.factor_b_label
        ),
    }
}

fn ttest_headline(result: &TTestOutcome) -> String {
    format!(
        "{} = {} ({} CI [{}, {}], p {})",
        difference_name(result),
        fmt_num(result.estimate),
        fmt_level(result.confidence_level),
        fmt_num(result.confidence_lower),
        fmt_num(result.confidence_upper),
        fmt_p_prose(result.p_value)
    )
}

/// The name of the quantity a t test estimated, always spelling out the
/// subtraction direction so a one-sided or paired result is unambiguous.
pub fn difference_name(result: &TTestOutcome) -> String {
    match (&result.kind, &result.right_label) {
        (TTestKind::OneSample, _) => {
            format!("{} − {}", result.left_label, fmt_num(result.null_value))
        }
        (_, Some(right)) => format!("{} − {}", result.left_label, right),
        (_, None) => format!("{} − reference", result.left_label),
    }
}

/// Compact, scannable lines for the on-screen results card. Unlike
/// [`report_text`] these are plain sentences (no tab columns), sized to the
/// narrow task card, and never reduce a result to "significant".
pub fn detail_lines(analysis: &StatAnalysis) -> Vec<String> {
    match &analysis.outcome {
        StatOutcome::Descriptive(records) => records
            .iter()
            .map(|record| {
                format!(
                    "{}: mean {}, SD {}, median {} (n = {})",
                    record.column,
                    fmt_num(record.mean),
                    optional(record.standard_deviation),
                    fmt_num(record.median),
                    record.count
                )
            })
            .collect(),
        StatOutcome::Normality(records) => {
            let mut lines = vec![
                "High p is consistent with normality; it does not prove the data are normal."
                    .to_owned(),
            ];
            lines.extend(records.iter().map(|record| {
                format!(
                    "{}: W = {}, p {} (n = {})",
                    record.column,
                    fmt_num(record.w_statistic),
                    fmt_p_prose(record.p_value),
                    record.count
                )
            }));
            lines
        }
        StatOutcome::TTest(result) => ttest_lines(result),
        StatOutcome::Correlation(result) => vec![
            format!(
                "{} = {} (n = {})",
                result.method.label(),
                fmt_num(result.coefficient),
                result.count
            ),
            format!(
                "t = {}, df = {}, two-sided p {}",
                fmt_num(result.statistic),
                result.degrees_of_freedom,
                fmt_p_prose(result.p_value)
            ),
        ],
        StatOutcome::OneWay(result) => one_way_lines(result),
        StatOutcome::TwoWay(result) => two_way_lines(result),
    }
}

fn ttest_lines(result: &TTestOutcome) -> Vec<String> {
    let mut lines = vec![
        format!("Direction: {}", result.direction.label()),
        format!(
            "{} confidence interval: [{}, {}]",
            fmt_level(result.confidence_level),
            fmt_num(result.confidence_lower),
            fmt_num(result.confidence_upper)
        ),
        format!(
            "t = {}, df = {}, p {}",
            fmt_num(result.statistic),
            fmt_num(result.degrees_of_freedom),
            fmt_p_prose(result.p_value)
        ),
        format!(
            "SE = {}, Cohen's d = {}",
            fmt_num(result.standard_error),
            fmt_num(result.cohens_d)
        ),
    ];
    if let (Some(right), Some(label)) = (result.count_right, &result.right_label) {
        lines.push(format!(
            "n: {} = {}, {} = {}",
            result.left_label, result.count_left, label, right
        ));
    } else {
        lines.push(format!("n = {}", result.count_left));
    }
    lines
}

fn one_way_lines(result: &OneWayOutcome) -> Vec<String> {
    let mut lines = Vec::new();
    for group in &result.groups {
        lines.push(format!(
            "{}: mean {} (n = {})",
            group.label,
            fmt_num(group.mean),
            group.count
        ));
    }
    lines.push(format!(
        "F({}, {}) = {}, p {}",
        result.factor.degrees_of_freedom,
        result.residual.degrees_of_freedom,
        optional(result.factor.f_statistic),
        result
            .factor
            .p_value
            .map(fmt_p_prose)
            .unwrap_or_else(|| "= n/a".to_owned())
    ));
    lines.push(format!(
        "Effect size: η² = {}, ω² = {}",
        fmt_num(result.eta_squared),
        fmt_num(result.omega_squared)
    ));
    if let Some(tukey) = &result.tukey {
        lines.push(format!(
            "Tukey HSD, {} intervals:",
            fmt_level(tukey.confidence_level)
        ));
        for comparison in &tukey.comparisons {
            lines.push(format!(
                "  {} − {} = {} [{}, {}], p {}",
                comparison.group_a,
                comparison.group_b,
                fmt_num(comparison.mean_difference),
                fmt_num(comparison.confidence_lower),
                fmt_num(comparison.confidence_upper),
                fmt_p_prose(comparison.p_value)
            ));
        }
    }
    lines
}

fn two_way_lines(result: &TwoWayOutcome) -> Vec<String> {
    let mut lines = vec![
        format!(
            "{}: {} levels ({})",
            result.factor_a_label,
            result.levels_a.len(),
            result.levels_a.join(", ")
        ),
        format!(
            "{}: {} levels ({})",
            result.factor_b_label,
            result.levels_b.len(),
            result.levels_b.join(", ")
        ),
    ];
    let effect = |label: &str, row: &super::statistics::AnovaRowRecord| {
        format!(
            "{label}: F({}, {}) = {}, p {}",
            row.degrees_of_freedom,
            result.residual.degrees_of_freedom,
            optional(row.f_statistic),
            row.p_value
                .map(fmt_p_prose)
                .unwrap_or_else(|| "= n/a".to_owned())
        )
    };
    lines.push(effect(&result.factor_a_label, &result.factor_a));
    lines.push(effect(&result.factor_b_label, &result.factor_b));
    match &result.interaction {
        Some(interaction) => lines.push(effect("Interaction", interaction)),
        None => lines
            .push("Interaction: not estimable without replicate observations per cell.".to_owned()),
    }
    lines
}

/// The full, checkable text report copied to the clipboard.
pub fn report_text(analysis: &StatAnalysis) -> String {
    let mut out = String::new();
    out.push_str(&analysis.title);
    out.push('\n');
    out.push_str(&analysis.configuration);
    out.push('\n');
    if let Some(note) = &analysis.data_note {
        out.push_str(note);
        out.push('\n');
    }
    out.push('\n');
    match &analysis.outcome {
        StatOutcome::Descriptive(records) => descriptive_report(records, &mut out),
        StatOutcome::Normality(records) => normality_report(records, &mut out),
        StatOutcome::TTest(result) => ttest_report(result, &mut out),
        StatOutcome::Correlation(result) => correlation_report(result, &mut out),
        StatOutcome::OneWay(result) => one_way_report(result, &mut out),
        StatOutcome::TwoWay(result) => two_way_report(result, &mut out),
    }
    out
}

const DESCRIPTIVE_STATS: [&str; 12] = [
    "n",
    "mean",
    "median",
    "std dev",
    "std error",
    "minimum",
    "Q1",
    "Q3",
    "maximum",
    "IQR",
    "skewness",
    "excess kurtosis",
];

fn descriptive_cell(record: &DescriptiveRecord, stat: usize) -> String {
    match stat {
        0 => record.count.to_string(),
        1 => fmt_num(record.mean),
        2 => fmt_num(record.median),
        3 => optional(record.standard_deviation),
        4 => optional(record.standard_error),
        5 => fmt_num(record.minimum),
        6 => fmt_num(record.first_quartile),
        7 => fmt_num(record.third_quartile),
        8 => fmt_num(record.maximum),
        9 => fmt_num(record.interquartile_range),
        10 => optional(record.skewness),
        11 => optional(record.excess_kurtosis),
        _ => String::new(),
    }
}

fn descriptive_report(records: &[DescriptiveRecord], out: &mut String) {
    out.push_str("statistic");
    for record in records {
        out.push('\t');
        out.push_str(&record.column);
    }
    out.push('\n');
    for (stat, name) in DESCRIPTIVE_STATS.iter().enumerate() {
        out.push_str(name);
        for record in records {
            out.push('\t');
            out.push_str(&descriptive_cell(record, stat));
        }
        out.push('\n');
    }
}

fn normality_report(records: &[NormalityRecord], out: &mut String) {
    out.push_str(
        "Shapiro–Wilk tests agreement with a normal distribution. A high p-value means the sample \
         is consistent with normality; it does not prove the data are normal.\n\n",
    );
    out.push_str("column\tn\tW\tp-value\n");
    for record in records {
        out.push_str(&format!(
            "{}\t{}\t{}\t{}\n",
            record.column,
            record.count,
            fmt_num(record.w_statistic),
            fmt_p(record.p_value)
        ));
    }
}

fn ttest_report(result: &TTestOutcome, out: &mut String) {
    let test = match result.kind {
        TTestKind::OneSample => "One-sample t test".to_owned(),
        TTestKind::Paired => "Paired t test".to_owned(),
        TTestKind::Independent(VarianceModel::Equal) => "Student's t test".to_owned(),
        TTestKind::Independent(VarianceModel::Welch) => "Welch's t test".to_owned(),
    };
    out.push_str(&format!("{test}\n"));
    out.push_str(&format!("Direction: {}\n", result.direction.label()));
    out.push_str(&format!(
        "{} = {}\n",
        difference_name(result),
        fmt_num(result.estimate)
    ));
    out.push_str(&format!(
        "{} confidence interval: [{}, {}]\n",
        fmt_level(result.confidence_level),
        fmt_num(result.confidence_lower),
        fmt_num(result.confidence_upper)
    ));
    out.push_str(&format!(
        "t = {}, df = {}, p {}\n",
        fmt_num(result.statistic),
        fmt_num(result.degrees_of_freedom),
        fmt_p_prose(result.p_value)
    ));
    out.push_str(&format!(
        "Standard error = {}, Cohen's d = {}\n",
        fmt_num(result.standard_error),
        fmt_num(result.cohens_d)
    ));
    match (result.count_right, &result.right_label) {
        (Some(right), Some(label)) => out.push_str(&format!(
            "n: {} = {}, {} = {}\n",
            result.left_label, result.count_left, label, right
        )),
        _ => out.push_str(&format!("n = {}\n", result.count_left)),
    }
}

fn correlation_report(result: &CorrelationOutcome, out: &mut String) {
    out.push_str(&format!("{} correlation\n", result.method.label()));
    out.push_str(&format!(
        "{} vs {}\n",
        result.left_label, result.right_label
    ));
    out.push_str(&format!(
        "r = {}, n = {}\n",
        fmt_num(result.coefficient),
        result.count
    ));
    out.push_str(&format!(
        "t = {}, df = {}, two-sided p {}\n",
        fmt_num(result.statistic),
        result.degrees_of_freedom,
        fmt_p_prose(result.p_value)
    ));
}

fn anova_row(label: &str, row: &super::statistics::AnovaRowRecord, out: &mut String) {
    out.push_str(&format!(
        "{label}\t{}\t{}\t{}\t{}\t{}\n",
        fmt_num(row.sum_squares),
        row.degrees_of_freedom,
        fmt_num(row.mean_square),
        optional(row.f_statistic),
        row.p_value.map(fmt_p).unwrap_or_else(|| "".to_owned())
    ));
}

fn one_way_report(result: &OneWayOutcome, out: &mut String) {
    out.push_str("Group\tn\tmean\n");
    for group in &result.groups {
        out.push_str(&format!(
            "{}\t{}\t{}\n",
            group.label,
            group.count,
            fmt_num(group.mean)
        ));
    }
    out.push('\n');
    out.push_str("source\tSS\tdf\tMS\tF\tp\n");
    anova_row("Between groups", &result.factor, out);
    anova_row("Within groups", &result.residual, out);
    anova_row("Total", &result.total, out);
    out.push_str(&format!(
        "\nEffect size: η² = {}, ω² = {}\n",
        fmt_num(result.eta_squared),
        fmt_num(result.omega_squared)
    ));
    if let Some(tukey) = &result.tukey {
        out.push_str(&format!(
            "\nTukey HSD ({} simultaneous intervals, error df = {})\n",
            fmt_level(tukey.confidence_level),
            tukey.error_degrees_of_freedom
        ));
        out.push_str("comparison\tdifference\tSE\tq\tp\tCI low\tCI high\n");
        for comparison in &tukey.comparisons {
            out.push_str(&format!(
                "{} − {}\t{}\t{}\t{}\t{}\t{}\t{}\n",
                comparison.group_a,
                comparison.group_b,
                fmt_num(comparison.mean_difference),
                fmt_num(comparison.standard_error),
                fmt_num(comparison.q_statistic),
                fmt_p(comparison.p_value),
                fmt_num(comparison.confidence_lower),
                fmt_num(comparison.confidence_upper)
            ));
        }
    }
}

fn two_way_report(result: &TwoWayOutcome, out: &mut String) {
    out.push_str(&format!(
        "Value: {}\nFactor A: {} ({} levels: {})\nFactor B: {} ({} levels: {})\n",
        result.value_label,
        result.factor_a_label,
        result.levels_a.len(),
        result.levels_a.join(", "),
        result.factor_b_label,
        result.levels_b.len(),
        result.levels_b.join(", ")
    ));
    match result.replication {
        TwoWayReplication::With => out.push_str(
            "Design: replicated cells; interaction is tested against within-cell error.\n",
        ),
        TwoWayReplication::Without => out.push_str(
            "Design: one observation per cell; interaction cannot be separated from error and is \
             not tested.\n",
        ),
    }
    out.push('\n');
    out.push_str("source\tSS\tdf\tMS\tF\tp\n");
    anova_row(&result.factor_a_label, &result.factor_a, out);
    anova_row(&result.factor_b_label, &result.factor_b, out);
    if let Some(interaction) = &result.interaction {
        anova_row("Interaction", interaction, out);
    }
    anova_row("Residual", &result.residual, out);
    anova_row("Total", &result.total, out);
    out.push_str(&format!("\nObservations: {}\n", result.observations));
}

/// Materialise a derived typed table for outcomes whose columns keep the
/// original group names as headers. Returns `None` when [`StatOutcome`] does not
/// support a table (see `supports_table`).
pub fn outcome_table(analysis: &StatAnalysis) -> Option<TableDataset> {
    match &analysis.outcome {
        StatOutcome::Descriptive(records) if !records.is_empty() => {
            Some(descriptive_table(records))
        }
        StatOutcome::Normality(records) if !records.is_empty() => Some(normality_table(records)),
        StatOutcome::OneWay(result) => Some(one_way_means_table(result)),
        _ => None,
    }
}

/// Columns are the analysed groups (headers keep their names); rows follow the
/// fixed `DESCRIPTIVE_STATS` order. The x axis carries only the row index —
/// The numeric presentation table has no row labels, so the fully labelled form
/// remains the copied text report.
fn descriptive_table(records: &[DescriptiveRecord]) -> TableDataset {
    let series = records
        .iter()
        .map(|record| FloatSeries {
            name: record.column.clone(),
            unit: String::new(),
            values: (0..DESCRIPTIVE_STATS.len())
                .map(|stat| descriptive_value(record, stat))
                .collect(),
            uncertainty: None,
            fit: None,
        })
        .collect();
    materialized_float_series_table(
        (
            "statistic index".into(),
            "".into(),
            (0..DESCRIPTIVE_STATS.len())
                .map(|i| Some(i as f64))
                .collect(),
        ),
        series,
        "plotx.statistics.descriptive-table.v1",
    )
    .expect("descriptive statistics form aligned typed columns")
}

fn descriptive_value(record: &DescriptiveRecord, stat: usize) -> Option<f64> {
    match stat {
        0 => Some(record.count as f64),
        1 => Some(record.mean),
        2 => Some(record.median),
        3 => record.standard_deviation,
        4 => record.standard_error,
        5 => Some(record.minimum),
        6 => Some(record.first_quartile),
        7 => Some(record.third_quartile),
        8 => Some(record.maximum),
        9 => Some(record.interquartile_range),
        10 => record.skewness,
        11 => record.excess_kurtosis,
        _ => None,
    }
}

fn normality_table(records: &[NormalityRecord]) -> TableDataset {
    let series = records
        .iter()
        .map(|record| FloatSeries {
            name: record.column.clone(),
            unit: String::new(),
            values: vec![Some(record.w_statistic), Some(record.p_value)],
            uncertainty: None,
            fit: None,
        })
        .collect();
    materialized_float_series_table(
        (
            "statistic index".into(),
            "".into(),
            vec![Some(0.0), Some(1.0)],
        ),
        series,
        "plotx.statistics.normality-table.v1",
    )
    .expect("normality statistics form aligned typed columns")
}

/// One row of group means, plottable as a bar chart straight from the board.
fn one_way_means_table(result: &OneWayOutcome) -> TableDataset {
    let series = result
        .groups
        .iter()
        .map(|group| FloatSeries {
            name: group.label.clone(),
            unit: String::new(),
            values: vec![Some(group.mean)],
            uncertainty: None,
            fit: None,
        })
        .collect();
    materialized_float_series_table(
        ("group index".into(), "".into(), vec![Some(0.0)]),
        series,
        "plotx.statistics.one-way-means-table.v1",
    )
    .expect("one-way means form aligned typed columns")
}

#[cfg(test)]
mod tests {
    use super::fmt_level;

    #[test]
    fn confidence_levels_print_without_float_noise() {
        // 0.565 * 100.0 is 56.499999999999993 in f64; the formatter must not
        // leak that into the configuration line or the copied report.
        assert_eq!(fmt_level(0.565), "56.5%");
        assert_eq!(fmt_level(0.577), "57.7%");
        assert_eq!(fmt_level(0.95), "95%");
        assert_eq!(fmt_level(0.999), "99.9%");
        assert_eq!(fmt_level(0.9735), "97.35%");
    }
}
