//! The chart registry: the catalog of chart types and the data domains they apply to.

use super::*;

/// Derived from a `Dataset` via [`Dataset::domain`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DataDomain {
    Nmr1d,
    Nmr2d,
    PseudoNmr,
    Table,
    Electrophysiology,
    Afm,
}

/// How a domain's datasets combine when several are stacked onto one plot:
/// [`Line`](StackKind::Line) traces share an axis (superimposed / offset), while
/// [`Field`](StackKind::Field) datasets overlay their 2D contours in distinct
/// colours on one canvas.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StackKind {
    Line,
    Field,
}

impl DataDomain {
    /// `None` for domains excluded from generic stacking; `PseudoNmr` self-stacks
    /// its own increments so it is excluded.
    pub fn stack_kind(self) -> Option<StackKind> {
        match self {
            DataDomain::Nmr1d | DataDomain::Table | DataDomain::Electrophysiology => {
                Some(StackKind::Line)
            }
            DataDomain::Nmr2d => Some(StackKind::Field),
            DataDomain::PseudoNmr | DataDomain::Afm => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct ChartContext {
    /// Stable column binding for a column-oriented chart. `None` resolves to
    /// the first numeric response only at the legacy rendering boundary.
    pub column: Option<plotx_data::ColumnId>,
    /// Histogram bin count; `None` = automatic (Freedman–Diaconis).
    pub bins: Option<usize>,
    /// Multi-column bar mode: stacked instead of grouped.
    pub stacked: bool,
    /// Colormap for value-mapped charts (heatmap, 3D surface).
    pub colormap: plotx_figure::ColormapId,
    /// 3D surface view as `[azimuth°, elevation°]`.
    pub view_angles: [f32; 2],
}

/// `id` is persisted in `.plotx`.
pub struct ChartType {
    pub id: &'static str,
    pub name: &'static str,
    pub domains: &'static [DataDomain],
    pub needs_column: bool,
    pub build: fn(&Dataset, &ChartContext) -> Option<Figure>,
}

/// The catalog. The first entry for a domain is that domain's default chart, so
/// old `.plotx` files (no recorded chart type) map to it.
static CHART_TYPES: &[ChartType] = &[
    ChartType {
        id: "afm_map",
        name: "AFM Map",
        domains: &[DataDomain::Afm],
        needs_column: false,
        build: build_afm_map,
    },
    ChartType {
        id: "afm_force_curve",
        name: "Force Curve",
        domains: &[DataDomain::Afm],
        needs_column: false,
        build: build_afm_force,
    },
    ChartType {
        id: "electrophysiology_sweeps",
        name: "Sweeps",
        domains: &[DataDomain::Electrophysiology],
        needs_column: false,
        build: build_electrophysiology,
    },
    ChartType {
        id: "nmr_spectrum",
        name: "Spectrum",
        domains: &[DataDomain::Nmr1d],
        needs_column: false,
        build: build_nmr_spectrum,
    },
    ChartType {
        id: "nmr_contour",
        name: "Contour",
        domains: &[DataDomain::Nmr2d],
        needs_column: false,
        build: build_nmr_2d,
    },
    ChartType {
        id: "nmr_pseudo",
        name: "Stack / analysis",
        domains: &[DataDomain::PseudoNmr],
        needs_column: false,
        build: build_nmr_2d,
    },
    ChartType {
        id: "table_line",
        name: "Line",
        domains: &[DataDomain::Table],
        needs_column: false,
        build: build_table_line,
    },
    ChartType {
        id: "table_bar",
        name: "Bar",
        domains: &[DataDomain::Table],
        needs_column: true,
        build: build_table_bar,
    },
    ChartType {
        id: "table_bar_grouped",
        name: "Grouped bars",
        domains: &[DataDomain::Table],
        needs_column: false,
        build: build_table_bar_grouped,
    },
    ChartType {
        id: "table_histogram",
        name: "Histogram",
        domains: &[DataDomain::Table],
        needs_column: true,
        build: build_table_histogram,
    },
    ChartType {
        id: "table_box",
        name: "Box",
        domains: &[DataDomain::Table],
        needs_column: false,
        build: build_table_box,
    },
    ChartType {
        id: "table_violin",
        name: "Violin",
        domains: &[DataDomain::Table],
        needs_column: false,
        build: build_table_violin,
    },
    ChartType {
        id: "table_heatmap",
        name: "Heatmap",
        domains: &[DataDomain::Table],
        needs_column: false,
        build: build_table_heatmap,
    },
    ChartType {
        id: "table_pie",
        name: "Pie",
        domains: &[DataDomain::Table],
        needs_column: true,
        build: build_table_pie,
    },
    ChartType {
        id: "table_surface",
        name: "Surface 3D",
        domains: &[DataDomain::Table],
        needs_column: false,
        build: build_table_surface,
    },
];

pub fn chart_type(id: &str) -> Option<&'static ChartType> {
    CHART_TYPES.iter().find(|c| c.id == id)
}

pub fn chart_types_for(domain: DataDomain) -> Vec<&'static ChartType> {
    CHART_TYPES
        .iter()
        .filter(|c| c.domains.contains(&domain))
        .collect()
}

/// The chart type a stored id resolves to for a domain: the id itself when it
/// is valid there, otherwise the domain default (old files, ids from newer
/// builds, or a binding switched to another domain).
pub fn resolved_chart_type(domain: DataDomain, id: &str) -> &'static ChartType {
    chart_type(id)
        .filter(|candidate| candidate.domains.contains(&domain))
        .unwrap_or_else(|| default_chart_type(domain))
}

