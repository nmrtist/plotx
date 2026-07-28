//! Processing page in the shared top-right task dock.
//!
//! Pipelines are vertical because order and domain transitions are structural,
//! not decoration. FFT is an ordinary typed Time → Frequency step.

use super::*;
use crate::ui::tools::task_card::{self, TaskCardGeometry};
use egui::{Area, Key, Order, RichText};
use plotx_core::state::TaskDockTab;
use plotx_io::Domain;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SourceShape {
    RawFid,
    RawAcquisition2D,
    ImportedSpectrum,
}

impl SourceShape {
    fn label(self) -> &'static str {
        match self {
            Self::RawFid => "Raw FID",
            Self::RawAcquisition2D => "2D acquisition",
            Self::ImportedSpectrum => "Imported spectrum",
        }
    }
}

#[derive(Clone)]
struct AxisShape {
    axis: PhaseAxis,
    pipeline: AxisPipeline,
}

struct SurfaceShape {
    input_domain: Domain,
    source: SourceShape,
    axes: Vec<AxisShape>,
}

fn surface_shape(dataset: &Dataset) -> Option<SurfaceShape> {
    let (input_domain, source) = match dataset {
        Dataset::Nmr(dataset) => (
            dataset.data.domain,
            match dataset.data.domain {
                Domain::Time => SourceShape::RawFid,
                Domain::Frequency => SourceShape::ImportedSpectrum,
            },
        ),
        Dataset::Nmr2D(dataset) => (
            dataset.data.domain,
            match dataset.data.domain {
                Domain::Time => SourceShape::RawAcquisition2D,
                Domain::Frequency => SourceShape::ImportedSpectrum,
            },
        ),
        Dataset::Table(_) | Dataset::Electrophysiology(_) | Dataset::Afm(_) => return None,
    };
    let axes = dataset
        .phase_axes()
        .iter()
        .filter_map(|&axis| {
            dataset
                .axis_pipeline(axis)
                .cloned()
                .map(|pipeline| AxisShape { axis, pipeline })
        })
        .collect::<Vec<_>>();
    (!axes.is_empty()).then_some(SurfaceShape {
        input_domain,
        source,
        axes,
    })
}

