//! Serde model for classical statistics run against a data table.
//!
//! The analysis crate's result structs (`plotx_analysis::statistics`) are not
//! serialisable and phrase everything in group *indices*. This module mirrors
//! them into records that carry the group and factor-level *names* the user
//! actually chose, plus the configuration and the missing-value handling, so a
//! completed analysis persists in the project and stays fully readable after
//! the configuration card is closed. It parallels `StoredLineFit`.

use plotx_analysis::statistics::{
    Alternative, CorrelationMethod, CorrelationResult, DescriptiveStatistics, NormalityResult,
    OneWayAnova, TTestResult, TukeyHsd, TwoWayAnova, TwoWayDesign, VarianceAssumption,
};
use plotx_data::ColumnId;
use serde::{Deserialize, Serialize};

/// The plain-language question the user picked, which selects an analysis
/// family. Kept as an explicit enum (rather than derived from the outcome) so
/// the configuration card can restore the last question for a table.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum StatQuestion {
    /// Descriptive statistics for one or more columns.
    Summarize,
    /// Shapiro–Wilk agreement with a normal distribution.
    Normality,
    /// One-sample t test against a reference value.
    CompareToValue,
    /// Independent two-sample t test (Student or Welch).
    CompareTwoGroups,
    /// Paired t test over `left - right` row differences.
    ComparePaired,
    /// One-way ANOVA across three or more columns, with optional Tukey HSD.
    CompareManyGroups,
    /// Pearson or Spearman correlation between two columns.
    Relationship,
    /// Two-way ANOVA with a value column and two factor columns.
    TwoFactors,
}

impl StatQuestion {
    pub const ALL: [Self; 8] = [
        Self::Summarize,
        Self::Normality,
        Self::CompareToValue,
        Self::CompareTwoGroups,
        Self::ComparePaired,
        Self::CompareManyGroups,
        Self::Relationship,
        Self::TwoFactors,
    ];

    /// The plain-language prompt shown in the question picker.
    pub const fn prompt(self) -> &'static str {
        match self {
            Self::Summarize => "Summarize one or more columns",
            Self::Normality => "Check whether a column looks normal",
            Self::CompareToValue => "Compare one column with a reference value",
            Self::CompareTwoGroups => "Compare two independent groups",
            Self::ComparePaired => "Compare paired or before/after measurements",
            Self::CompareManyGroups => "Compare three or more groups",
            Self::Relationship => "See whether two columns are related",
            Self::TwoFactors => "Study two factors at once",
        }
    }

    /// The formal test name, so an experienced user can confirm what will run.
    pub const fn formal_name(self) -> &'static str {
        match self {
            Self::Summarize => "Descriptive statistics",
            Self::Normality => "Shapiro–Wilk normality test",
            Self::CompareToValue => "One-sample t test",
            Self::CompareTwoGroups => "Independent two-sample t test",
            Self::ComparePaired => "Paired t test",
            Self::CompareManyGroups => "One-way ANOVA",
            Self::Relationship => "Correlation",
            Self::TwoFactors => "Two-way ANOVA",
        }
    }
}

/// Serde mirror of [`Alternative`], plus the direction wording the results view
/// needs. Stored so a one-sided result never loses which tail it tested.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TestDirection {
    TwoSided,
    Less,
    Greater,
}

impl TestDirection {
    pub const fn label(self) -> &'static str {
        match self {
            Self::TwoSided => "Two-sided",
            Self::Less => "One-sided (less)",
            Self::Greater => "One-sided (greater)",
        }
    }
}

impl From<TestDirection> for Alternative {
    fn from(direction: TestDirection) -> Self {
        match direction {
            TestDirection::TwoSided => Alternative::TwoSided,
            TestDirection::Less => Alternative::Less,
            TestDirection::Greater => Alternative::Greater,
        }
    }
}

/// Serde mirror of [`VarianceAssumption`]. The UI never defaults silently to
/// equal variances; the user chooses explicitly.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum VarianceModel {
    /// Student pooled-variance test.
    Equal,
    /// Welch test with Satterthwaite degrees of freedom.
    Welch,
}

impl VarianceModel {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Equal => "Student (equal variances)",
            Self::Welch => "Welch (unequal variances)",
        }
    }
}

