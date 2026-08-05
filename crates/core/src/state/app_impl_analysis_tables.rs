use super::app_impl_analysis::{
    TableFitInputs, build_table_fit_inputs, resolve_table_fit_model, table_fit_plot_samples,
};
use super::*;

fn reduce_electrophysiology_region(
    recording: &ElectrophysiologyDataset,
    sweep: usize,
    channel: usize,
    region: &Region,
    metric: RegionMetric,
) -> Result<f64, String> {
    let values = recording
        .processed_trace(sweep, channel)
        .map_err(|error| error.to_string())?;
    let rate = recording.data.sample_rate_hz;
    if !rate.is_finite() || rate <= 0.0 {
        return Err("The recording has an invalid sample rate.".to_owned());
    }
    let lo = region.lo_min().max(0.0);
    let hi = region.hi_max().max(lo);
    let start = (lo * rate).floor().max(0.0) as usize;
    let end = ((hi * rate).ceil() as usize).min(values.len());
    let slice = values
        .get(start..end)
        .filter(|slice| !slice.is_empty())
        .ok_or_else(|| "A region does not overlap the selected sweep.".to_owned())?;
    if slice.iter().any(|value| !value.is_finite()) {
        return Err("A region contains non-finite samples.".to_owned());
    }
    let value = match metric {
        RegionMetric::Height => {
            plotx_analysis::electrophysiology::window_statistics(
                &values,
                rate,
                0.0,
                plotx_analysis::electrophysiology::TimeWindow {
                    start_s: lo,
                    end_s: hi,
                },
                recording.peak_mode,
            )
            .map_err(|error| error.to_string())?
            .peak
        }
        RegionMetric::Area => slice
            .windows(2)
            .map(|pair| 0.5 * (pair[0] + pair[1]) / rate)
            .sum(),
        RegionMetric::Mean => slice.iter().sum::<f64>() / slice.len() as f64,
        RegionMetric::Max => slice.iter().copied().fold(f64::NEG_INFINITY, f64::max),
        RegionMetric::Min => slice.iter().copied().fold(f64::INFINITY, f64::min),
    };
    value
        .is_finite()
        .then_some(value)
        .ok_or_else(|| "A region contains no finite samples.".to_owned())
}

impl PlotxApp {
    /// Create an empty editable data table from scratch: a small starter grid,
    /// placed as a collision-free board sheet frame, selected, with its
    /// editable sheet window opened for immediate row/column authoring.
    pub fn new_table_dataset(&mut self) {
        let (mut x_schema, x) =
            materialized_float_column("x", "", [Some(0.0), Some(1.0), Some(2.0)]);
        x_schema.role = plotx_data::SemanticRole::Custom("space.nmrtist.plotx.axis.x".into());
        let x_binding = x_schema.id;
        let (y_schema, y) = materialized_float_column("y", "", [Some(0.0), Some(0.0), Some(0.0)]);
        let series = TableSeriesBinding {
            value_column: y_schema.id,
            uncertainty_column: None,
            fit: None,
        };
        let sheet_index = self
            .doc
            .datasets
            .iter()
            .filter(|d| matches!(d, Dataset::Table(_)))
            .count();
        let mut tds = TableDataset::from_materialized(
            vec![(x_schema, x), (y_schema, y)],
            Vec::new(),
            Some(x_binding),
            vec![series],
            "plotx.table.new.v1",
        )
        .expect("the fixed starter table is valid");
        tds.name = Some(format!("Table {}", sheet_index + 1));
        let sheet = tds.board_rect_pt();
        tds.board_pos = crate::state::next_board_frame_pos(self, [sheet.width, sheet.height]);
        self.doc.datasets.push(Dataset::Table(Box::new(tds)));
        let di = self.doc.datasets.len() - 1;
        self.focus_single(di);
        self.session.view = PrimaryView::Data;
        self.session.ui.frame_selection =
            vec![BoardFrameId::Sheet(self.doc.datasets[di].resource_id())];
        self.session.ui.sheet_open = Some(di);
        self.mark_document_dirty();
        self.session.status = "Created a data table.".to_owned();
    }

