use crate::ui::switcher;
use egui::Ui;
use egui_phosphor::regular as icon;
use plotx_core::actions::{Action, ZOrder};
use plotx_core::state::{
    CanvasObjectKind, FrameRef, ObjectId, PlotxApp, PrimaryView, RenameState, RenameTarget,
};

mod board_views;
mod data_browser;
use board_views::board_views_section;
use data_browser::{AnalysisItem, AnalysisKind, DataTree, DatasetNode};

pub fn render(app: &mut PlotxApp, ui: &mut Ui) {
    ui.add_space(6.0);
    switcher::segmented(ui, &mut app.session.view);
    ui.add_space(8.0);
    ui.separator();
    ui.add_space(4.0);

    if app.session.view == PrimaryView::Data {
        ui.add(
            egui::TextEdit::singleline(&mut app.session.ui.data_browser_filter)
                .hint_text(format!(
                    "{} Search data and analysis",
                    icon::MAGNIFYING_GLASS
                ))
                .desired_width(f32::INFINITY),
        );
        ui.add_space(4.0);
    }

    // Board bookmarks pin to the bottom; the list scrolls in the space above.
    if app.session.view == PrimaryView::Canvas && !app.doc.canvases.is_empty() {
        egui::Panel::bottom("board_views")
            .resizable(false)
            .show_inside(ui, |ui| board_views_section(app, ui));
    }

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| match app.session.view {
            PrimaryView::Canvas => canvas_list(app, ui),
            PrimaryView::Data => data_list(app, ui),
        });
}

enum RenameOutcome {
    Commit(String),
    Cancel,
    Editing,
}

/// The shared inline text box for an active rename. Clicking elsewhere keeps
/// the draft; only Enter commits and Escape cancels it.
fn rename_edit(ui: &mut Ui, rs: &mut RenameState, id: egui::Id) -> RenameOutcome {
    let resp = ui.add(
        egui::TextEdit::singleline(&mut rs.buffer)
            .desired_width(f32::INFINITY)
            .id(id),
    );
    if rs.focus {
        resp.request_focus();
        rs.focus = false;
    }
    let empty = rs.buffer.trim().is_empty();
    if empty {
        ui.colored_label(ui.visuals().error_fg_color, "Name cannot be empty.");
    }
    let (commit, cancel) = ui.input(|input| {
        (
            input.key_pressed(egui::Key::Enter),
            input.key_pressed(egui::Key::Escape),
        )
    });
    if cancel {
        RenameOutcome::Cancel
    } else if commit && (resp.has_focus() || resp.lost_focus()) {
        if empty {
            resp.request_focus();
            RenameOutcome::Editing
        } else {
            RenameOutcome::Commit(rs.buffer.clone())
        }
    } else {
        RenameOutcome::Editing
    }
}

