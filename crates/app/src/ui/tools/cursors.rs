use egui::Ui;
use egui_phosphor::regular as icon;
use plotx_core::state::{PlotxApp, Tool};

pub(super) fn cursor_group(app: &mut PlotxApp, dataset: usize, ui: &mut Ui) {
    let Some(data) = app.doc.datasets.get(dataset) else {
        return;
    };
    let true_2d = data.as_nmr2d().is_some_and(|dataset| dataset.is_true_2d());
    let frequency_1d = data
        .as_nmr()
        .is_some_and(|dataset| dataset.output_domain() == plotx_io::Domain::Frequency);
    if !frequency_1d && !true_2d {
        return;
    }
    let symmetry = data
        .as_nmr2d()
        .is_some_and(|dataset| dataset.supports_symmetry_review());
    let count = if symmetry { 3 } else { 2 };

    ui.separator();
    ui.strong("Cursors");
    ui.small(format!(
        "Press C to cycle through the {count} cursor modes; Esc exits."
    ));
    cursor_button(
        app,
        ui,
        Tool::InspectCursor,
        format!("{}  Inspect", icon::CROSSHAIR),
        "Read the exact plot coordinates and sampled intensity; click to pin.",
    );
    cursor_button(
        app,
        ui,
        Tool::DeltaCursor,
        format!("{}  Delta", icon::ARROWS_OUT_LINE_HORIZONTAL),
        "Click two positions to measure coordinate, frequency, and intensity differences.",
    );
    if symmetry {
        cursor_button(
            app,
            ui,
            Tool::Symmetry,
            format!("{}  Symmetry", icon::CROSSHAIR),
            "Compare a cross peak with its reflected position across the diagonal.",
        );
    }

    match app.session.tool {
        Tool::InspectCursor => {
            ui.small("Move to read · click to pin · click elsewhere to move the pin.");
            if app.session.ui.inspect_cursor_pin.is_some() && ui.small_button("Clear pin").clicked()
            {
                app.session.ui.inspect_cursor_pin = None;
            }
        }
        Tool::DeltaCursor => {
            ui.small("Click A · move to preview · click B to pin the measurement.");
            if (app.session.ui.delta_cursor_anchor.is_some()
                || app.session.ui.delta_cursor_pin.is_some())
                && ui.small_button("Clear measurement").clicked()
            {
                app.session.ui.delta_cursor_anchor = None;
                app.session.ui.delta_cursor_pin = None;
            }
        }
        _ => {}
    }
}

fn cursor_button(app: &mut PlotxApp, ui: &mut Ui, tool: Tool, label: String, help: &str) {
    let active = app.session.tool == tool;
    if ui
        .selectable_label(active, label)
        .on_hover_text(help)
        .clicked()
    {
        app.toggle_tool(tool);
    }
}
