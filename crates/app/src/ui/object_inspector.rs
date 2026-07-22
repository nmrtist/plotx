//! The Object inspector: geometry + per-kind style editing for the current
//! page-space selection, at the top of the Secondary Side Bar.

mod axes;
mod chart_gallery;
mod panel_note;

use axes::{axes_section, commit_if_target_changed};
use chart_gallery::chart_gallery;
use egui::{DragValue, Ui};
use egui_phosphor::regular as icon;
use panel_note::{commit_panel_note_edit, panel_note_section};
use plotx_core::actions::{Action, PendingInspectorEdit};
use plotx_core::state::{
    CanvasObject, DataBinding, Dataset, MM_TO_PT, OVERLAY_PALETTE, ObjectFrame, ObjectId, PlotxApp,
    SeriesBinding, ShapeKind, StackKind, StackMode, StackSpec, TextAlign,
};
use plotx_figure::Color;

pub(crate) fn render(app: &mut PlotxApp, ui: &mut Ui) {
    let ids: Vec<ObjectId> = app.session.ui.selection.objects().to_vec();
    let axis_target = app.session.active_canvas.and_then(|ci| {
        (ids.len() == 1
            && app
                .doc
                .canvases
                .get(ci)?
                .object(ids[0])
                .is_some_and(|object| object.plot().is_some()))
        .then(|| (ci, ids[0]))
    });
    commit_if_target_changed(app, axis_target);
    let Some(ci) = app.session.active_canvas else {
        return;
    };
    if ci >= app.doc.canvases.len() {
        return;
    }
    if ids.is_empty() {
        commit_panel_note_edit(app);
        return;
    }

    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.strong("Object");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.weak(selection_label(app, ci, &ids));
        });
    });
    ui.add_space(4.0);

    geometry_section(app, ci, &ids, ui);

    let mut note_focused = false;
    let mut axes_focused = false;
    if ids.len() == 1
        && app.doc.canvases[ci]
            .object(ids[0])
            .map(|o| o.plot().is_some())
            .unwrap_or(false)
    {
        ui.separator();
        axes_focused = axes_section(app, ci, ids[0], ui);
        ui.separator();
        note_focused = panel_note_section(app, ci, ids[0], ui);
        data_section(app, ci, ids[0], ui);
    }

    let text_ids = kind_targets(app, ci, &ids, |o| o.text().is_some());
    let shape_ids = kind_targets(app, ci, &ids, |o| o.shape().is_some());

    let mut text_focused = false;
    if !text_ids.is_empty() {
        ui.separator();
        text_focused = text_section(app, ci, &text_ids, ui);
    }
    if !shape_ids.is_empty() {
        ui.separator();
        shape_section(app, ci, &shape_ids, ui);
    }

    let primary = ids[0];
    if app.doc.canvases[ci]
        .object(primary)
        .and_then(|o| o.style())
        .is_some()
    {
        ui.separator();
        format_once_section(app, ci, primary, ui);
    }

    flush_inspector_edit(app, ui, text_focused || note_focused || axes_focused);
    ui.separator();
    ui.add_space(2.0);
}

fn geometry_section(app: &mut PlotxApp, ci: usize, ids: &[ObjectId], ui: &mut Ui) {
    let primary = ids[0];
    let Some(o) = app.doc.canvases[ci].object(primary) else {
        return;
    };
    let enabled = !o.locked;
    let frame = o.frame;
    let mut x = frame.x / MM_TO_PT;
    let mut y = frame.y / MM_TO_PT;
    let mut w = frame.width / MM_TO_PT;
    let mut h = frame.height / MM_TO_PT;

    egui::Grid::new("object_geometry")
        .num_columns(4)
        .spacing([6.0, 4.0])
        .show(ui, |ui| {
            ui.label("X");
            let rx = ui.add_enabled(enabled, mm_drag(&mut x));
            ui.label("Y");
            let ry = ui.add_enabled(enabled, mm_drag(&mut y));
            ui.end_row();
            ui.label("W");
            let rw = ui.add_enabled(enabled, mm_drag(&mut w));
            ui.label("H");
            let rh = ui.add_enabled(enabled, mm_drag(&mut h));
            ui.end_row();

            if rx.changed() || ry.changed() || rw.changed() || rh.changed() {
                note_inspector_edit(app, ci, ids);
                let new = ObjectFrame::new(x * MM_TO_PT, y * MM_TO_PT, w * MM_TO_PT, h * MM_TO_PT);
                app.set_object_frame(ci, primary, new);
            }
        });

    if !enabled {
        ui.weak("Locked — unlock to edit geometry.");
    } else if ids.len() > 1 {
        ui.weak("Geometry edits the primary selection.");
    }
}

