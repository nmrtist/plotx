//! Renders a single `ToolGroup` for the active dataset; new data domains plug
//! in here without touching the sidebar or the toolbar.

mod curve_fit;
mod electrophysiology;
mod line_fit;
mod processing;
mod pseudo;
mod region_analysis;
mod slice;
mod statistics;
mod statistics_config;
mod task_card;

use curve_fit::curve_fit_group;
use egui::{Button, DragValue, Response, Ui};
use egui_phosphor::regular as icon;
use line_fit::line_fit_group;
use plotx_core::actions::{DatasetProcessingState, PendingProcessingEdit};
use plotx_core::state::{Dataset, PlotxApp, Tool, ToolGroup};
use pseudo::experiment_group;
use region_analysis::region_analysis_group;
use slice::slice_group;

pub(super) use line_fit::line_fit_shape_id;

pub(crate) fn render_region_task(app: &mut PlotxApp, ui: &mut Ui) {
    region_analysis::render_task(app, ui);
}

pub(crate) fn render_curve_fit_task(app: &mut PlotxApp, ui: &mut Ui) {
    curve_fit::render_task(app, ui);
}

pub(crate) fn render_statistics_task(app: &mut PlotxApp, ui: &mut Ui) {
    statistics::render_task(app, ui);
}

pub(crate) fn open_statistics_task(app: &mut PlotxApp, dataset: usize) {
    statistics::open_task(app, dataset);
}

pub(crate) fn open_region_table(app: &mut PlotxApp, dataset: usize) {
    region_analysis::open_region_table(app, dataset);
}

pub(crate) fn run_curve_fit(app: &mut PlotxApp, dataset: usize) {
    curve_fit::run_curve_fit(app, dataset);
}

pub(crate) fn open_region_task(app: &mut PlotxApp, dataset: usize) {
    region_analysis::open_task(app, dataset);
}

pub(crate) fn open_curve_fit_task(app: &mut PlotxApp, dataset: usize) {
    curve_fit::open_task(app, dataset);
}

/// Returns `true` when the edit dirties the dataset and a rebuild is needed.
pub fn render_group(app: &mut PlotxApp, di: usize, group: ToolGroup, ui: &mut Ui) -> bool {
    match group {
        ToolGroup::Processing => processing::processing_group(app, di, ui),
        ToolGroup::Nmr1dAnalysis => analysis_group(app, di, ui),
        ToolGroup::Nmr2dExperiment => experiment_group(app, di, ui),
        ToolGroup::RegionAnalysis => region_analysis_group(app, di, ui),
        ToolGroup::Peaks => peaks_group(app, di, ui),
        ToolGroup::CurveFit => curve_fit_group(app, di, ui),
        ToolGroup::LineFit => line_fit_group(app, di, ui),
        ToolGroup::Statistics => statistics::statistics_group(app, di, ui),
        ToolGroup::Electrophysiology => electrophysiology::electrophysiology_group(app, di, ui),
    }
}

fn analysis_group(app: &mut PlotxApp, di: usize, ui: &mut Ui) -> bool {
    if !matches!(app.doc.datasets.get(di), Some(Dataset::Nmr(_))) {
        ui.small("2D analysis is not available in this phase.");
        return false;
    }

    let has_selection = app
        .session
        .ui
        .analysis_selection
        .as_ref()
        .map(|selection| selection.dataset == di)
        .unwrap_or(false);
    ui.horizontal(|ui| {
        let selected = app.session.tool == Tool::SelectRegion;
        if ui.selectable_label(selected, "Analysis range").clicked() {
            app.set_tool(Tool::SelectRegion);
        }
        if ui
            .add_enabled(has_selection, Button::new("Clear"))
            .on_disabled_hover_text("No active analysis selection")
            .clicked()
        {
            app.clear_analysis_selection();
        }
    });

    let range = app.analysis_range_for(di);
    if let Some(range) = range {
        ui.label(format!("Range: {:.3}-{:.3} ppm", range.min, range.max));
    } else {
        ui.weak("No active plot range.");
    }

    integrate_group(app, di, ui);

    ui.separator();
    ui.strong("Arithmetic");
    if ui
        .button("Spectrum arithmetic…")
        .on_hover_text("Add or subtract spectra (A ± k·B) or apply a constant; solvent subtraction and difference spectra.")
        .clicked()
    {
        crate::ui::arithmetic::open_spectrum_arithmetic_dialog(app);
    }
    if ui
        .add_enabled(app.can_align_spectra(), Button::new("Align spectra…"))
        .on_hover_text(
            "Shift the selected spectra so a shared reference peak lands on one position.",
        )
        .on_disabled_hover_text("Needs at least two 1D spectra.")
        .clicked()
    {
        crate::ui::align::open_align_spectra_dialog(app);
    }

    false
}