    pub fn insert_typed_table_dataset(&mut self, mut dataset: TableDataset, name: String) -> usize {
        dataset.name = Some(name.clone());
        self.insert_table_dataset(dataset, name)
    }

    pub fn import_table_dataset_typed(
        &mut self,
        name: String,
        import_sources: Vec<TableImportSource>,
        typed_state: TypedTableState,
        x_binding: Option<plotx_data::ColumnId>,
        series_bindings: Vec<TableSeriesBinding>,
    ) -> usize {
        let mut dataset = TableDataset::from_typed(typed_state);
        dataset.name = Some(name.clone());
        dataset.import_sources = import_sources;
        dataset.x_binding = x_binding;
        dataset.series_bindings = series_bindings;
        self.insert_table_dataset(dataset, name)
    }

    fn insert_table_dataset(&mut self, dataset: TableDataset, name: String) -> usize {
        let dataset_index = self.doc.datasets.len();
        let action = Action::insert_dataset_with_default_canvas(
            self,
            Dataset::Table(Box::new(dataset)),
            format!("Canvas {} - {name}", self.doc.canvases.len() + 1),
            DEFAULT_CANVAS_SIZE_MM,
        );
        self.execute_action(action);
        self.focus_single(dataset_index);
        self.session.view = PrimaryView::Data;
        self.session.ui.frame_selection = vec![BoardFrameId::Sheet(
            self.doc.datasets[dataset_index].resource_id(),
        )];
        self.session.ui.sheet_open = Some(dataset_index);
        dataset_index
    }

