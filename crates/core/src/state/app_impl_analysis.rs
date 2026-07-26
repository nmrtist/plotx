use super::*;

impl PlotxApp {
    /// Apply a user-entered non-uniform-sampling schedule to a 2D dataset and
    /// re-run the reconstruction. Returns the validation error (if any) so the
    /// caller can surface it next to the input field.
    pub fn apply_nus_schedule(
        &mut self,
        dataset: usize,
        values: &[usize],
        base: usize,
    ) -> Result<(), String> {
        let Some(d2) = self
            .doc
            .datasets
            .get_mut(dataset)
            .and_then(Dataset::as_nmr2d_mut)
        else {
            return Err("NUS reconstruction needs a 2D dataset.".into());
        };
        d2.set_nus_schedule(values, base)?;
        self.schedule_2d_processing(dataset, true);
        self.doc.dirty = true;
        self.session.status =
            "Reconstructing the NUS spectrum from the entered sampling list…".into();
        Ok(())
    }

    /// Fit every column to build the DOSY contour map (diffusion datasets only).
    pub fn build_dosy_map_for(&mut self, dataset: usize) {
        let Some(d2) = self
            .doc
            .datasets
            .get_mut(dataset)
            .and_then(Dataset::as_nmr2d_mut)
        else {
            self.session.status = "DOSY maps need a diffusion dataset.".into();
            return;
        };
        if d2.data.diffusion.is_none() {
            self.session.status =
                "This dataset has no diffusion parameters (not a DOSY array).".into();
            return;
        }
        let any = d2.build_dosy_map();
        // The builder installs the map and its provenance whether or not any
        // column fitted, and both are persisted state. Dirtying only the populated
        // branch would let an empty result be lost on close with no save prompt.
        self.doc.dirty = true;
        if any {
            self.rebuild_canvases_for(dataset);
            self.session.status = "Built DOSY map.".into();
        } else {
            self.session.status =
                "DOSY map is empty: no columns fit above the noise threshold.".into();
        }
    }

    /// Build the regularized ILT/CONTIN DOSY contour (diffusion datasets with a
    /// gradient ruler), resolving the parameters through the value lifecycle.
    pub fn build_ilt_map_for(&mut self, dataset: usize) {
        self.build_ilt_map_for_with_params(dataset, self.explicit_ilt_input_for(dataset));
    }

    pub fn build_ilt_map_for_with_params(&mut self, dataset: usize, explicit: Option<IltParams>) {
        let params = self.resolve_ilt_params_for(dataset, explicit);
        if let Err(message) = validate_ilt_params(params) {
            self.session.status = message;
            return;
        }
        let Some(d2) = self
            .doc
            .datasets
            .get_mut(dataset)
            .and_then(Dataset::as_nmr2d_mut)
        else {
            self.session.status = "ILT DOSY maps need a diffusion dataset.".into();
            return;
        };
        if d2.data.diffusion.is_none() {
            self.session.status =
                "This dataset has no diffusion parameters (not a DOSY array).".into();
            return;
        }
        let is_gradient = d2
            .data
            .pseudo_axis
            .as_ref()
            .map(|a| a.kind == plotx_io::PseudoKind::Gradient)
            .unwrap_or(false);
        if !is_gradient {
            self.session.status =
                "ILT DOSY needs a gradient-encoded ruler (this array is not gradient-encoded)."
                    .into();
            return;
        }
        let any = d2.build_ilt_map(params);
        // See `build_dosy_map_for`: the method switch, the map and its provenance
        // land whether or not the inversion produced anything.
        self.doc.dirty = true;
        if any {
            self.rebuild_canvases_for(dataset);
            self.session.status = "Built ILT DOSY map.".into();
        } else {
            self.session.status = format!(
                "ILT DOSY map is empty with λ = {} (legal range {}–{}): no columns are above the \
                 noise threshold.",
                params.lambda,
                crate::settings::MIN_ILT_LAMBDA,
                crate::settings::MAX_ILT_LAMBDA
            );
        }
    }