fn canvas_list(app: &mut PlotxApp, ui: &mut Ui) {
    if app.doc.canvases.is_empty() {
        ui.weak("No canvases yet. Opening data creates one automatically.");
        return;
    }

    let mut select: Option<(usize, bool)> = None;
    let mut open_settings: Option<usize> = None;
    let mut delete: Option<usize> = None;
    let mut start_rename: Option<usize> = None;
    let mut commit: Option<(usize, String)> = None;
    let mut cancel = false;

    for ci in 0..app.doc.canvases.len() {
        let renaming = matches!(
            &app.session.ui.rename,
            Some(RenameState { target: RenameTarget::Canvas(i), .. }) if *i == ci
        );
        if renaming {
            let rs = app.session.ui.rename.as_mut().unwrap();
            match rename_edit(ui, rs, egui::Id::new(("rename_canvas", ci))) {
                RenameOutcome::Commit(s) => commit = Some((ci, s)),
                RenameOutcome::Cancel => cancel = true,
                RenameOutcome::Editing => {}
            }
            continue;
        }

        let name = app.doc.canvases[ci].name.clone();
        let selected = crate::ui::canvas::frame_is_selected(app, FrameRef::Page(ci));
        let resp = ui.selectable_label(selected, name);
        if resp.clicked() {
            let extend = ui.input(|i| i.modifiers.shift || i.modifiers.command || i.modifiers.ctrl);
            select = Some((ci, extend));
        }
        if resp.double_clicked() {
            start_rename = Some(ci);
        }
        resp.context_menu(|ui| {
            if ui.button("Rename").clicked() {
                start_rename = Some(ci);
                ui.close();
            }
            if ui.button("Canvas settings…").clicked() {
                open_settings = Some(ci);
                ui.close();
            }
            if ui.button("Delete canvas").clicked() {
                delete = Some(ci);
                ui.close();
            }
        });
        resp.on_hover_text("Double-click to rename");
    }

    if let Some(ci) = app.session.active_canvas
        && ci < app.doc.canvases.len()
        && !app.doc.canvases[ci].objects.is_empty()
    {
        ui.add_space(8.0);
        ui.separator();
        ui.add_space(4.0);
        ui.strong("Layers");
        object_list(app, ci, ui);
    }

    if let Some((ci, name)) = commit {
        let trimmed = name.trim();
        if !trimmed.is_empty() {
            let before = app.doc.canvases[ci].name.clone();
            app.execute_action(Action::rename_canvas(ci, before, trimmed.to_owned()));
        }
        app.session.ui.rename = None;
    }
    if cancel {
        app.session.ui.rename = None;
    }
    if let Some((ci, extend)) = select {
        if extend {
            plotx_core::state::toggle_frame_selection_synced(app, FrameRef::Page(ci));
        } else {
            app.session.active_canvas = Some(ci);
            let datasets = app.doc.canvases[ci].dataset_indices();
            let lead = app.doc.canvases[ci].active_dataset();
            app.focus_datasets(&datasets, lead);
            app.sync_selection_to_active_canvas();
            app.reset_interaction();
            app.session.ui.panel_note_inline_edit = None;
            app.session.ui.panel_note_edit = None;
            app.session.ui.frame_selection = vec![FrameRef::Page(ci)];
            crate::ui::canvas::request_board_fit(app, ui.ctx(), FrameRef::Page(ci));
        }
    }
    if let Some(ci) = start_rename {
        app.session.active_canvas = Some(ci);
        let active = app.doc.canvases[ci].active_dataset();
        app.set_active_dataset(active);
        app.sync_selection_to_active_canvas();
        app.reset_interaction();
        app.session.ui.panel_note_inline_edit = None;
        app.session.ui.panel_note_edit = None;
        app.session.ui.rename = Some(RenameState {
            target: RenameTarget::Canvas(ci),
            buffer: app.doc.canvases[ci].name.clone(),
            focus: true,
        });
    }
    if let Some(ci) = open_settings {
        app.session.ui.canvas_settings = Some(ci);
    }
    if let Some(ci) = delete
        && let Some(action) = Action::delete_canvas(app, ci)
    {
        app.execute_action(action);
    }
}

