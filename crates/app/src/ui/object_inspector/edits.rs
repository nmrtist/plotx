//! Shared inspector edit coalescing, selection, and style helpers.

use super::*;

pub(super) fn format_once_section(app: &mut PlotxApp, ci: usize, primary: ObjectId, ui: &mut Ui) {
    let noun = app.doc.canvases[ci]
        .object(primary)
        .map(kind_noun)
        .unwrap_or("objects");
    ui.horizontal_wrapped(|ui| {
        if ui.button(format!("Apply to all {noun}")).clicked() {
            app.apply_style_to_kind(ci, primary);
        }
        if ui.button(format!("Set as default {noun}")).clicked() {
            app.set_style_default(ci, primary);
        }
    });
}

fn kind_noun(o: &CanvasObject) -> &'static str {
    if o.is_panel_label() {
        "panel labels"
    } else if o.text().is_some() {
        "text"
    } else if o.shape().is_some() {
        "shapes"
    } else {
        "objects"
    }
}

pub(super) fn kind_targets(
    app: &PlotxApp,
    ci: usize,
    ids: &[ObjectId],
    pred: impl Fn(&CanvasObject) -> bool,
) -> Vec<ObjectId> {
    ids.iter()
        .copied()
        .filter(|&id| {
            app.doc.canvases[ci]
                .object(id)
                .map(|o| !o.locked && pred(o))
                .unwrap_or(false)
        })
        .collect()
}

pub(super) fn selection_context_label(app: &PlotxApp, ci: usize, ids: &[ObjectId]) -> String {
    let Some(canvas) = app.doc.canvases.get(ci) else {
        return "No canvas".to_owned();
    };
    if ids.is_empty() {
        if let Some(panel) = app
            .session
            .ui
            .hierarchical_selection
            .lead()
            .and_then(|path| path.panel)
            .and_then(|id| canvas.panel(id))
        {
            let label = match &panel.label.mode {
                plotx_core::state::PanelLabelMode::Auto { slot } => {
                    canvas.panel_label_style.format(*slot as usize)
                }
                plotx_core::state::PanelLabelMode::LockedAuto { value }
                | plotx_core::state::PanelLabelMode::Manual { value } => value.clone(),
            };
            return format!(
                "{} / {}",
                canvas.name,
                if label.is_empty() {
                    panel.name.clone()
                } else {
                    format!("Panel {label}")
                }
            );
        }
        return format!("Canvas · {}", canvas.name);
    }
    let datasets: std::collections::HashSet<_> = ids
        .iter()
        .filter_map(|&id| canvas.object(id))
        .filter_map(CanvasObject::plot)
        .flat_map(|plot| {
            plot.binding
                .series
                .iter()
                .map(|series| series.source.resource)
        })
        .collect();
    if ids.len() > 1 {
        let objects = format!("{} objects", ids.len());
        return if datasets.is_empty() {
            objects
        } else {
            format!(
                "{objects} · {} {}",
                datasets.len(),
                if datasets.len() == 1 {
                    "dataset"
                } else {
                    "datasets"
                }
            )
        };
    }
    let object = canvas
        .object(ids[0])
        .map(|object| object.name.clone())
        .unwrap_or_else(|| "Object".to_owned());
    if let Some(panel) = canvas.parent_panel(ids[0]).and_then(|id| canvas.panel(id)) {
        let label = match &panel.label.mode {
            plotx_core::state::PanelLabelMode::Auto { slot } => {
                canvas.panel_label_style.format(*slot as usize)
            }
            plotx_core::state::PanelLabelMode::LockedAuto { value }
            | plotx_core::state::PanelLabelMode::Manual { value } => value.clone(),
        };
        return format!(
            "{} / {} / {}",
            canvas.name,
            if label.is_empty() {
                panel.name.clone()
            } else {
                format!("Panel {label}")
            },
            object
        );
    }
    let dataset = datasets
        .iter()
        .next()
        .and_then(|id| app.doc.dataset_by_id(*id))
        .map(Dataset::display_name);
    dataset.map_or(object.clone(), |dataset| format!("{object} · {dataset}"))
}

/// Snapshot the touched objects' pre-edit frames once per interaction; later
/// widget frames in the same drag see it already set and
/// leave the earliest snapshot in place.
pub(super) fn note_inspector_edit(app: &mut PlotxApp, ci: usize, ids: &[ObjectId]) {
    if app.session.ui.inspector_edit.is_some() {
        return;
    }
    let Some(c) = app.doc.canvases.get(ci) else {
        return;
    };
    let frames = ids
        .iter()
        .filter_map(|&id| c.layout_frame(id).map(|frame| (id, frame)))
        .collect();
    app.session.ui.inspector_edit = Some(PendingInspectorEdit { canvas: ci, frames });
}

/// Commit the coalesced interaction once it ends (pointer released and no text
/// field focused), emitting at most one frame action. Style fields use the
/// property catalog's independent gesture coalescing.
pub(super) fn flush_inspector_edit(app: &mut PlotxApp, ui: &Ui, text_focused: bool) {
    if app.session.ui.inspector_edit.is_none() {
        return;
    }
    if text_focused || ui.input(|i| i.pointer.any_down()) {
        return;
    }
    let Some(edit) = app.session.ui.inspector_edit.take() else {
        return;
    };
    let ci = edit.canvas;
    let (fb, fa) = {
        let Some(c) = app.doc.canvases.get(ci) else {
            return;
        };
        let mut fb = Vec::new();
        let mut fa = Vec::new();
        for &(id, before) in &edit.frames {
            if let Some(after) = c.layout_frame(id)
                && after != before
            {
                fb.push((id, before));
                fa.push((id, after));
            }
        }
        (fb, fa)
    };

    if !fb.is_empty() {
        app.execute_action(Action::set_object_frames(ci, fb, fa));
    }
}
