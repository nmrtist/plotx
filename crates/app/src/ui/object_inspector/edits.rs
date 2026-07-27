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

pub(super) fn selection_label(app: &PlotxApp, ci: usize, ids: &[ObjectId]) -> String {
    if ids.len() > 1 {
        format!("{} selected", ids.len())
    } else {
        app.doc.canvases[ci]
            .object(ids[0])
            .map(|o| o.name.clone())
            .unwrap_or_default()
    }
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
        .filter_map(|&id| c.object(id).map(|o| (id, o.frame)))
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
            if let Some(o) = c.object(id)
                && o.frame != before
            {
                fb.push((id, before));
                fa.push((id, o.frame));
            }
        }
        (fb, fa)
    };

    if !fb.is_empty() {
        app.execute_action(Action::set_object_frames(ci, fb, fa));
    }
}