impl From<VarianceModel> for VarianceAssumption {
    fn from(model: VarianceModel) -> Self {
        match model {
            VarianceModel::Equal => VarianceAssumption::Equal,
            VarianceModel::Welch => VarianceAssumption::Unequal,
        }
    }
}

/// Serde mirror of [`CorrelationMethod`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CorrelationKind {
    Pearson,
    Spearman,
}

impl CorrelationKind {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Pearson => "Pearson (linear)",
            Self::Spearman => "Spearman (rank)",
        }
    }
}

impl From<CorrelationMethod> for CorrelationKind {
    fn from(method: CorrelationMethod) -> Self {
        match method {
            CorrelationMethod::Pearson => Self::Pearson,
            CorrelationMethod::Spearman => Self::Spearman,
        }
    }
}

/// Which t test produced a [`TTestOutcome`], so the direction of the estimate
/// and the effect size can be described precisely.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TTestKind {
    OneSample,
    Paired,
    Independent(VarianceModel),
}

/// The in-progress configuration for the active table's statistics card.
///
/// Runtime UI state (never serialised): it is rebuilt when the card opens for a
/// table. Column choices use stable IDs and survive projection or reordering. Defaults avoid
/// implying an equal-variance assumption and start with exclusion unconfirmed so
/// a table with missing values cannot run until the user acknowledges it.
#[derive(Clone, Debug, PartialEq)]
pub struct StatDraft {
    pub dataset: usize,
    pub question: StatQuestion,
    /// Columns for the summary and normality questions (one or more).
    pub columns: Vec<ColumnId>,
    /// Primary column for paired, two-group, one-sample and correlation roles.
    pub column_a: ColumnId,
    /// Secondary column for paired, two-group and correlation roles.
    pub column_b: ColumnId,
    /// Columns treated as independent groups for one-way ANOVA.
    pub group_columns: Vec<ColumnId>,
    pub value_column: ColumnId,
    pub factor_a_column: ColumnId,
    pub factor_b_column: ColumnId,
    pub variance: VarianceModel,
    pub direction: TestDirection,
    pub correlation: CorrelationKind,
    pub reference_value: f64,
    pub confidence: f64,
    pub run_tukey: bool,
    /// Set by the user to permit dropping rows or cells with missing values.
    pub exclusion_confirmed: bool,
}

impl StatDraft {
    pub fn new(dataset: usize, columns: &[ColumnId]) -> Self {
        let first = columns.first().copied().unwrap_or_default();
        let second = columns.get(1).copied().unwrap_or(first);
        let third = columns.get(2).copied().unwrap_or(second);
        Self {
            dataset,
            question: StatQuestion::Summarize,
            columns: columns.first().copied().into_iter().collect(),
            column_a: first,
            column_b: second,
            group_columns: Vec::new(),
            value_column: first,
            factor_a_column: second,
            factor_b_column: third,
            variance: VarianceModel::Welch,
            direction: TestDirection::TwoSided,
            correlation: CorrelationKind::Pearson,
            reference_value: 0.0,
            confidence: 0.95,
            run_tukey: true,
            exclusion_confirmed: false,
        }
    }

    /// Whether two drafts select the same cells to analyse. A missing-value
    /// exclusion the user confirmed for `other` still covers `self` only when
    /// this holds; any change of question or column roles needs fresh consent.
    pub fn selects_same_data(&self, other: &Self) -> bool {
        self.question == other.question
            && self.columns == other.columns
            && self.column_a == other.column_a
            && self.column_b == other.column_b
            && self.group_columns == other.group_columns
            && self.value_column == other.value_column
            && self.factor_a_column == other.factor_a_column
            && self.factor_b_column == other.factor_b_column
    }
}

#[derive(Clone, Debug)]
pub(super) struct ResolvedStatDraft {
    pub question: StatQuestion,
    pub columns: Vec<usize>,
    pub column_a: usize,
    pub column_b: usize,
    pub group_columns: Vec<usize>,
    pub value_column: usize,
    pub factor_a_column: usize,
    pub factor_b_column: usize,
    pub variance: VarianceModel,
    pub direction: TestDirection,
    pub correlation: CorrelationKind,
    pub reference_value: f64,
    pub confidence: f64,
    pub run_tukey: bool,
}