/// Binding edits rebuild through `SetDataBinding`; stack-layout edits through
/// `SetStackSpec`.
fn data_section(app: &mut PlotxApp, ci: usize, object: ObjectId, ui: &mut Ui) {
    let Some((binding, stack)) = app.doc.canvases[ci]
        .object(object)
        .and_then(|o| o.plot())
        .map(|p| (p.binding.clone(), p.stack))
    else {
        return;
    };

    ui.separator();
    ui.strong("Data");

    let is_stack = binding.series.len() > 1 && app.series_stackable(&binding);
    let count = binding.series.len();
    let mut next_binding: Option<DataBinding> = None;
    let mut next_stack: Option<StackSpec> = None;
    for (i, sb) in binding.series.iter().enumerate() {
        ui.horizontal(|ui| {
            if is_stack {
                let mut visible = sb.visible;
                if ui
                    .checkbox(&mut visible, "")
                    .on_hover_text("Visible")
                    .changed()
                {
                    let mut b = binding.clone();
                    b.series[i].visible = visible;
                    next_binding = Some(b);
                }
            }
            let color = sb
                .color
                .unwrap_or(OVERLAY_PALETTE[i % OVERLAY_PALETTE.len()]);
            swatch(ui, color);
            let name = app
                .doc
                .datasets
                .get(sb.dataset)
                .map(Dataset::display_name)
                .unwrap_or_default();
            let label = if i == 0 {
                format!("{name} (primary)")
            } else {
                name
            };
            if is_stack {
                if ui
                    .selectable_label(stack.active == Some(i), label)
                    .on_hover_text("Highlight this trace")
                    .clicked()
                {
                    next_stack = Some(StackSpec {
                        active: Some(i),
                        ..stack
                    });
                }
            } else {
                ui.label(label);
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if count > 1 && ui.small_button(icon::X).on_hover_text("Remove").clicked() {
                    let mut b = binding.clone();
                    b.series.remove(i);
                    next_binding = Some(b);
                }
                if is_stack {
                    if ui
                        .add_enabled(i + 1 < count, egui::Button::new(icon::CARET_DOWN).small())
                        .on_hover_text("Move down")
                        .clicked()
                    {
                        let mut b = binding.clone();
                        b.series.swap(i, i + 1);
                        next_binding = Some(b);
                    }
                    if ui
                        .add_enabled(i > 0, egui::Button::new(icon::CARET_UP).small())
                        .on_hover_text("Move up")
                        .clicked()
                    {
                        let mut b = binding.clone();
                        b.series.swap(i, i - 1);
                        next_binding = Some(b);
                    }
                    let mut scale = sb.scale;
                    if ui
                        .add(DragValue::new(&mut scale).speed(0.02).range(0.01..=100.0))
                        .on_hover_text("Scale")
                        .changed()
                    {
                        let mut b = binding.clone();
                        b.series[i].scale = scale;
                        next_binding = Some(b);
                    }
                } else if i != 0 && ui.small_button("Primary").clicked() {
                    let mut b = binding.clone();
                    b.series.swap(0, i);
                    next_binding = Some(b);
                }
            });
        });
    }

    let candidates = app.stack_candidates(&binding);
    if app
        .doc
        .datasets
        .get(binding.primary_dataset())
        .map(Dataset::domain)
        .is_some_and(|d| d.stack_kind().is_some())
    {
        if candidates.is_empty() {
            ui.weak("No other datasets to stack.");
        } else {
            egui::ComboBox::from_id_salt("object_add_series")
                .selected_text("Add series…")
                .show_ui(ui, |ui| {
                    for di in &candidates {
                        let label = app.doc.datasets[*di].display_name();
                        if ui.selectable_label(false, label).clicked() {
                            let mut b = binding.clone();
                            b.series.push(SeriesBinding::new(*di));
                            next_binding = Some(b);
                        }
                    }
                });
        }
    } else {
        ui.weak("Stacking is available for line-series plots.");
    }

    if is_stack {
        let kind = app
            .doc
            .datasets
            .get(binding.primary_dataset())
            .and_then(|d| d.domain().stack_kind());
        if let Some(kind) = kind {
            stack_controls(kind, &stack, &mut next_stack, ui);
        }
    }

    if let Some(after) = next_binding
        && after != binding
    {
        app.execute_action(Action::set_data_binding(ci, object, binding, after));
        app.session.status = "Updated plot data.".to_owned();
    } else if let Some(after) = next_stack
        && after != stack
    {
        app.execute_action(Action::set_stack_spec(ci, object, stack, after));
        app.session.status = "Updated stack layout.".to_owned();
    }

    chart_gallery(app, ci, object, ui);
}