/// A domain's default chart type (its first registered entry). Every domain has
/// at least one, so this never fails for a domain produced by `Dataset::domain`.
pub fn default_chart_type(domain: DataDomain) -> &'static ChartType {
    CHART_TYPES
        .iter()
        .find(|c| c.domains.contains(&domain))
        .expect("every data domain registers at least one chart type")
}

/// Default 3D surface view: slightly rotated and elevated so all three faces read.
pub const SURFACE_DEFAULT_VIEW: [f32; 2] = [-50.0, 30.0];

/// An empty `type_id` resolves to the dataset domain's default when the figure
/// is built.
#[derive(Clone, Debug, PartialEq)]
pub struct ChartSpec {
    pub type_id: String,
    /// Stable table column binding. `None` selects the first numeric column.
    pub column: Option<plotx_data::ColumnId>,
    /// Histogram bin count; `None` = automatic.
    pub bins: Option<usize>,
    /// Multi-column bars: stacked instead of grouped.
    pub stacked: bool,
    pub colormap: plotx_figure::ColormapId,
    /// 3D surface view as `[azimuth°, elevation°]`.
    pub view_angles: [f32; 2],
}

impl Default for ChartSpec {
    fn default() -> Self {
        Self {
            type_id: String::new(),
            column: None,
            bins: None,
            stacked: false,
            colormap: plotx_figure::ColormapId::default(),
            view_angles: SURFACE_DEFAULT_VIEW,
        }
    }
}

impl ChartSpec {
    pub fn default_for(domain: DataDomain) -> Self {
        Self {
            type_id: default_chart_type(domain).id.to_owned(),
            ..Self::default()
        }
    }

    pub fn context(&self, _dataset: &Dataset) -> ChartContext {
        ChartContext {
            column: self.column,
            bins: self.bins,
            stacked: self.stacked,
            colormap: self.colormap,
            view_angles: self.view_angles,
        }
    }
}

fn build_nmr_spectrum(dataset: &Dataset, _ctx: &ChartContext) -> Option<Figure> {
    let n = dataset.as_nmr()?;
    Some(build_figure(&n.data, &n.spectrum, &n.peaks.resolve()))
}

fn build_electrophysiology(dataset: &Dataset, _ctx: &ChartContext) -> Option<Figure> {
    Some(dataset.as_electrophysiology()?.figure())
}