/// The result of checking a draft before the Run button is offered.
///
/// `role_error` blocks the run outright (bad column choices, a missing reference
/// value). `missing_note` describes rows or cells that would be dropped and, when
/// present, forces the user to tick the exclusion confirmation. `warnings` are
/// non-blocking cautions such as a factor column that looks continuous.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct StatPreflight {
    pub role_error: Option<String>,
    pub missing_note: Option<String>,
    pub warnings: Vec<String>,
}

impl StatPreflight {
    pub fn needs_confirmation(&self) -> bool {
        self.missing_note.is_some()
    }
}

/// One column's descriptive summary. Optional moments mirror the analysis
/// crate: variance and its derivatives need two observations, skewness three,
/// excess kurtosis four.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DescriptiveRecord {
    pub column: String,
    pub count: usize,
    pub mean: f64,
    pub median: f64,
    pub standard_deviation: Option<f64>,
    pub standard_error: Option<f64>,
    pub minimum: f64,
    pub first_quartile: f64,
    pub third_quartile: f64,
    pub maximum: f64,
    pub interquartile_range: f64,
    pub skewness: Option<f64>,
    pub excess_kurtosis: Option<f64>,
}

impl DescriptiveRecord {
    pub fn from_result(column: String, result: &DescriptiveStatistics) -> Self {
        Self {
            column,
            count: result.count,
            mean: result.mean,
            median: result.median,
            standard_deviation: result.standard_deviation,
            standard_error: result.standard_error,
            minimum: result.minimum,
            first_quartile: result.first_quartile,
            third_quartile: result.third_quartile,
            maximum: result.maximum,
            interquartile_range: result.interquartile_range,
            skewness: result.skewness,
            excess_kurtosis: result.excess_kurtosis,
        }
    }
}

/// One column's Shapiro–Wilk result.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NormalityRecord {
    pub column: String,
    pub count: usize,
    pub w_statistic: f64,
    pub p_value: f64,
}

impl NormalityRecord {
    pub fn from_result(column: String, result: &NormalityResult) -> Self {
        Self {
            column,
            count: result.observations,
            w_statistic: result.statistic,
            p_value: result.p_value,
        }
    }
}

/// A named two-sample or one-sample t test result. `estimate` is always the
/// signed quantity the test measured: `left - right` for paired and independent
/// tests, `mean - reference` for a one-sample test.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TTestOutcome {
    pub kind: TTestKind,
    pub direction: TestDirection,
    /// Name shown as the left/primary sample. For a one-sample test this is the
    /// analysed column; the reference value lives in `null_value`.
    pub left_label: String,
    /// Name of the right sample for two-sample tests; `None` for one-sample.
    pub right_label: Option<String>,
    pub count_left: usize,
    pub count_right: Option<usize>,
    pub estimate: f64,
    pub null_value: f64,
    pub standard_error: f64,
    pub statistic: f64,
    pub degrees_of_freedom: f64,
    pub p_value: f64,
    pub confidence_level: f64,
    pub confidence_lower: f64,
    pub confidence_upper: f64,
    pub cohens_d: f64,
}

impl TTestOutcome {
    #[allow(clippy::too_many_arguments)]
    pub fn from_result(
        kind: TTestKind,
        left_label: String,
        right_label: Option<String>,
        count_left: usize,
        count_right: Option<usize>,
        result: &TTestResult,
    ) -> Self {
        Self {
            kind,
            direction: match result.alternative {
                Alternative::TwoSided => TestDirection::TwoSided,
                Alternative::Less => TestDirection::Less,
                Alternative::Greater => TestDirection::Greater,
            },
            left_label,
            right_label,
            count_left,
            count_right,
            estimate: result.estimate,
            null_value: result.null_value,
            standard_error: result.standard_error,
            statistic: result.statistic,
            degrees_of_freedom: result.degrees_of_freedom,
            p_value: result.p_value,
            confidence_level: result.confidence_interval.level,
            confidence_lower: result.confidence_interval.lower,
            confidence_upper: result.confidence_interval.upper,
            cohens_d: result.cohens_d,
        }
    }
}