fn stack_controls(kind: StackKind, stack: &StackSpec, next: &mut Option<StackSpec>, ui: &mut Ui) {
    ui.separator();
    if kind == StackKind::Field {
        ui.horizontal(|ui| {
            ui.label("Mode");
            ui.label("Color overlay");
        });
        return;
    }
    ui.horizontal(|ui| {
        ui.label("Mode");
        if ui
            .selectable_label(stack.mode == StackMode::Superimposed, "Superimposed")
            .clicked()
        {
            *next = Some(StackSpec {
                mode: StackMode::Superimposed,
                ..*stack
            });
        }
        if ui
            .selectable_label(stack.mode == StackMode::Offset, "Offset")
            .clicked()
        {
            *next = Some(StackSpec {
                mode: StackMode::Offset,
                ..*stack
            });
        }
    });

    if stack.mode != StackMode::Offset {
        return;
    }
    ui.horizontal(|ui| {
        ui.label("Vertical spacing");
        let mut v = stack.spacing_y;
        if ui.add(egui::Slider::new(&mut v, 0.0..=1.0)).changed() {
            *next = Some(StackSpec {
                spacing_y: v,
                ..*stack
            });
        }
    });
    ui.horizontal(|ui| {
        ui.label("3D shear");
        // Signed: positive leans traces up-and-right (bottom-left → top-right),
        // negative up-and-left (bottom-right → top-left). Zero = pure vertical.
        let mut v = stack.shear_x;
        if ui
            .add(egui::Slider::new(&mut v, -0.5..=0.5))
            .on_hover_text("Drag right to lean up-and-right, left to lean up-and-left")
            .changed()
        {
            *next = Some(StackSpec {
                shear_x: v,
                ..*stack
            });
        }
    });
    let mut normalize = stack.normalize;
    if ui.checkbox(&mut normalize, "Normalize").changed() {
        *next = Some(StackSpec {
            normalize,
            ..*stack
        });
    }
}

fn swatch(ui: &mut Ui, color: Color) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(14.0, 10.0), egui::Sense::hover());
    ui.painter().rect_filled(
        rect,
        2.0,
        egui::Color32::from_rgb(color.r, color.g, color.b),
    );
}

fn mm_drag(value: &mut f32) -> DragValue<'_> {
    DragValue::new(value)
        .speed(0.5)
        .max_decimals(1)
        .suffix(" mm")
}

