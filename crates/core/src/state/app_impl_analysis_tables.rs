use super::app_impl_analysis::{
    TableFitInputs, build_table_fit_inputs, next_sheet_pos_after_new_canvas,
    resolve_table_fit_model, table_fit_plot_samples,
};
use super::*;

impl PlotxApp {
    /// Create an empty editable data table from scratch: a small starter grid,
    /// placed as a board sheet frame (right of the page grid), selected, with its
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
        tds.board_pos = next_sheet_pos_after_new_canvas(self);
        self.doc.datasets.push(Dataset::Table(Box::new(tds)));
        let di = self.doc.datasets.len() - 1;
        self.focus_single(di);
        self.session.view = PrimaryView::Data;
        self.session.ui.frame_selection = vec![FrameRef::Sheet(di)];
        self.session.ui.sheet_open = Some(di);
        self.doc.dirty = true;
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

    fn insert_table_dataset(&mut self, mut dataset: TableDataset, name: String) -> usize {
        dataset.board_pos = next_sheet_pos_after_new_canvas(self);
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
        self.session.ui.frame_selection = vec![FrameRef::Sheet(dataset_index)];
        self.session.ui.sheet_open = Some(dataset_index);
        dataset_index
    }

    /// Build a fresh series table from a pseudo-2D dataset's regions: one column
    /// per region (x = the raw indirect ruler, y = the region reduced by its
    /// metric). `None` when the dataset is not a series or has no regions.
    fn build_region_table(&self, dataset: usize) -> Option<TableDataset> {
        let source_resource = self.doc.datasets.get(dataset)?.resource_id().to_string();
        let d2 = self.doc.datasets.get(dataset).and_then(Dataset::as_nmr2d)?;
        let (Processed2D::Stack(stack), Some(axis)) = (&d2.processed, &d2.data.pseudo_axis) else {
            return None;
        };
        if d2.regions.is_empty() {
            return None;
        }
        let x_label = match axis.kind {
            plotx_io::PseudoKind::Gradient => "Gradient".to_owned(),
            plotx_io::PseudoKind::Delay => "Delay".to_owned(),
            plotx_io::PseudoKind::Generic if !axis.name.is_empty() => axis.name.clone(),
            plotx_io::PseudoKind::Generic => "Ruler".to_owned(),
        };
        let (mut x_schema, x_values) =
            materialized_float_column(x_label, &axis.unit, axis.values.iter().copied().map(Some));
        x_schema.role = plotx_data::SemanticRole::Custom("space.nmrtist.plotx.axis.x".into());
        let x_binding = x_schema.id;
        let mut columns = vec![(x_schema, x_values)];
        let mut series_bindings = Vec::with_capacity(d2.regions.len());
        let mut windows = Vec::with_capacity(d2.regions.len());
        for region in &d2.regions {
            let op = region.metric.unwrap_or(d2.region_metric).into();
            let series = extract_region_series(stack, axis, (region.lo, region.hi), op);
            let (schema, values) =
                materialized_float_column(region.column_name(), "", series.y.into_iter().map(Some));
            series_bindings.push(TableSeriesBinding {
                value_column: schema.id,
                uncertainty_column: None,
                fit: None,
            });
            columns.push((schema, values));
            windows.push((region.lo_min(), region.hi_max()));
        }
        let mut table = TableDataset::from_materialized(
            columns,
            Vec::new(),
            Some(x_binding),
            series_bindings,
            "plotx.analysis.region-table.v1",
        )
        .ok()?;
        table.meta.diffusion = d2
            .data
            .diffusion
            .as_ref()
            .map(DiffusionConstants::from_meta);
        table.provenance = Some(TableProvenance {
            source_resource,
            regions: windows,
            metric: match d2.region_metric {
                RegionMetric::Area => TableMetric::Integral,
                _ => TableMetric::PeakHeight,
            },
        });
        Some(table)
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
        let Some(table) = self.build_region_table(source) else {
            return;
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
        let Some(table) = self.build_region_table(dataset) else {
            self.session.status = "Add at least one region before creating a table.".into();
            return;
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
        tds.board_pos = next_sheet_pos_after_new_canvas(self);
        let ds = Dataset::Table(Box::new(tds));

        let action = Action::insert_dataset_with_default_canvas(
            self,
            ds,
            format!("Canvas {} — Data table", self.doc.canvases.len() + 1),
            DEFAULT_CANVAS_SIZE_MM,
        );
        self.execute_action(action);
        self.session.status = format!("Created a live series table with {count} region(s).");
    }

    /// Place an independent, unlinked snapshot of the current region values as a
    /// new table (no provenance), so later region edits leave it untouched.
    pub fn freeze_region_table(&mut self, dataset: usize) {
        let Some(mut tds) = self.build_region_table(dataset) else {
            self.session.status = "Add at least one region before freezing a copy.".into();
            return;
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
        tds.board_pos = crate::state::next_sheet_board_pos(self);
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
