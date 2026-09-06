//! Task-oriented, collapsible command Ribbon. Its visual vocabulary stays close
//! to PlotX's existing light egui chrome; the task/group hierarchy is the only
//! idea borrowed from the supplied Office reference.

mod buttons;
mod layout;

use egui::{
    Align, Button, Label, Layout, PointerButton, RichText, Sense, TextWrapMode, Ui, UiBuilder,
    Vec2, vec2,
};
use egui_phosphor::regular as icon;
use plotx_core::state::{ArrangeGridDraft, PlotxApp, ToolGroup, WorkflowTab};

use super::clipboard_table::ClipboardTablePaste;
use super::commands::{self, CommandDescriptor, CommandId};
use buttons::{collapsed_tile, overflow_item, ribbon_button};
use layout::{GroupScale, STACK_GAP, TILE_HEIGHT, TabPlan};

/// The native metric includes a little more bottom breathing room than the
/// tab highlight needs visually; trim it so the highlight has equal margins.
const MACOS_TITLE_ROW_BOTTOM_TRIM: f32 = 2.0;

/// The task row's summary of the command area: hidden by the user, every
/// group at its Large scale, or at least one group scaled down.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum RibbonDensity {
    Collapsed,
    Scaled,
    Full,
}

pub(crate) fn render(
    app: &mut PlotxApp,
    clipboard: &mut ClipboardTablePaste,
    ui: &mut Ui,
    chrome: super::RibbonChrome,
) {
    let width = ui.available_width();
    // Density is content-aware: measured against the active tab's groups, not a
    // fixed window-width breakpoint (which UI scaling would silently retune).
    // Measured before `task_row`, so a tab click adopts the new tab's density
    // one frame later — invisible in practice.
    let density = {
        let measure = layout::text_measure(ui.ctx().clone());
        let catalog = commands::catalog(app);
        let groups = layout::groups_for_tab(&catalog, app.session.ui.ribbon_tab);
        let plan = tab_plan(ui, &groups, &measure, width);
        layout::density(app.session.ui.ribbon_expanded, &plan)
    };
    task_row(app, clipboard, ui, chrome);
    if density != RibbonDensity::Collapsed {
        ui.separator();
        command_row(app, clipboard, ui, density);
    }
}

fn task_row(
    app: &mut PlotxApp,
    clipboard: &mut ClipboardTablePaste,
    ui: &mut Ui,
    chrome: super::RibbonChrome,
) {
    let Some(traffic_lights) = chrome.macos_traffic_lights else {
        // Windows and Linux retain the original Ribbon task-row layout; their
        // separate custom title bar owns all window chrome and dragging.
        ui.horizontal(|ui| render_task_row_contents(app, clipboard, ui, false, None));
        return;
    };

    let row_height =
        (traffic_lights.y - MACOS_TITLE_ROW_BOTTOM_TRIM).max(ui.spacing().interact_size.y);
    let vertical_spacing = ui.spacing().item_spacing.y;
    // The next widget is the rule below the unified title row. Suppress the
    // normal inter-widget gap so the rule sits on the row boundary; otherwise
    // the selected tab appears high despite being centered.
    ui.spacing_mut().item_spacing.y = 0.0;
    let (row_rect, _) =
        ui.allocate_exact_size(vec2(ui.available_width(), row_height), Sense::hover());
    ui.spacing_mut().item_spacing.y = vertical_spacing;
    // Register the background first; interactive children below take
    // precedence while every remaining pixel continues to drag the window.
    let drag = ui.interact(
        row_rect,
        ui.id().with("macos_unified_titlebar_drag"),
        Sense::click_and_drag(),
    );
    if drag.drag_started_by(PointerButton::Primary) {
        ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
    }

    let mut ui = ui.new_child(
        UiBuilder::new()
            .max_rect(row_rect)
            .layout(Layout::left_to_right(Align::Center)),
    );
    let leading = traffic_lights.x + 4.0;
    ui.add_space(leading);
    let compact_controls =
        super::ribbon_chrome::controls_need_compacting(app, &ui, leading, row_rect.width());
    let inline_title_width = super::ribbon_chrome::available_title_width(
        app,
        &ui,
        leading,
        row_rect.width(),
        compact_controls,
    );
    render_task_row_contents(
        app,
        clipboard,
        &mut ui,
        compact_controls,
        inline_title_width,
    );
}

