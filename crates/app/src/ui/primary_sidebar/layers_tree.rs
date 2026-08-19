use egui::Ui;
use egui_phosphor::regular as icon;
use plotx_core::actions::{Action, PanelState};
use plotx_core::state::{ContentId, Panel, PanelId, PanelLabelMode, PlotxApp, SelectionPath};

pub(super) fn render_panels(app: &mut PlotxApp, ci: usize, ui: &mut Ui) {
    let panels: Vec<_> = app.doc.canvases[ci]
        .panels
        .iter()
        .map(|panel| panel.id)
        .rev()
        .collect();
    for panel in panels {
        render_panel(app, ci, panel, ui);
    }
    if app.session.ui.layers_drag_content.is_some()
        && ui.input(|input| input.pointer.primary_released())
    {
        app.session.ui.layers_drag_content = None;
        app.session.ui.panel_drop_target = None;
    }
}

fn render_panel(app: &mut PlotxApp, ci: usize, panel_id: PanelId, ui: &mut Ui) {
    let Some(panel) = app.doc.canvases[ci].panel(panel_id).cloned() else {
        return;
    };
    let collapse_id = egui::Id::new(("figure-layer-panel-open", panel_id));
    let mut open = ui.data(|data| data.get_temp::<bool>(collapse_id).unwrap_or(true));
    let selected_path = SelectionPath::panel(app.doc.canvases[ci].resource_id, panel_id);
    let selected = app
        .session
        .ui
        .hierarchical_selection
        .contains(selected_path);
    let mut select_panel = false;
    let mut enter_panel = false;
    let mut flags = None;
    let mut drop_content = None;
    let display_name = panel_tree_name(app, ci, &panel);
    let (visible_flags, locked_flags) = super::layer_controls::row(
        ui,
        |ui| {
            if ui
                .small_button(if open {
                    icon::CARET_DOWN
                } else {
                    icon::CARET_RIGHT
                })
                .clicked()
            {
                open = !open;
                ui.data_mut(|data| data.insert_temp(collapse_id, open));
            }
            let mut visible = panel.visible;
            if super::layer_controls::visibility_button(ui, &mut visible).changed() {
                flags = Some((visible, panel.locked));
            }
            ui.weak(icon::RECTANGLE).on_hover_text("Panel");
            let response = super::layer_controls::truncated_selectable(ui, selected, display_name)
                .interact(egui::Sense::click_and_drag());
            if response.drag_started() {
                app.session.ui.panel_drop_target = None;
            }
            if let Some(content) = app.session.ui.layers_drag_content
                && response.hovered()
                && !panel.locked
                && app.doc.canvases[ci].parent_panel(content) != Some(panel_id)
            {
                app.session.ui.panel_drop_target = Some(panel_id);
                if ui.input(|input| input.pointer.primary_released()) {
                    drop_content = Some(content);
                }
            }
            if app.session.ui.panel_drop_target == Some(panel_id) {
                ui.painter().rect_stroke(
                    response.rect,
                    2.0,
                    egui::Stroke::new(1.5_f32, ui.visuals().selection.stroke.color),
                    egui::StrokeKind::Inside,
                );
            }
            if response.clicked() || (response.secondary_clicked() && !selected) {
                select_panel = true;
            }
            if response.double_clicked() {
                enter_panel = true;
            }
            response.context_menu(|ui| panel_context_menu(app, ui));
            flags
        },
        |ui| {
            let mut locked = panel.locked;
            if super::layer_controls::lock_button(ui, &mut locked).changed() {
                Some((panel.visible, locked))
            } else {
                None
            }
        },
    );
    flags = visible_flags.or(locked_flags);
    if let Some(content) = drop_content {
        app.select_content(ci, content);
        crate::ui::commands::execute_without_clipboard(
            crate::ui::commands::CommandId::MoveContentToPanel(Some(panel_id)),
            app,
            ui.ctx(),
        );
        app.session.ui.layers_drag_content = None;
        app.session.ui.panel_drop_target = None;
    }
    if let Some((visible, locked)) = flags {
        replace_panel(app, ci, panel_id, |panel| {
            panel.visible = visible;
            panel.locked = locked;
        });
    }
    if select_panel {
        app.session.ui.selection_scope = plotx_core::state::SelectionScope::Layers;
        if ui.input(|input| input.modifiers.command || input.modifiers.shift) {
            if let Err(reason) = app.toggle_panel_sibling(ci, panel_id) {
                app.session.status = reason.to_owned();
            }
        } else {
            app.select_panel(ci, panel_id);
        }
    }
    if enter_panel {
        app.enter_panel(ci, panel_id);
    }
    if open {
        for content in panel.item_order.iter().rev().copied() {
            render_content(app, ci, panel_id, content, ui);
        }
        if panel.item_order.is_empty() {
            super::layer_controls::row(
                ui,
                |ui| {
                    ui.add_space(36.0);
                    ui.add(egui::Label::new("Empty panel — add or move content here.").truncate());
                },
                |_| {},
            );
        }
    }
}