fn text_section(app: &mut PlotxApp, ci: usize, ids: &[ObjectId], ui: &mut Ui) -> bool {
    let rep_id = ids[0];
    let Some(rep) = app.doc.canvases[ci]
        .object(rep_id)
        .and_then(|o| o.text())
        .cloned()
    else {
        return false;
    };
    ui.strong("Text");

    let mut focused = false;
    if ids.len() == 1 {
        let mut buf = rep.text.clone();
        let resp = ui.add(
            egui::TextEdit::multiline(&mut buf)
                .desired_rows(2)
                .desired_width(f32::INFINITY),
        );
        if resp.changed() {
            note_inspector_edit(app, ci, ids);
            if let Some(t) = app.doc.canvases[ci]
                .object_mut(rep_id)
                .and_then(|o| o.text_mut())
            {
                t.text = buf;
            }
        }
        focused = resp.has_focus();
    }

    let mut font = rep.font_size;
    ui.horizontal(|ui| {
        ui.label("Size");
        if ui
            .add(DragValue::new(&mut font).speed(0.5).range(4.0..=200.0))
            .changed()
        {
            note_inspector_edit(app, ci, ids);
            for &id in ids {
                if let Some(t) = app.doc.canvases[ci]
                    .object_mut(id)
                    .and_then(|o| o.text_mut())
                {
                    t.font_size = font;
                }
            }
        }
    });

    let mut bold = rep.bold;
    if ui.checkbox(&mut bold, "Bold").changed() {
        note_inspector_edit(app, ci, ids);
        for &id in ids {
            if let Some(t) = app.doc.canvases[ci]
                .object_mut(id)
                .and_then(|o| o.text_mut())
            {
                t.bold = bold;
            }
        }
    }

    ui.horizontal(|ui| {
        ui.label("Align");
        for (align, label) in [
            (TextAlign::Left, "Left"),
            (TextAlign::Center, "Center"),
            (TextAlign::Right, "Right"),
        ] {
            if ui.selectable_label(rep.align == align, label).clicked() {
                note_inspector_edit(app, ci, ids);
                for &id in ids {
                    if let Some(t) = app.doc.canvases[ci]
                        .object_mut(id)
                        .and_then(|o| o.text_mut())
                    {
                        t.align = align;
                    }
                }
            }
        }
    });

    ui.horizontal(|ui| {
        ui.label("Colour");
        let mut rgb = rgb_of(rep.color);
        if ui.color_edit_button_srgb(&mut rgb).changed() {
            note_inspector_edit(app, ci, ids);
            let color = color_of(rgb);
            for &id in ids {
                if let Some(t) = app.doc.canvases[ci]
                    .object_mut(id)
                    .and_then(|o| o.text_mut())
                {
                    t.color = color;
                }
            }
        }
    });

    focused
}

fn shape_section(app: &mut PlotxApp, ci: usize, ids: &[ObjectId], ui: &mut Ui) {
    let rep_id = ids[0];
    let Some(rep) = app.doc.canvases[ci]
        .object(rep_id)
        .and_then(|o| o.shape())
        .cloned()
    else {
        return;
    };
    ui.strong("Shape");

    ui.horizontal(|ui| {
        ui.label("Kind");
        for (kind, label) in [
            (ShapeKind::Rect, "Rect"),
            (ShapeKind::Ellipse, "Ellipse"),
            (ShapeKind::Line, "Line"),
            (ShapeKind::Arrow, "Arrow"),
        ] {
            if ui.selectable_label(rep.shape == kind, label).clicked() {
                note_inspector_edit(app, ci, ids);
                for &id in ids {
                    if let Some(s) = app.doc.canvases[ci]
                        .object_mut(id)
                        .and_then(|o| o.shape_mut())
                    {
                        s.shape = kind;
                    }
                }
            }
        }
    });

    ui.horizontal(|ui| {
        ui.label("Stroke");
        let mut rgb = rgb_of(rep.stroke);
        if ui.color_edit_button_srgb(&mut rgb).changed() {
            note_inspector_edit(app, ci, ids);
            let color = color_of(rgb);
            for &id in ids {
                if let Some(s) = app.doc.canvases[ci]
                    .object_mut(id)
                    .and_then(|o| o.shape_mut())
                {
                    s.stroke = color;
                }
            }
        }
        let mut width = rep.stroke_width;
        if ui
            .add(DragValue::new(&mut width).speed(0.1).range(0.1..=40.0))
            .changed()
        {
            note_inspector_edit(app, ci, ids);
            for &id in ids {
                if let Some(s) = app.doc.canvases[ci]
                    .object_mut(id)
                    .and_then(|o| o.shape_mut())
                {
                    s.stroke_width = width;
                }
            }
        }
    });

    ui.horizontal(|ui| {
        let mut fill_on = rep.fill.is_some();
        if ui.checkbox(&mut fill_on, "Fill").changed() {
            note_inspector_edit(app, ci, ids);
            let fill = fill_on.then(|| rep.fill.unwrap_or(Color::rgb(200, 200, 200)));
            for &id in ids {
                if let Some(s) = app.doc.canvases[ci]
                    .object_mut(id)
                    .and_then(|o| o.shape_mut())
                {
                    s.fill = fill;
                }
            }
        }
        if let Some(current) = rep.fill {
            let mut rgb = rgb_of(current);
            if ui.color_edit_button_srgb(&mut rgb).changed() {
                note_inspector_edit(app, ci, ids);
                let color = color_of(rgb);
                for &id in ids {
                    if let Some(s) = app.doc.canvases[ci]
                        .object_mut(id)
                        .and_then(|o| o.shape_mut())
                    {
                        s.fill = Some(color);
                    }
                }
            }
        }
    });
}