fn render_task_row_contents(
    app: &mut PlotxApp,
    clipboard: &mut ClipboardTablePaste,
    ui: &mut Ui,
    compact_controls: bool,
    inline_title_width: Option<f32>,
) {
    for (tab, response) in task_tab_buttons(ui, app.session.ui.ribbon_tab) {
        if response.clicked() {
            select_workflow_tab(app, tab);
            // Picking a task re-opens a manually collapsed command area.
            app.session.ui.ribbon_expanded = true;
        }
    }

    if let (Some(title), Some(width)) = (
        super::ribbon_chrome::inline_project_title(app),
        inline_title_width,
    ) {
        ui.separator();
        let title = ui.add_sized(
            [width, ui.spacing().interact_size.y],
            Label::new(RichText::new(title).color(ui.visuals().weak_text_color()))
                .truncate()
                .sense(Sense::click_and_drag()),
        );
        if title.drag_started_by(PointerButton::Primary) {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
        }
    }

    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        // The controls keep their own spacing instead of inheriting the tab
        // strip's gap.
        ui.spacing_mut().item_spacing.x = super::ribbon_chrome::CONTROL_SPACING;
        // Stable id scope: siblings earlier in this row (the inline project
        // title, tab labels) come and go with app state; without this every
        // control in the strip changes id when they do, which drops focus
        // and trips egui's rect-changed-id debug overlay.
        let scope = UiBuilder::new()
            .id_salt(egui::Id::new("ribbon_chrome_controls"))
            .global_scope(true);
        ui.scope_builder(scope, |ui| {
            render_chrome_controls(app, clipboard, ui, compact_controls)
        });
    });
}

fn task_tab_buttons(ui: &mut Ui, selected: WorkflowTab) -> [(WorkflowTab, egui::Response); 6] {
    ui.spacing_mut().item_spacing.x = super::ribbon_chrome::TASK_TAB_SPACING;
    WorkflowTab::ALL.map(|tab| {
        let response =
            ui.selectable_label(selected == tab, crate::typography::headline(tab.label()));
        (tab, response)
    })
}

fn render_chrome_controls(
    app: &mut PlotxApp,
    clipboard: &mut ClipboardTablePaste,
    ui: &mut Ui,
    compact_controls: bool,
) {
    let collapse = super::panel_chrome::toggle(
        ui,
        super::panel_chrome::Edge::Top,
        app.session.ui.ribbon_expanded,
        if app.session.ui.ribbon_expanded {
            "Collapse ribbon"
        } else {
            "Expand ribbon"
        },
    );
    if collapse.clicked() {
        app.session.ui.ribbon_expanded = !app.session.ui.ribbon_expanded;
    }
    update_button(app, ui, compact_controls);
    let palette = commands::describe(app, CommandId::CommandPalette);
    let search_label = if compact_controls {
        icon::MAGNIFYING_GLASS.to_owned()
    } else {
        format!("{} Search commands", icon::MAGNIFYING_GLASS)
    };
    if ui
        .add(Button::new(search_label).frame_when_inactive(false))
        .on_hover_text(format!(
            "Search every command ({})",
            palette.shortcut.as_deref().unwrap_or("Ctrl+K")
        ))
        .clicked()
    {
        commands::execute(CommandId::CommandPalette, app, clipboard, ui.ctx());
    }
    ui.separator();
    // Right-to-left layout: added secondary-first so the pair reads
    // [left sidebar][right sidebar], mirroring the window.
    super::ribbon_chrome::sidebar_toggle_button(
        app,
        clipboard,
        ui,
        CommandId::ToggleSecondarySidebar,
    );
    super::ribbon_chrome::sidebar_toggle_button(
        app,
        clipboard,
        ui,
        CommandId::TogglePrimarySidebar,
    );
}

/// Open the Arrange tab's grid popover, or close it when it is already open.
/// The popover anchors to its Layout tile, so opening it brings the Ribbon to
/// the Arrange tab for a palette or menu invocation. A canvas already laid
/// out as a grid seeds its own size; otherwise the draft keeps the last size
/// used.
pub(crate) fn toggle_arrange_grid_popover(app: &mut PlotxApp, ctx: &egui::Context) {
    let popup_id = buttons::arrange_grid_popup_id();
    if egui::Popup::is_id_open(ctx, popup_id) {
        egui::Popup::close_id(ctx, popup_id);
        return;
    }
    let layout = app
        .session
        .active_canvas
        .and_then(|ci| app.doc.canvases.get(ci))
        .map(|canvas| canvas.layout);
    if let Some(layout) = layout
        && (layout.rows > 1 || layout.cols > 1)
    {
        app.session.ui.arrange_grid_draft = ArrangeGridDraft {
            rows: layout.rows,
            cols: layout.cols,
        };
    }
    app.session.ui.ribbon_tab = WorkflowTab::Arrange;
    app.session.ui.ribbon_expanded = true;
    egui::Popup::open_id(ctx, popup_id);
}

