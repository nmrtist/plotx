use super::*;

#[derive(Debug, thiserror::Error)]
pub enum ActionApplyError {
    #[error("action target is stale: {0}")]
    StaleTarget(String),
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
        Action::Composite(actions) => {
            for child in actions {
                validate_action(app, child, shape)?;
            }
        }
        Action::RenameDataset { dataset, .. } | Action::UpdateDatasetProcessing { dataset, .. } => {
            if !shape.has_dataset(app, *dataset) {
                return Err(ActionApplyError::StaleTarget(format!("dataset {dataset}")));
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