fn peaks_group(app: &mut PlotxApp, di: usize, ui: &mut Ui) -> bool {
    let column = app.session.ui.peak_column;
    if app
        .doc
        .datasets
        .get(di)
        .and_then(|d| d.displayed_trace(column))
        .is_none()
    {
        ui.small("Peaks are available for 1D traces (spectra or a table column).");
        return false;
    }
    let Some(peaks) = app.doc.datasets.get(di).and_then(Dataset::peaks).cloned() else {
        return false;
    };

    let active = app.session.tool == Tool::Peaks;
    if ui
        .selectable_label(active, format!("{}  Peaks", icon::MAP_PIN))
        .on_hover_text("Pick peaks by region, click, or one-shot detection — one set.")
        .clicked()
    {
        app.set_tool(if active {
            Tool::BrowseZoom
        } else {
            Tool::Peaks
        });
    }
    if active {
        ui.small(
            "Drag across a region to pick every peak in it · click a maximum to add one · drag \
             the line then release to detect at that level · click a marker then Delete to remove.",
        );
    }

    ui.horizontal(|ui| {
        if ui
            .button("Detect peaks")
            .on_hover_text("Find peaks above the current threshold across the whole trace.")
            .clicked()
        {
            app.run_detection(di, peaks.detector.threshold, column);
        }
        let label = match peaks.detector.threshold {
            Some(y) => format!("at {y:.3}"),
            None => "at auto".to_owned(),
        };
        ui.label(label);
        if ui
            .add_enabled(peaks.detector.threshold.is_some(), Button::new("Auto"))
            .on_disabled_hover_text("Already at the noise floor")
            .clicked()
        {
            app.run_detection(di, None, column);
        }
    });
    let mut capped = peaks.detector.max_count.is_some();
    ui.horizontal(|ui| {
        if ui.checkbox(&mut capped, "Limit count").changed() {
            app.set_peak_max_count(di, capped.then_some(20), column);
        }
        if let Some(max) = peaks.detector.max_count {
            let mut n = max;
            if ui
                .add(DragValue::new(&mut n).speed(1.0).range(1..=999))
                .changed()
            {
                app.set_peak_max_count(di, Some(n), column);
            }
        }
    });

    let resolved = peaks.resolve();
    ui.horizontal(|ui| {
        ui.label(format!("Peaks: {}", resolved.len()));
        if ui
            .add_enabled(!resolved.is_empty(), Button::new("Clear"))
            .on_disabled_hover_text("No peaks to clear")
            .clicked()
        {
            app.clear_peaks(di);
        }
    });

    let selected = app.session.ui.selected_peak;
    let mut select: Option<u64> = None;
    let mut delete: Option<u64> = None;
    egui::ScrollArea::vertical()
        .max_height(220.0)
        .show(ui, |ui| {
            for peak in &resolved {
                let Some(id) = peak.mark_id else { continue };
                ui.horizontal(|ui| {
                    let mark = match peak.origin {
                        plotx_core::state::PeakOrigin::Manual => icon::MAP_PIN,
                        plotx_core::state::PeakOrigin::Detected => icon::CIRCLE,
                    };
                    if ui
                        .selectable_label(selected == Some(id), format!("{mark}  {}", peak.label))
                        .clicked()
                    {
                        select = Some(id);
                    }
                    ui.label(format!("{:.3}", peak.y));
                    if ui.small_button(icon::X).clicked() {
                        delete = Some(id);
                    }
                });
            }
        });
    if let Some(id) = select {
        app.session.ui.selected_peak = Some(id);
    }
    if let Some(id) = delete {
        app.remove_peak(di, id);
    }

    false
}