    /// Build a fresh series table from any field that exposes ordered 1D
    /// members. Rows follow the member ruler; every region becomes one column.
    fn build_region_table(&self, dataset: usize) -> Result<TableDataset, String> {
        let source = self
            .doc
            .datasets
            .get(dataset)
            .ok_or_else(|| "The region source is no longer available.".to_owned())?;
        let source_resource = source.resource_id().to_string();
        let source_field = source
            .region_source_field()
            .ok_or_else(|| "Plot a region-analyzable field before building a table.".to_owned())?;
        let state = source
            .region_analysis()
            .ok_or_else(|| "The selected field does not support region analysis.".to_owned())?;
        if state.regions.is_empty() {
            return Err("Add at least one region before creating a table.".to_owned());
        }

        let (x_label, x_unit, x, values, value_units) = match source {
            Dataset::Nmr2D(d2) => {
                let (Processed2D::Stack(stack), Some(axis)) = (&d2.processed, &d2.data.pseudo_axis)
                else {
                    return Err("The selected NMR field is not an ordered series.".to_owned());
                };
                let x_label = match axis.kind {
                    plotx_io::PseudoKind::Gradient => "Gradient".to_owned(),
                    plotx_io::PseudoKind::Delay => "Delay".to_owned(),
                    plotx_io::PseudoKind::Generic if !axis.name.is_empty() => axis.name.clone(),
                    plotx_io::PseudoKind::Generic => "Ruler".to_owned(),
                };
                let values = state
                    .regions
                    .iter()
                    .map(|region| {
                        let op = region.metric.unwrap_or(state.default_metric).into();
                        extract_region_series(stack, axis, (region.lo, region.hi), op).y
                    })
                    .collect::<Vec<_>>();
                (
                    x_label,
                    axis.unit.clone(),
                    axis.values.clone(),
                    values,
                    vec!["".to_owned(); state.regions.len()],
                )
            }
            Dataset::Electrophysiology(recording) => {
                let selected = recording
                    .selected_sweeps
                    .iter()
                    .enumerate()
                    .filter_map(|(index, selected)| (*selected).then_some(index))
                    .collect::<Vec<_>>();
                let signal_unit = recording
                    .data
                    .channels
                    .get(recording.selected_channel)
                    .map(|channel| channel.unit.symbol.clone())
                    .unwrap_or_default();
                let mut values = Vec::with_capacity(state.regions.len());
                let mut units = Vec::with_capacity(state.regions.len());
                for region in &state.regions {
                    let metric = region.metric.unwrap_or(state.default_metric);
                    let mut column = Vec::with_capacity(selected.len());
                    for &sweep in &selected {
                        column.push(reduce_electrophysiology_region(
                            recording,
                            sweep,
                            recording.selected_channel,
                            region,
                            metric,
                        )?);
                    }
                    units.push(if metric == RegionMetric::Area && !signal_unit.is_empty() {
                        format!("{signal_unit}·s")
                    } else {
                        signal_unit.clone()
                    });
                    values.push(column);
                }
                (
                    "Sweep".to_owned(),
                    "".to_owned(),
                    selected.iter().map(|index| (*index + 1) as f64).collect(),
                    values,
                    units,
                )
            }
            Dataset::Nmr(_)
            | Dataset::Table(_)
            | Dataset::Afm(_)
            | Dataset::MassSpec(_)
            | Dataset::Xrd(_)
            | Dataset::Xps(_) => {
                return Err("The selected field does not contain an ordered series.".to_owned());
            }
        };

        let (mut x_schema, x_values) =
            materialized_float_column(x_label, &x_unit, x.into_iter().map(Some));
        x_schema.role = plotx_data::SemanticRole::Custom("space.nmrtist.plotx.axis.x".into());
        let x_binding = x_schema.id;
        let mut columns = vec![(x_schema, x_values)];
        let mut series_bindings = Vec::with_capacity(state.regions.len());
        let mut region_provenance = Vec::with_capacity(state.regions.len());
        let axis_unit = source.region_axis_unit().unwrap_or("");
        for ((region, series), unit) in state.regions.iter().zip(values).zip(value_units) {
            let metric = region.metric.unwrap_or(state.default_metric);
            let label = region.column_name(axis_unit);
            let (schema, values) =
                materialized_float_column(&label, &unit, series.into_iter().map(Some));
            let column = schema.id;
            series_bindings.push(TableSeriesBinding {
                value_column: column,
                uncertainty_column: None,
                fit: None,
            });
            region_provenance.push(RegionColumnProvenance {
                region: region.id,
                column,
                bounds: [region.lo_min(), region.hi_max()],
                metric,
                label,
                unit,
                color: region.color,
            });
            columns.push((schema, values));
        }
        let mut table = TableDataset::from_materialized(
            columns,
            Vec::new(),
            Some(x_binding),
            series_bindings,
            "plotx.analysis.region-table.v1",
        )
        .map_err(|error| error.to_string())?;
        if let Dataset::Nmr2D(d2) = source {
            table.meta.diffusion = d2
                .data
                .diffusion
                .as_ref()
                .map(DiffusionConstants::from_meta);
        }
        table.provenance = Some(TableProvenance {
            source_resource,
            source_field,
            regions: region_provenance,
        });
        Ok(table)
    }

    /// The `Dataset::Table` linked to `source` (its provenance points back), if any.
    pub fn region_table_index(&self, source: usize) -> Option<usize> {
        let source_resource = self.doc.datasets.get(source)?.resource_id().to_string();
        self.doc.datasets.iter().position(|d| {
            d.as_table()
                .and_then(|t| t.provenance.as_ref())
                .map(|p| p.source_resource == source_resource)
                .unwrap_or(false)
        })
    }

    /// Re-derive the linked series table from `source`'s regions in place. A no-op
    /// when no table is linked yet (creation is explicit) or the regions cleared.
    pub fn sync_region_table(&mut self, source: usize) {
        let Some(tj) = self.region_table_index(source) else {
            return;
        };
        let table = match self.build_region_table(source) {
            Ok(table) => table,
            Err(error) => {
                self.session.status = error;
                return;
            }
        };
        if let Some(t) = self.doc.datasets[tj].as_table_mut() {
            t.typed_state = table.typed_state;
            t.x_binding = table.x_binding;
            t.series_bindings = table.series_bindings;
            t.provenance = table.provenance;
            t.meta = table.meta;
            t.curve_fit_analyses.clear();
        }
        self.rebuild_canvases_for(tj);
    }