    /// Resolve the invocation snapshot in lifecycle priority order: explicit
    /// input, this dataset's last result provenance, then the app default.
    ///
    /// The result is not trusted: the provenance stage reads a project file, so
    /// every caller must run it through [`validate_ilt_params`] before handing it
    /// to the inversion.
    pub fn resolve_ilt_params_for(&self, dataset: usize, explicit: Option<IltParams>) -> IltParams {
        if let Some(params) = explicit {
            return params;
        }
        if let Some(params) = self
            .doc
            .datasets
            .get(dataset)
            .and_then(Dataset::as_nmr2d)
            .and_then(|dataset| dataset.ilt_provenance.as_ref())
            .and_then(|provenance| match &provenance.input {
                DosyInvocation::Ilt { params } => Some(*params),
                DosyInvocation::MonoExp { .. } => None,
            })
        {
            return params;
        }
        // The grid fields come from the algorithm default, never from whatever the
        // panel last held: carrying them over would hand this dataset the grid a
        // different dataset was inverted on, which the user never chose for it.
        IltParams {
            lambda: self.settings.processing.ilt_lambda,
            ..IltParams::default()
        }
    }

    /// The parameters the user explicitly entered *for this dataset*, if any.
    ///
    /// An explicit input belongs to one target. Scoping it here is what keeps the
    /// other two lifecycle stages reachable: every entry point asks this question
    /// first and gets `None` whenever the user has not overridden anything for
    /// this dataset, whether or not any panel has been painted.
    pub fn explicit_ilt_input_for(&self, dataset: usize) -> Option<IltParams> {
        let id = self.doc.datasets.get(dataset)?.resource_id();
        if self.session.ui.ilt_params_dataset != Some(id) {
            return None;
        }
        self.session.ui.ilt_params
    }

    /// Record an explicit ILT input for one dataset, replacing any previous one.
    pub fn set_explicit_ilt_input(&mut self, dataset: usize, params: IltParams) {
        let Some(id) = self.doc.datasets.get(dataset).map(Dataset::resource_id) else {
            return;
        };
        self.session.ui.ilt_params = Some(params);
        self.session.ui.ilt_params_dataset = Some(id);
    }

    /// Worker behind `SetRegions`: install the regions and re-derive the linked
    /// table so apply and undo both land in a consistent state.
    pub fn set_regions(&mut self, dataset: usize, regions: &[Region]) {
        if let Some(d2) = self
            .doc
            .datasets
            .get_mut(dataset)
            .and_then(Dataset::as_nmr2d_mut)
        {
            d2.regions = regions.to_vec();
        }
        self.sync_region_table(dataset);
    }

    /// Snapshot the regions, let `edit` mutate a working copy (and hand out fresh
    /// ids), then commit the change as one undoable step.
    pub fn edit_regions(&mut self, dataset: usize, edit: impl FnOnce(&mut Vec<Region>, &mut u64)) {
        let Some(d2) = self.doc.datasets.get(dataset).and_then(Dataset::as_nmr2d) else {
            return;
        };
        let before = d2.regions.clone();
        let mut after = before.clone();
        let mut next_id = d2.next_region_id;
        edit(&mut after, &mut next_id);
        if let Some(d2) = self
            .doc
            .datasets
            .get_mut(dataset)
            .and_then(Dataset::as_nmr2d_mut)
        {
            d2.next_region_id = next_id;
        }
        self.execute_action(Action::set_regions(
            self.doc.datasets[dataset].resource_id(),
            before,
            after,
        ));
    }

    /// Switch how a pseudo-2D dataset is displayed and rebuild its figure.
    pub fn set_pseudo_display(&mut self, dataset: usize, display: PseudoDisplay) {
        let mut changed = false;
        if let Some(d2) = self
            .doc
            .datasets
            .get_mut(dataset)
            .and_then(Dataset::as_nmr2d_mut)
        {
            changed = d2.display != display;
            if changed {
                d2.display = display;
            }
        }
        if changed {
            self.rebuild_canvases_for(dataset);
            self.doc.dirty = true;
        }
    }