pub(super) fn render(app: &mut PlotxApp, host: &mut Ui) {
    if !task_card::is_active(app, TaskDockTab::Processing) {
        return;
    }
    let Some(owner) = app.session.ui.processing_task_dataset else {
        return;
    };
    let Some(di) = app.doc.dataset_index(owner) else {
        app.session.ui.close_task_tab(TaskDockTab::Processing);
        return;
    };
    if app.active_dataset() != Some(di) {
        return;
    }
    let Some(shape) = surface_shape(&app.doc.datasets[di]) else {
        app.session.ui.close_task_tab(TaskDockTab::Processing);
        return;
    };
    let TaskCardGeometry {
        pos,
        width,
        min_body_height,
        max_body_height,
    } = task_card::geometry(host, 300.0);
    let collapsed = app.session.ui.processing_task_collapsed;
    let dark = host.visuals().dark_mode;
    let mut close = false;
    let mut toggle = false;

    Area::new(egui::Id::new("processing_task_card"))
        .order(Order::Foreground)
        .fixed_pos(pos)
        .show(host.ctx(), |ui| {
            ui.set_width(width);
            crate::ui::card_frame(dark, egui::Margin::ZERO).show(ui, |ui| {
                if task_card::tab_bar(app, TaskDockTab::Processing, ui) {
                    ui.separator();
                }
                let name = app.doc.datasets[di].display_name();
                let output = shape
                    .axes
                    .iter()
                    .map(|axis| {
                        axis.pipeline
                            .output_domain(shape.input_domain)
                            .unwrap_or(shape.input_domain)
                    })
                    .collect::<Vec<_>>();
                ui.horizontal(|ui| {
                    ui.strong("Processing");
                    ui.weak(if output.iter().all(|d| *d == Domain::Time) {
                        "Time-domain output"
                    } else {
                        "Frequency-domain output"
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .small_button(icon::X)
                            .on_hover_text("Close Processing")
                            .clicked()
                        {
                            close = true;
                        }
                        let glyph = if collapsed {
                            icon::CARET_DOWN
                        } else {
                            icon::CARET_UP
                        };
                        if ui.small_button(glyph).clicked() {
                            toggle = true;
                        }
                        ui.menu_button(icon::DOTS_THREE_VERTICAL, |ui| panel_menu(app, di, ui));
                    });
                });
                ui.add(egui::Label::new(RichText::new(name).small()).truncate());
                ui.small(format!(
                    "{} · {}",
                    shape.source.label(),
                    if is_default_processing(&app.doc.datasets[di]) {
                        "default recipe"
                    } else {
                        "modified recipe"
                    }
                ));
                if !collapsed {
                    ui.separator();
                    egui::Resize::default()
                        .id_salt("processing_task_body_resize")
                        .default_size([ui.available_width(), 430.0])
                        .min_size([ui.available_width(), min_body_height])
                        .max_size([ui.available_width(), max_body_height])
                        .resizable([false, true])
                        .with_stroke(false)
                        .show(ui, |ui| {
                            egui::ScrollArea::vertical()
                                .id_salt(("processing_task", owner))
                                .auto_shrink([false, true])
                                .show(ui, |ui| {
                                    if shape.axes.len() == 2 {
                                        ui.small("Axes are processed F2 direct, then F1 indirect.");
                                    }
                                    for axis in &shape.axes {
                                        render_axis(app, di, owner, shape.input_domain, axis, ui);
                                    }
                                    action_bar(app, ui);
                                    analysis_card(app, di, ui);
                                });
                        });
                }
            });
        });
    if toggle {
        app.session.ui.processing_task_collapsed = !collapsed;
    }
    if close {
        app.session.ui.close_task_tab(TaskDockTab::Processing);
    }
}

fn render_axis(
    app: &mut PlotxApp,
    di: usize,
    owner: DatasetId,
    input_domain: Domain,
    shape: &AxisShape,
    ui: &mut Ui,
) {
    let axis = shape.axis;
    let output = shape
        .pipeline
        .output_domain(input_domain)
        .unwrap_or(input_domain);
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.strong(axis.label());
        ui.weak(format!(
            "· {} to {}",
            domain_label(input_domain),
            domain_label(output)
        ));
    });

    let mut op = None;
    let mut domain = input_domain;
    for (index, step) in shape.pipeline.steps.iter().enumerate() {
        let before = domain;
        if step.enabled && step.kind.input_domain() == domain {
            domain = step.kind.output_domain();
        }
        step_row(
            app,
            owner,
            axis,
            &shape.pipeline.steps,
            index,
            before,
            ui,
            &mut op,
        );
    }
    if let Some((step, operation)) = op {
        apply_row_op(app, di, axis, step, operation);
    }
    add_step_menu(app, di, axis, ui);

    if let Some((_, selected)) = app
        .session
        .ui
        .proc_expanded_step
        .filter(|(dataset, _)| *dataset == owner)
        && let Some(step) = shape.pipeline.steps.iter().find(|step| step.id == selected)
    {
        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.strong(format!("{} parameters", editors::kind_label(&step.kind)));
            if matches!(step.kind, StepKind::Fft) {
                ui.small("Transforms the signal from time domain to frequency domain.");
            } else {
                editors::editor(app, di, axis, step, ui);
            }
        });
    }
    if let Some((dataset, message)) = app.session.ui.processing_surface_feedback.as_ref()
        && *dataset == owner
    {
        ui.colored_label(ui.visuals().warn_fg_color, message);
    }
}