    /// Create the live series table for a dataset's regions.
    pub fn create_region_table(&mut self, dataset: usize) {
        if self.region_table_index(dataset).is_some() {
            self.session.status = "This dataset already has a linked series table.".into();
            return;
        }
        if self.doc.datasets[dataset]
            .as_electrophysiology()
            .is_some_and(|recording| !recording.selected_sweeps.iter().any(|selected| *selected))
        {
            self.session.status =
                "Select at least one sweep before building a region table.".to_owned();
            return;
        }
        let table = match self.build_region_table(dataset) {
            Ok(table) => table,
            Err(error) => {
                self.session.status = error;
                return;
            }
        };
        let count = table.series_bindings.len();
        let mut tds = table;
        tds.lineage = Some(DatasetLineage::new(
            DerivationKind::LiveRegionTable,
            [self.doc.datasets[dataset].resource_id()],
        ));
        tds.name = Some(format!(
            "{} — regions",
            self.doc.datasets[dataset].display_name()
        ));
        let ds = Dataset::Table(Box::new(tds));

        let action = Action::insert_dataset_with_default_canvas(
            self,
            ds,
            format!("Canvas {} — Extracted curves", self.doc.canvases.len() + 1),
            DEFAULT_CANVAS_SIZE_MM,
        );
        self.execute_action(action);
        let result = self.doc.canvases.len() - 1;
        self.reveal_board_frame(FrameRef::Page(result));
        let regions = if count == 1 { "region" } else { "regions" };
        self.session.status = format!(
            "Created extracted curves for {count} {regions} with a synchronized data table."
        );
    }

    /// Place an independent, unlinked snapshot of the current region values as a
    /// new table (no provenance), so later region edits leave it untouched.
    pub fn freeze_region_table(&mut self, dataset: usize) {
        let mut tds = match self.build_region_table(dataset) {
            Ok(table) => table,
            Err(error) => {
                self.session.status = error;
                return;
            }
        };
        tds.provenance = None;
        tds.lineage = Some(DatasetLineage::new(
            DerivationKind::FrozenRegionTable,
            [self.doc.datasets[dataset].resource_id()],
        ));
        tds.name = Some(format!(
            "{} — regions (frozen)",
            self.doc.datasets[dataset].display_name()
        ));
        let ds = Dataset::Table(Box::new(tds));
        let action = Action::insert_dataset_with_default_canvas(
            self,
            ds,
            format!("Canvas {} — Data table", self.doc.canvases.len() + 1),
            DEFAULT_CANVAS_SIZE_MM,
        );
        self.execute_action(action);
        self.session.status = "Froze a static copy of the series table.".into();
    }

