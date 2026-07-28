//! The right dock (Secondary Side Bar): the Object inspector on top and
//! contextual analysis tools below. Processing has one canonical editor in the
//! shared canvas task dock.

use crate::ui::{object_inspector, tools};
use egui::Ui;
use plotx_core::state::{Dataset, PlotxApp, ToolGroup};

/// One scroll viewport for the whole dock.
///
/// The inspector and the dataset tools used to be siblings, and only the lower
/// half scrolled: the tools took whatever height the inspector left over. An
/// inspector taller than the panel therefore pushed the tools — and with them
/// the entire Processing group — past the clip rect, where they could not be
/// scrolled to at all, so the most basic manual processing became unreachable
/// exactly when the selection was richest. Nothing the dock draws may be a
/// sibling of this viewport; a section's length must never decide whether
/// another section can be reached.
pub fn render(app: &mut PlotxApp, ui: &mut Ui) {
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| body(app, ui));
}

fn body(app: &mut PlotxApp, ui: &mut Ui) {
    object_inspector::render(app, ui);

    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.strong("Dataset tools");
        if let Some(di) = app.active_dataset() {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.weak(app.doc.datasets[di].kind_label());
            });
        }
    });
    ui.add_space(6.0);
    ui.separator();

    let Some(di) = app.active_dataset() else {
        ui.add_space(10.0);
        ui.weak("Select a dataset in the Primary Side Bar to see its tools.");
        return;
    };
    if di >= app.doc.datasets.len() {
        app.clear_selection();
        return;
    }

    let groups = visible_tool_groups(&app.doc.datasets[di]);
    if groups.is_empty() {
        ui.weak("This dataset has no additional inspector tools.");
        return;
    }
    let mut dirty = false;
    for (position, group) in groups.into_iter().enumerate() {
        ui.add_space(2.0);
        let id = ui.make_persistent_id(("secondary_tool_group", group.title()));
        if app.session.ui.requested_tool_group == Some(group) {
            let mut state = egui::collapsing_header::CollapsingState::load_with_default_open(
                ui.ctx(),
                id,
                true,
            );
            state.set_open(true);
            state.store(ui.ctx());
            app.session.ui.requested_tool_group = None;
        }
        egui::CollapsingHeader::new(group.title())
            .id_salt(("secondary_tool_group", group.title()))
            .default_open(position == 0)
            .show(ui, |ui| {
                dirty |= tools::render_group(app, di, group, ui);
            });
    }

    if dirty {
        app.apply_dataset_edit(di);
    }
}

fn visible_tool_groups(dataset: &Dataset) -> Vec<ToolGroup> {
    dataset
        .tool_groups()
        .iter()
        .copied()
        .filter(|group| *group != ToolGroup::Processing)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn processing_has_no_second_editor_in_the_secondary_sidebar() {
        let dataset = crate::ui::properties::fixture::time_domain_2d();
        let groups = visible_tool_groups(&dataset);
        assert!(!groups.contains(&ToolGroup::Processing));
        assert!(
            !groups.is_empty(),
            "the filter removes only Processing, not the remaining analysis tools"
        );
    }
}
