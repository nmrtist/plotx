use plotx_core::state::{NmrImportDraft, PlotxApp};

pub(crate) fn open(app: &mut PlotxApp) {
    if let Some(path) = rfd::FileDialog::new()
        .set_title("Select a Bruker ser or JEOL JDF acquisition")
        .add_filter("NMR acquisition", &["ser", "jdf"])
        .add_filter("All files", &["*"])
        .pick_file()
    {
        app.session.ui.nmr_import = Some(NmrImportDraft::new(path));
    }
}

pub(crate) fn window(app: &mut PlotxApp, ctx: &egui::Context) {
    let Some(mut draft) = app.session.ui.nmr_import.take() else {
        return;
    };
    let mut import = false;
    let mut cancel = false;
    let modal = super::super::modal(ctx, "nmr_sampling_import", super::super::ModalKind::Dialog)
        .show(ctx, |ui| {
            ui.set_width(520.0);
            ui.heading("Import NMR with sampling table");
            ui.label(draft.path.display().to_string());
            ui.label("For 2D Bruker NUS or JEOL reduced-grid acquisitions. Supply the original acquisition grid and observation order.");
            ui.separator();
            egui::Grid::new("nmr_sampling_fields").num_columns(2).show(ui, |ui| {
                ui.label("Table source / explanation");
                ui.text_edit_singleline(&mut draft.source);
                ui.end_row();
                ui.label("Original indirect grid points");
                ui.text_edit_singleline(&mut draft.grid);
                ui.end_row();
                ui.label("Lanes per observation");
                ui.text_edit_singleline(&mut draft.lanes);
                ui.end_row();
                ui.label("Index base");
                ui.horizontal(|ui| {
                    ui.radio_value(&mut draft.one_based, Some(false), "Zero-based");
                    ui.radio_value(&mut draft.one_based, Some(true), "One-based");
                });
                ui.end_row();
            });
            ui.label("Indirect index: one observation per line, including repeats");
            egui::ScrollArea::vertical().max_height(180.0).show(ui, |ui| {
                ui.add(egui::TextEdit::multiline(&mut draft.rows).desired_rows(6).desired_width(f32::INFINITY));
            });
            ui.label("Each row includes all lanes. The table must agree with the acquisition and any embedded list. Repeated observations are preserved; IST rejects repeats.");
            if let Some(error) = &draft.error {
                ui.colored_label(ui.visuals().error_fg_color, error);
            }
            ui.horizontal(|ui| {
                import = ui.button("Validate and import").clicked();
                cancel = ui.button("Cancel").clicked();
            });
        });
    if import {
        match draft.declaration() {
            Ok(declaration) => {
                if app.load_nmr_with_sampling(&draft.path, declaration) {
                    app.note_recent_file(&draft.path);
                    return;
                }
                draft.error = Some(app.session.status.clone());
            }
            Err(error) => draft.error = Some(error),
        }
    }
    if !cancel && !modal.should_close() {
        app.session.ui.nmr_import = Some(draft);
    }
}
