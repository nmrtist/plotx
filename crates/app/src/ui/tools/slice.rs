use egui::{Button, ComboBox, DragValue, Ui};
use plotx_core::actions::Action;
use plotx_core::state::{
    AxisProjection, Dataset, ObjectId, PlotxApp, ProjectionSource, SliceCursor, Tool,
};
use plotx_processing::{Processed2D, ProjectionMode, SliceKind};

pub(super) fn slice_group(app: &mut PlotxApp, di: usize, ui: &mut Ui) {
    let (is_true_2d, increments) = {
        let Some(n) = app.doc.datasets.get(di).and_then(Dataset::as_nmr2d) else {
            return;
        };
        let increments = match &n.processed {
            Processed2D::Stack(s) => s.increments(),
            Processed2D::Ft(_) => 0,
        };
        (n.is_true_2d(), increments)
    };

    ui.separator();
    ui.strong("Slice");
    ui.small(if is_true_2d {
        "Pull a 1D row or column out of the contour as an independent spectrum."
    } else {
        "Pull one increment's 1D spectrum out of the stack."
    });

    let active = app.session.tool == Tool::Slice;
    if ui
        .selectable_label(active, "✂  Extract slices")
        .on_hover_text("Turn on, then hover the plot to position the cut (S).")
        .clicked()
    {
        app.toggle_tool(Tool::Slice);
    }

    if is_true_2d {
        slice_group_2d(app, di, active, ui);
    } else if increments > 0 {
        slice_group_stack(app, di, increments, ui);
    } else {
        ui.weak("No slice-able data for this dataset.");
    }
}

fn slice_group_2d(app: &mut PlotxApp, di: usize, active: bool, ui: &mut Ui) {
    let mut kind = app.session.ui.slice_kind;
    ui.horizontal(|ui| {
        ui.label("Orientation");
        ui.selectable_value(&mut kind, SliceKind::Row, "Row (vs F2)");
        ui.selectable_value(&mut kind, SliceKind::Column, "Column (vs F1)");
    });
    if kind != app.session.ui.slice_kind {
        app.session.ui.slice_kind = kind;
        if let Some(c) = app.session.ui.slice.as_mut().filter(|c| c.dataset == di) {
            c.kind = kind;
        }
    }
    if active {
        ui.small("Hover the contour: the guide line snaps to the nearest grid line.");
    }

    match app.session.ui.slice.filter(|c| c.dataset == di) {
        Some(c) => match slice_position_ppm(app, di, c) {
            Some(p) => {
                ui.small(format!(
                    "At {} = {p:.3} ppm  (index {})",
                    fixed_axis(c.kind),
                    c.index
                ));
            }
            None => {
                ui.small(format!("Index {}", c.index));
            }
        },
        None => {
            ui.weak("Move the cursor over the plot to place a slice.");
        }
    }

    let has_cursor = matches!(app.session.ui.slice, Some(c) if c.dataset == di);
    if ui
        .add_enabled(has_cursor, Button::new("Extract to new dataset"))
        .on_disabled_hover_text("Position a slice on the plot first")
        .clicked()
    {
        app.extract_slice_dataset(di);
    }

    ui.separator();
    ui.small("Projections collapse the whole axis onto F2 or F1:");
    ui.horizontal(|ui| {
        ui.label("F2");
        if ui.button("Sum").clicked() {
            app.extract_projection_dataset(di, SliceKind::Row, ProjectionMode::Sum);
        }
        if ui.button("Skyline").clicked() {
            app.extract_projection_dataset(di, SliceKind::Row, ProjectionMode::Skyline);
        }
    });
    ui.horizontal(|ui| {
        ui.label("F1");
        if ui.button("Sum").clicked() {
            app.extract_projection_dataset(di, SliceKind::Column, ProjectionMode::Sum);
        }
        if ui.button("Skyline").clicked() {
            app.extract_projection_dataset(di, SliceKind::Column, ProjectionMode::Skyline);
        }
    });

    projection_group(app, di, ui);
}

fn projection_group(app: &mut PlotxApp, di: usize, ui: &mut Ui) {
    ui.separator();
    ui.strong("Axis projections");
    ui.small("1D traces drawn alongside the contour's top (F2) and left (F1) axes.");

    let Some((ci, object)) = active_plot_for(app, di) else {
        ui.weak("Open this spectrum on a page to add axis projections.");
        return;
    };

    let current = app.doc.canvases[ci]
        .object(object)
        .and_then(|o| o.plot())
        .map(|p| p.projections.clone())
        .unwrap_or_default();

    let attachable: Vec<(usize, String)> = app
        .doc
        .datasets
        .iter()
        .enumerate()
        .filter(|(_, d)| d.as_nmr().is_some())
        .map(|(i, d)| (i, d.display_name()))
        .collect();

    let (f2_max, f1_max) = match &app.doc.datasets[di].as_nmr2d().unwrap().processed {
        Processed2D::Ft(s) => (s.f2_size.saturating_sub(1), s.f1_size.saturating_sub(1)),
        Processed2D::Stack(_) => (0, 0),
    };
    let cursor = app.session.ui.slice.filter(|c| c.dataset == di);
    let top_seed = cursor.filter(|c| c.kind == SliceKind::Row).map(|c| c.index);
    let left_seed = cursor
        .filter(|c| c.kind == SliceKind::Column)
        .map(|c| c.index);

    let mut after = current.clone();
    axis_projection_row(
        ui,
        "Top (F2)",
        "top",
        &mut after.top,
        &attachable,
        top_seed,
        f1_max,
    );
    axis_projection_row(
        ui,
        "Left (F1)",
        "left",
        &mut after.left,
        &attachable,
        left_seed,
        f2_max,
    );

    if after != current {
        app.execute_action(Action::SetAxisProjections {
            canvas: ci,
            object,
            before: current,
            after,
        });
    }
}

