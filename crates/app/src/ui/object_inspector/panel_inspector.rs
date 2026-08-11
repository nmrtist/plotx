use egui::Ui;
use plotx_core::actions::{Action, PanelState};
use plotx_core::state::{PanelAlignment, PanelId, PanelLabelMode, PanelLayout, PlotxApp};

pub(super) fn render(app: &mut PlotxApp, ci: usize, id: PanelId, ui: &mut Ui) {
    let Some(before) = app.doc.canvases[ci].panel(id).cloned() else {
        ui.weak("The selected panel no longer exists. Select another layer.");
        return;
    };
    let mut after = before.clone();
    ui.label(crate::typography::headline("Panel"));
    let name = ui.add(egui::TextEdit::singleline(&mut after.name).hint_text("Panel name"));
    ui.checkbox(&mut after.visible, "Visible");
    ui.checkbox(&mut after.locked, "Locked");
    ui.checkbox(&mut after.clip_children, "Clip contents to panel");
    ui.add_enabled_ui(!before.locked, |ui| {
        ui.label(crate::typography::headline("Frame"));
        ui.horizontal(|ui| {
            ui.label("X");
            ui.add(egui::DragValue::new(&mut after.frame.x).speed(0.25));
            ui.label("Y");
            ui.add(egui::DragValue::new(&mut after.frame.y).speed(0.25));
        });
        ui.horizontal(|ui| {
            ui.label("Width");
            let width = ui.add(
                egui::DragValue::new(&mut after.frame.width)
                    .range(1.0..=10_000.0)
                    .speed(0.25),
            );
            if width.changed() {
                after.frame.height = after.frame.width * before.frame.height / before.frame.width;
            }
            ui.label(format!(
                "Height {:.1} pt (proportional)",
                after.frame.height
            ));
        });
    });
    ui.separator();
    ui.label(crate::typography::headline("Internal layout"));
    egui::ComboBox::from_label("Layout")
        .selected_text(layout_label(after.layout))
        .show_ui(ui, |ui| {
            ui.selectable_value(&mut after.layout, PanelLayout::Free, "Free");
            ui.selectable_value(
                &mut after.layout,
                PanelLayout::VerticalStack,
                "Vertical Stack",
            );
            ui.selectable_value(
                &mut after.layout,
                PanelLayout::HorizontalStack,
                "Horizontal Stack",
            );
            ui.selectable_value(
                &mut after.layout,
                PanelLayout::Grid { rows: 2, cols: 2 },
                "Grid 2 × 2",
            );
        });
    if let PanelLayout::Grid { rows, cols } = &mut after.layout {
        ui.horizontal(|ui| {
            ui.label("Rows");
            ui.add(egui::DragValue::new(rows).range(1..=64));
            ui.label("Columns");
            ui.add(egui::DragValue::new(cols).range(1..=64));
        });
    }
    ui.horizontal(|ui| {
        ui.label("Gap");
        ui.add(
            egui::DragValue::new(&mut after.layout_gap)
                .range(0.0..=144.0)
                .suffix(" pt"),
        );
        ui.label("Padding");
        ui.add(
            egui::DragValue::new(&mut after.layout_padding)
                .range(0.0..=144.0)
                .suffix(" pt"),
        );
    });
    egui::ComboBox::from_label("Alignment")
        .selected_text(alignment_label(after.layout_alignment))
        .show_ui(ui, |ui| {
            ui.selectable_value(
                &mut after.layout_alignment,
                PanelAlignment::Stretch,
                "Stretch",
            );
            ui.selectable_value(&mut after.layout_alignment, PanelAlignment::Start, "Start");
            ui.selectable_value(
                &mut after.layout_alignment,
                PanelAlignment::Center,
                "Center",
            );
            ui.selectable_value(&mut after.layout_alignment, PanelAlignment::End, "End");
        });
    ui.separator();
    ui.label(crate::typography::headline("Panel label"));
    ui.checkbox(&mut after.label.visible, "Show label");
    ui.checkbox(
        &mut after.label.participates_in_sequence,
        "Participates in sequence",
    );
    let mut mode = match after.label.mode {
        PanelLabelMode::Auto { .. } => 0,
        PanelLabelMode::LockedAuto { .. } => 1,
        PanelLabelMode::Manual { .. } => 2,
    };
    egui::ComboBox::from_label("Mode")
        .selected_text(["Auto", "Locked auto", "Manual"][mode])
        .show_ui(ui, |ui| {
            ui.selectable_value(&mut mode, 0, "Auto");
            ui.selectable_value(&mut mode, 1, "Locked auto");
            ui.selectable_value(&mut mode, 2, "Manual");
        });
    let displayed = panel_label(&app.doc.canvases[ci], &before);
    after.label.mode = match mode {
        0 => match before.label.mode {
            PanelLabelMode::Auto { slot } => PanelLabelMode::Auto { slot },
            _ => PanelLabelMode::Auto {
                slot: app.doc.canvases[ci].next_panel_label_slot,
            },
        },
        1 => PanelLabelMode::LockedAuto {
            value: displayed.clone(),
        },
        _ => {
            let mut value = match &before.label.mode {
                PanelLabelMode::Manual { value } => value.clone(),
                _ => displayed,
            };
            ui.add(egui::TextEdit::singleline(&mut value).hint_text("Label text"));
            PanelLabelMode::Manual { value }
        }
    };
    ui.add(
        egui::DragValue::new(&mut after.label.font_size)
            .range(1.0..=72.0)
            .suffix(" pt"),
    );
    ui.horizontal(|ui| {
        ui.label("Position");
        ui.add(egui::DragValue::new(&mut after.label.position[0]).speed(0.25));
        ui.add(egui::DragValue::new(&mut after.label.position[1]).speed(0.25));
    });
    ui.label("Note");
    ui.add(egui::TextEdit::multiline(&mut after.note).desired_rows(3));
    let layout_changed = after.layout != before.layout
        || after.layout_gap != before.layout_gap
        || after.layout_padding != before.layout_padding
        || after.layout_alignment != before.layout_alignment;
    if layout_changed {
        match app.set_panel_layout_action(
            ci,
            id,
            after.layout,
            after.layout_gap,
            after.layout_padding,
            after.layout_alignment,
        ) {
            Ok(action) => app.execute_action(action),
            Err(error) => app.session.status = error.to_string(),
        }
        return;
    }
    if after != before
        && after.label.validate().is_ok()
        && (!name.has_focus() || name.lost_focus())
        && !after.name.trim().is_empty()
    {
        let state_before = PanelState::of(&app.doc.canvases[ci]);
        let mut page = app.doc.canvases[ci].clone();
        if let Some(panel) = page.panel_mut(id) {
            *panel = after;
            app.execute_action(Action::ReplacePanelState {
                canvas: ci,
                before: state_before,
                after: PanelState::of(&page),
            });
        }
    }
}

fn alignment_label(alignment: PanelAlignment) -> &'static str {
    match alignment {
        PanelAlignment::Stretch => "Stretch",
        PanelAlignment::Start => "Start",
        PanelAlignment::Center => "Center",
        PanelAlignment::End => "End",
    }
}

fn layout_label(layout: PanelLayout) -> &'static str {
    match layout {
        PanelLayout::Free => "Free",
        PanelLayout::VerticalStack => "Vertical Stack",
        PanelLayout::HorizontalStack => "Horizontal Stack",
        PanelLayout::Grid { .. } => "Grid",
    }
}

fn panel_label(
    page: &plotx_core::state::CanvasDocument,
    panel: &plotx_core::state::Panel,
) -> String {
    match &panel.label.mode {
        PanelLabelMode::Auto { slot } => page.panel_label_style.format(*slot as usize),
        PanelLabelMode::LockedAuto { value } | PanelLabelMode::Manual { value } => value.clone(),
    }
}