    /// Select the DOSY result family through core state, so the persisted method
    /// cannot be changed without the document being marked dirty.
    pub fn set_pseudo_dosy_method(&mut self, dataset: usize, method: DosyMethod) {
        let mut changed = false;
        if let Some(d2) = self
            .doc
            .datasets
            .get_mut(dataset)
            .and_then(Dataset::as_nmr2d_mut)
        {
            changed = d2.dosy_method != method;
            if changed {
                d2.dosy_method = method;
            }
        }
        if changed {
            self.rebuild_canvases_for(dataset);
            self.doc.dirty = true;
        }
    }

    pub fn clear_analysis_selection(&mut self) {
        self.session.ui.analysis_selection = None;
        if matches!(self.session.ui.interaction, Interaction::Selection(_)) {
            self.reset_interaction();
        }
        self.session.status = "Cleared analysis selection.".into();
    }

    pub fn analysis_range_for(&self, dataset: usize) -> Option<AxisRange> {
        let dataset = self.doc.datasets.get(dataset)?.resource_id();
        self.session
            .ui
            .analysis_selection
            .as_ref()
            .filter(|selection| selection.dataset == dataset)
            .map(|selection| selection.x_range)
            .or_else(|| self.visible_range_for_dataset(dataset))
    }

    fn visible_range_for_dataset(&self, dataset: DatasetId) -> Option<AxisRange> {
        let ci = self.session.active_canvas?;
        let canvas = self.doc.canvases.get(ci)?;
        let object_id = canvas.active_plot_object_id()?;
        let plot = canvas.object(object_id)?.plot()?;
        (plot.primary_dataset() == Some(dataset)).then_some(plot.viewport.view_x)
    }
}

/// Reject ILT parameters the inversion cannot safely run on.
///
/// One stage of the value lifecycle reads a project file, so these values are
/// external input rather than something a widget has already constrained. The
/// grid size in particular sizes a dense `n_grid x n_grid` system inside the
/// inversion, so an unchecked value read back from a malformed project would
/// size an unbounded allocation rather than produce an error. Each message names
/// both the value that was set and the boundary it missed.
pub fn validate_ilt_params(params: IltParams) -> Result<(), String> {
    use crate::settings::{
        MAX_ILT_DIFFUSION, MAX_ILT_GRID, MAX_ILT_LAMBDA, MIN_ILT_DIFFUSION, MIN_ILT_GRID,
        MIN_ILT_LAMBDA,
    };
    if !params.lambda.is_finite() || !(MIN_ILT_LAMBDA..=MAX_ILT_LAMBDA).contains(&params.lambda) {
        return Err(format!(
            "ILT lambda {} is outside {MIN_ILT_LAMBDA}–{MAX_ILT_LAMBDA}; choose a value within \
             that range.",
            params.lambda
        ));
    }
    if !(MIN_ILT_GRID..=MAX_ILT_GRID).contains(&params.n_grid) {
        return Err(format!(
            "ILT grid size {} is outside {MIN_ILT_GRID}–{MAX_ILT_GRID}; choose a value within \
             that range.",
            params.n_grid
        ));
    }
    // Diffusion coefficients span decades, so report them in scientific notation:
    // `{}` renders 1e-11 as a run of zeros nobody can read back against a bound.
    for (label, value) in [("D min", params.d_min), ("D max", params.d_max)] {
        if !value.is_finite() || !(MIN_ILT_DIFFUSION..=MAX_ILT_DIFFUSION).contains(&value) {
            return Err(format!(
                "ILT {label} {value:e} is outside {MIN_ILT_DIFFUSION:e}–{MAX_ILT_DIFFUSION:e}; \
                 choose a value within that range."
            ));
        }
    }
    if params.d_min >= params.d_max {
        return Err(format!(
            "ILT D min {:e} must be below D max {:e}; the diffusion grid would otherwise be empty \
             or reversed.",
            params.d_min, params.d_max
        ));
    }
    Ok(())
}

