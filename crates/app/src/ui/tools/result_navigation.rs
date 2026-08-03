//! Accessible navigation for a live region result. The backing table is not a
//! board frame, so the result card owns the explicit routes among fitting, raw
//! values, and the source regions.

use egui::Ui;
use plotx_core::state::{FrameRef, PlotxApp, Tool, page_frame_showing_dataset};

fn source_dataset(app: &PlotxApp, table: usize) -> Option<usize> {
    let source = app
        .doc
        .datasets
        .get(table)?
        .as_table()?
        .provenance
        .as_ref()?
        .source_resource
        .as_str();
    app.doc
        .datasets
        .iter()
        .position(|dataset| dataset.resource_id().to_string() == source)
}

pub(super) fn show(app: &mut PlotxApp, table: usize, ui: &mut Ui) {
    let Some(source) = source_dataset(app, table) else {
        return;
    };
    let mut fit = false;
    let mut view_data = false;
    let mut back = false;
    ui.horizontal_wrapped(|ui| {
        fit = ui.button("Fit curves").clicked();
        view_data = ui.button("View data").clicked();
        back = ui.button("Back to regions").clicked();
    });
    ui.separator();

    if fit {
        app.focus_single(table);
        if let Some(frame) = page_frame_showing_dataset(app, table) {
            app.reveal_board_frame(frame);
        }
        app.session.status = "Ready to fit the extracted curves.".to_owned();
    }
    if view_data {
        app.focus_single(table);
        app.session.ui.sheet_open = Some(table);
        app.session.ui.curve_fit_task_collapsed = true;
        app.session.status = "Opened the synchronized region data (read-only).".to_owned();
    }
    if back {
        app.focus_single(source);
        if let Some(FrameRef::Page(page)) = page_frame_showing_dataset(app, source) {
            app.reveal_board_frame(FrameRef::Page(page));
        }
        app.set_tool(Tool::Regions);
        super::region_analysis::open_task(app, source);
        app.session.status = "Returned to the source regions.".to_owned();
    }
}