/// The z-order (front-to-back, top-to-bottom) layers list. Rows mirror the
/// canvas front: the top row is the frontmost object (last in `objects`).
fn object_list(app: &mut PlotxApp, ci: usize, ui: &mut Ui) {
    let mut select = None;
    let mut reorder: Option<(ObjectId, ZOrder)> = None;
    // (object, destination canvas, is_move). Deferred so the row's context menu
    // doesn't hold an app borrow while mutating the canvases.
    let mut transfer: Option<(ObjectId, usize, bool)> = None;
    let others = crate::ui::menus::other_canvas_destinations(app, ci);
    let count = app.doc.canvases[ci].objects.len();
    for row in 0..count {
        let oi = count - 1 - row;
        let object_id = app.doc.canvases[ci].objects[oi].id;
        ui.horizontal(|ui| {
            let mut visible = app.doc.canvases[ci].objects[oi].visible;
            if ui
                .checkbox(&mut visible, "")
                .on_hover_text("Visible")
                .changed()
            {
                let before = (
                    app.doc.canvases[ci].objects[oi].visible,
                    app.doc.canvases[ci].objects[oi].locked,
                );
                app.execute_action(Action::set_object_flags(
                    ci,
                    object_id,
                    before,
                    (visible, before.1),
                ));
            }
            ui.weak(kind_glyph(&app.doc.canvases[ci].objects[oi].kind))
                .on_hover_text(kind_label(&app.doc.canvases[ci].objects[oi].kind));
            if app.doc.canvases[ci].objects[oi].group.is_some() {
                ui.weak(egui::RichText::new("⛓").small())
                    .on_hover_text("Grouped");
            }
            let selected = app.session.ui.selection.contains(object_id)
                || app.session.ui.selection.object() == Some(object_id);
            let resp = ui.selectable_label(selected, app.doc.canvases[ci].objects[oi].name.clone());
            if resp.clicked() {
                select = Some(object_id);
            }
            resp.context_menu(|ui| {
                object_transfer_menu(ui, object_id, &others, &mut transfer);
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let mut locked = app.doc.canvases[ci].objects[oi].locked;
                if ui
                    .checkbox(&mut locked, "")
                    .on_hover_text("Locked")
                    .changed()
                {
                    let before = (
                        app.doc.canvases[ci].objects[oi].visible,
                        app.doc.canvases[ci].objects[oi].locked,
                    );
                    app.execute_action(Action::set_object_flags(
                        ci,
                        object_id,
                        before,
                        (before.0, locked),
                    ));
                }
                if ui
                    .add_enabled(
                        row + 1 < count,
                        egui::Button::new(icon::CARET_DOWN).small().frame(false),
                    )
                    .on_hover_text("Send backward")
                    .clicked()
                {
                    reorder = Some((object_id, ZOrder::Backward));
                }
                if ui
                    .add_enabled(
                        row > 0,
                        egui::Button::new(icon::CARET_UP).small().frame(false),
                    )
                    .on_hover_text("Bring forward")
                    .clicked()
                {
                    reorder = Some((object_id, ZOrder::Forward));
                }
            });
        });
    }
    if let Some(object_id) = select {
        app.select_object(ci, object_id);
        let active = app.doc.canvases[ci]
            .object(object_id)
            .and_then(|object| object.dataset());
        app.set_active_dataset(active);
        app.reset_interaction();
        app.session.ui.panel_note_inline_edit = None;
        app.session.ui.panel_note_edit = None;
    }
    if let Some((object_id, op)) = reorder {
        app.apply_z_order(ci, &[object_id], op);
    }
    if let Some((object_id, to, is_move)) = transfer {
        app.transfer_objects_to_canvas(ci, &[object_id], to, is_move);
    }
}

fn object_transfer_menu(
    ui: &mut Ui,
    object_id: ObjectId,
    others: &[(usize, String)],
    transfer: &mut Option<(ObjectId, usize, bool)>,
) {
    let mut picked = None;
    crate::ui::menus::transfer_to_canvas_menu(
        ui,
        others,
        "Move to canvas",
        "Copy to canvas",
        &mut picked,
    );
    if let Some((to, is_move)) = picked {
        *transfer = Some((object_id, to, is_move));
    }
}

fn kind_glyph(kind: &CanvasObjectKind) -> &'static str {
    match kind {
        CanvasObjectKind::Plot(_) => icon::CHART_LINE,
        CanvasObjectKind::Text(_) => "T",
        CanvasObjectKind::Shape(_) => icon::SHAPES,
        CanvasObjectKind::PanelLabel(_) => icon::TAG,
    }
}

fn kind_label(kind: &CanvasObjectKind) -> &'static str {
    match kind {
        CanvasObjectKind::Plot(_) => "Plot",
        CanvasObjectKind::Text(_) => "Text",
        CanvasObjectKind::Shape(_) => "Shape",
        CanvasObjectKind::PanelLabel(_) => "Panel label",
    }
}

fn data_list(app: &mut PlotxApp, ui: &mut Ui) {
    if app.doc.datasets.is_empty() {
        ui.weak("No data yet. Open an acquisition from the toolbar.");
        return;
    }

    data_browser::reveal_active_path(app);
    let query = app.session.ui.data_browser_filter.clone();
    let filtering = !query.trim().is_empty();
    let tree = DataTree::build(app).filtered(app, &query);
    if tree.roots.is_empty() {
        ui.weak("No matching data or analysis results.");
        return;
    }

    let mut event = None;
    let mut rename_rendered = false;
    for node in &tree.roots {
        render_dataset_node(
            app,
            ui,
            node,
            0,
            filtering,
            &mut rename_rendered,
            &mut event,
        );
    }
    apply_browser_event(app, ui, event);
}

#[derive(Clone)]
enum BrowserEvent {
    SelectDataset(usize, bool),
    OpenSheet(usize),
    StartRename(usize),
    RenameCommit(usize, String),
    RenameCancel,
    Stack,
    RevealSources(usize),
    Jump(FrameRef),
    SelectAnalysis(usize, AnalysisItem, bool),
}