/// A named correlation result between two columns.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CorrelationOutcome {
    pub method: CorrelationKind,
    pub left_label: String,
    pub right_label: String,
    pub count: usize,
    pub coefficient: f64,
    pub statistic: f64,
    pub degrees_of_freedom: usize,
    pub p_value: f64,
}

impl CorrelationOutcome {
    pub fn from_result(left: String, right: String, result: &CorrelationResult) -> Self {
        Self {
            method: result.method.into(),
            left_label: left,
            right_label: right,
            count: result.observations,
            coefficient: result.coefficient,
            statistic: result.statistic,
            degrees_of_freedom: result.degrees_of_freedom,
            p_value: result.p_value,
        }
    }
}

/// One row of an ANOVA table. `f_statistic`/`p_value` are absent for the
/// residual and total rows.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct AnovaRowRecord {
    pub sum_squares: f64,
    pub degrees_of_freedom: usize,
    pub mean_square: f64,
    pub f_statistic: Option<f64>,
    pub p_value: Option<f64>,
}

impl From<plotx_analysis::statistics::AnovaRow> for AnovaRowRecord {
    fn from(row: plotx_analysis::statistics::AnovaRow) -> Self {
        Self {
            sum_squares: row.sum_squares,
            degrees_of_freedom: row.degrees_of_freedom,
            mean_square: row.mean_square,
            f_statistic: row.f_statistic,
            p_value: row.p_value,
        }
    }
}

/// A per-group cell of a one-way ANOVA, carrying the original column name.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GroupSummary {
    pub label: String,
    pub count: usize,
    pub mean: f64,
}

/// One Tukey pairwise comparison with both group names. `mean_difference` is
/// `mean(group_a) - mean(group_b)`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TukeyComparisonRecord {
    pub group_a: String,
    pub group_b: String,
    pub mean_difference: f64,
    pub standard_error: f64,
    pub q_statistic: f64,
    pub p_value: f64,
    pub confidence_lower: f64,
    pub confidence_upper: f64,
}

/// Family-wise Tukey HSD comparisons following a one-way ANOVA.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TukeyOutcome {
    pub confidence_level: f64,
    pub error_degrees_of_freedom: usize,
    pub comparisons: Vec<TukeyComparisonRecord>,
}

impl TukeyOutcome {
    pub fn from_result(names: &[String], result: &TukeyHsd) -> Self {
        let name = |index: usize| {
            names
                .get(index)
                .cloned()
                .unwrap_or_else(|| format!("group {}", index + 1))
        };
        Self {
            confidence_level: result.confidence_level,
            error_degrees_of_freedom: result.error_degrees_of_freedom,
            comparisons: result
                .comparisons
                .iter()
                .map(|comparison| TukeyComparisonRecord {
                    group_a: name(comparison.group_a),
                    group_b: name(comparison.group_b),
                    mean_difference: comparison.mean_difference,
                    standard_error: comparison.standard_error,
                    q_statistic: comparison.q_statistic,
                    p_value: comparison.p_value,
                    confidence_lower: comparison.confidence_lower,
                    confidence_upper: comparison.confidence_upper,
                })
                .collect(),
        }
    }
}

/// A one-way ANOVA outcome with named groups and an optional post-hoc test.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct OneWayOutcome {
    pub groups: Vec<GroupSummary>,
    pub factor: AnovaRowRecord,
    pub residual: AnovaRowRecord,
    pub total: AnovaRowRecord,
    pub grand_mean: f64,
    pub eta_squared: f64,
    pub omega_squared: f64,
    pub tukey: Option<TukeyOutcome>,
}

impl OneWayOutcome {
    pub fn from_result(
        names: &[String],
        result: &OneWayAnova,
        tukey: Option<TukeyOutcome>,
    ) -> Self {
        let groups = names
            .iter()
            .enumerate()
            .map(|(index, label)| GroupSummary {
                label: label.clone(),
                count: result.group_sizes.get(index).copied().unwrap_or(0),
                mean: result.group_means.get(index).copied().unwrap_or(f64::NAN),
            })
            .collect();
        Self {
            groups,
            factor: result.factor.into(),
            residual: result.residual.into(),
            total: result.total.into(),
            grand_mean: result.grand_mean,
            eta_squared: result.eta_squared,
            omega_squared: result.omega_squared,
            tukey,
        }
    }
}