fn active_plot_for(app: &PlotxApp, di: usize) -> Option<(usize, ObjectId)> {
    let ci = app.session.active_canvas?;
    let canvas = app.doc.canvases.get(ci)?;
    let id = canvas
        .objects
        .iter()
        .find(|o| o.plot().map(|p| p.primary_dataset()) == Some(di))
        .map(|o| o.id)?;
    Some((ci, id))
}

#[allow(clippy::too_many_arguments)]
fn axis_projection_row(
    ui: &mut Ui,
    label: &str,
    salt: &str,
    axis: &mut AxisProjection,
    attachable: &[(usize, String)],
    slice_seed: Option<usize>,
    slice_max: usize,
) {
    let slice_default = ProjectionSource::Slice(slice_seed.unwrap_or(match axis.source {
        ProjectionSource::Slice(i) => i,
        _ => 0,
    }));
    ui.horizontal(|ui| {
        ui.label(label);
        ComboBox::from_id_salt((salt, "src"))
            .selected_text(source_label(&axis.source, attachable))
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut axis.source, ProjectionSource::None, "None");
                ui.selectable_value(&mut axis.source, ProjectionSource::Sum, "Sum projection");
                ui.selectable_value(
                    &mut axis.source,
                    ProjectionSource::Skyline,
                    "Skyline projection",
                );
                ui.selectable_value(&mut axis.source, slice_default.clone(), "Current slice");
                if let Some((first, _)) = attachable.first() {
                    let attach = match axis.source {
                        ProjectionSource::Attached(d) => ProjectionSource::Attached(d),
                        _ => ProjectionSource::Attached(*first),
                    };
                    ui.selectable_value(&mut axis.source, attach, "Attach 1D dataset…");
                }
            });
        if !matches!(axis.source, ProjectionSource::None) {
            ui.checkbox(&mut axis.visible, "Show");
        }
    });

    match &mut axis.source {
        ProjectionSource::Attached(sel) => {
            ui.horizontal(|ui| {
                ui.add_space(12.0);
                ComboBox::from_id_salt((salt, "ds"))
                    .selected_text(
                        attachable
                            .iter()
                            .find(|(i, _)| i == sel)
                            .map(|(_, n)| n.as_str())
                            .unwrap_or("(pick a dataset)"),
                    )
                    .show_ui(ui, |ui| {
                        for (i, name) in attachable {
                            ui.selectable_value(sel, *i, name);
                        }
                    });
            });
        }
        ProjectionSource::Slice(index) => {
            ui.horizontal(|ui| {
                ui.add_space(12.0);
                ui.label("Grid index");
                ui.add(DragValue::new(index).range(0..=slice_max).speed(0.3));
            });
        }
        _ => {}
    }
}

fn source_label(source: &ProjectionSource, attachable: &[(usize, String)]) -> String {
    match source {
        ProjectionSource::None => "None".to_owned(),
        ProjectionSource::Sum => "Sum projection".to_owned(),
        ProjectionSource::Skyline => "Skyline projection".to_owned(),
        ProjectionSource::Slice(_) => "Current slice".to_owned(),
        ProjectionSource::Attached(d) => attachable
            .iter()
            .find(|(i, _)| i == d)
            .map(|(_, n)| format!("Attached: {n}"))
            .unwrap_or_else(|| "Attached".to_owned()),
    }
}

fn slice_group_stack(app: &mut PlotxApp, di: usize, increments: usize, ui: &mut Ui) {
    let object = app
        .session
        .active_canvas
        .and_then(|ci| app.doc.canvases.get(ci))
        .and_then(|c| c.selected_plot_object_id())
        .unwrap_or(0);
    let mut index = app
        .session
        .ui
        .slice
        .filter(|c| c.dataset == di)
        .map(|c| c.index)
        .unwrap_or(0)
        .min(increments - 1);

    ui.horizontal(|ui| {
        ui.label("Increment");
        ui.add(
            DragValue::new(&mut index)
                .range(0..=increments - 1)
                .speed(0.2),
        );
        ui.weak(format!("of {increments}"));
    });
    app.session.ui.slice = Some(SliceCursor {
        dataset: di,
        object,
        kind: SliceKind::Row,
        index,
    });

    if let Some(value) = stack_ruler_value(app, di, index) {
        ui.small(format!("Ruler: {value}"));
    }

    if ui.button("Extract increment to new dataset").clicked() {
        app.extract_slice_dataset(di);
    }
}

fn slice_position_ppm(app: &PlotxApp, di: usize, c: SliceCursor) -> Option<f64> {
    let Processed2D::Ft(s) = &app.doc.datasets.get(di)?.as_nmr2d()?.processed else {
        return None;
    };
    match c.kind {
        SliceKind::Row => s.f1_ppm.get(c.index).copied(),
        SliceKind::Column => s.f2_ppm.get(c.index).copied(),
    }
}

fn stack_ruler_value(app: &PlotxApp, di: usize, index: usize) -> Option<String> {
    let axis = app
        .doc
        .datasets
        .get(di)?
        .as_nmr2d()?
        .data
        .pseudo_axis
        .as_ref()?;
    let v = axis.values.get(index)?;
    Some(format!("{v:.4} {}", axis.unit))
}

/// The axis a cut of `kind` holds fixed (opposite of the one its trace runs along).
fn fixed_axis(kind: SliceKind) -> &'static str {
    match kind {
        SliceKind::Row => "F1",
        SliceKind::Column => "F2",
    }
}