fn build_afm_map(dataset: &Dataset, ctx: &ChartContext) -> Option<Figure> {
    dataset.as_afm()?.map_figure(ctx.colormap)
}

fn build_afm_force(dataset: &Dataset, _ctx: &ChartContext) -> Option<Figure> {
    dataset.as_afm()?.force_figure()
}

fn build_nmr_2d(dataset: &Dataset, _ctx: &ChartContext) -> Option<Figure> {
    Some(dataset.as_nmr2d()?.figure())
}

fn build_table_line(dataset: &Dataset, _ctx: &ChartContext) -> Option<Figure> {
    let t = dataset.as_table()?;
    Some(apply_peak_labels(t.figure(), &t.peaks.resolve()))
}

fn build_table_bar(dataset: &Dataset, ctx: &ChartContext) -> Option<Figure> {
    let table = dataset.as_table()?;
    Some(table.bar_figure(ctx.column))
}

fn build_table_bar_grouped(dataset: &Dataset, ctx: &ChartContext) -> Option<Figure> {
    Some(table_charts::grouped_bar_figure(dataset.as_table()?, ctx))
}

fn build_table_histogram(dataset: &Dataset, ctx: &ChartContext) -> Option<Figure> {
    Some(table_charts::histogram_figure(dataset.as_table()?, ctx))
}

fn build_table_box(dataset: &Dataset, ctx: &ChartContext) -> Option<Figure> {
    Some(table_charts::box_figure(dataset.as_table()?, ctx))
}

fn build_table_violin(dataset: &Dataset, ctx: &ChartContext) -> Option<Figure> {
    Some(table_charts::violin_figure(dataset.as_table()?, ctx))
}

fn build_table_heatmap(dataset: &Dataset, ctx: &ChartContext) -> Option<Figure> {
    Some(table_charts::heatmap_figure(dataset.as_table()?, ctx))
}

fn build_table_pie(dataset: &Dataset, ctx: &ChartContext) -> Option<Figure> {
    Some(table_charts::pie_figure(dataset.as_table()?, ctx))
}

fn build_table_surface(dataset: &Dataset, ctx: &ChartContext) -> Option<Figure> {
    Some(table_charts::surface_figure(dataset.as_table()?, ctx))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_domain_lists_all_generic_charts_with_line_default() {
        let ids: Vec<&str> = chart_types_for(DataDomain::Table)
            .iter()
            .map(|c| c.id)
            .collect();
        assert_eq!(
            ids,
            vec![
                "table_line",
                "table_bar",
                "table_bar_grouped",
                "table_histogram",
                "table_box",
                "table_violin",
                "table_heatmap",
                "table_pie",
                "table_surface",
            ]
        );
        assert_eq!(default_chart_type(DataDomain::Table).id, "table_line");
    }

    #[test]
    fn chart_type_lookup_round_trips_and_reports_column_need() {
        assert_eq!(chart_type("table_bar").unwrap().name, "Bar");
        assert!(chart_type("table_bar").unwrap().needs_column);
        assert!(!chart_type("table_line").unwrap().needs_column);
        assert!(chart_type("does_not_exist").is_none());
    }

    #[test]
    fn stack_kind_maps_line_field_and_excludes_pseudo() {
        assert_eq!(DataDomain::Nmr1d.stack_kind(), Some(StackKind::Line));
        assert_eq!(DataDomain::Table.stack_kind(), Some(StackKind::Line));
        assert_eq!(DataDomain::Nmr2d.stack_kind(), Some(StackKind::Field));
        assert_eq!(DataDomain::PseudoNmr.stack_kind(), None);
    }

    #[test]
    fn each_domain_has_a_default_chart() {
        for domain in [
            DataDomain::Nmr1d,
            DataDomain::Nmr2d,
            DataDomain::PseudoNmr,
            DataDomain::Table,
        ] {
            assert!(chart_type(default_chart_type(domain).id).is_some());
        }
    }
}
