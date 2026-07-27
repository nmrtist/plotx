//! Data binding, series, and stack controls for plot objects.

use super::*;

/// Binding edits rebuild through `SetDataBinding`; stack-layout edits through
/// `SetStackSpec`.
pub(super) fn data_section(app: &mut PlotxApp, ci: usize, object: ObjectId, ui: &mut Ui) {
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
                    && let Some(target) = app.series_target(ci, object, sb.id)
                    && let Ok(commit) = app.plan_property_write(
                        plotx_core::properties::object::SERIES_VISIBLE,
                        std::slice::from_ref(&target),
                        &plotx_core::properties::PropertyValue::Bool(visible),
                    )
                {
                    app.commit_property(commit);
                }
            }
            let color = sb
                .primary_color()
                .unwrap_or(OVERLAY_PALETTE[i % OVERLAY_PALETTE.len()]);
            swatch(ui, color);
            let name = app
                .doc
                .dataset_index(sb.source.resource)
                .and_then(|index| app.doc.datasets.get(index))
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
                    if matches!(sb.encoding, plotx_figure::SeriesEncoding::Line(_)) {
                        let mut scale = sb.line_scale();
                        if ui
                            .add(DragValue::new(&mut scale).speed(0.02).range(0.01..=100.0))
                            .on_hover_text("Scale")
                            .changed()
                        {
                            let mut b = binding.clone();
                            if let plotx_figure::SeriesEncoding::Line(line) =
                                &mut b.series[i].encoding
                            {
                                line.scale = scale;
                            }
                            next_binding = Some(b);
                        }
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
    if binding
        .primary_dataset()
        .and_then(|id| app.doc.dataset_by_id(id))
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
                            let Some(series_id) = app
                                .doc
                                .canvases
                                .get_mut(ci)
                                .and_then(|canvas| canvas.object_mut(object))
                                .and_then(|object| object.plot_mut())
                                .map(|plot| plot.allocate_series_id())
                            else {
                                continue;
                            };
                            if let Some(mut series) =
                                SeriesBinding::from_dataset(&app.doc.datasets[*di])
                            {
                                series.id = series_id;
                                b.series.push(series);
                                next_binding = Some(b);
                            }
                        }
                    }
                });
        }
    } else {
        ui.weak("Stacking is available for line-series plots.");
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

    crate::ui::properties::panel::stack_section(app, ci, &[object], ui);
    crate::ui::properties::panel::chart_section(app, ci, &[object], ui);
    chart_gallery(app, ci, object, ui);
}

fn swatch(ui: &mut Ui, color: Color) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(14.0, 10.0), egui::Sense::hover());
    ui.painter().rect_filled(
        rect,
        2.0,
        egui::Color32::from_rgb(color.r, color.g, color.b),
    );
}
