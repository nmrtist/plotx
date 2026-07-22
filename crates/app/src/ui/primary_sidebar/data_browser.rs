use plotx_core::data::ColumnId;
use plotx_core::state::{Dataset, PlotxApp};
use std::collections::HashSet;

#[derive(Clone, Debug, PartialEq)]
pub(super) struct DataTree {
    pub roots: Vec<DatasetNode>,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct DatasetNode {
    pub dataset: usize,
    pub linked_reference: bool,
    pub cycle_cut: bool,
    pub analysis: Vec<AnalysisItem>,
    pub derived: Vec<DatasetNode>,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct AnalysisItem {
    pub kind: AnalysisKind,
    pub label: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum AnalysisKind {
    Peak(u64),
    Integral(u64),
    Region(u64),
    LineFit(u64),
    Multiplet(u64),
    CurveFitResponse(ColumnId),
}

impl AnalysisKind {
    pub fn key(&self, dataset: usize) -> String {
        match self {
            Self::Peak(id) => format!("peak:{dataset}:{id}"),
            Self::Integral(id) => format!("integral:{dataset}:{id}"),
            Self::Region(id) => format!("region:{dataset}:{id}"),
            Self::LineFit(id) => format!("line_fit:{dataset}:{id}"),
            Self::Multiplet(id) => format!("multiplet:{dataset}:{id}"),
            Self::CurveFitResponse(column) => format!("curve_fit_response:{dataset}:{column}"),
        }
    }

    pub fn type_label(&self) -> &'static str {
        match self {
            Self::Peak(_) => "Peak",
            Self::Integral(_) => "Integral",
            Self::Region(_) => "Region",
            Self::LineFit(_) => "Peak fit",
            Self::Multiplet(_) => "Multiplet",
            Self::CurveFitResponse(_) => "Curve fit response",
        }
    }
}

impl DataTree {
    pub fn build(app: &PlotxApp) -> Self {
        let count = app.doc.datasets.len();
        let mut children = vec![Vec::new(); count];
        for (derived, dataset) in app.doc.datasets.iter().enumerate() {
            if let Some(lineage) = dataset.lineage() {
                for &source in &lineage.sources {
                    if source < count && source != derived {
                        children[source].push(derived);
                    }
                }
            }
        }

        let mut roots: Vec<usize> = app
            .doc
            .datasets
            .iter()
            .enumerate()
            .filter(|(di, dataset)| {
                dataset.lineage().is_none_or(|lineage| {
                    lineage.sources.is_empty()
                        || lineage
                            .sources
                            .iter()
                            .all(|&source| source >= count || source == *di)
                })
            })
            .map(|(di, _)| di)
            .collect();

        // Corrupt in-memory graphs can consist only of a cycle. Keep every
        // dataset reachable even then; recursion below cuts repeated path nodes.
        let mut reachable = HashSet::new();
        for &root in &roots {
            mark_reachable(root, &children, &mut reachable);
        }
        for di in 0..count {
            if !reachable.contains(&di) {
                roots.push(di);
                mark_reachable(di, &children, &mut reachable);
            }
        }

        Self {
            roots: roots
                .into_iter()
                .map(|di| build_node(app, di, &children, &mut Vec::new()))
                .collect(),
        }
    }

    pub fn filtered(&self, app: &PlotxApp, query: &str) -> Self {
        let query = query.trim().to_lowercase();
        if query.is_empty() {
            return self.clone();
        }
        Self {
            roots: self
                .roots
                .iter()
                .filter_map(|node| filter_node(node, app, &query))
                .collect(),
        }
    }
}

fn mark_reachable(di: usize, children: &[Vec<usize>], seen: &mut HashSet<usize>) {
    if !seen.insert(di) {
        return;
    }
    for &child in &children[di] {
        mark_reachable(child, children, seen);
    }
}

fn build_node(
    app: &PlotxApp,
    di: usize,
    children: &[Vec<usize>],
    path: &mut Vec<usize>,
) -> DatasetNode {
    if path.contains(&di) {
        return DatasetNode {
            dataset: di,
            linked_reference: is_linked(app, di),
            cycle_cut: true,
            analysis: Vec::new(),
            derived: Vec::new(),
        };
    }
    path.push(di);
    let derived = children[di]
        .iter()
        .copied()
        .map(|child| build_node(app, child, children, path))
        .collect();
    path.pop();
    DatasetNode {
        dataset: di,
        linked_reference: is_linked(app, di),
        cycle_cut: false,
        analysis: analysis_items(&app.doc.datasets[di]),
        derived,
    }
}

fn is_linked(app: &PlotxApp, di: usize) -> bool {
    app.doc.datasets[di]
        .lineage()
        .is_some_and(|lineage| lineage.sources.len() > 1)
}

fn analysis_items(dataset: &Dataset) -> Vec<AnalysisItem> {
    let mut result = Vec::new();
    if let Some(peaks) = dataset.peaks() {
        result.extend(peaks.marks.iter().map(|peak| {
            AnalysisItem {
                kind: AnalysisKind::Peak(peak.id),
                label: peak
                    .label
                    .clone()
                    .filter(|label| !label.trim().is_empty())
                    .unwrap_or_else(|| format!("Peak {:.3}", peak.x)),
            }
        }));
    }
    if let Some(nmr) = dataset.as_nmr() {
        result.extend(nmr.integrals.iter().map(|integral| AnalysisItem {
            kind: AnalysisKind::Integral(integral.id),
            label: format!("Integral {:.3}–{:.3}", integral.start_ppm, integral.end_ppm),
        }));
    }
    if let Some(nmr2d) = dataset.as_nmr2d() {
        result.extend(nmr2d.integrals.iter().map(|integral| AnalysisItem {
            kind: AnalysisKind::Integral(integral.id),
            label: if integral.name.trim().is_empty() {
                format!("Integral #{}", integral.id + 1)
            } else {
                integral.name.clone()
            },
        }));
        result.extend(nmr2d.regions.iter().map(|region| AnalysisItem {
            kind: AnalysisKind::Region(region.id),
            label: region.column_name(),
        }));
    }
    result.extend(dataset.line_fits().iter().map(|fit| AnalysisItem {
        kind: AnalysisKind::LineFit(fit.id),
        label: format!("Peak fit #{} ({:.3}–{:.3})", fit.id + 1, fit.lo, fit.hi),
    }));
    result.extend(dataset.multiplets().iter().map(|multiplet| AnalysisItem {
        kind: AnalysisKind::Multiplet(multiplet.id),
        label: multiplet.descriptor(),
    }));
    if let Some(table) = dataset.as_table() {
        result.extend(
            table
                .series_bindings
                .iter()
                .enumerate()
                .filter(|(_, binding)| binding.fit.is_some())
                .map(|(_, binding)| AnalysisItem {
                    kind: AnalysisKind::CurveFitResponse(binding.value_column),
                    label: format!(
                        "{} fit",
                        table
                            .typed_state
                            .envelope
                            .revision
                            .snapshot
                            .schema
                            .column(binding.value_column)
                            .map_or("Value", |value| value.name.as_str())
                    ),
                }),
        );
    }
    result
}

fn filter_node(node: &DatasetNode, app: &PlotxApp, query: &str) -> Option<DatasetNode> {
    let dataset = &app.doc.datasets[node.dataset];
    let lineage_label = dataset
        .lineage()
        .map(|lineage| lineage.kind.label())
        .unwrap_or("Original data");
    let self_matches = [
        dataset.display_name(),
        dataset.kind_label().to_owned(),
        dataset.summary(),
        lineage_label.to_owned(),
    ]
    .iter()
    .any(|text| text.to_lowercase().contains(query));

    if self_matches {
        return Some(node.clone());
    }
    let analysis: Vec<_> = node
        .analysis
        .iter()
        .filter(|item| {
            item.label.to_lowercase().contains(query)
                || item.kind.type_label().to_lowercase().contains(query)
        })
        .cloned()
        .collect();
    let derived: Vec<_> = node
        .derived
        .iter()
        .filter_map(|child| filter_node(child, app, query))
        .collect();
    (!analysis.is_empty() || !derived.is_empty()).then_some(DatasetNode {
        dataset: node.dataset,
        linked_reference: node.linked_reference,
        cycle_cut: node.cycle_cut,
        analysis,
        derived,
    })
}

pub(super) fn reveal_active_path(app: &mut PlotxApp) {
    let active = app.active_dataset();
    if app.session.ui.data_browser_last_active == active {
        return;
    }
    app.session.ui.data_browser_last_active = active;
    let Some(active) = active else { return };
    let mut visiting = HashSet::new();
    reveal_sources(app, active, &mut visiting);
}

fn reveal_sources(app: &mut PlotxApp, di: usize, visiting: &mut HashSet<usize>) {
    if !visiting.insert(di) {
        return;
    }
    app.session.ui.data_browser_collapsed_datasets.remove(&di);
    let sources = app.doc.datasets[di]
        .lineage()
        .map(|lineage| lineage.sources.clone())
        .unwrap_or_default();
    for source in sources {
        if source < app.doc.datasets.len() {
            app.session
                .ui
                .data_browser_collapsed_datasets
                .remove(&source);
            app.session
                .ui
                .data_browser_collapsed_derived
                .remove(&source);
            reveal_sources(app, source, visiting);
        }
    }
    visiting.remove(&di);
}

pub(super) fn sources_tooltip(app: &PlotxApp, di: usize) -> Option<String> {
    let lineage = app.doc.datasets.get(di)?.lineage()?;
    let names: Vec<_> = lineage
        .sources
        .iter()
        .filter_map(|&source| app.doc.datasets.get(source))
        .map(Dataset::display_name)
        .collect();
    Some(format!(
        "{}\nSources: {}",
        lineage.kind.label(),
        names.join(", ")
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use plotx_core::state::{
        CurveFitReference, DatasetLineage, DerivationKind, FloatSeries, LineShapeKind,
        MultipletPatternKind, NmrDataset, PeakMark, PeakOrigin, StoredLineFit, StoredMultiplet,
        materialized_float_series_table,
    };
    use plotx_core::{DisplayModeLabel, IntegralResult};
    use plotx_io::{Domain, NmrData};

    fn root(name: &str) -> Dataset {
        let mut dataset = NmrDataset::load(NmrData {
            points: vec![1.0.into(), 0.0.into()],
            domain: Domain::Frequency,
            spectral_width_hz: 1.0,
            observe_freq_mhz: 1.0,
            carrier_ppm: 0.0,
            nucleus: "1H".into(),
            source: name.into(),
            group_delay: 0.0,
        });
        dataset.name = Some(name.into());
        Dataset::Nmr(Box::new(dataset))
    }

    fn derived(name: &str, kind: DerivationKind, sources: &[usize]) -> Dataset {
        let mut table = materialized_float_series_table(
            ("x".into(), "".into(), vec![Some(0.0)]),
            Vec::new(),
            "plotx.test.derived-table.v1",
        )
        .unwrap();
        table.name = Some(name.into());
        table.lineage = Some(DatasetLineage::new(kind, sources.iter().copied()));
        Dataset::Table(Box::new(table))
    }

    #[test]
    fn builds_deep_multi_source_references_in_stable_order() {
        let mut app = PlotxApp::new();
        app.doc.datasets = vec![
            root("A"),
            root("B"),
            derived("AB", DerivationKind::SpectrumArithmetic, &[0, 1]),
            derived("deep", DerivationKind::LineFitTable, &[2]),
        ];
        let tree = DataTree::build(&app);
        assert_eq!(
            tree.roots.iter().map(|n| n.dataset).collect::<Vec<_>>(),
            vec![0, 1]
        );
        assert_eq!(tree.roots[0].derived[0].dataset, 2);
        assert!(tree.roots[0].derived[0].linked_reference);
        assert_eq!(tree.roots[0].derived[0].derived[0].dataset, 3);
        assert_eq!(tree.roots[1].derived[0].dataset, 2);
    }

    #[test]
    fn filtering_keeps_ancestor_path() {
        let mut app = PlotxApp::new();
        app.doc.datasets = vec![
            root("source"),
            derived("result", DerivationKind::LineFitTable, &[0]),
        ];
        let filtered = DataTree::build(&app).filtered(&app, "peak fit table");
        assert_eq!(filtered.roots.len(), 1);
        assert_eq!(filtered.roots[0].dataset, 0);
        assert_eq!(filtered.roots[0].derived[0].dataset, 1);
    }

    #[test]
    fn cycles_are_cut_and_remain_accessible() {
        let mut app = PlotxApp::new();
        app.doc.datasets = vec![
            derived("A", DerivationKind::Slice, &[1]),
            derived("B", DerivationKind::Projection, &[0]),
        ];
        let tree = DataTree::build(&app);
        assert!(!tree.roots.is_empty());
        assert!(tree.roots[0].derived[0].derived[0].cycle_cut);
    }

    #[test]
    fn enumerates_stable_analysis_ids_and_fitted_columns() {
        let mut spectrum = root("analysis");
        let nmr = spectrum.as_nmr_mut().unwrap();
        nmr.peaks.marks.push(PeakMark {
            id: 11,
            x: 2.5,
            y: 1.0,
            origin: PeakOrigin::Manual,
            label: Some("solvent".to_owned()),
        });
        nmr.integrals.push(IntegralResult {
            id: 12,
            start_ppm: 1.0,
            end_ppm: 2.0,
            area: 1.0,
            normalized_area: 1.0,
            mode: DisplayModeLabel::Real,
            reference_value: None,
        });
        nmr.line_fits.push(StoredLineFit {
            id: 13,
            lo: 1.0,
            hi: 2.0,
            shape: LineShapeKind::Lorentzian,
            peaks: Vec::new(),
            offset: 0.0,
            offset_sigma: None,
            r2: 0.99,
        });
        nmr.multiplets.push(StoredMultiplet {
            id: 14,
            lo: 1.0,
            hi: 2.0,
            center_ppm: 1.5,
            pattern: MultipletPatternKind::Singlet,
            j_values: Vec::new(),
            area: 1.0,
            peak_ppm: vec![1.5],
        });

        let table = materialized_float_series_table(
            ("x".into(), "".into(), vec![Some(0.0)]),
            vec![FloatSeries {
                name: "decay".to_owned(),
                unit: String::new(),
                values: vec![Some(1.0)],
                uncertainty: None,
                fit: Some(CurveFitReference {
                    analysis_id: 0,
                    instance_id: "column-0".into(),
                    response: "y".into(),
                }),
            }],
            "plotx.test.analysis-table.v1",
        )
        .unwrap();

        let mut app = PlotxApp::new();
        app.doc.datasets = vec![spectrum, Dataset::Table(Box::new(table))];
        let tree = DataTree::build(&app);
        let kinds: Vec<_> = tree.roots[0]
            .analysis
            .iter()
            .map(|item| item.kind.clone())
            .collect();
        assert_eq!(
            kinds,
            vec![
                AnalysisKind::Peak(11),
                AnalysisKind::Integral(12),
                AnalysisKind::LineFit(13),
                AnalysisKind::Multiplet(14),
            ]
        );
        assert_eq!(
            tree.roots[1].analysis[0].kind,
            AnalysisKind::CurveFitResponse(
                app.doc.datasets[1].as_table().unwrap().series_bindings[0].value_column
            )
        );
    }

    #[test]
    fn external_focus_reveals_ancestor_branches() {
        let mut app = PlotxApp::new();
        app.doc.datasets = vec![
            root("source"),
            derived("child", DerivationKind::Slice, &[0]),
            derived("grandchild", DerivationKind::LineFitTable, &[1]),
        ];
        app.session
            .ui
            .data_browser_collapsed_datasets
            .extend([0, 1, 2]);
        app.session.ui.data_browser_collapsed_derived.extend([0, 1]);
        app.focus_single(2);

        reveal_active_path(&mut app);

        assert!(app.session.ui.data_browser_collapsed_datasets.is_empty());
        assert!(app.session.ui.data_browser_collapsed_derived.is_empty());
    }
}
