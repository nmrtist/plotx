use plotx_core::data_export::{
    DataExportAvailability, DataExportContent, DataExportSnapshot, IntensityChannel, TableShape,
};
use plotx_core::state::PlotxApp;
use plotx_io::delimited::Delimiter;

use super::ModalKind;

#[derive(Clone, Copy)]
enum Action {
    Save(Delimiter),
    SaveXlsx,
    Copy,
}

pub(super) fn data_export_window(app: &mut PlotxApp, ctx: &egui::Context) {
    let Some(dialog) = app.session.ui.data_export else {
        return;
    };
    let Some(dataset) = app.doc.datasets.get(dialog.dataset) else {
        app.session.ui.data_export = None;
        return;
    };
    let availability = DataExportAvailability::for_dataset(dataset);
    if availability.is_empty() {
        app.session.ui.data_export = None;
        return;
    }
    let dataset_name = dataset.display_name();
    let busy = app.data_export_busy();
    let mut action = None;
    let mut close = false;
    let modal = super::modal(ctx, "data_export_modal", ModalKind::Dialog).show(ctx, |ui| {
        ui.set_width(440.0);
        ui.heading("Export Data");
        ui.label(dataset_name);
        ui.separator();

        let Some(state) = app.session.ui.data_export.as_mut() else {
            return;
        };
        if !availability.contents.contains(&state.request.content) {
            state.request.content = availability.contents[0];
        }
        ui.horizontal(|ui| {
            ui.label("Content");
            egui::ComboBox::from_id_salt("data_export_content")
                .selected_text(state.request.content.label())
                .width(260.0)
                .show_ui(ui, |ui| {
                    for content in &availability.contents {
                        ui.selectable_value(&mut state.request.content, *content, content.label());
                    }
                });
        });

        if state.request.content == DataExportContent::ProcessedData
            && availability.has_channel_choice
        {
            ui.horizontal(|ui| {
                ui.label("Intensity");
                for channel in IntensityChannel::ALL {
                    ui.radio_value(&mut state.request.channel, channel, channel.label());
                }
            });
        }
        if state.request.content == DataExportContent::ProcessedData
            && availability.has_shape_choice
        {
            ui.horizontal(|ui| {
                ui.label("2D layout");
                ui.radio_value(&mut state.request.shape, TableShape::Matrix, "Matrix");
                ui.radio_value(&mut state.request.shape, TableShape::Long, "Long");
            });
        }

        ui.add_space(10.0);
        if busy {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Export is running in the background…");
            });
        }
        ui.horizontal(|ui| {
            if ui
                .add_enabled(!busy, egui::Button::new("Save CSV…"))
                .clicked()
            {
                action = Some(Action::Save(Delimiter::Comma));
            }
            if ui
                .add_enabled(!busy, egui::Button::new("Save TSV…"))
                .clicked()
            {
                action = Some(Action::Save(Delimiter::Tab));
            }
            if ui
                .add_enabled(!busy, egui::Button::new("Save XLSX…"))
                .clicked()
            {
                action = Some(Action::SaveXlsx);
            }
            if ui
                .add_enabled(!busy, egui::Button::new("Copy TSV"))
                .clicked()
            {
                action = Some(Action::Copy);
            }
            if ui.button("Close").clicked() {
                close = true;
            }
        });
        ui.small("XLSX stores deterministic values and a hidden PlotX schema; formulas are not generated.");
    });

    if close || modal.should_close() {
        app.session.ui.data_export = None;
        return;
    }
    match action {
        Some(Action::Copy) => app.start_data_export_clipboard(),
        Some(Action::Save(delimiter)) => save_file(app, delimiter),
        Some(Action::SaveXlsx) => save_xlsx(app),
        None => {}
    }
}

fn save_xlsx(app: &mut PlotxApp) {
    let Some(dialog) = app.session.ui.data_export else {
        return;
    };
    let Some(dataset) = app.doc.datasets.get(dialog.dataset) else {
        return;
    };
    let snapshot = match DataExportSnapshot::capture(dataset, dialog.request) {
        Ok(snapshot) => snapshot,
        Err(error) => {
            app.report_data_export_unavailable(error);
            return;
        }
    };
    let Some(mut path) = rfd::FileDialog::new()
        .add_filter("Excel workbook", &["xlsx"])
        .set_file_name(snapshot.default_file_name("xlsx"))
        .set_title("Save XLSX data")
        .save_file()
    else {
        return;
    };
    if !path
        .extension()
        .is_some_and(|value| value.eq_ignore_ascii_case("xlsx"))
    {
        path.set_extension("xlsx");
    }
    app.start_data_export_xlsx_file(snapshot, path);
}

fn save_file(app: &mut PlotxApp, delimiter: Delimiter) {
    let Some(dialog) = app.session.ui.data_export else {
        return;
    };
    let Some(dataset) = app.doc.datasets.get(dialog.dataset) else {
        return;
    };
    let snapshot = match DataExportSnapshot::capture(dataset, dialog.request) {
        Ok(snapshot) => snapshot,
        Err(error) => {
            app.report_data_export_unavailable(error);
            return;
        }
    };
    let (label, extension) = format_details(delimiter);
    let Some(mut path) = rfd::FileDialog::new()
        .add_filter(label, &[extension])
        .set_file_name(snapshot.default_file_name(extension))
        .set_title(format!("Save {label} data"))
        .save_file()
    else {
        return;
    };
    if !path
        .extension()
        .is_some_and(|value| value.eq_ignore_ascii_case(extension))
    {
        path.set_extension(extension);
    }
    app.start_data_export_file(snapshot, path, delimiter);
}

fn format_details(delimiter: Delimiter) -> (&'static str, &'static str) {
    match delimiter {
        Delimiter::Comma => ("CSV", "csv"),
        Delimiter::Tab => ("TSV", "tsv"),
        // The dialog only offers comma and tab; semicolon-delimited files
        // conventionally use the .csv extension.
        Delimiter::Semicolon => ("CSV", "csv"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_actions_map_extensions_to_their_delimiters() {
        assert_eq!(format_details(Delimiter::Comma), ("CSV", "csv"));
        assert_eq!(format_details(Delimiter::Tab), ("TSV", "tsv"));
    }
}