/// Everything the table fit/preview workflow derives from a model and a table.
pub(super) struct TableFitInputs {
    pub(super) model: plotx_analysis::fit_model::FitModelDefinition,
    pub(super) input_name: String,
    pub(super) response_name: String,
    pub(super) targets: Vec<usize>,
    pub(super) datasets: Vec<plotx_analysis::fit_model::FitDataset>,
    pub(super) bindings: Vec<ModelInstanceBinding>,
}

struct ResolvedTableConstants {
    values: std::collections::BTreeMap<String, f64>,
    bindings: std::collections::BTreeMap<String, FitDataBinding>,
}

/// Resolve a model id against the builtins and the on-disk library, applying
/// the shared-parameters override. `Err` is the user-facing status message.
pub(super) fn resolve_table_fit_model(
    model_id: &str,
    global_parameters: bool,
) -> Result<plotx_analysis::fit_model::FitModelDefinition, String> {
    let library = crate::fit_model_library::FitModelLibrary::load();
    let custom = library.as_ref().ok().and_then(|library| {
        library
            .models
            .iter()
            .find(|model| model.id == model_id)
            .cloned()
    });
    let Some(mut model) = plotx_analysis::models::builtin_model(model_id).or(custom) else {
        // A failed library load is the actionable cause when the model is not
        // a builtin, so surface it instead of "unknown model".
        return Err(match library {
            Err(error) => format!("Could not load the fit model library: {error}"),
            Ok(_) => format!("Unknown fit model '{model_id}'."),
        });
    };
    if global_parameters {
        for parameter in &mut model.parameters {
            parameter.sharing = plotx_analysis::fit_model::ParameterSharing::Shared;
        }
    }
    Ok(model)
}