    /// Fit one or more table responses through a single declarative analysis.
    /// Each selected column receives a reference into the shared snapshot.
    pub fn fit_table_columns(
        &mut self,
        dataset: usize,
        model_id: &str,
        all_columns: bool,
        column: plotx_data::ColumnId,
        global_parameters: bool,
        options: plotx_analysis::fit_model::FitOptions,
    ) {
        let model = match resolve_table_fit_model(model_id, global_parameters) {
            Ok(model) => model,
            Err(status) => {
                self.session.status = status;
                return;
            }
        };
        let Some(t) = self.doc.datasets.get(dataset).and_then(Dataset::as_table) else {
            self.session.status = "Curve fitting needs a data table.".into();
            return;
        };
        let Some(column) = t
            .series_bindings
            .iter()
            .position(|binding| binding.value_column == column)
        else {
            self.session.status = "The selected fit column is no longer available.".into();
            return;
        };
        let table = match t.fit_analysis_view() {
            Ok(table) => table,
            Err(status) => {
                self.session.status = status;
                return;
            }
        };
        let TableFitInputs {
            model,
            input_name,
            response_name,
            targets,
            datasets: fit_datasets,
            bindings,
        } = match build_table_fit_inputs(&table, model, all_columns, column) {
            Ok(inputs) => inputs,
            Err(status) => {
                self.session.status = status;
                return;
            }
        };
        let before_refs: Vec<Option<CurveFitReference>> = t
            .series_bindings
            .iter()
            .map(|binding| binding.fit.clone())
            .collect();
        let mut after_refs = before_refs.clone();
        let before_analyses = t.curve_fit_analyses.clone();
        let mut after_analyses = before_analyses.clone();
        let model_name = model.name.clone();
        let result = match plotx_analysis::fit_model::fit_model(model, fit_datasets, &[], options) {
            Ok(result) => result,
            Err(error) => {
                self.session.status = format!("Curve fit failed: {error}");
                return;
            }
        };
        let selection = match fit_selection::snapshot(&table, &bindings, &result) {
            Ok(selection) => selection,
            Err(error) => {
                self.session.status = format!("Could not record the fit selection: {error}");
                return;
            }
        };
        let plot_samples = match table_fit_plot_samples(&result, &input_name, &table, &targets) {
            Ok(samples) => samples,
            Err(error) => {
                self.session.status = format!("Could not evaluate the fitted curve: {error}");
                return;
            }
        };
        let analysis_id = t.next_curve_fit_id();
        let instance_ids: Vec<String> = bindings
            .iter()
            .map(|binding| binding.dataset_id.clone())
            .collect();
        after_analyses.push(StoredCurveFitAnalysis {
            id: analysis_id,
            name: model_name.clone(),
            bindings,
            result,
            selection: Some(selection),
            plot_samples,
        });
        for (&index, instance_id) in targets.iter().zip(instance_ids) {
            after_refs[index] = Some(CurveFitReference {
                analysis_id,
                instance_id,
                response: response_name.clone(),
            });
        }
        // Refitting replaces column references, so drop superseded snapshots —
        // each embeds a full copy of the fitted data and would otherwise grow
        // the project and the diagnostics list on every refit.
        after_analyses.retain(|analysis| {
            after_refs
                .iter()
                .flatten()
                .any(|reference| reference.analysis_id == analysis.id)
        });
        self.execute_action(Action::set_curve_fit_analyses(
            self.doc.datasets[dataset].resource_id(),
            (before_refs, before_analyses),
            (after_refs, after_analyses),
        ));
        self.session.status = format!("Fitted {} curve(s) with {model_name}.", targets.len());
    }

    /// Validate and evaluate the initial curve without running optimisation.
    pub fn preview_table_fit(
        &mut self,
        dataset: usize,
        model_id: &str,
        all_columns: bool,
        column: plotx_data::ColumnId,
        global_parameters: bool,
        options: plotx_analysis::fit_model::FitOptions,
    ) {
        let model = match resolve_table_fit_model(model_id, global_parameters) {
            Ok(model) => model,
            Err(status) => {
                self.session.status = status;
                return;
            }
        };
        let Some(dataset) = self.doc.datasets.get(dataset).and_then(Dataset::as_table) else {
            self.session.status = "Initial preview needs a data table.".into();
            return;
        };
        let Some(column) = dataset
            .series_bindings
            .iter()
            .position(|binding| binding.value_column == column)
        else {
            self.session.status = "The selected fit column is no longer available.".into();
            return;
        };
        let table = match dataset.fit_analysis_view() {
            Ok(table) => table,
            Err(status) => {
                self.session.status = status;
                return;
            }
        };
        let inputs = match build_table_fit_inputs(&table, model, all_columns, column) {
            Ok(inputs) => inputs,
            Err(status) => {
                self.session.status = status;
                return;
            }
        };
        match plotx_analysis::fit_model::preview_initial_model(
            inputs.model,
            inputs.datasets,
            &[],
            options,
        ) {
            Ok(preview) => {
                self.session.status = format!(
                    "Initial curve is valid for {} point(s).",
                    preview.points.len()
                )
            }
            Err(error) => self.session.status = format!("Initial curve is invalid: {error}"),
        }
    }
}
