use super::*;
use plotx_core::export::{
    ComplianceStatus, ExportPreset, PrecheckReport, image_precheck_items, page_metrics,
    precheck_report,
};
use plotx_core::settings::{MAX_EXPORT_DPI, MIN_EXPORT_DPI};

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
                ui.add(
                    egui::DragValue::new(&mut pending.dpi)
                        .range(MIN_EXPORT_DPI..=MAX_EXPORT_DPI),
                );
            });
        }

        ui.add_space(8.0);
        ui.checkbox(
            &mut pending.trim_to_visible_content,
            "Trim page to visible content",
        )
        .on_hover_text(
            "Removes page whitespace around visible content without enlarging the content.\n\
             With journal/column presets, the final physical page width may be smaller than the preset.\n\
             Empty pages keep their original size.",
        );

        let has_images = selected_pages_have_images(
            &app.doc.canvases,
            pending.scope,
            active_page,
            page_count,
        );
        if has_images {
            ui.add_space(8.0);
            ui.checkbox(
                &mut pending.allow_missing_images,
                "Export with missing-image placeholders",
            )
            .on_hover_text(
                "If an embedded image cannot be read, export a labelled placeholder instead of stopping.",
            );
        } else {
            pending.allow_missing_images = false;
        }

        let preset = pending.preset;
        let scope = pending.scope;
        let dpi = pending.dpi;
        let report = build_report(app, preset, scope, dpi, active_page, page_count);
        if !report.items.is_empty() {
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
            let trim = settings.trim_to_visible_content;
            if let Some(path) = crate::ui::file_dialogs::choose_export_path(&settings) {
                app.export_to(settings, &path);
                set_confirmed_trim_default(app, trim);
            }
        }
    } else if cancel || modal.should_close() {
        app.session.ui.export_options = None;
    }
}

fn selected_pages_have_images(
    canvases: &[plotx_core::state::CanvasDocument],
    scope: ExportPageScope,
    active_page: usize,
    page_count: usize,
) -> bool {
    plotx_core::export::resolve_page_scope(scope, Some(active_page), page_count)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|page| canvases.get(page))
        .flat_map(|canvas| &canvas.objects)
        .any(|item| {
            matches!(
                &item.kind,
                plotx_core::state::CanvasObjectKind::RasterImage(_)
            )
        })
}

fn set_confirmed_trim_default(app: &mut PlotxApp, trim_to_visible_content: bool) {
    let target = app.app_target();
    match app.plan_property_write(
        plotx_core::properties::app_preferences::TRIM_TO_VISIBLE_CONTENT,
        std::slice::from_ref(&target),
        &plotx_core::properties::PropertyValue::Bool(trim_to_visible_content),
    ) {
        Ok(commit) => {
            app.commit_property(commit);
        }
        Err(error) => {
            app.session.status = format!("Could not save the export trim preference: {error}");
        }
    }
}

fn build_report(
    app: &PlotxApp,
    preset: Option<ExportPreset>,
    scope: ExportPageScope,
    dpi: u16,
    active_page: usize,
    page_count: usize,
) -> PrecheckReport {
    let pages = plotx_core::export::resolve_page_scope(scope, Some(active_page), page_count)
        .unwrap_or_else(|_| vec![active_page]);
    let mut report = if let Some(preset) = preset {
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
    } else {
        PrecheckReport { items: Vec::new() }
    };
    let canvases: Vec<_> = pages
        .iter()
        .filter_map(|&page| app.doc.canvases.get(page))
        .collect();
    report.items.extend(image_precheck_items(
        &canvases,
        &app.doc.assets,
        preset.and_then(ExportPreset::target_width_mm),
    ));
    report
}

fn draw_precheck(ui: &mut Ui, report: &PrecheckReport) {
    let worst = report.worst();
    ui.horizontal(|ui| {
        status_dot(ui, worst);
        ui.label(crate::typography::headline(match worst {
            ComplianceStatus::Pass => "Compliance: passes",
            ComplianceStatus::Warn => "Compliance: review",
            ComplianceStatus::Fail => "Compliance: violations (export allowed)",
        }));
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

#[cfg(test)]
mod image_tests {
    use super::*;
    use plotx_core::state::{
        AssetId, CanvasObject, CanvasObjectKind, ObjectFrame, ObjectId, RasterImageContent,
    };

    #[test]
    fn placeholder_option_is_available_for_every_scope_containing_an_image() {
        let plain = plotx_core::state::CanvasDocument::new("plain".to_owned(), [100.0, 80.0]);
        let mut image_page =
            plotx_core::state::CanvasDocument::new("image".to_owned(), [100.0, 80.0]);
        image_page.objects.push(CanvasObject {
            id: ObjectId::new(1),
            name: "image".to_owned(),
            frame: ObjectFrame::new(0.0, 0.0, 20.0, 20.0),
            locked: false,
            visible: true,
            kind: CanvasObjectKind::RasterImage(RasterImageContent::new(AssetId::new())),
        });
        let pages = [plain, image_page];

        assert!(!selected_pages_have_images(
            &pages,
            ExportPageScope::Current,
            0,
            pages.len(),
        ));
        assert!(selected_pages_have_images(
            &pages,
            ExportPageScope::Current,
            1,
            pages.len(),
        ));
        assert!(selected_pages_have_images(
            &pages,
            ExportPageScope::All,
            0,
            pages.len(),
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn confirmed_export_updates_trim_through_the_catalog_and_never_dpi() {
        let mut settings = plotx_core::settings::Settings::default();
        settings.export.dpi = 600;
        let mut app = PlotxApp::new_with_settings(settings);

        set_confirmed_trim_default(&mut app, true);
        assert!(app.settings.export.trim_to_visible_content);
        assert_eq!(app.settings.export.dpi, 600);
        let resolved = app
            .resolve_property(&plotx_core::properties::PropertyAddress::new(
                app.app_target(),
                plotx_core::properties::app_preferences::TRIM_TO_VISIBLE_CONTENT,
            ))
            .expect("the catalog reads the confirmed default");
        assert_eq!(
            resolved.value.uniform(),
            Some(&plotx_core::properties::PropertyValue::Bool(true))
        );
    }
}
