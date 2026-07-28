//! The chart registry. Data domains provide recommended defaults only; concrete
//! encoding applicability is determined by field capabilities.

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

/// A chart registration. `recommended_domains` preserves sensible defaults for
/// existing entry points, but it is never an applicability gate.
pub struct ChartDescriptor {
    pub id: &'static str,
    pub name: &'static str,
    pub recommended_domains: &'static [DataDomain],
    /// Stable `CapabilityId` string identities required by this chart.
    pub required_capabilities: &'static [&'static str],
    pub needs_column: bool,
    pub build: fn(&Dataset, &ChartContext) -> Option<Figure>,
}

/// Legacy spelling retained for the broad table-chart surface. New code should
/// use `ChartDescriptor` and field capabilities.
pub type ChartType = ChartDescriptor;

impl ChartDescriptor {
    pub fn is_applicable_to(&self, capabilities: &FieldCapabilities) -> bool {
        capabilities.supports(self.required_capabilities)
    }
}

/// The catalog. The first entry for a domain is that domain's default chart, so
/// old `.plotx` files (no recorded chart type) map to it.
static CHART_TYPES: &[ChartDescriptor] = &[
    ChartDescriptor {
        id: "afm_map",
        name: "AFM Map",
        recommended_domains: &[DataDomain::Afm],
        required_capabilities: &[
            crate::automation::CAP_FIELD_SCALAR_GRID_2D_REGULAR,
            crate::automation::CAP_FIELD_AFM_MAP,
        ],
        needs_column: false,
        build: build_afm_map,
    },
    ChartDescriptor {
        id: "afm_force_curve",
        name: "Force Curve",
        recommended_domains: &[DataDomain::Afm],
        required_capabilities: &[
            crate::automation::CAP_FIELD_CURVE_1D,
            crate::automation::CAP_FIELD_FORCE_CURVE,
        ],
        needs_column: false,
        build: build_afm_force,
    },
    ChartDescriptor {
        id: "electrophysiology_sweeps",
        name: "Sweeps",
        recommended_domains: &[DataDomain::Electrophysiology],
        required_capabilities: &[
            crate::automation::CAP_FIELD_CURVE_1D,
            crate::automation::CAP_FIELD_SWEEP_COLLECTION,
        ],
        needs_column: false,
        build: build_electrophysiology,
    },
    ChartDescriptor {
        id: "nmr_spectrum",
        name: "NMR signal",
        recommended_domains: &[DataDomain::Nmr1d],
        required_capabilities: &[
            crate::automation::CAP_FIELD_CURVE_1D,
            crate::automation::CAP_FIELD_NMR_SIGNAL,
        ],
        needs_column: false,
        build: build_nmr_spectrum,
    },
    ChartDescriptor {
        id: "nmr_contour",
        name: "Contour",
        recommended_domains: &[DataDomain::Nmr2d],
        required_capabilities: &[
            crate::automation::CAP_FIELD_SCALAR_GRID_2D_REGULAR,
            crate::automation::CAP_FIELD_NMR_CONTOUR,
        ],
        needs_column: false,
        build: build_nmr_2d,
    },
    ChartDescriptor {
        id: "nmr_pseudo",
        name: "Stack / analysis",
        recommended_domains: &[DataDomain::PseudoNmr],
        required_capabilities: &[
            crate::automation::CAP_FIELD_CURVE_1D,
            crate::automation::CAP_FIELD_NMR_STACK,
        ],
        needs_column: false,
        build: build_nmr_2d,
    },
    ChartDescriptor {
        id: "table_line",
        name: "Line",
        recommended_domains: &[DataDomain::Table],
        required_capabilities: &[
            crate::automation::CAP_FIELD_CURVE_1D,
            crate::automation::CAP_FIELD_TABLE,
        ],
        needs_column: false,
        build: build_table_line,
    },
    ChartDescriptor {
        id: "table_bar",
        name: "Bar",
        recommended_domains: &[DataDomain::Table],
        required_capabilities: &[crate::automation::CAP_FIELD_TABLE],
        needs_column: true,
        build: build_table_bar,
    },
    ChartDescriptor {
        id: "table_bar_grouped",
        name: "Grouped bars",
        recommended_domains: &[DataDomain::Table],
        required_capabilities: &[crate::automation::CAP_FIELD_TABLE],
        needs_column: false,
        build: build_table_bar_grouped,
    },
    ChartDescriptor {
        id: "table_histogram",
        name: "Histogram",
        recommended_domains: &[DataDomain::Table],
        required_capabilities: &[crate::automation::CAP_FIELD_TABLE],
        needs_column: true,
        build: build_table_histogram,
    },
    ChartDescriptor {
        id: "table_box",
        name: "Box",
        recommended_domains: &[DataDomain::Table],
        required_capabilities: &[crate::automation::CAP_FIELD_TABLE],
        needs_column: false,
        build: build_table_box,
    },
    ChartDescriptor {
        id: "table_violin",
        name: "Violin",
        recommended_domains: &[DataDomain::Table],
        required_capabilities: &[crate::automation::CAP_FIELD_TABLE],
        needs_column: false,
        build: build_table_violin,
    },
    ChartDescriptor {
        id: "table_heatmap",
        name: "Heatmap",
        recommended_domains: &[DataDomain::Table],
        required_capabilities: &[crate::automation::CAP_FIELD_TABLE],
        needs_column: false,
        build: build_table_heatmap,
    },
    ChartDescriptor {
        id: "table_pie",
        name: "Pie",
        recommended_domains: &[DataDomain::Table],
        required_capabilities: &[crate::automation::CAP_FIELD_TABLE],
        needs_column: true,
        build: build_table_pie,
    },
    ChartDescriptor {
        id: "table_surface",
        name: "Surface 3D",
        recommended_domains: &[DataDomain::Table],
        required_capabilities: &[crate::automation::CAP_FIELD_TABLE],
        needs_column: false,
        build: build_table_surface,
    },
];