fn render_dataset_node(
    app: &mut PlotxApp,
    ui: &mut Ui,
    node: &DatasetNode,
    depth: usize,
    filtering: bool,
    rename_rendered: &mut bool,
    event: &mut Option<BrowserEvent>,
) {
    let di = node.dataset;
    let has_children = !node.analysis.is_empty() || !node.derived.is_empty();
    let open = filtering || !app.session.ui.data_browser_collapsed_datasets.contains(&di);
    let renaming = !*rename_rendered
        && matches!(
            &app.session.ui.rename,
            Some(RenameState { target: RenameTarget::Data(i), .. }) if *i == di
        );

    ui.horizontal(|ui| {
        ui.add_space(depth as f32 * 12.0);
        if has_children {
            let glyph = if open {
                icon::CARET_DOWN
            } else {
                icon::CARET_RIGHT
            };
            if ui.small_button(glyph).clicked() && !filtering {
                if open {
                    app.session.ui.data_browser_collapsed_datasets.insert(di);
                } else {
                    app.session.ui.data_browser_collapsed_datasets.remove(&di);
                }
            }
        } else {
            ui.add_space(18.0);
        }
        if node.linked_reference {
            ui.weak(icon::LINK)
                .on_hover_text("Reference to the same multi-source dataset");
        }
        if node.cycle_cut {
            ui.colored_label(ui.visuals().warn_fg_color, icon::WARNING)
                .on_hover_text("Recursive lineage was cut here");
        }
        if renaming {
            *rename_rendered = true;
            let rs = app.session.ui.rename.as_mut().unwrap();
            match rename_edit(ui, rs, egui::Id::new(("rename_data", di))) {
                RenameOutcome::Commit(name) => *event = Some(BrowserEvent::RenameCommit(di, name)),
                RenameOutcome::Cancel => *event = Some(BrowserEvent::RenameCancel),
                RenameOutcome::Editing => {}
            }
            return;
        }

        let selected = app.active_dataset() == Some(di)
            || app.session.ui.data_selection.contains(&di)
            || (app.doc.datasets[di].as_table().is_some()
                && crate::ui::canvas::frame_is_selected(app, FrameRef::Sheet(di)));
        let mut resp = ui.selectable_label(selected, app.doc.datasets[di].display_name());
        if let Some(tooltip) = data_browser::sources_tooltip(app, di) {
            resp = resp.on_hover_text(tooltip);
        } else {
            resp = resp.on_hover_text(format!(
                "{} · Double-click to open",
                app.doc.datasets[di].kind_label()
            ));
        }
        if resp.clicked() {
            let extend = ui.input(|i| i.modifiers.shift || i.modifiers.command || i.modifiers.ctrl);
            *event = Some(BrowserEvent::SelectDataset(di, extend));
        }
        if resp.double_clicked() {
            *event = Some(BrowserEvent::OpenSheet(di));
        }

        let can_stack = app.stackable_selection().is_some();
        let source_target = provenance_source_frame(app, di);
        let chart_target = plotx_core::state::page_frame_showing_dataset(app, di);
        resp.context_menu(|ui| {
            if ui.button("Open data sheet").clicked() {
                *event = Some(BrowserEvent::OpenSheet(di));
                ui.close();
            }
            if ui.button("Rename").clicked() {
                *event = Some(BrowserEvent::StartRename(di));
                ui.close();
            }
            if app.doc.datasets[di].lineage().is_some() && ui.button("Reveal sources").clicked() {
                *event = Some(BrowserEvent::RevealSources(di));
                ui.close();
            }
            ui.separator();
            if ui
                .add_enabled(can_stack, egui::Button::new("Stack selected data"))
                .on_hover_text("Build a new page stacking the selected datasets")
                .on_disabled_hover_text(
                    "Select 2 or more datasets of the same type (1D NMR, data tables, or 2D NMR).",
                )
                .clicked()
            {
                *event = Some(BrowserEvent::Stack);
                ui.close();
            }
            if source_target.is_some() || chart_target.is_some() {
                ui.separator();
            }
            if let Some(frame) = source_target
                && ui.button("Jump to source spectrum").clicked()
            {
                *event = Some(BrowserEvent::Jump(frame));
                ui.close();
            }
            if let Some(frame) = chart_target
                && ui.button("Jump to plot").clicked()
            {
                *event = Some(BrowserEvent::Jump(frame));
                ui.close();
            }
        });
    });

    if !open || node.cycle_cut {
        return;
    }
    if !node.analysis.is_empty() {
        render_analysis_group(app, ui, node, depth + 1, filtering, event);
    }
    if !node.derived.is_empty() {
        render_derived_group(app, ui, node, depth + 1, filtering, rename_rendered, event);
    }
}