fn format_once_section(app: &mut PlotxApp, ci: usize, primary: ObjectId, ui: &mut Ui) {
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

fn kind_targets(
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

fn selection_label(app: &PlotxApp, ci: usize, ids: &[ObjectId]) -> String {
    if ids.len() > 1 {
        format!("{} selected", ids.len())
    } else {
        app.doc.canvases[ci]
            .object(ids[0])
            .map(|o| o.name.clone())
            .unwrap_or_default()
    }
}

/// Snapshot the touched objects' pre-edit frames and styles once per
/// interaction; later widget frames in the same drag see it already set and
/// leave the earliest snapshot in place.
fn note_inspector_edit(app: &mut PlotxApp, ci: usize, ids: &[ObjectId]) {
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
    let styles = ids
        .iter()
        .filter_map(|&id| c.object(id).and_then(|o| o.style().map(|s| (id, s))))
        .collect();
    app.session.ui.inspector_edit = Some(PendingInspectorEdit {
        canvas: ci,
        frames,
        styles,
    });
}

/// Commit the coalesced interaction once it ends (pointer released and no text
/// field focused), emitting at most one frame action and one style action for
/// whichever properties actually changed.
fn flush_inspector_edit(app: &mut PlotxApp, ui: &Ui, text_focused: bool) {
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
    let (fb, fa, sb, sa) = {
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
        let mut sb = Vec::new();
        let mut sa = Vec::new();
        for (id, before) in &edit.styles {
            if let Some(cur) = c.object(*id).and_then(|o| o.style())
                && cur != *before
            {
                sb.push((*id, before.clone()));
                sa.push((*id, cur));
            }
        }
        (fb, fa, sb, sa)
    };

    if !fb.is_empty() {
        app.execute_action(Action::set_object_frames(ci, fb, fa));
    }
    if !sb.is_empty() {
        app.execute_action(Action::set_object_style(ci, sb, sa));
    }
}

fn rgb_of(c: Color) -> [u8; 3] {
    [c.r, c.g, c.b]
}

fn color_of(rgb: [u8; 3]) -> Color {
    Color::rgb(rgb[0], rgb[1], rgb[2])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_safely_during_active_canvas_transition() {
        let mut app = PlotxApp::new();
        app.session.active_canvas = Some(0);
        assert!(app.doc.canvases.is_empty());
        assert!(app.session.ui.selection.objects().is_empty());

        let ctx = egui::Context::default();
        let _ = ctx.run_ui(egui::RawInput::default(), |ui| render(&mut app, ui));
    }
}