fn select_workflow_tab(app: &mut PlotxApp, tab: WorkflowTab) {
    app.session.ui.ribbon_tab = tab;
    match tab {
        WorkflowTab::Data => {}
        WorkflowTab::Process => super::tools::expand_processing_surface(app),
        WorkflowTab::Analyze => {
            if let Some(dataset) = app.active_dataset().and_then(|di| app.doc.datasets.get(di)) {
                app.session.ui.requested_tool_group = dataset
                    .tool_groups()
                    .iter()
                    .copied()
                    .find(|group| *group != ToolGroup::Processing);
            }
        }
        WorkflowTab::View | WorkflowTab::Figure | WorkflowTab::Arrange => {}
    }
}

fn command_row(
    app: &mut PlotxApp,
    clipboard: &mut ClipboardTablePaste,
    ui: &mut Ui,
    density: RibbonDensity,
) {
    let tab = app.session.ui.ribbon_tab;
    let catalog = commands::catalog(app);
    let groups = layout::groups_for_tab(&catalog, tab);
    if density == RibbonDensity::Collapsed {
        return;
    }
    let measure = layout::text_measure(ui.ctx().clone());
    let plan = tab_plan(ui, &groups, &measure, ui.available_width());
    let (visible, hidden): (Vec<_>, Vec<_>) = groups
        .into_iter()
        .enumerate()
        .partition(|(index, _)| plan.shown[*index]);

    ui.horizontal(|ui| {
        ui.set_min_height(TILE_HEIGHT + 18.0);
        // The plan charged every group `GROUP_SLOT_EXTRA` for its separator;
        // the row must spend exactly that.
        ui.spacing_mut().item_spacing.x = layout::GROUP_GAP;
        for (index, (group, _, commands)) in visible {
            ribbon_group(
                app,
                clipboard,
                ui,
                group,
                commands,
                plan.scales[index],
                &measure,
            );
            ui.add(egui::Separator::default().spacing(layout::SEPARATOR_WIDTH));
        }
        if !hidden.is_empty() {
            ui.menu_button(more_label(), |ui| {
                for (_, (group, _, entries)) in hidden {
                    ui.label(crate::typography::headline(group));
                    for command in entries {
                        overflow_item(app, clipboard, ui, command.id);
                    }
                    ui.separator();
                }
            })
            .response
            .on_hover_text("Commands moved here to keep targets readable at this width");
        }
    });
}

/// The scale of every group of the tab within `width`, with the More menu's
/// reservation as the last resort. Group widths include their separator.
fn tab_plan(
    ui: &Ui,
    groups: &[(&'static str, u8, Vec<&CommandDescriptor>)],
    measure: layout::Measure,
    width: f32,
) -> TabPlan {
    let priorities: Vec<u8> = groups.iter().map(|(_, priority, _)| *priority).collect();
    let widths: Vec<[f32; 4]> = groups
        .iter()
        .map(|(title, _, entries)| {
            GroupScale::ALL.map(|scale| {
                layout::group_width(title, entries, scale, measure) + layout::GROUP_SLOT_EXTRA
            })
        })
        .collect();
    layout::plan(&priorities, &widths, width, more_button_width(ui, measure))
}

fn more_label() -> String {
    format!("{} More", icon::DOTS_THREE)
}

/// Measured reservation for the More overflow button, so the budget tracks the
/// live fonts instead of a fixed guess.
fn more_button_width(ui: &Ui, measure: layout::Measure) -> f32 {
    let font = egui::TextStyle::Button.resolve(ui.style());
    measure(&more_label(), font) + ui.spacing().button_padding.x * 2.0 + ui.spacing().item_spacing.x
}

fn ribbon_group(
    app: &mut PlotxApp,
    clipboard: &mut ClipboardTablePaste,
    ui: &mut Ui,
    title: &str,
    entries: Vec<&CommandDescriptor>,
    scale: GroupScale,
    measure: layout::Measure,
) {
    if scale == GroupScale::Collapsed {
        collapsed_group(app, clipboard, ui, title, entries, measure);
        return;
    }
    let width = layout::group_width(title, &entries, scale, measure);
    let columns = layout::columns(&entries, scale, measure);
    let spacing = layout::column_spacing(scale);
    let content_width = columns.iter().map(|column| column.width).sum::<f32>()
        + spacing * columns.len().saturating_sub(1) as f32;
    // The group's rect comes straight from the measured plan, and every
    // column is placed at its planned offset inside it, so the pixels and
    // the width budget agree by construction: content can never grow the
    // group, and a stack of two rows never grows the row.
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, TILE_HEIGHT + 16.0), Sense::hover());
    let mut x = rect.left() + ((width - content_width) / 2.0).max(0.0);
    for column in columns {
        let column_rect = egui::Rect::from_min_size(
            egui::pos2(x, rect.top()),
            Vec2::new(column.width, TILE_HEIGHT),
        );
        let mut column_ui = ui.new_child(
            UiBuilder::new()
                .max_rect(column_rect)
                .layout(Layout::top_down(Align::Min)),
        );
        column_ui.spacing_mut().item_spacing.y = STACK_GAP;
        // Two stacked rows must fit the tile height: 2 × 22 + 2. The default
        // vertical padding would push a 12 pt row past 22.
        column_ui.spacing_mut().button_padding.y = 2.0;
        for cell in column.cells {
            ribbon_button(
                app,
                clipboard,
                &mut column_ui,
                cell.command,
                cell.scale,
                column.width,
            );
        }
        x += column.width + spacing;
    }
    let caption_rect = egui::Rect::from_min_size(
        egui::pos2(rect.left(), rect.top() + TILE_HEIGHT + 1.0),
        Vec2::new(width, rect.height() - TILE_HEIGHT - 1.0),
    );
    ui.new_child(
        UiBuilder::new()
            .max_rect(caption_rect)
            .layout(Layout::top_down(Align::Center)),
    )
    .add(
        Label::new(crate::typography::caption(title).color(ui.visuals().weak_text_color()))
            .wrap_mode(TextWrapMode::Extend),
    );
}