#[allow(clippy::too_many_arguments)]
fn step_row(
    app: &mut PlotxApp,
    owner: DatasetId,
    axis: PhaseAxis,
    steps: &[ProcessingStep],
    index: usize,
    domain_before: Domain,
    ui: &mut Ui,
    op: &mut Option<(StepId, RowOp)>,
) {
    let step = &steps[index];
    let id = step.id;
    let blocks = MoveBlocks {
        up: move_block(steps, index, RowOp::MoveUp),
        down: move_block(steps, index, RowOp::MoveDown),
    };
    let expanded = app.session.ui.proc_expanded_step == Some((owner, id));
    let target = TargetRef {
        resource: ResourceRef::from(owner),
        component: Some(plotx_core::automation::ComponentRef::ProcessingStep(id)),
    };
    egui::Frame::group(ui.style()).show(ui, |ui| {
        ui.horizontal(|ui| {
            crate::ui::properties::panel::processing_step_section(app, &target, ui);
            let response = ui
                .selectable_label(
                    expanded,
                    format!(
                        "{}  {}",
                        editors::kind_icon(&step.kind),
                        editors::kind_label(&step.kind)
                    ),
                )
                .on_hover_text(editors::kind_summary(&step.kind));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.menu_button(icon::DOTS_THREE, |ui| {
                    row_menu(
                        app,
                        owner,
                        id,
                        matches!(step.kind, StepKind::Fft),
                        blocks,
                        op,
                        ui,
                    )
                });
            });
            if response.clicked() {
                app.session.ui.proc_expanded_step = if expanded { None } else { Some((owner, id)) };
                app.session.ui.phase_axis = axis;
            }
            if response.has_focus() {
                let earlier =
                    ui.input(|input| input.modifiers.alt && input.key_pressed(Key::ArrowUp));
                let later =
                    ui.input(|input| input.modifiers.alt && input.key_pressed(Key::ArrowDown));
                if earlier {
                    request_keyboard_move(app, owner, id, RowOp::MoveUp, blocks.up, op);
                } else if later {
                    request_keyboard_move(app, owner, id, RowOp::MoveDown, blocks.down, op);
                }
            }
        });
        ui.horizontal(|ui| {
            ui.add_space(26.0);
            let summary = if matches!(step.kind, StepKind::Fft) {
                "Time to Frequency".to_owned()
            } else {
                editors::kind_summary(&step.kind)
            };
            ui.small(RichText::new(summary).weak());
        });
        if !step.enabled && step.kind.input_domain() != domain_before {
            ui.horizontal(|ui| {
                ui.add_space(26.0);
                ui.colored_label(
                    ui.visuals().warn_fg_color,
                    format!(
                        "Disabled: requires {} input",
                        domain_label(step.kind.input_domain())
                    ),
                );
            });
        }
    });
}

fn request_keyboard_move(
    app: &mut PlotxApp,
    owner: DatasetId,
    step: StepId,
    operation: RowOp,
    block: Option<&'static str>,
    op: &mut Option<(StepId, RowOp)>,
) {
    if let Some(reason) = block {
        let reason = reason.to_owned();
        app.session.status = reason.clone();
        app.session.ui.processing_surface_feedback = Some((owner, reason));
    } else {
        app.session.ui.processing_surface_feedback = None;
        *op = Some((step, operation));
    }
}

fn domain_label(domain: Domain) -> &'static str {
    match domain {
        Domain::Time => "Time",
        Domain::Frequency => "Frequency",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::properties::fixture;

    #[test]
    fn raw_1d_contains_a_real_fft_step() {
        let shape = surface_shape(&fixture::time_domain_1d()).expect("processable");
        assert_eq!(shape.source, SourceShape::RawFid);
        assert!(
            shape.axes[0]
                .pipeline
                .steps
                .iter()
                .any(|step| matches!(step.kind, StepKind::Fft))
        );
    }

    #[test]
    fn imported_spectrum_has_no_fictitious_fft() {
        let shape = surface_shape(&fixture::frequency_domain_1d()).expect("processable");
        assert_eq!(shape.source, SourceShape::ImportedSpectrum);
        assert!(shape.axes.iter().all(|axis| {
            axis.pipeline
                .steps
                .iter()
                .all(|step| !matches!(step.kind, StepKind::Fft))
        }));
    }
}