pub fn chart_type(id: &str) -> Option<&'static ChartDescriptor> {
    CHART_TYPES.iter().find(|c| c.id == id)
}

pub fn chart_types_for_capabilities(
    capabilities: &FieldCapabilities,
    preferred_domain: DataDomain,
) -> Vec<&'static ChartDescriptor> {
    let mut charts = CHART_TYPES
        .iter()
        .filter(|chart| chart.is_applicable_to(capabilities))
        .collect::<Vec<_>>();
    // Domains express presentation preference only. Applicability was decided
    // above from the field's capabilities.
    charts.sort_by_key(|chart| !chart.recommended_domains.contains(&preferred_domain));
    charts
}

/// Resolve an unknown stored id to the domain's presentation default. Callers
/// that materialize a binding must additionally check field capabilities.
pub fn resolved_chart_type(domain: DataDomain, id: &str) -> &'static ChartDescriptor {
    chart_type(id).unwrap_or_else(|| default_chart_type(domain))
}

/// Resolve a stored chart through the same field-capability gate used by the
/// gallery. This is the legacy-chart counterpart of encoding materialization.
pub fn resolved_chart_type_for_field(
    capabilities: &FieldCapabilities,
    domain: DataDomain,
    id: &str,
) -> &'static ChartDescriptor {
    chart_type(id)
        .filter(|chart| chart.is_applicable_to(capabilities))
        .unwrap_or_else(|| default_chart_type(domain))
}

/// A domain's default chart type (its first registered entry). Every domain has
/// at least one, so this never fails for a domain produced by `Dataset::domain`.
pub fn default_chart_type(domain: DataDomain) -> &'static ChartDescriptor {
    CHART_TYPES
        .iter()
        .find(|c| c.recommended_domains.contains(&domain))
        .expect("every data domain registers at least one chart type")
}

/// The domain-neutral visual encoding catalog. Adding a provider that exposes
/// a capability automatically makes the matching descriptor available; no
/// chart registry domain branch is needed.
pub struct EncodingDescriptor {
    pub id: &'static str,
    pub required_capabilities: &'static [&'static str],
}

impl EncodingDescriptor {
    pub fn is_applicable_to(&self, capabilities: &FieldCapabilities) -> bool {
        capabilities.supports(self.required_capabilities)
    }
}