pub(super) fn integrate_group(app: &mut PlotxApp, di: usize, ui: &mut Ui) {
    if app
        .doc
        .datasets
        .get(di)
        .and_then(Dataset::as_nmr2d)
        .is_some()
    {
        integrate_2d_group(app, di, ui);
        return;
    }
    ui.separator();
    ui.strong("Integrate");
    let drawing = app.session.tool == Tool::Integrate;
    if ui
        .selectable_label(drawing, "∫  Draw integrals")
        .on_hover_text("Drag across a multiplet to add an integral; drag its edges to adjust it.")
        .clicked()
    {
        app.set_tool(if drawing {
            Tool::BrowseZoom
        } else {
            Tool::Integrate
        });
    }
    if drawing {
        ui.small(
            "Drag across a peak to add · drag edges to resize · drag middle to move · \
             right-click to set the reference or delete.",
        );
    }

    let selected = app.session.ui.selected_integral;
    let mut set_ref: Option<u64> = None;
    let mut delete_id: Option<u64> = None;
    let mut select_id: Option<u64> = None;

    let integrals = app
        .doc
        .datasets
        .get(di)
        .and_then(Dataset::as_nmr)
        .map(|n| n.integrals.clone())
        .unwrap_or_default();
    if integrals.is_empty() {
        ui.weak("No integrals yet — turn on Draw integrals and drag across a peak.");
    }
    for integ in &integrals {
        ui.horizontal(|ui| {
            let is_sel = selected == Some(integ.id);
            if ui
                .selectable_label(
                    is_sel,
                    format!("{:.2}–{:.2} ppm", integ.start_ppm, integ.end_ppm),
                )
                .clicked()
            {
                select_id = Some(integ.id);
            }
            ui.label(format!("{:.3}", integ.normalized_area));
            if ui
                .selectable_label(integ.is_reference, "ref")
                .on_hover_text("Use this integral as the =1 reference")
                .clicked()
            {
                set_ref = Some(integ.id);
            }
            if ui.small_button(icon::X).clicked() {
                delete_id = Some(integ.id);
            }
        });
    }

    if let Some(id) = select_id {
        app.session.ui.selected_integral = Some(id);
    }
    if let Some(id) = set_ref {
        app.set_integral_reference(di, id);
    }
    if let Some(id) = delete_id {
        app.delete_integral(di, id);
    }
}