fn render_analysis_group(
    app: &mut PlotxApp,
    ui: &mut Ui,
    node: &DatasetNode,
    depth: usize,
    filtering: bool,
    event: &mut Option<BrowserEvent>,
) {
    let di = node.dataset;
    let open = filtering || app.session.ui.data_browser_expanded_analysis.contains(&di);
    ui.horizontal(|ui| {
        ui.add_space(depth as f32 * 12.0);
        let glyph = if open {
            icon::CARET_DOWN
        } else {
            icon::CARET_RIGHT
        };
        if ui.small_button(glyph).clicked() && !filtering {
            if open {
                app.session.ui.data_browser_expanded_analysis.remove(&di);
            } else {
                app.session.ui.data_browser_expanded_analysis.insert(di);
            }
        }
        ui.weak(format!("Analysis ({})", node.analysis.len()));
    });
    if !open {
        return;
    }
    for item in &node.analysis {
        ui.horizontal(|ui| {
            ui.add_space((depth + 1) as f32 * 12.0 + 18.0);
            let key = item.kind.key(di);
            let selected = app.session.ui.data_browser_selected_node.as_deref() == Some(&key);
            let resp = ui
                .selectable_label(selected, &item.label)
                .on_hover_text(format!(
                    "{} · Double-click to open tool",
                    item.kind.type_label()
                ));
            if resp.clicked() {
                *event = Some(BrowserEvent::SelectAnalysis(di, item.clone(), false));
            }
            if resp.double_clicked() {
                *event = Some(BrowserEvent::SelectAnalysis(di, item.clone(), true));
            }
        });
    }
}

fn render_derived_group(
    app: &mut PlotxApp,
    ui: &mut Ui,
    node: &DatasetNode,
    depth: usize,
    filtering: bool,
    rename_rendered: &mut bool,
    event: &mut Option<BrowserEvent>,
) {
    let di = node.dataset;
    let open = filtering || !app.session.ui.data_browser_collapsed_derived.contains(&di);
    ui.horizontal(|ui| {
        ui.add_space(depth as f32 * 12.0);
        let glyph = if open {
            icon::CARET_DOWN
        } else {
            icon::CARET_RIGHT
        };
        if ui.small_button(glyph).clicked() && !filtering {
            if open {
                app.session.ui.data_browser_collapsed_derived.insert(di);
            } else {
                app.session.ui.data_browser_collapsed_derived.remove(&di);
            }
        }
        ui.weak(format!("Derived data ({})", node.derived.len()));
    });
    if open {
        for child in &node.derived {
            render_dataset_node(app, ui, child, depth + 1, filtering, rename_rendered, event);
        }
    }
}

fn apply_browser_event(app: &mut PlotxApp, ui: &Ui, event: Option<BrowserEvent>) {
    match event {
        Some(BrowserEvent::RenameCommit(di, name)) => {
            let trimmed = name.trim();
            let before = app.doc.datasets[di].name();
            let after = (!trimmed.is_empty()).then(|| trimmed.to_owned());
            app.execute_action(Action::rename_dataset(di, before, after));
            app.session.ui.rename = None;
        }
        Some(BrowserEvent::RenameCancel) => app.session.ui.rename = None,
        Some(BrowserEvent::SelectDataset(di, extend)) => select_dataset(app, ui, di, extend),
        Some(BrowserEvent::OpenSheet(di)) => {
            app.focus_single(di);
            app.session.ui.data_browser_selected_node = Some(format!("dataset:{di}"));
            app.session.ui.sheet_open = Some(di);
        }
        Some(BrowserEvent::StartRename(di)) => {
            app.focus_single(di);
            app.session.ui.rename = Some(RenameState {
                target: RenameTarget::Data(di),
                buffer: app.doc.datasets[di].display_name(),
                focus: true,
            });
        }
        Some(BrowserEvent::Stack) => app.stack_selected_data(),
        Some(BrowserEvent::RevealSources(di)) => {
            app.focus_single(di);
            app.session.ui.data_browser_last_active = None;
            data_browser::reveal_active_path(app);
            app.session.status = "Revealed this dataset's source path.".to_owned();
        }
        Some(BrowserEvent::Jump(frame)) => jump_to_frame(app, ui, frame),
        Some(BrowserEvent::SelectAnalysis(di, item, open)) => {
            select_analysis(app, ui, di, &item, open)
        }
        None => {}
    }
}