pub static ENCODING_DESCRIPTORS: &[EncodingDescriptor] = &[
    EncodingDescriptor {
        id: "line",
        required_capabilities: &[crate::automation::CAP_FIELD_CURVE_1D],
    },
    EncodingDescriptor {
        id: "contour",
        required_capabilities: &[crate::automation::CAP_FIELD_SCALAR_GRID_2D_REGULAR],
    },
    EncodingDescriptor {
        id: "heatmap",
        required_capabilities: &[crate::automation::CAP_FIELD_SCALAR_GRID_2D_REGULAR],
    },
    EncodingDescriptor {
        id: "image",
        required_capabilities: &[crate::automation::CAP_FIELD_COLORED_RASTER_2D],
    },
];

pub fn encoding_descriptors_for(
    capabilities: &FieldCapabilities,
) -> Vec<&'static EncodingDescriptor> {
    ENCODING_DESCRIPTORS
        .iter()
        .filter(|descriptor| descriptor.is_applicable_to(capabilities))
        .collect()
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
    Some(build_processed_1d_figure(
        &n.data,
        &n.processed,
        &n.peaks.resolve(),
    ))
}

fn build_electrophysiology(dataset: &Dataset, _ctx: &ChartContext) -> Option<Figure> {
    Some(dataset.as_electrophysiology()?.figure())
}

fn build_afm_map(dataset: &Dataset, ctx: &ChartContext) -> Option<Figure> {
    let afm = dataset.as_afm()?;
    let field = dataset
        .field_descriptors()
        .into_iter()
        .find(|field| field.local_id.starts_with("afm.channel."))?
        .id;
    afm.map_figure(field, ctx.colormap)
}

fn build_afm_force(dataset: &Dataset, _ctx: &ChartContext) -> Option<Figure> {
    let afm = dataset.as_afm()?;
    let field = dataset
        .field_descriptors()
        .into_iter()
        .find(|field| field.local_id == "afm.force_curve")?
        .id;
    afm.force_figure(field)
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
        let capabilities = FieldCapabilities::new([
            crate::automation::CapabilityId::new(crate::automation::CAP_FIELD_CURVE_1D),
            crate::automation::CapabilityId::new(crate::automation::CAP_FIELD_TABLE),
        ]);
        let ids: Vec<&str> = chart_types_for_capabilities(&capabilities, DataDomain::Table)
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

    #[test]
    fn scalar_field_capability_unlocks_contour_and_heatmap_without_domain_registration() {
        let capabilities = FieldCapabilities::new([crate::automation::CapabilityId::new(
            crate::automation::CAP_FIELD_SCALAR_GRID_2D_REGULAR,
        )]);
        let ids = encoding_descriptors_for(&capabilities)
            .into_iter()
            .map(|descriptor| descriptor.id)
            .collect::<Vec<_>>();
        assert_eq!(ids, ["contour", "heatmap"]);
    }

    #[test]
    fn colored_raster_unlocks_only_image_not_scalar_encodings() {
        let capabilities = FieldCapabilities::new([crate::automation::CapabilityId::new(
            crate::automation::CAP_FIELD_COLORED_RASTER_2D,
        )]);
        let ids = encoding_descriptors_for(&capabilities)
            .into_iter()
            .map(|descriptor| descriptor.id)
            .collect::<Vec<_>>();
        assert_eq!(ids, ["image"]);
    }

    #[test]
    fn malformed_colored_scalar_field_is_excluded_from_contour_and_heatmap() {
        let capabilities = FieldCapabilities::new([
            crate::automation::CapabilityId::new(
                crate::automation::CAP_FIELD_SCALAR_GRID_2D_REGULAR,
            ),
            crate::automation::CapabilityId::new(crate::automation::CAP_FIELD_COLORED_RASTER_2D),
        ]);
        let ids = encoding_descriptors_for(&capabilities)
            .into_iter()
            .map(|descriptor| descriptor.id)
            .collect::<Vec<_>>();
        assert_eq!(ids, ["image"]);
    }
}
