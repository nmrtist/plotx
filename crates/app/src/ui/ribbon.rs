//! Task-oriented, collapsible command Ribbon. Its visual vocabulary stays close
//! to PlotX's existing light egui chrome; the task/group hierarchy is the only
//! idea borrowed from the supplied Office reference.

use egui::text::LayoutJob;
use egui::{Align2, Button, Color32, FontId, Label, RichText, TextFormat, Ui, Vec2};
use egui_phosphor::regular as icon;
use plotx_core::actions::ZOrder;
use plotx_core::export::ExportFormat;
use plotx_core::state::{PlotxApp, Tool, ToolGroup, WorkflowTab};

use super::clipboard_table::ClipboardTablePaste;
use super::commands::{self, CommandDescriptor, CommandId};

const AUTO_COLLAPSE_WIDTH: f32 = 760.0;
/// One shared tile height (Full density) and row height (Compact) keeps every
/// command in a group visually equal-sized.
const TILE_HEIGHT: f32 = 46.0;
const ROW_HEIGHT: f32 = 26.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RibbonDensity {
    Collapsed,
    Compact,
    Full,
}

pub(crate) fn render(app: &mut PlotxApp, clipboard: &mut ClipboardTablePaste, ui: &mut Ui) {
    let width = ui.available_width();
    // Density is content-aware: measured against the active tab's groups, not a
    // fixed window-width breakpoint (which UI scaling would silently retune).
    // Measured before `task_row`, so a tab click adopts the new tab's density
    // one frame later — invisible in practice.
    let density = {
        let catalog = commands::catalog(app);
        let groups = groups_for_tab(&catalog, app.session.ui.ribbon_tab);
        density(width, app.session.ui.ribbon_expanded, &groups)
    };
    task_row(app, clipboard, ui, density);
    if density != RibbonDensity::Collapsed {
        ui.separator();
        command_row(app, clipboard, ui, density);
    }
    ui.separator();
    context_summary(app, ui);
}

fn task_row(
    app: &mut PlotxApp,
    clipboard: &mut ClipboardTablePaste,
    ui: &mut Ui,
    density: RibbonDensity,
) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = if density == RibbonDensity::Full {
            8.0
        } else {
            3.0
        };
        for tab in WorkflowTab::ALL {
            let selected = app.session.ui.ribbon_tab == tab;
            let response = ui.selectable_label(selected, RichText::new(tab.label()).strong());
            if response.clicked() {
                select_workflow_tab(app, tab);
                // Picking a task re-opens a manually collapsed command area;
                // width-driven auto-collapse still wins in `density()`.
                app.session.ui.ribbon_expanded = true;
            }
        }

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let auto_collapsed =
                density == RibbonDensity::Collapsed && app.session.ui.ribbon_expanded;
            let collapse_label = if auto_collapsed {
                format!("{} Ribbon auto-collapsed", icon::CARET_DOWN)
            } else if app.session.ui.ribbon_expanded {
                format!("{} Collapse ribbon", icon::CARET_UP)
            } else {
                format!("{} Expand ribbon", icon::CARET_DOWN)
            };
            // The strip next to the task tabs stays quiet: chrome buttons show
            // their frame only on hover so they read no heavier than the tabs.
            let collapse = ui.add_enabled(
                !auto_collapsed,
                Button::new(collapse_label).frame_when_inactive(false),
            );
            let collapse = if auto_collapsed {
                collapse.on_disabled_hover_text(
                    "The ribbon collapses automatically at this width; use menus or Search commands",
                )
            } else {
                collapse.on_hover_text("Collapse or expand the ribbon command area")
            };
            if collapse.clicked() {
                app.session.ui.ribbon_expanded = !app.session.ui.ribbon_expanded;
            }
            update_button(app, ui);
            let palette = commands::describe(app, CommandId::CommandPalette);
            if ui
                .add(
                    Button::new(format!("{} Search commands", icon::MAGNIFYING_GLASS))
                        .frame_when_inactive(false),
                )
                .on_hover_text(format!(
                    "Search every command ({})",
                    palette.shortcut.as_deref().unwrap_or("Ctrl+K")
                ))
                .clicked()
            {
                commands::execute(CommandId::CommandPalette, app, clipboard, ui.ctx());
            }
        });
    });
}

