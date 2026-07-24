use super::{TableImportSource, TypedTableState};
use plotx_analysis::series::IntensityMode;
use plotx_data::{ColumnId, RevisionId, RowId};
use plotx_figure::{Axis, Color, ErrorBar, Figure, Series};
use plotx_io::DiffusionMeta;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

fn new_resource_id() -> crate::state::DatasetId {
    crate::state::DatasetId::new()
}

/// A column points into a table-level analysis so multiple responses and
/// datasets can share one parameter covariance matrix.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct CurveFitReference {
    pub analysis_id: u64,
    pub instance_id: String,
    pub response: String,
}

/// Complete immutable fit snapshot. The model definition, bindings, effective
/// settings, point predictions, and diagnostics live in `result`.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct StoredCurveFitAnalysis {
    pub id: u64,
    pub name: String,
    pub bindings: Vec<ModelInstanceBinding>,
    pub result: plotx_analysis::fit_model::FitResult,
    /// Exact immutable source-row selection used by this fit. Older v1
    /// snapshots did not carry it, so absence is preserved rather than
    /// inventing an audit trail during load.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selection: Option<FitSelectionSnapshot>,
    /// Curves evaluated on the table's display x axis by the core binding
    /// workflow. Numerical fit results remain independent of presentation.
    pub plot_samples: FitPlotSamples,
}

/// Source identity and row-level inclusion decisions for one completed fit.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct FitSelectionSnapshot {
    pub source_revision: RevisionId,
    pub input_columns: Vec<ColumnId>,
    pub instances: Vec<FitInstanceSelection>,
    pub rule: FitSelectionRule,
}

#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct FitInstanceSelection {
    pub dataset_id: String,
    pub response_column: ColumnId,
    pub included_rows: Vec<RowId>,
    pub excluded_rows: Vec<FitRowExclusion>,
}

#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct FitRowExclusion {
    pub row: RowId,
    pub reason: FitRowExclusionReason,
    /// Semantic model quantities whose values caused this decision.
    pub quantities: Vec<String>,
    /// Source columns corresponding to `quantities`; constants and metadata
    /// have no column identity and are therefore omitted.
    pub columns: Vec<ColumnId>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FitRowExclusionReason {
    NullRequiredValue,
    NonFiniteRequiredValue,
    NullAndNonFiniteRequiredValues,
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FitSelectionRule {
    RejectNonFinite,
    ExcludeNonFinite,
}

pub type FitPlotSamples = BTreeMap<String, BTreeMap<String, Vec<[f64; 2]>>>;

#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "source", rename_all = "snake_case")]
pub enum FitDataBinding {
    Column { column: ColumnId },
    DatasetConstant { value: f64 },
    Metadata { key: String },
}

#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelInstanceBinding {
    pub dataset_id: String,
    pub variables: BTreeMap<String, FitDataBinding>,
    pub responses: BTreeMap<String, FitDataBinding>,
    pub constants: BTreeMap<String, FitDataBinding>,
}

#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct TableProvenance {
    pub source_resource: String,
    pub regions: Vec<(f64, f64)>,
    pub metric: TableMetric,
}

/// Serialisable mirror of the extraction `IntensityMode`, so the table owns its
/// provenance without depending on the processing layer's non-serde enum.
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TableMetric {
    PeakHeight,
    Integral,
}

impl From<IntensityMode> for TableMetric {
    fn from(mode: IntensityMode) -> Self {
        match mode {
            IntensityMode::PeakHeight => TableMetric::PeakHeight,
            IntensityMode::Integral => TableMetric::Integral,
        }
    }
}

impl From<TableMetric> for IntensityMode {
    fn from(metric: TableMetric) -> Self {
        match metric {
            TableMetric::PeakHeight => IntensityMode::PeakHeight,
            TableMetric::Integral => IntensityMode::Integral,
        }
    }
}

/// Model constants a fit preset may need, copied from the source acquisition.
#[derive(Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct TableMeta {
    pub diffusion: Option<DiffusionConstants>,
}

/// Stejskal–Tanner constants held as plain fields so the table keeps them across
/// save/load independently of the io `DiffusionMeta`.
#[derive(Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct DiffusionConstants {
    pub gamma: f64,
    pub delta: f64,
    pub big_delta: f64,
    pub tau: f64,
    pub shape_factor: f64,
}

impl DiffusionConstants {
    pub fn from_meta(m: &DiffusionMeta) -> Self {
        Self {
            gamma: m.gamma,
            delta: m.delta,
            big_delta: m.big_delta,
            tau: m.tau,
            shape_factor: m.shape_factor,
        }
    }

    pub fn to_meta(self) -> DiffusionMeta {
        DiffusionMeta {
            gamma: self.gamma,
            delta: self.delta,
            big_delta: self.big_delta,
            tau: self.tau,
            shape_factor: self.shape_factor,
        }
    }
}

