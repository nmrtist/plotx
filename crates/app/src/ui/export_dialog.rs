use super::*;
use plotx_core::export::{
    ComplianceStatus, ExportPreset, PrecheckReport, page_metrics, precheck_report,
};

pub(super) fn export_options_window(app: &mut PlotxApp, ctx: &egui::Context) {
    if app.session.ui.export_options.is_none() {
        return;
    }
    let page_count = app.doc.canvases.len();
    if page_count == 0 {
        app.session.ui.export_options = None;
        return;
    }

    let active_page = app.session.active_canvas.unwrap_or(0).min(page_count - 1);
    let mut export = false;
    let mut cancel = false;
    let mut settings = None;

    let modal = super::modal(ctx, "export_modal", ModalKind::Dialog).show(ctx, |ui| {
        ui.set_width(430.0);
        ui.heading("Export");
        ui.separator();
        let Some(pending) = app.session.ui.export_options.as_mut() else {
            return;
        };

        ui.horizontal(|ui| {
            ui.label("Preset");
            let current = pending.preset;
            let selected = current.map(ExportPreset::label).unwrap_or("Free-form");
            egui::ComboBox::from_id_salt("export_preset")
                .selected_text(selected)
                .width(240.0)
                .show_ui(ui, |ui| {
                    if ui
                        .selectable_label(current.is_none(), "Free-form")
                        .clicked()
                    {
                        pending.apply_preset(None);
                    }
                    for preset in ExportPreset::all() {
                        if ui
                            .selectable_label(current == Some(*preset), preset.label())
                            .clicked()
                        {
                            pending.apply_preset(Some(*preset));
                        }
                    }
                });
        });
        ui.label(format!("Format: {}", pending.format.label()));
        ui.add_space(8.0);

        let mut kind = pending.scope_kind();
        ui.radio_value(
            &mut kind,
            ExportScopeKind::Current,
            format!("Current page ({})", active_page + 1),
        );
        ui.radio_value(
            &mut kind,
            ExportScopeKind::All,
            format!("All pages ({page_count})"),
        );
        ui.radio_value(&mut kind, ExportScopeKind::Range, "Range");
        pending.set_scope_kind(kind, active_page, page_count);

        if let ExportPageScope::Range { start, end } = &mut pending.scope {
            ui.horizontal(|ui| {
                ui.label("From");
                ui.add(egui::DragValue::new(start).range(1..=page_count));
                ui.label("to");
                ui.add(egui::DragValue::new(end).range(1..=page_count));
            });
        }

        if pending.format.is_bitmap() {
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.label("DPI");
                ui.add(egui::DragValue::new(&mut pending.dpi).range(72..=1200));
            });
        }

        let preset = pending.preset;
        let scope = pending.scope;
        let dpi = pending.dpi;
        if let Some(preset) = preset {
            let report = build_report(app, preset, scope, dpi, active_page, page_count);
            ui.add_space(10.0);
            ui.separator();
            draw_precheck(ui, &report);
        }

        let Some(pending) = app.session.ui.export_options.as_ref() else {
            return;
        };
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            if ui.button("Export").clicked() {
                settings = Some(ExportSettings::from(pending));
                export = true;
            }
            if ui.button("Cancel").clicked() {
                cancel = true;
            }
        });
    });

    if export {
        app.session.ui.export_options = None;
        if let Some(settings) = settings {
            crate::ui::file_dialogs::export_with_options(app, settings);
        }
    } else if cancel || modal.should_close() {
        app.session.ui.export_options = None;
    }
}

fn build_report(
    app: &PlotxApp,
    preset: ExportPreset,
    scope: ExportPageScope,
    dpi: u16,
    active_page: usize,
    page_count: usize,
) -> PrecheckReport {
    let pages = plotx_core::export::resolve_page_scope(scope, Some(active_page), page_count)
        .unwrap_or_else(|_| vec![active_page]);
    let metrics: Vec<_> = pages
        .iter()
        .filter_map(|&page| app.doc.canvases.get(page))
        .map(page_metrics)
        .collect();
    precheck_report(
        &metrics,
        preset.target_width_mm(),
        &preset.thresholds(),
        preset.format(),
        dpi,
    )
}

fn draw_precheck(ui: &mut Ui, report: &PrecheckReport) {
    let worst = report.worst();
    ui.horizontal(|ui| {
        status_dot(ui, worst);
        ui.strong(match worst {
            ComplianceStatus::Pass => "Compliance: passes",
            ComplianceStatus::Warn => "Compliance: review",
            ComplianceStatus::Fail => "Compliance: violations (export allowed)",
        });
    });
    for item in &report.items {
        ui.horizontal(|ui| {
            status_dot(ui, item.status);
            ui.label(format!("{}: {}", item.label, item.detail));
        });
    }
}

fn status_dot(ui: &mut Ui, status: ComplianceStatus) {
    let color = match status {
        ComplianceStatus::Pass => Color32::from_rgb(0x2e, 0xa4, 0x4e),
        ComplianceStatus::Warn => Color32::from_rgb(0xbf, 0x8f, 0x00),
        ComplianceStatus::Fail => Color32::from_rgb(0xd7, 0x3a, 0x49),
    };
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(10.0), Sense::hover());
    ui.painter().circle_filled(rect.center(), 4.0, color);
}