/// Whether a two-way design could estimate interaction.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TwoWayReplication {
    /// One observation per cell: interaction is confounded with error.
    Without,
    /// At least one replicated cell: interaction is tested against error.
    With,
}

impl From<TwoWayDesign> for TwoWayReplication {
    fn from(design: TwoWayDesign) -> Self {
        match design {
            TwoWayDesign::WithoutReplication => Self::Without,
            TwoWayDesign::WithReplication => Self::With,
        }
    }
}

/// A two-way ANOVA outcome carrying the factor names and detected level labels.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TwoWayOutcome {
    pub value_label: String,
    pub factor_a_label: String,
    pub factor_b_label: String,
    pub levels_a: Vec<String>,
    pub levels_b: Vec<String>,
    pub replication: TwoWayReplication,
    pub observations: usize,
    pub factor_a: AnovaRowRecord,
    pub factor_b: AnovaRowRecord,
    pub interaction: Option<AnovaRowRecord>,
    pub residual: AnovaRowRecord,
    pub total: AnovaRowRecord,
}

impl TwoWayOutcome {
    pub fn from_result(
        value_label: String,
        factor_a_label: String,
        factor_b_label: String,
        levels_a: Vec<String>,
        levels_b: Vec<String>,
        result: &TwoWayAnova,
    ) -> Self {
        Self {
            value_label,
            factor_a_label,
            factor_b_label,
            levels_a,
            levels_b,
            replication: result.design.into(),
            observations: result.observations,
            factor_a: result.factor_a.into(),
            factor_b: result.factor_b.into(),
            interaction: result.interaction.map(Into::into),
            residual: result.residual.into(),
            total: result.total.into(),
        }
    }
}

/// The numeric payload of a completed analysis.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum StatOutcome {
    Descriptive(Vec<DescriptiveRecord>),
    Normality(Vec<NormalityRecord>),
    TTest(TTestOutcome),
    Correlation(CorrelationOutcome),
    OneWay(OneWayOutcome),
    TwoWay(TwoWayOutcome),
}

/// Frozen record of the exact table cells admitted to a statistical analysis.
/// Runtime column positions are deliberately absent.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatSelectionSnapshot {
    pub source_revision: plotx_data::RevisionId,
    pub selections: Vec<StatRowSelection>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatRowSelection {
    /// Human-readable role such as `control` or `complete_cases`.
    pub role: String,
    pub columns: Vec<plotx_data::ColumnId>,
    pub included_rows: Vec<plotx_data::RowId>,
    pub excluded_rows: Vec<StatExcludedRow>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatExcludedRow {
    pub row: plotx_data::RowId,
    pub cells: Vec<StatCellExclusion>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatCellExclusion {
    pub column: plotx_data::ColumnId,
    pub reason: StatExclusionReason,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StatExclusionReason {
    Null,
    NonFinite,
}

/// One completed, persisted analysis attached to a source table dataset.
///
/// `title` and `configuration` are rendered when the analysis runs and stored,
/// so the results list stays readable without re-deriving anything. `data_note`
/// records how missing or non-finite values were handled, satisfying the rule
/// that the used sample sizes are always visible.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StatAnalysis {
    pub id: u64,
    pub question: StatQuestion,
    pub title: String,
    pub configuration: String,
    pub data_note: Option<String>,
    pub selection: StatSelectionSnapshot,
    pub outcome: StatOutcome,
}

impl StatOutcome {
    /// Whether "Add table to board" is meaningful for this outcome.
    ///
    /// Offered only where the derived table keeps the original column names as
    /// its headers, because identifying the source column is a hard requirement.
    /// A numeric presentation table has one string dimension (column headers) and a
    /// numeric x ruler, so a two-way ANOVA table — whose rows are the labelled
    /// effect sources — cannot be represented without losing those labels; it is
    /// exported through the fully labelled copy text instead. Scalar results (a
    /// single t test, one correlation) are likewise better copied as text.
    pub fn supports_table(&self) -> bool {
        match self {
            Self::Descriptive(records) => !records.is_empty(),
            Self::Normality(records) => !records.is_empty(),
            Self::OneWay(_) => true,
            Self::TwoWay(_) | Self::TTest(_) | Self::Correlation(_) => false,
        }
    }
}