/// Fixed board-frame metrics (pt) for a sheet's painted spreadsheet preview: a
/// header row plus up to `SHEET_MAX_ROWS` value rows. Chosen so a typical sheet
/// frame is comparable in board footprint to a small page.
pub const SHEET_COL_W_PT: f32 = 96.0;
pub const SHEET_ROW_H_PT: f32 = 18.0;
pub const SHEET_HEADER_H_PT: f32 = 22.0;
pub const SHEET_MAX_ROWS: usize = 24;

/// App-layer wrapper around an immutable typed table revision.
#[derive(Clone)]
pub struct TableDataset {
    pub resource_id: crate::state::DatasetId,
    /// Executable extraction recipe used to refresh this immutable table.
    pub provenance: Option<TableProvenance>,
    /// Domain constants consumed by analysis bindings.
    pub meta: TableMeta,
    /// Immutable curve-fit snapshots owned by this dataset revision.
    pub curve_fit_analyses: Vec<StoredCurveFitAnalysis>,
    /// Optional chart/analysis presentation bindings. These are not structural
    /// table fields: generic and relational result tables may leave them empty.
    pub x_binding: Option<ColumnId>,
    pub series_bindings: Vec<TableSeriesBinding>,
    pub name: Option<String>,
    /// Generic data-browser relationship, separate from executable provenance.
    pub lineage: Option<crate::state::DatasetLineage>,
    /// Top-left of this sheet's frame on the board, in world (pt) space. Parallels
    /// `CanvasDocument.board_pos` — a sheet is a first-class board frame. Defaulted
    /// on load so projects saved before board sheets still open.
    pub board_pos: [f32; 2],
    pub peaks: crate::state::PeakSet,
    pub line_fits: Vec<crate::state::StoredLineFit>,
    /// Id source for stored line fits; rebuilt from the loaded fits.
    pub next_line_fit_id: u64,
    /// Classical statistics run against this table's columns. Defaulted on load
    /// so projects saved before the statistics feature still open.
    pub statistics: Vec<crate::state::StatAnalysis>,
    /// Exact original import objects retained by the typed project envelope.
    pub import_sources: Vec<TableImportSource>,
    pub typed_state: TypedTableState,
    /// Id source for stored analyses; rebuilt from the loaded list.
    pub next_stat_id: u64,
}

#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct TableSeriesBinding {
    pub value_column: ColumnId,
    pub uncertainty_column: Option<ColumnId>,
    pub fit: Option<CurveFitReference>,
}

impl TableDataset {
    /// Construct a generic typed table with no implicit x/y assumptions.
    pub fn from_typed(typed_state: TypedTableState) -> Self {
        Self {
            resource_id: new_resource_id(),
            provenance: None,
            meta: TableMeta::default(),
            curve_fit_analyses: Vec::new(),
            x_binding: None,
            series_bindings: Vec::new(),
            name: None,
            lineage: None,
            board_pos: [0.0, 0.0],
            peaks: crate::state::PeakSet::default(),
            line_fits: Vec::new(),
            next_line_fit_id: 0,
            statistics: Vec::new(),
            import_sources: Vec::new(),
            typed_state,
            next_stat_id: 0,
        }
    }

    pub fn next_curve_fit_id(&self) -> u64 {
        self.curve_fit_analyses
            .iter()
            .map(|analysis| analysis.id.saturating_add(1))
            .max()
            .unwrap_or(0)
    }

    /// Columns drawn in the sheet preview: the x ruler plus one per data column.
    pub fn sheet_cols(&self) -> usize {
        self.typed_state
            .envelope
            .revision
            .snapshot
            .schema
            .columns
            .len()
    }

    /// Value rows actually painted, capped so a long table stays a tidy frame.
    pub fn visible_rows(&self) -> usize {
        self.typed_row_count().min(SHEET_MAX_ROWS)
    }

    /// Whether the table has more rows than the preview shows (an overflow footer
    /// row is then reserved).
    pub fn rows_overflow(&self) -> bool {
        self.typed_row_count() > SHEET_MAX_ROWS
    }

    fn typed_row_count(&self) -> usize {
        usize::try_from(self.typed_state.envelope.revision.snapshot.row_count).unwrap_or(usize::MAX)
    }

    /// The sheet frame's size on the board (pt), from its column/row counts.
    pub fn sheet_size_pt(&self) -> [f32; 2] {
        let cols = self.sheet_cols().max(1) as f32;
        let rows = self.visible_rows() as f32;
        let footer = if self.rows_overflow() {
            SHEET_ROW_H_PT
        } else {
            0.0
        };
        [
            cols * SHEET_COL_W_PT,
            SHEET_HEADER_H_PT + rows * SHEET_ROW_H_PT + footer,
        ]
    }

    /// This sheet's rect on the board (pt), analogous to `CanvasDocument::board_rect_pt`.
    pub fn board_rect_pt(&self) -> plotx_render::Rect {
        let [w, h] = self.sheet_size_pt();
        plotx_render::Rect::new(self.board_pos[0], self.board_pos[1], w, h)
    }

