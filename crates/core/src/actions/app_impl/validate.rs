use super::*;

#[derive(Debug, thiserror::Error)]
pub enum ActionApplyError {
    #[error("action target is stale: {0}")]
    StaleTarget(String),
    #[error("action value is invalid: {0}")]
    InvalidValue(String),
}

/// The document shape a composite has projected so far. Validation runs before
/// anything is applied, so a child action that targets a dataset an earlier
/// child inserts must be judged against this projection, not against the live
/// document.
pub(super) struct ValidationShape {
    datasets: usize,
    canvases: usize,
    /// Datasets inserted by earlier children of the composite under validation.
    inserted: Vec<crate::state::DatasetId>,
}

impl ValidationShape {
    pub(super) fn from_app(app: &PlotxApp) -> Self {
        Self {
            datasets: app.doc.datasets.len(),
            canvases: app.doc.canvases.len(),
            inserted: Vec::new(),
        }
    }

    fn has_dataset(&self, app: &PlotxApp, id: crate::state::DatasetId) -> bool {
        app.doc.dataset_index(id).is_some() || self.inserted.contains(&id)
    }
}

pub(super) fn validate_action(
    app: &PlotxApp,
    action: &Action,
    shape: &mut ValidationShape,
) -> Result<(), ActionApplyError> {
    match action {
        Action::ReplacePanelState {
            canvas,
            before,
            after,
        } => {
            if *canvas >= shape.canvases {
                return Err(ActionApplyError::StaleTarget(format!("canvas {canvas}")));
            }
            for (label, state) in [("before", before), ("after", after)] {
                crate::state::validate_panel_structure(
                    &state.panels,
                    state.objects.iter().map(|item| item.id),
                    &state.groups,
                )
                .map_err(|error| {
                    ActionApplyError::InvalidValue(format!("{label} panel state: {error}"))
                })?;
                for item in &state.objects {
                    crate::state::validate_frame(item.frame, "content")
                        .map_err(ActionApplyError::InvalidValue)?;
                }
            }
        }
        Action::SetObjectGroups { canvas, after, .. } => {
            let Some(mut projected) = app.doc.canvases.get(*canvas).cloned() else {
                return Err(ActionApplyError::StaleTarget(format!("canvas {canvas}")));
            };
            if after.iter().any(|(id, _)| projected.object(*id).is_none()) {
                return Err(ActionApplyError::StaleTarget("group content".to_owned()));
            }
            projected.apply_content_group_assignments(after);
            projected
                .validate_structure()
                .map_err(ActionApplyError::InvalidValue)?;
        }
        Action::Composite(actions) => {
            for child in actions {
                validate_action(app, child, shape)?;
            }
        }
        Action::RenameDataset { dataset, .. } => {
            if !shape.has_dataset(app, *dataset) {
                return Err(ActionApplyError::StaleTarget(format!("dataset {dataset}")));
            }
        }
        Action::UpdateDatasetProcessing {
            dataset,
            before,
            after,
        } => {
            if !shape.has_dataset(app, *dataset) {
                return Err(ActionApplyError::StaleTarget(format!("dataset {dataset}")));
            }
            if let Some(index) = app.doc.dataset_index(*dataset) {
                for state in [before, after] {
                    super::processing::validate_processing_state(&app.doc.datasets[index], state)
                        .map_err(ActionApplyError::InvalidValue)?;
                }
            }
        }
        Action::SetMassSpecStream {
            dataset,
            before,
            after,
        } => {
            let Some(mass_spec) = app
                .doc
                .dataset_index(*dataset)
                .and_then(|index| app.doc.datasets[index].as_mass_spec())
            else {
                return Err(ActionApplyError::InvalidValue(format!(
                    "dataset {dataset} is not LC–MS data"
                )));
            };
            if [before, after].into_iter().any(|stream| {
                !mass_spec
                    .supported_ms_streams()
                    .any(|candidate| candidate == *stream)
            }) {
                return Err(ActionApplyError::InvalidValue(format!(
                    "dataset {dataset} does not contain both MS streams"
                )));
            }
        }
        Action::SetMassSpectrumExtractions {
            dataset,
            before,
            after,
        } => {
            let Some(mass_spec) = app
                .doc
                .dataset_index(*dataset)
                .and_then(|index| app.doc.datasets[index].as_mass_spec())
            else {
                return Err(ActionApplyError::InvalidValue(format!(
                    "dataset {dataset} is not LC–MS data"
                )));
            };
            for (label, (extractions, next_id)) in [("before", before), ("after", after)] {
                let mut extractions = extractions.clone();
                let mut next_id = *next_id;
                crate::state::MassSpecDataset::validate_extraction_state(
                    &mass_spec.run,
                    &mut extractions,
                    &mut next_id,
                )
                .map_err(|error| {
                    ActionApplyError::InvalidValue(format!(
                        "{label} mass-spectrum extraction state: {error}"
                    ))
                })?;
            }
        }
        Action::SetMassSpecIonChromatograms {
            dataset,
            before,
            after,
        } => {
            let Some(mass_spec) = app
                .doc
                .dataset_index(*dataset)
                .and_then(|index| app.doc.datasets[index].as_mass_spec())
            else {
                return Err(ActionApplyError::InvalidValue(format!(
                    "dataset {dataset} is not LC–MS data"
                )));
            };
            for (label, (chromatograms, next_id)) in [("before", before), ("after", after)] {
                let mut chromatograms = chromatograms.clone();
                let mut next_id = *next_id;
                crate::state::MassSpecDataset::validate_ion_chromatogram_state(
                    &mass_spec.run,
                    &mut chromatograms,
                    &mut next_id,
                )
                .map_err(|error| {
                    ActionApplyError::InvalidValue(format!(
                        "{label} extracted-ion chromatogram state: {error}"
                    ))
                })?;
            }
        }
        Action::RenameCanvas { canvas, .. }
        | Action::ApplyTheme { canvas, .. }
        | Action::SetCanvasSize { canvas, .. }
        | Action::SetCanvasCaption { canvas, .. }
        | Action::SetPanelLabelStyle { canvas, .. } => {
            if *canvas >= shape.canvases {
                return Err(ActionApplyError::StaleTarget(format!("canvas {canvas}")));
            }
        }
        Action::InsertDatasetWithCanvas {
            dataset_index,
            canvas_index,
            inserted_into_existing_canvas,
            dataset,
            ..
        } => {
            if *dataset_index != shape.datasets {
                return Err(ActionApplyError::StaleTarget(format!(
                    "dataset insertion index {dataset_index}"
                )));
            }
            if let Some(canvas) = inserted_into_existing_canvas {
                if *canvas >= shape.canvases {
                    return Err(ActionApplyError::StaleTarget(format!("canvas {canvas}")));
                }
            } else {
                if *canvas_index != shape.canvases {
                    return Err(ActionApplyError::StaleTarget(format!(
                        "canvas insertion index {canvas_index}"
                    )));
                }
                shape.canvases += 1;
            }
            shape.datasets += 1;
            shape.inserted.push(dataset.resource_id());
        }
        Action::DeleteCanvas { index, .. } => {
            if *index >= shape.canvases {
                return Err(ActionApplyError::StaleTarget(format!("canvas {index}")));
            }
            shape.canvases -= 1;
        }
        Action::InsertCanvas { index, .. } => {
            if *index > shape.canvases {
                return Err(ActionApplyError::StaleTarget(format!("canvas {index}")));
            }
            shape.canvases += 1;
        }
        Action::SetObjectViewport { canvas, object, .. }
        | Action::SetAxisOverrides { canvas, object, .. }
        | Action::MoveResizeObject { canvas, object, .. }
        | Action::SetPanelMeta { canvas, object, .. }
        | Action::SetObjectFlags { canvas, object, .. }
        | Action::SetDataBinding { canvas, object, .. }
        | Action::SetSeriesPresentation { canvas, object, .. }
        | Action::SetChartType { canvas, object, .. }
        | Action::SetStackSpec { canvas, object, .. }
        | Action::SetAxisProjections { canvas, object, .. }
        | Action::SetObjectText { canvas, object, .. }
        | Action::RenameObject { canvas, object, .. } => {
            let valid = app
                .doc
                .canvases
                .get(*canvas)
                .and_then(|canvas| canvas.object(*object))
                .is_some();
            if !valid {
                return Err(ActionApplyError::StaleTarget(format!(
                    "object {object} on canvas {canvas}"
                )));
            }
        }
        _ => {}
    }
    Ok(())
}