fn select_workflow_tab(app: &mut PlotxApp, tab: WorkflowTab) {
    app.session.ui.ribbon_tab = tab;
    match tab {
        WorkflowTab::Data => {}
        WorkflowTab::Process => reveal_context(app, ToolGroup::Processing),
        WorkflowTab::Analyze => {
            if let Some(dataset) = app.active_dataset().and_then(|di| app.doc.datasets.get(di)) {
                let groups = dataset.tool_groups();
                app.session.ui.requested_tool_group = [
                    ToolGroup::Nmr1dAnalysis,
                    ToolGroup::Nmr2dExperiment,
                    ToolGroup::RegionAnalysis,
                    ToolGroup::CurveFit,
                    ToolGroup::LineFit,
                    ToolGroup::Peaks,
                ]
                .into_iter()
                .find(|group| groups.contains(group));
            }
        }
        WorkflowTab::View | WorkflowTab::Figure | WorkflowTab::Arrange => {}
    }
}

fn reveal_context(app: &mut PlotxApp, group: ToolGroup) {
    app.session.ui.requested_tool_group = Some(group);
}

fn command_row(
    app: &mut PlotxApp,
    clipboard: &mut ClipboardTablePaste,
    ui: &mut Ui,
    density: RibbonDensity,
) {
    let tab = app.session.ui.ribbon_tab;
    let catalog = commands::catalog(app);
    let groups = groups_for_tab(&catalog, tab);
    if density == RibbonDensity::Collapsed {
        return;
    }
    let mut ranked: Vec<usize> = groups.iter().enumerate().map(|(index, _)| index).collect();
    ranked.sort_by_key(|&index| groups[index].1);
    let required = required_width(&groups, density);
    let available = ui.available_width();
    let budget = if required <= available {
        available
    } else {
        (available - 86.0).max(0.0)
    };
    let mut used = 0.0;
    let mut shown = vec![false; groups.len()];
    for index in ranked {
        let width = group_width(&groups[index].2, density) + 8.0;
        if used + width <= budget {
            shown[index] = true;
            used += width;
        }
    }
    let (visible, hidden): (Vec<_>, Vec<_>) = groups
        .into_iter()
        .enumerate()
        .partition(|(index, _)| shown[*index]);

    ui.horizontal(|ui| {
        ui.set_min_height(if density == RibbonDensity::Full {
            TILE_HEIGHT + 18.0
        } else {
            ROW_HEIGHT + 18.0
        });
        ui.spacing_mut().item_spacing.x = if density == RibbonDensity::Full {
            7.0
        } else {
            3.0
        };
        for (_, (group, _, commands)) in visible {
            ribbon_group(app, clipboard, ui, group, commands, density);
            ui.separator();
        }
        if !hidden.is_empty() {
            ui.menu_button(format!("{} More", icon::DOTS_THREE), |ui| {
                for (_, (group, _, entries)) in hidden {
                    ui.strong(group);
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

fn ribbon_group(
    app: &mut PlotxApp,
    clipboard: &mut ClipboardTablePaste,
    ui: &mut Ui,
    title: &str,
    entries: Vec<&CommandDescriptor>,
    density: RibbonDensity,
) {
    let width = group_width(&entries, density);
    let tile = tile_width(&entries);
    ui.allocate_ui_with_layout(
        Vec2::new(
            width,
            if density == RibbonDensity::Full {
                TILE_HEIGHT + 16.0
            } else {
                ROW_HEIGHT + 16.0
            },
        ),
        egui::Layout::top_down(egui::Align::Center),
        |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = if density == RibbonDensity::Full {
                    4.0
                } else {
                    2.0
                };
                for command in entries {
                    ribbon_button(app, clipboard, ui, command, density, tile);
                }
            });
            ui.add_space(1.0);
            ui.label(
                RichText::new(title)
                    .small()
                    .color(ui.visuals().weak_text_color()),
            );
        },
    );
}

/// Width the whole tab needs at `density`: every group plus its separator.
/// The same estimate drives the density choice and the overflow budget, so a
/// tab shown Full is guaranteed to fit without a More menu.
fn required_width(
    groups: &[(&'static str, u8, Vec<&CommandDescriptor>)],
    density: RibbonDensity,
) -> f32 {
    groups
        .iter()
        .map(|(_, _, entries)| group_width(entries, density) + 8.0)
        .sum()
}

fn group_width(entries: &[&CommandDescriptor], density: RibbonDensity) -> f32 {
    let spacing = 4.0 * entries.len().saturating_sub(1) as f32;
    if density == RibbonDensity::Full {
        tile_width(entries) * entries.len() as f32 + spacing
    } else {
        entries
            .iter()
            .map(|command| button_width(command))
            .sum::<f32>()
            + spacing
    }
}

/// All tiles in a group share the width of the widest short label, so a group
/// reads as one row of even targets instead of a ragged strip.
fn tile_width(entries: &[&CommandDescriptor]) -> f32 {
    entries
        .iter()
        .map(|command| short_label(command).chars().count() as f32 * 5.8 + 18.0)
        .fold(58.0, f32::max)
        .min(112.0)
}

fn button_width(command: &CommandDescriptor) -> f32 {
    if command.icon.is_some() {
        ROW_HEIGHT
    } else {
        (short_label(command).chars().count() as f32 * 6.2 + 16.0).clamp(40.0, 140.0)
    }
}

/// Ribbon buttons carry short verb labels; the full command name and shortcut
/// stay in the tooltip, menus and the command palette.
fn short_label(command: &CommandDescriptor) -> String {
    match command.id {
        CommandId::NewCanvas(index) => match index {
            0 => "Slides",
            1 => "1 Column",
            2 => "2 Columns",
            3 => "Poster",
            _ => "Canvas",
        }
        .to_owned(),
        CommandId::ChartType => "Chart".to_owned(),
        CommandId::ApplyTheme(id) => match id {
            "publication" => "Paper",
            "presentation_dark" => "Dark",
            "vibrant" => "Vibrant",
            _ => "Theme",
        }
        .to_owned(),
        CommandId::CopyFigure => "Copy".to_owned(),
        CommandId::Export(format) => match format {
            ExportFormat::Png => "PNG",
            ExportFormat::Svg => "SVG",
            _ => format.label(),
        }
        .to_owned(),
        CommandId::ImportTable => "Import Table".to_owned(),
        CommandId::PasteTable => "Paste Table".to_owned(),
        CommandId::NewTable => "New Table".to_owned(),
        CommandId::StackData => "Stack Data".to_owned(),
        CommandId::SaveProcessingTemplate => "Save Template".to_owned(),
        CommandId::ApplyProcessingTemplate => "Apply Template".to_owned(),
        CommandId::SpectrumArithmetic => "Arithmetic".to_owned(),
        CommandId::AlignSpectra => "Align Spectra".to_owned(),
        CommandId::TidyBoard => "Tidy Frames".to_owned(),
        CommandId::ToggleSnap => "Snapping".to_owned(),
        CommandId::TogglePrimarySidebar => "Left Bar".to_owned(),
        CommandId::ToggleSecondarySidebar => "Right Bar".to_owned(),
        CommandId::ArrangeGrid(rows, cols) => format!("Plots {rows} × {cols}"),
        CommandId::ZOrder(mode) => match mode {
            ZOrder::Front => "To Front",
            ZOrder::Forward => "Forward",
            ZOrder::Backward => "Backward",
            ZOrder::Back => "To Back",
        }
        .to_owned(),
        CommandId::Align(_) => command.label.trim_start_matches("Align ").to_owned(),
        CommandId::Distribute(_) => command.label.trim_start_matches("Distribute ").to_owned(),
        CommandId::Tool(Tool::BrowseZoom) => "Zoom".to_owned(),
        CommandId::Tool(_) => command.label.trim_start_matches("Tool: ").to_owned(),
        _ => command.label.clone(),
    }
}

fn ribbon_button(
    app: &mut PlotxApp,
    clipboard: &mut ClipboardTablePaste,
    ui: &mut Ui,
    command: &CommandDescriptor,
    density: RibbonDensity,
    tile: f32,
) {
    let label = short_label(command);
    // Icons carry the accent colour; label text keeps the theme colour via the
    // placeholder, which also inherits the correct disabled/selected colours.
    let icon_color = if command.enabled && command.checked != Some(true) {
        ui.visuals().hyperlink_color
    } else {
        Color32::PLACEHOLDER
    };
    let mut job = LayoutJob::default();
    let response = if density == RibbonDensity::Full {
        let icon_font = FontId::proportional(16.0);
        let label_font = FontId::proportional(11.0);
        let selected = command.checked == Some(true);
        // Keep the command name in the button for accessibility, but paint the
        // two visible rows ourselves so both share the tile's exact centre.
        // LayoutJob's per-row offsets otherwise make differently sized glyphs
        // appear alternately left- and right-aligned.
        let button = Button::selectable(
            selected,
            RichText::new(&label).size(1.0).color(Color32::TRANSPARENT),
        )
        .min_size(Vec2::new(tile, TILE_HEIGHT));
        let response = ui.add_enabled(command.enabled, button);
        let text_color = ui
            .style()
            .button_style(response.widget_state(), selected)
            .text_style
            .color;
        let center = response.rect.center();
        if let Some(icon) = command.icon {
            ui.painter().text(
                center - Vec2::new(0.0, 7.5),
                Align2::CENTER_CENTER,
                icon,
                icon_font,
                if command.enabled && !selected {
                    icon_color
                } else {
                    text_color
                },
            );
            ui.painter().text(
                center + Vec2::new(0.0, 9.0),
                Align2::CENTER_CENTER,
                &label,
                label_font,
                text_color,
            );
        } else {
            ui.painter().text(
                center,
                Align2::CENTER_CENTER,
                &label,
                label_font,
                text_color,
            );
        }
        response
    } else {
        if let Some(icon) = command.icon {
            job.append(
                icon,
                0.0,
                TextFormat {
                    font_id: FontId::proportional(14.0),
                    color: icon_color,
                    ..Default::default()
                },
            );
        } else {
            job.append(
                &label,
                0.0,
                TextFormat {
                    font_id: FontId::proportional(12.0),
                    color: Color32::PLACEHOLDER,
                    ..Default::default()
                },
            );
        }
        let button = Button::selectable(command.checked == Some(true), job)
            .min_size(Vec2::new(button_width(command), ROW_HEIGHT));
        ui.add_enabled(command.enabled, button)
    };
    let tip = match &command.shortcut {
        Some(shortcut) => format!("{} ({shortcut})", command.label),
        None => command.label.clone(),
    };
    let clicked = response.clicked();
    if command.enabled {
        response.on_hover_text(tip);
    } else {
        let reason = command
            .disabled_reason
            .unwrap_or("Unavailable in the current context.");
        response.on_disabled_hover_text(format!("{tip} · {reason}"));
    }
    if clicked {
        commands::execute(command.id, app, clipboard, ui.ctx());
    }
}

fn overflow_item(
    app: &mut PlotxApp,
    clipboard: &mut ClipboardTablePaste,
    ui: &mut Ui,
    id: CommandId,
) {
    let command = commands::describe(app, id);
    let mut button = Button::new(&command.label).selected(command.checked == Some(true));
    if let Some(shortcut) = &command.shortcut {
        button = button.shortcut_text(shortcut);
    }
    let response = ui.add_enabled(command.enabled, button);
    let clicked = response.clicked();
    if !command.enabled
        && let Some(reason) = command.disabled_reason
    {
        response.on_disabled_hover_text(reason);
    }
    if clicked {
        commands::execute(id, app, clipboard, ui.ctx());
        ui.close();
    }
}

fn groups_for_tab(
    catalog: &[CommandDescriptor],
    tab: WorkflowTab,
) -> Vec<(&'static str, u8, Vec<&CommandDescriptor>)> {
    let mut groups: Vec<(&'static str, u8, Vec<&CommandDescriptor>)> = Vec::new();
    for command in catalog {
        let Some(placement) = command.ribbon.filter(|placement| placement.tab == tab) else {
            continue;
        };
        if let Some((_, priority, entries)) = groups
            .iter_mut()
            .find(|(group, _, _)| *group == placement.group)
        {
            *priority = (*priority).min(placement.priority);
            entries.push(command);
        } else {
            groups.push((placement.group, placement.priority, vec![command]));
        }
    }
    groups.sort_by_key(|(group, _, _)| group_order(tab, group));
    groups
}

fn group_order(tab: WorkflowTab, group: &str) -> u8 {
    match (tab, group) {
        (WorkflowTab::View, "Navigate")
        | (WorkflowTab::Data, "Import")
        | (WorkflowTab::Process, "Correct")
        | (WorkflowTab::Analyze, "Range")
        | (WorkflowTab::Figure, "Create")
        | (WorkflowTab::Arrange, "Layout") => 0,
        (WorkflowTab::Analyze, "Regions") => 1,
        (WorkflowTab::View, "Display")
        | (WorkflowTab::Data, "Build")
        | (WorkflowTab::Process, "Transform")
        | (WorkflowTab::Figure, "Chart")
        | (WorkflowTab::Arrange, "Align") => 1,
        (WorkflowTab::Figure, "Style") => 2,
        (WorkflowTab::Figure, "Output") => 3,
        (WorkflowTab::Analyze, "Peaks") => 2,
        (WorkflowTab::Data, "Inspect")
        | (WorkflowTab::Process, "Recipes")
        | (WorkflowTab::Arrange, "Distribute") => 2,
        (WorkflowTab::Analyze, "Peak Fit") | (WorkflowTab::Arrange, "Order") => 3,
        (WorkflowTab::Analyze, "Curve Fit") => 4,
        (WorkflowTab::Analyze, "Interpret") => 5,
        (WorkflowTab::Arrange, "Guides") => 4,
        (WorkflowTab::Arrange, "Annotate") => 5,
        _ => u8::MAX,
    }
}

fn context_summary(app: &PlotxApp, ui: &mut Ui) {
    let task = app.session.ui.ribbon_tab.label();
    let tool = app.session.tool.label();
    ui.horizontal(|ui| {
        let summary = active_context(app);
        let reserve = 150.0_f32.min(ui.available_width() * 0.4);
        ui.add_sized(
            [ui.available_width() - reserve, ui.spacing().interact_size.y],
            Label::new(
                RichText::new(summary)
                    .small()
                    .color(ui.visuals().weak_text_color()),
            )
            .truncate(),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(RichText::new(format!("{task} / {tool}")).small().strong());
        });
    });
}

fn active_context(app: &PlotxApp) -> String {
    let Some(ci) = app
        .session
        .active_canvas
        .filter(|&ci| ci < app.doc.canvases.len())
    else {
        return "Canvas — no active canvas".to_owned();
    };
    let canvas = &app.doc.canvases[ci];
    let object_id = app
        .session
        .ui
        .selection
        .object()
        .or_else(|| canvas.active_plot_object_id());
    let object = object_id
        .and_then(|id| canvas.object(id))
        .map(|object| object.name.as_str())
        .unwrap_or("No object");
    let data = app
        .active_dataset()
        .filter(|&di| di < app.doc.datasets.len())
        .map(|di| app.doc.datasets[di].display_name())
        .unwrap_or_else(|| "no data".to_owned());
    format!("{} › {object} · {data}", canvas.name)
}

/// The richest density whose content actually fits `width`: full icon-and-text
/// tiles whenever the active tab's groups all fit, otherwise the compact icon
/// row (whose own overflow moves whole groups into More). Below the absolute
/// floor even icon rows crowd, so the command area collapses to menus.
fn density(
    width: f32,
    expanded: bool,
    groups: &[(&'static str, u8, Vec<&CommandDescriptor>)],
) -> RibbonDensity {
    if !expanded || width < AUTO_COLLAPSE_WIDTH {
        RibbonDensity::Collapsed
    } else if required_width(groups, RibbonDensity::Full) <= width {
        RibbonDensity::Full
    } else {
        RibbonDensity::Compact
    }
}

fn update_button(app: &mut PlotxApp, ui: &mut Ui) {
    use plotx_core::update::UpdateStatus;
    match app.session.updates.status().clone() {
        UpdateStatus::Downloading { percent, .. } => {
            let text =
                percent.map_or_else(|| "Updating…".to_owned(), |p| format!("Updating… {p}%"));
            ui.label(
                RichText::new(text)
                    .small()
                    .color(ui.visuals().weak_text_color()),
            );
        }
        UpdateStatus::Installed { version, .. }
            if ui
                .button(format!("{} Restart to update", icon::ARROW_CLOCKWISE))
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

    #[test]
    fn density_follows_the_active_tabs_measured_content() {
        let app = PlotxApp::new_with_settings(plotx_core::settings::Settings::default());
        let catalog = commands::catalog(&app);
        let groups = groups_for_tab(&catalog, WorkflowTab::View);
        let full_need = required_width(&groups, RibbonDensity::Full);
        assert!(
            full_need > AUTO_COLLAPSE_WIDTH,
            "test premise: the View tab's full-density content ({full_need}) must exceed the collapse floor"
        );

        // Full the moment the tab's content fits — no fixed window breakpoint.
        assert_eq!(density(full_need + 1.0, true, &groups), RibbonDensity::Full);
        assert_eq!(
            density(full_need - 1.0, true, &groups),
            RibbonDensity::Compact
        );
        assert_eq!(density(700.0, true, &groups), RibbonDensity::Collapsed);
        assert_eq!(
            density(full_need + 1.0, false, &groups),
            RibbonDensity::Collapsed
        );
    }

    #[test]
    fn figure_tiles_use_short_labels() {
        let app = PlotxApp::new_with_settings(plotx_core::settings::Settings::default());
        let cases = [
            (CommandId::NewCanvas(0), "Slides"),
            (CommandId::NewCanvas(1), "1 Column"),
            (CommandId::NewCanvas(2), "2 Columns"),
            (CommandId::NewCanvas(3), "Poster"),
            (CommandId::ChartType, "Chart"),
            (CommandId::ApplyTheme("publication"), "Paper"),
            (CommandId::ApplyTheme("presentation_dark"), "Dark"),
            (CommandId::ApplyTheme("vibrant"), "Vibrant"),
            (CommandId::CopyFigure, "Copy"),
            (CommandId::Export(ExportFormat::Png), "PNG"),
            (CommandId::Export(ExportFormat::Svg), "SVG"),
        ];
        for (id, expected) in cases {
            assert_eq!(short_label(&commands::describe(&app, id)), expected);
        }
    }
}