    pub fn summary(&self) -> String {
        let snapshot = &self.typed_state.envelope.revision.snapshot;
        format!(
            "{} columns · {} rows",
            snapshot.schema.columns.len(),
            snapshot.row_count,
        )
    }

    pub fn figure(&self) -> Figure {
        const MAX_PLOT_POINTS: usize = 20_000;
        let plot = self.typed_plot_data(MAX_PLOT_POINTS).unwrap_or_default();
        let (mut xlo, mut xhi) = (f64::INFINITY, f64::NEG_INFINITY);
        for &v in &plot.x {
            if v.is_finite() {
                xlo = xlo.min(v);
                xhi = xhi.max(v);
            }
        }
        // Fit overlays come from the immutable analysis snapshot, so reopening a
        // project never depends on the current global model library.
        let fit_curves: Vec<(usize, Vec<[f64; 2]>)> = if xhi > xlo {
            plot.series
                .iter()
                .enumerate()
                .filter_map(|(index, series)| {
                    let reference = series.binding.fit.as_ref()?;
                    let analysis = self
                        .curve_fit_analyses
                        .iter()
                        .find(|analysis| analysis.id == reference.analysis_id)?;
                    let curve = analysis
                        .plot_samples
                        .get(&reference.instance_id)?
                        .get(&reference.response)?
                        .clone();
                    Some((index, curve))
                })
                .collect()
        } else {
            Vec::new()
        };

        let (mut ylo, mut yhi) = (f64::INFINITY, f64::NEG_INFINITY);
        for series in &plot.series {
            for (row, (&px, &v)) in plot.x.iter().zip(&series.y).enumerate() {
                if px.is_finite() && v.is_finite() {
                    ylo = ylo.min(v);
                    yhi = yhi.max(v);
                    if let Some(sigma) = series
                        .uncertainty
                        .as_ref()
                        .and_then(|values| values.get(row))
                        .copied()
                        .filter(|value| value.is_finite() && *value > 0.0)
                    {
                        ylo = ylo.min(v - sigma);
                        yhi = yhi.max(v + sigma);
                    }
                }
            }
        }
        for (_, curve) in &fit_curves {
            for &[_, v] in curve {
                if v.is_finite() {
                    ylo = ylo.min(v);
                    yhi = yhi.max(v);
                }
            }
        }
        if !xlo.is_finite() {
            (xlo, xhi) = (0.0, 1.0);
        }
        if !ylo.is_finite() {
            (ylo, yhi) = (0.0, 1.0);
        }
        let xr = (xhi - xlo).max(f64::MIN_POSITIVE);
        let yr = (yhi - ylo).max(f64::MIN_POSITIVE);
        let x_label = if plot.x_label.is_empty() {
            "x".to_owned()
        } else {
            plot.x_label.clone()
        };
        let x = Axis::new(x_label, xlo - 0.02 * xr, xhi + 0.02 * xr);
        let y = Axis::new(
            "Intensity (a.u.)",
            (ylo - 0.05 * yr).min(0.0),
            yhi + 0.08 * yr,
        );
        let title = self.name.clone().unwrap_or_else(|| "Data table".to_owned());
        let mut fig = Figure::new(title, x, y);
        for (i, series) in plot.series.iter().enumerate() {
            let color = series_color(i);
            let mut points = Vec::with_capacity(plot.x.len().min(series.y.len()));
            for (row, (&px, &py)) in plot.x.iter().zip(&series.y).enumerate() {
                points.push([px, py]);
                if px.is_finite()
                    && py.is_finite()
                    && let Some(sigma) = series
                        .uncertainty
                        .as_ref()
                        .and_then(|values| values.get(row))
                        .copied()
                        .filter(|value| value.is_finite() && *value > 0.0)
                {
                    fig.error_bars
                        .push(ErrorBar::symmetric([px, py], sigma).colored(color));
                }
            }
            fig.series
                .push(Series::points(series.name.clone(), points).colored(color));
        }
        for (i, curve) in fit_curves {
            let name = format!("{} fit", plot.series[i].name);
            fig = fig.with_series(Series::line(name, curve).colored(series_color(i)));
        }
        fig
    }
}

const PALETTE: [Color; 6] = [
    Color::rgb(0x1f, 0x6f, 0xeb),
    Color::rgb(0xd1, 0x24, 0x2a),
    Color::rgb(0x1a, 0x7f, 0x37),
    Color::rgb(0x94, 0x3a, 0xba),
    Color::rgb(0xbf, 0x83, 0x00),
    Color::rgb(0x0a, 0x7d, 0x8c),
];

pub(crate) fn series_color(index: usize) -> Color {
    PALETTE[index % PALETTE.len()]
}

#[cfg(test)]
#[path = "table_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "table_curve_fit_tests.rs"]
mod curve_fit_snapshot_tests;
