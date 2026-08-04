use super::{KIND_CANVAS, KIND_DATASET, ResourceRef, top_ref};
use crate::state::{FrameRef, PlotxApp};

pub(super) fn current(app: &PlotxApp) -> Vec<ResourceRef> {
    let mut selected = Vec::new();
    if !app.session.ui.frame_selection.is_empty() {
        for frame in &app.session.ui.frame_selection {
            let target = match *frame {
                FrameRef::Page(index) => app
                    .doc
                    .canvases
                    .get(index)
                    .map(|canvas| top_ref(canvas.resource_id, KIND_CANVAS)),
                FrameRef::Sheet(index) => app
                    .doc
                    .datasets
                    .get(index)
                    .map(|dataset| top_ref(dataset.resource_id(), KIND_DATASET)),
            };
            if let Some(target) = target
                && !selected.contains(&target)
            {
                selected.push(target);
            }
        }
        return selected;
    }
    if let Some(dataset) = app
        .active_dataset()
        .and_then(|index| app.doc.datasets.get(index))
    {
        selected.push(top_ref(dataset.resource_id(), KIND_DATASET));
    }
    if let Some(canvas) = app
        .session
        .active_canvas
        .and_then(|index| app.doc.canvases.get(index))
    {
        selected.push(top_ref(canvas.resource_id, KIND_CANVAS));
    }
    selected
}