/// Resolve a model's semantic quantities against a table and assemble the
/// plain numerical datasets consumed by the analysis crate.
pub(super) fn build_table_fit_inputs(
    table: &super::table_fit::FitAnalysisTable,
    model: plotx_analysis::fit_model::FitModelDefinition,
    all_columns: bool,
    column: usize,
) -> Result<TableFitInputs, String> {
    if table.series.is_empty() {
        return Err("This table has no columns to fit.".into());
    }
    let [input_variable] = model.independent_variables.as_slice() else {
        return Err("The table workflow needs exactly one independent variable.".into());
    };
    let [response] = model.responses.as_slice() else {
        return Err("The table workflow needs exactly one response.".into());
    };
    let input_name = input_variable.id.clone();
    let response_name = response.id.clone();
    let resolved_constants = resolve_table_constants(table, &model)?;
    let targets: Vec<usize> = if all_columns {
        (0..table.series.len()).collect()
    } else {
        vec![column.min(table.series.len() - 1)]
    };
    let datasets = targets
        .iter()
        .map(|&index| -> Result<_, String> {
            let table_column = &table.series[index];
            let column_id = table_column.value.id;
            let mut sigmas = std::collections::BTreeMap::new();
            if let Some(uncertainty) = &table_column.uncertainty {
                sigmas.insert(
                    response_name.clone(),
                    super::table_fit::backend_values(&uncertainty.values),
                );
            }
            Ok(plotx_analysis::fit_model::FitDataset {
                id: format!("column-{column_id}"),
                inputs: std::collections::BTreeMap::from([(
                    input_name.clone(),
                    super::table_fit::backend_values(&table.x.values),
                )]),
                responses: std::collections::BTreeMap::from([(
                    response_name.clone(),
                    super::table_fit::backend_values(&table_column.value.values),
                )]),
                sigmas,
                constants: resolved_constants.values.clone(),
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let x_column = table.x.id;
    let bindings = targets
        .iter()
        .map(|&index| -> Result<_, String> {
            let column = table.series[index].value.id;
            Ok(ModelInstanceBinding {
                dataset_id: format!("column-{column}"),
                variables: std::collections::BTreeMap::from([(
                    input_name.clone(),
                    FitDataBinding::Column { column: x_column },
                )]),
                responses: std::collections::BTreeMap::from([(
                    response_name.clone(),
                    FitDataBinding::Column { column },
                )]),
                constants: resolved_constants.bindings.clone(),
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(TableFitInputs {
        model,
        input_name,
        response_name,
        targets,
        datasets,
        bindings,
    })
}

fn resolve_table_constants(
    table: &super::table_fit::FitAnalysisTable,
    model: &plotx_analysis::fit_model::FitModelDefinition,
) -> Result<ResolvedTableConstants, String> {
    let mut values = std::collections::BTreeMap::new();
    let mut bindings = std::collections::BTreeMap::new();
    // This profile is selected by the identity of one exact built-in model.
    // Local symbols such as `tau` never acquire global binding semantics.
    let profile = (model.id == plotx_analysis::models::STEJSKAL_TANNER_ID)
        .then(|| super::table_fit::stejskal_tanner_binding_profile(table))
        .transpose()?;
    for constant in &model.constants {
        if let Some((value, key)) = profile
            .as_ref()
            .and_then(|profile| profile.get(constant.id.as_str()))
        {
            values.insert(constant.id.clone(), *value);
            bindings.insert(
                constant.id.clone(),
                FitDataBinding::Metadata { key: (*key).into() },
            );
            continue;
        }
        let value = constant.default_value.ok_or_else(|| {
            format!(
                "Model constant '{}' has no source in this table and no default value.",
                constant.display_name
            )
        })?;
        values.insert(constant.id.clone(), value);
        bindings.insert(
            constant.id.clone(),
            FitDataBinding::DatasetConstant { value },
        );
    }
    Ok(ResolvedTableConstants { values, bindings })
}

pub(super) fn table_fit_plot_samples(
    result: &plotx_analysis::fit_model::FitResult,
    input_name: &str,
    table: &super::table_fit::FitAnalysisTable,
    targets: &[usize],
) -> Result<FitPlotSamples, String> {
    if !matches!(
        result.model.kind,
        plotx_analysis::fit_model::FitModelKind::Explicit { .. }
    ) {
        return Ok(std::collections::BTreeMap::new());
    }
    let finite_x = table
        .x
        .values
        .iter()
        .filter_map(|value| value.filter(|value| value.is_finite()));
    let min = finite_x.clone().reduce(f64::min).unwrap_or(0.0);
    let max = finite_x.reduce(f64::max).unwrap_or(1.0);
    let display_x: Vec<f64> = (0..=200)
        .map(|index| min + (max - min) * index as f64 / 200.0)
        .collect();
    let grid = std::collections::BTreeMap::from([(input_name.to_owned(), display_x.clone())]);
    let mut samples = std::collections::BTreeMap::new();
    for &target in targets {
        let column = table.series[target].value.id;
        let dataset_id = format!("column-{column}");
        let predicted =
            plotx_analysis::fit_model::evaluate_fit_result_on_grid(result, &dataset_id, &grid)
                .map_err(|error| error.to_string())?;
        let responses = predicted
            .into_iter()
            .map(|(response, values)| {
                let points = display_x
                    .iter()
                    .copied()
                    .zip(values)
                    .map(|(x, y)| [x, y])
                    .collect();
                (response, points)
            })
            .collect();
        samples.insert(dataset_id, responses);
    }
    Ok(samples)
}

pub(super) fn next_sheet_pos_after_new_canvas(app: &PlotxApp) -> [f32; 2] {
    let mut canvas = CanvasDocument::new(String::new(), DEFAULT_CANVAS_SIZE_MM);
    canvas.board_pos = crate::state::next_page_board_pos(app);
    crate::state::next_sheet_board_pos_after_page(app, canvas.board_rect_pt())
}