fn select_dataset(app: &mut PlotxApp, ui: &Ui, di: usize, extend: bool) {
    let is_table = app.doc.datasets[di].as_table().is_some();
    app.toggle_selection(di, extend);
    app.session.ui.data_browser_selected_node = Some(format!("dataset:{di}"));
    if is_table {
        if extend {
            plotx_core::state::toggle_frame_selection(app, FrameRef::Sheet(di));
        } else {
            app.session.ui.frame_selection = vec![FrameRef::Sheet(di)];
            crate::ui::canvas::request_board_fit(app, ui.ctx(), FrameRef::Sheet(di));
        }
    }
}

fn select_analysis(app: &mut PlotxApp, ui: &Ui, di: usize, item: &AnalysisItem, open: bool) {
    app.focus_single(di);
    app.session.ui.data_browser_selected_node = Some(item.kind.key(di));
    match item.kind {
        AnalysisKind::Peak(id) => {
            app.session.ui.selected_peak = Some(id);
            if open {
                app.set_tool(plotx_core::state::Tool::Peaks);
                request_tool_group(app, plotx_core::state::ToolGroup::Peaks);
            }
        }
        AnalysisKind::Integral(id) => {
            app.session.ui.selected_integral = Some(id);
            if open {
                app.set_tool(plotx_core::state::Tool::Integrate);
                let group = if app.doc.datasets[di]
                    .as_nmr2d()
                    .is_some_and(|dataset| dataset.is_true_2d())
                {
                    plotx_core::state::ToolGroup::Nmr2dExperiment
                } else {
                    plotx_core::state::ToolGroup::Nmr1dAnalysis
                };
                request_tool_group(app, group);
            }
        }
        AnalysisKind::Region(id) => {
            app.session.ui.selected_region = Some(id);
            if open {
                app.set_tool(plotx_core::state::Tool::Regions);
                crate::ui::tools::open_region_task(app, di);
            }
        }
        AnalysisKind::LineFit(_) | AnalysisKind::Multiplet(_) => {
            if open {
                app.set_tool(plotx_core::state::Tool::LineFit);
                request_tool_group(app, plotx_core::state::ToolGroup::LineFit);
            }
        }
        AnalysisKind::CurveFitResponse(column) => {
            app.session.ui.fit_dataset = Some(di);
            app.session.ui.fit_column = Some(column);
            if open {
                crate::ui::tools::open_curve_fit_task(app, di);
                app.session.ui.sheet_open = Some(di);
                jump_to_frame(app, ui, FrameRef::Sheet(di));
            }
            return;
        }
    }
    if open && let Some(frame) = plotx_core::state::page_frame_showing_dataset(app, di) {
        jump_to_frame(app, ui, frame);
    }
}

fn request_tool_group(app: &mut PlotxApp, group: plotx_core::state::ToolGroup) {
    app.session.secondary_sidebar_visible = true;
    app.session.ui.requested_tool_group = Some(group);
}

fn jump_to_frame(app: &mut PlotxApp, ui: &Ui, frame: FrameRef) {
    match frame {
        FrameRef::Page(ci) => app.session.active_canvas = Some(ci),
        FrameRef::Sheet(sdi) => app.focus_single(sdi),
    }
    app.session.ui.frame_selection = vec![frame];
    crate::ui::canvas::request_board_fit(app, ui.ctx(), frame);
    app.session.status = "Jumped to linked frame.".to_owned();
}

/// The board frame holding the source spectrum of table dataset `di` (via its
/// extraction provenance): the page that charts the source, or the source's own
/// sheet if it is itself a table. `None` when `di` is not a linked table.
fn provenance_source_frame(app: &PlotxApp, di: usize) -> Option<FrameRef> {
    let source_resource = app
        .doc
        .datasets
        .get(di)?
        .as_table()?
        .provenance
        .as_ref()?
        .source_resource
        .as_str();
    let src = app
        .doc
        .datasets
        .iter()
        .position(|dataset| dataset.resource_id() == source_resource)?;
    plotx_core::state::page_frame_showing_dataset(app, src).or_else(|| {
        app.doc
            .datasets
            .get(src)
            .filter(|d| d.as_table().is_some())
            .map(|_| FrameRef::Sheet(src))
    })
}