fn integrate_2d_group(app: &mut PlotxApp, di: usize, ui: &mut Ui) {
    use plotx_core::BaselineMode;

    ui.separator();
    ui.strong("2D Integrals");
    let drawing = app.session.tool == Tool::Integrate;
    if ui
        .selectable_label(drawing, "∫  Draw rectangles")
        .on_hover_text(
            "Drag a rectangle around a cross-peak; use its edges, corners, or interior to edit it.",
        )
        .clicked()
    {
        app.set_tool(if drawing {
            Tool::BrowseZoom
        } else {
            Tool::Integrate
        });
    }
    if drawing {
        ui.small("Drag to add · corners/edges resize · interior moves · right-click for reference or delete.");
    }

    let integrals = app.doc.datasets[di].as_nmr2d().unwrap().integrals.clone();
    if let Some(error) = app.doc.datasets[di]
        .as_nmr2d()
        .and_then(|dataset| dataset.integral_error.as_deref())
    {
        ui.colored_label(
            ui.visuals().error_fg_color,
            format!("Volume error: {error}"),
        );
    }
    if integrals.is_empty() {
        ui.weak("No 2D integrals yet — draw a rectangle around a peak.");
    }
    if integrals
        .iter()
        .any(|integral| integral.normalized_volume.is_none())
    {
        ui.colored_label(
            ui.visuals().warn_fg_color,
            "The reference volume is too close to zero; normalized values are unavailable.",
        );
    }

    for integral in integrals {
        let selected = app.session.ui.selected_integral == Some(integral.id);
        ui.group(|ui| {
            ui.horizontal(|ui| {
                if ui
                    .selectable_label(selected, "S")
                    .on_hover_text("Summed rectangular volume")
                    .clicked()
                {
                    app.session.ui.selected_integral = Some(integral.id);
                }
                let mut name = integral.name.clone();
                if ui
                    .add(egui::TextEdit::singleline(&mut name).desired_width(100.0))
                    .changed()
                {
                    app.edit_integrals_2d(di, |values, _| {
                        if let Some(value) = values.iter_mut().find(|value| value.id == integral.id)
                        {
                            value.name = name;
                        }
                    });
                }
                let normalized = integral
                    .normalized_volume
                    .map_or_else(|| "—".to_owned(), |value| format!("{value:.3}"));
                ui.label(normalized);
                if ui.selectable_label(integral.is_reference, "ref").clicked() {
                    app.set_integral_2d_reference(di, integral.id);
                }
                if ui.small_button(icon::X).clicked() {
                    app.delete_integral_2d(di, integral.id);
                    if selected {
                        app.session.ui.selected_integral = None;
                    }
                }
            });
            ui.small(format!(
                "F2 {:.3}–{:.3} · F1 {:.3}–{:.3} ppm · raw {:.8} · {}",
                integral.f2.0,
                integral.f2.1,
                integral.f1.0,
                integral.f1.1,
                integral.volume,
                integral.mode.as_str(),
            ));
            ui.horizontal(|ui| {
                ui.label("Baseline");
                let mut baseline = integral.baseline;
                egui::ComboBox::from_id_salt(("integral_2d_baseline", integral.id))
                    .selected_text(match baseline {
                        BaselineMode::None => "None",
                        BaselineMode::Constant => "Constant",
                        BaselineMode::Plane => "Plane",
                    })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut baseline, BaselineMode::None, "None");
                        ui.selectable_value(&mut baseline, BaselineMode::Constant, "Constant");
                        ui.selectable_value(&mut baseline, BaselineMode::Plane, "Plane");
                    });
                if baseline != integral.baseline {
                    app.edit_integrals_2d(di, |values, _| {
                        if let Some(value) = values.iter_mut().find(|value| value.id == integral.id)
                        {
                            value.baseline = baseline;
                        }
                    });
                }
                if integral.is_reference {
                    ui.label("Reference value");
                    let mut reference_value = integral.reference_value;
                    if ui
                        .add(DragValue::new(&mut reference_value).speed(0.1))
                        .changed()
                    {
                        app.edit_integrals_2d(di, |values, _| {
                            if let Some(value) =
                                values.iter_mut().find(|value| value.id == integral.id)
                            {
                                value.reference_value = reference_value;
                            }
                        });
                    }
                }
            });
        });
    }
}

pub(super) fn begin_processing_widget(
    app: &mut PlotxApp,
    di: usize,
    resp: &Response,
    before: DatasetProcessingState,
) {
    if resp.drag_started() {
        app.session.ui.processing_edit = Some(PendingProcessingEdit {
            dataset: di,
            before,
        });
    }
}

/// Commit a DragValue interaction as one undo step, routed through the pause
/// gate so a paused edit defers its recompute. A drag coalesces via the
/// pending edit's `before`; a plain click commits with `fallback_before`.
pub(super) fn commit_processing_widget(
    app: &mut PlotxApp,
    di: usize,
    resp: &Response,
    fallback_before: DatasetProcessingState,
) {
    if resp.drag_stopped() {
        let before = app
            .session
            .ui
            .processing_edit
            .take()
            .filter(|edit| edit.dataset == di)
            .map(|edit| edit.before)
            .unwrap_or(fallback_before);
        let after = DatasetProcessingState::from_dataset(&app.doc.datasets[di]);
        app.commit_processing_edit(di, before, after);
    } else if resp.changed() && !resp.dragged() {
        let after = DatasetProcessingState::from_dataset(&app.doc.datasets[di]);
        app.commit_processing_edit(di, fallback_before, after);
    }
}