pub(super) fn panel_tree_name(app: &PlotxApp, ci: usize, panel: &Panel) -> String {
    let label = match &panel.label.mode {
        PanelLabelMode::Auto { slot } => app.doc.canvases[ci]
            .panel_label_style
            .format(*slot as usize),
        PanelLabelMode::LockedAuto { value } | PanelLabelMode::Manual { value } => value.clone(),
    };
    if panel.label.visible && !label.is_empty() {
        format!("{label}  {}", panel.name)
    } else {
        panel.name.clone()
    }
}

fn render_content(app: &mut PlotxApp, ci: usize, panel: PanelId, content: ContentId, ui: &mut Ui) {
    let Some(item) = app.doc.canvases[ci].object(content).cloned() else {
        return;
    };
    let path = SelectionPath::content(app.doc.canvases[ci].resource_id, Some(panel), content);
    let selected = app.session.ui.hierarchical_selection.contains(path);
    let mut select = false;
    let mut flags = None;
    let destinations: Vec<_> = app.doc.canvases[ci]
        .panels
        .iter()
        .filter(|candidate| candidate.id != panel && !candidate.locked)
        .map(|candidate| (candidate.id, candidate.name.clone()))
        .collect();
    let (visible_flags, locked_flags) = super::layer_controls::row(
        ui,
        |ui| {
            ui.add_space(24.0);
            let mut visible = item.visible;
            if super::layer_controls::visibility_button(ui, &mut visible).changed() {
                flags = Some((visible, item.locked));
            }
            ui.weak(super::layer_controls::kind_glyph(&item.kind))
                .on_hover_text(super::layer_controls::kind_label(&item.kind));
            let response =
                super::layer_controls::truncated_selectable(ui, selected, item.name.clone())
                    .interact(egui::Sense::click_and_drag());
            if response.drag_started() {
                app.session.ui.layers_drag_content = Some(content);
            }
            if response.clicked() || (response.secondary_clicked() && !selected) {
                select = true;
            }
            response.context_menu(|ui| {
                if ui.button("Move out of panel").clicked() {
                    app.select_content(ci, content);
                    crate::ui::commands::execute_without_clipboard(
                        crate::ui::commands::CommandId::MoveContentToPanel(None),
                        app,
                        ui.ctx(),
                    );
                    ui.close();
                }
                if !destinations.is_empty() {
                    ui.menu_button("Move to panel", |ui| {
                        for (target, name) in &destinations {
                            if ui.button(name).clicked() {
                                app.select_content(ci, content);
                                crate::ui::commands::execute_without_clipboard(
                                    crate::ui::commands::CommandId::MoveContentToPanel(Some(
                                        *target,
                                    )),
                                    app,
                                    ui.ctx(),
                                );
                                ui.close();
                            }
                        }
                    });
                }
            });
            flags
        },
        |ui| {
            let mut locked = item.locked;
            if super::layer_controls::lock_button(ui, &mut locked).changed() {
                Some((item.visible, locked))
            } else {
                None
            }
        },
    );
    flags = visible_flags.or(locked_flags);
    if let Some((visible, locked)) = flags {
        app.execute_action(Action::set_object_flags(
            ci,
            content,
            (item.visible, item.locked),
            (visible, locked),
        ));
    }
    if select {
        app.session.ui.selection_scope = plotx_core::state::SelectionScope::Layers;
        let additive = ui.input(|input| input.modifiers.command || input.modifiers.shift);
        if additive {
            if let Err(reason) = app.toggle_content_sibling(ci, content) {
                app.session.status = reason.to_owned();
            }
        } else {
            app.select_content(ci, content);
        }
    }
}

fn panel_context_menu(app: &mut PlotxApp, ui: &mut Ui) {
    ui.menu_button("Order", |ui| {
        for (label, order) in [
            ("Bring to Front", plotx_core::actions::ZOrder::Front),
            ("Bring Forward", plotx_core::actions::ZOrder::Forward),
            ("Send Backward", plotx_core::actions::ZOrder::Backward),
            ("Send to Back", plotx_core::actions::ZOrder::Back),
        ] {
            let command = crate::ui::commands::CommandId::ZOrder(order);
            let descriptor = crate::ui::commands::describe(app, command);
            if ui
                .add_enabled(descriptor.enabled, egui::Button::new(label))
                .clicked()
            {
                crate::ui::commands::execute_without_clipboard(command, app, ui.ctx());
                ui.close();
            }
        }
    });
    for command in [
        crate::ui::commands::CommandId::DuplicatePanel,
        crate::ui::commands::CommandId::DissolvePanel,
        crate::ui::commands::CommandId::DeletePanel,
    ] {
        let descriptor = crate::ui::commands::describe(app, command);
        if ui
            .add_enabled(descriptor.enabled, egui::Button::new(descriptor.label))
            .clicked()
        {
            crate::ui::commands::execute_without_clipboard(command, app, ui.ctx());
            ui.close();
        }
    }
}

fn replace_panel(
    app: &mut PlotxApp,
    ci: usize,
    id: PanelId,
    edit: impl FnOnce(&mut plotx_core::state::Panel),
) {
    let before = PanelState::of(&app.doc.canvases[ci]);
    let mut page = app.doc.canvases[ci].clone();
    if let Some(panel) = page.panel_mut(id) {
        edit(panel);
        app.execute_action(Action::ReplacePanelState {
            canvas: ci,
            before,
            after: PanelState::of(&page),
        });
    }
}