/// A Collapsed group: one tile in the group's place that opens the group's
/// Large layout in a popover, so a narrow window keeps every group where the
/// user learned to find it. Clicking a command closes the popover.
fn collapsed_group(
    app: &mut PlotxApp,
    clipboard: &mut ClipboardTablePaste,
    ui: &mut Ui,
    title: &str,
    entries: Vec<&CommandDescriptor>,
    measure: layout::Measure,
) {
    let width = layout::collapsed_width(title, measure);
    let icon = entries.iter().find_map(|command| command.icon);
    let popup_id = ui.id().with(("collapsed_group", title));
    let open = egui::Popup::is_id_open(ui.ctx(), popup_id);
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, TILE_HEIGHT + 16.0), Sense::hover());
    let mut tile_ui = ui.new_child(
        UiBuilder::new()
            .max_rect(egui::Rect::from_min_size(
                rect.min,
                Vec2::new(width, TILE_HEIGHT),
            ))
            .layout(Layout::top_down(Align::Min)),
    );
    let response = collapsed_tile(&mut tile_ui, title, icon, width, open);
    egui::Popup::menu(&response).id(popup_id).show(|ui| {
        ui.horizontal(|ui| {
            ribbon_group(
                app,
                clipboard,
                ui,
                title,
                entries,
                GroupScale::Large,
                measure,
            );
        });
    });
}

fn update_button(app: &mut PlotxApp, ui: &mut Ui, compact: bool) {
    use plotx_core::update::UpdateStatus;
    match app.session.updates.status().clone() {
        UpdateStatus::Downloading { percent, .. } => {
            let text = if compact {
                percent.map_or_else(|| icon::ARROW_CLOCKWISE.to_owned(), |p| format!("{p}%"))
            } else {
                percent.map_or_else(|| "Updating…".to_owned(), |p| format!("Updating… {p}%"))
            };
            ui.label(
                RichText::new(text)
                    .small()
                    .color(ui.visuals().weak_text_color()),
            );
        }
        UpdateStatus::Installed { version, .. }
            if ui
                .button(if compact {
                    icon::ARROW_CLOCKWISE.to_owned()
                } else {
                    format!("{} Restart to update", icon::ARROW_CLOCKWISE)
                })
                .on_hover_text(format!(
                    "PlotX {version} is installed and ready after restart"
                ))
                .clicked() =>
        {
            crate::request_relaunch();
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn task_tab_rects(
        ctx: &egui::Context,
        selected: WorkflowTab,
        _command_density: RibbonDensity,
    ) -> [egui::Rect; 6] {
        let mut rects = None;
        let _ = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    vec2(900.0, 120.0),
                )),
                ..Default::default()
            },
            |ui| {
                rects = Some(
                    ui.horizontal(|ui| {
                        task_tab_buttons(ui, selected).map(|(_, response)| response.rect)
                    })
                    .inner,
                );
            },
        );
        rects.expect("the task tab row was rendered")
    }

    #[test]
    fn task_tab_geometry_is_stable_across_selection_and_command_density() {
        let ctx = crate::typography::test_context();
        let expected = task_tab_rects(&ctx, WorkflowTab::Data, RibbonDensity::Full);

        for selected in WorkflowTab::ALL {
            for density in [RibbonDensity::Full, RibbonDensity::Scaled] {
                let actual = task_tab_rects(&ctx, selected, density);
                for (tab, (expected, actual)) in WorkflowTab::ALL
                    .into_iter()
                    .zip(expected.into_iter().zip(actual))
                {
                    assert_eq!(actual.left(), expected.left(), "{tab:?} x at {density:?}");
                    assert_eq!(
                        actual.width(),
                        expected.width(),
                        "{tab:?} width at {density:?}"
                    );
                }
            }
        }
    }
}
