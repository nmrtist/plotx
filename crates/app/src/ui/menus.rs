use egui::{RichText, Ui};
use plotx_core::export::ExportFormat;
use plotx_core::state::{PlotxApp, Tool};

#[cfg(not(target_os = "macos"))]
use super::clipboard_table::ClipboardTablePaste;
use super::commands::{self, CommandId};

/// One entry in the shared menu-bar structure.
pub(crate) enum MenuEntry {
    Command(CommandId),
    Submenu(&'static str, Vec<MenuEntry>),
    Separator,
    /// Placeholder for the live recent-files list: each renderer expands it to
    /// one `OpenRecent` item per current entry (the list length changes at
    /// runtime, so the spec cannot enumerate fixed commands here).
    RecentFiles,
}

/// The menu-bar structure shared by every platform. The in-window egui bar
/// below renders it verbatim; the macOS system menu (`native_menu`) renders
/// the same tree, relocating only the commands AppKit hosts in the application
/// menu (Preferences, Quit, About). One tree means the surfaces cannot drift.
pub(crate) fn menu_bar_spec() -> Vec<(&'static str, Vec<MenuEntry>)> {
    use MenuEntry::{Command, Separator, Submenu};
    let mut export: Vec<MenuEntry> = [
        ExportFormat::Svg,
        ExportFormat::Pdf,
        ExportFormat::Png,
        ExportFormat::Jpeg,
        ExportFormat::Tiff,
    ]
    .into_iter()
    .map(|format| Command(CommandId::Export(format)))
    .collect();
    export.push(Separator);
    export.push(Command(CommandId::CopyFigure));
    let templates = plotx_core::templates::CanvasTemplate::all()
        .iter()
        .enumerate()
        .map(|(index, _)| Command(CommandId::NewCanvas(index)))
        .collect();
    let themes = plotx_core::theme::Theme::all()
        .into_iter()
        .map(|theme| Command(CommandId::ApplyTheme(theme.id)))
        .collect();
    vec![
        (
            "File",
            vec![
                Command(CommandId::OpenProject),
                Command(CommandId::OpenFile),
                Command(CommandId::OpenFolder),
                Command(CommandId::RunBatchWorkflow),
                Submenu(
                    "Open Recent",
                    vec![
                        MenuEntry::RecentFiles,
                        Separator,
                        Command(CommandId::ClearRecentFiles),
                    ],
                ),
                Separator,
                Command(CommandId::ImportTable),
                Command(CommandId::PasteTable),
                Separator,
                Command(CommandId::SaveProject),
                Command(CommandId::ExportData),
                Submenu("Export", export),
                Separator,
                Command(CommandId::Quit),
            ],
        ),
        (
            "Edit",
            vec![
                Command(CommandId::Undo),
                Command(CommandId::Redo),
                Separator,
                Command(CommandId::SelectAll),
                Command(CommandId::Group),
                Command(CommandId::Ungroup),
                Separator,
                Command(CommandId::Preferences),
            ],
        ),
        (
            "View",
            vec![
                Command(CommandId::TogglePrimarySidebar),
                Command(CommandId::ToggleSecondarySidebar),
                Separator,
                Command(CommandId::ZoomToFit),
                Command(CommandId::ZoomToSelection),
                Command(CommandId::Present),
                Separator,
                Command(CommandId::UiScaleUp),
                Command(CommandId::UiScaleDown),
                Command(CommandId::UiScaleReset),
                Separator,
                Command(CommandId::ToggleGrid),
                Submenu("Canvas Theme", themes),
            ],
        ),
        (
            "Insert",
            vec![
                Submenu("New Canvas from Template", templates),
                Command(CommandId::NewTable),
                Separator,
                Command(CommandId::Tool(Tool::Text)),
                Command(CommandId::Tool(Tool::PanelLabel)),
                Command(CommandId::Tool(Tool::Rect)),
                Command(CommandId::Tool(Tool::Ellipse)),
                Command(CommandId::Tool(Tool::Line)),
                Command(CommandId::Tool(Tool::Arrow)),
            ],
        ),
        (
            "Help",
            vec![
                Command(CommandId::HelpManual),
                Separator,
                Command(CommandId::CommandPalette),
                Command(CommandId::OperationHistory),
                Separator,
                Command(CommandId::CheckUpdates),
                Command(CommandId::About),
            ],
        ),
    ]
}

/// Windows and Linux render this traditional menu inside the custom title bar
/// (`title_bar`). macOS installs the equivalent command structure in the
/// system menu bar and deliberately omits this row from the content area.
///
/// Returns the right edge of the last menu button, so the title bar can lay
/// out around the menus (the `MenuBar` strip itself stretches to full width).
#[cfg(not(target_os = "macos"))]
pub(crate) fn menu_bar(
    app: &mut PlotxApp,
    clipboard: &mut ClipboardTablePaste,
    ui: &mut Ui,
) -> f32 {
    egui::MenuBar::new()
        .ui(ui, |ui| {
            for (title, entries) in menu_bar_spec() {
                ui.menu_button(title, |ui| menu_entries(app, clipboard, ui, &entries));
            }
            ui.cursor().min.x
        })
        .inner
}

#[cfg(not(target_os = "macos"))]
fn menu_entries(
    app: &mut PlotxApp,
    clipboard: &mut ClipboardTablePaste,
    ui: &mut Ui,
    entries: &[MenuEntry],
) {
    for entry in entries {
        match entry {
            MenuEntry::Command(id) => command_item(app, clipboard, ui, *id),
            MenuEntry::Separator => {
                ui.separator();
            }
            MenuEntry::Submenu(title, children) => {
                ui.menu_button(*title, |ui| menu_entries(app, clipboard, ui, children));
            }
            MenuEntry::RecentFiles => {
                let count = app.session.recent_files.len();
                if count == 0 {
                    ui.weak("No recent files yet.");
                }
                for index in 0..count {
                    // The parent submenu already reads "Open Recent", so the row
                    // shows the bare entry name; the palette keeps the prefix.
                    let label = commands::recent_entry_label(app, index);
                    command_item_labeled(
                        app,
                        clipboard,
                        ui,
                        CommandId::OpenRecent(index),
                        label.as_deref(),
                    );
                }
            }
        }
    }
}

#[cfg(not(target_os = "macos"))]
fn command_item(
    app: &mut PlotxApp,
    clipboard: &mut ClipboardTablePaste,
    ui: &mut Ui,
    id: CommandId,
) {
    command_item_labeled(app, clipboard, ui, id, None);
}

/// Renders one menu command. `label_override` swaps only the visible text —
/// enabled state, shortcut, and execution still come from the catalog — so a
/// submenu can drop a redundant prefix without forking command behavior.
#[cfg(not(target_os = "macos"))]
fn command_item_labeled(
    app: &mut PlotxApp,
    clipboard: &mut ClipboardTablePaste,
    ui: &mut Ui,
    id: CommandId,
    label_override: Option<&str>,
) {
    let command = commands::describe(app, id);
    let label = label_override.unwrap_or(command.label.as_str());
    let mut button = egui::Button::new(label).selected(command.checked == Some(true));
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

pub(crate) fn about_window(app: &mut PlotxApp, ctx: &egui::Context) {
    if !app.session.ui.about_open {
        return;
    }
    egui::Window::new("About PlotX")
        .collapsible(false)
        .resizable(false)
        .open(&mut app.session.ui.about_open)
        .show(ctx, |ui| {
            ui.label(RichText::new("PlotX").heading());
            ui.label(format!("Version {}", env!("CARGO_PKG_VERSION")));
            ui.add_space(6.0);
            ui.label("Scientific data analysis and figure preparation.");
            ui.label("GPL-3.0-or-later");
            ui.add_space(6.0);
            ui.hyperlink_to("User manual", commands::MANUAL_URL);
            ui.hyperlink_to("Source code", commands::REPOSITORY_URL);
        });
}

pub(crate) fn other_canvas_destinations(app: &PlotxApp, ci: usize) -> Vec<(usize, String)> {
    app.doc
        .canvases
        .iter()
        .enumerate()
        .filter(|(i, _)| *i != ci)
        .map(|(i, c)| (i, c.name.clone()))
        .collect()
}

/// Shared by the Layers list and the canvas context menu; the picked
/// `(destination, is_move)` is written to `out`.
pub(crate) fn transfer_to_canvas_menu(
    ui: &mut Ui,
    destinations: &[(usize, String)],
    move_label: &str,
    copy_label: &str,
    out: &mut Option<(usize, bool)>,
) {
    for (is_move, label) in [(true, move_label), (false, copy_label)] {
        ui.menu_button(label, |ui| {
            if destinations.is_empty() {
                ui.weak("No other canvas");
                return;
            }
            for (idx, name) in destinations {
                if ui.button(name).clicked() {
                    *out = Some((*idx, is_move));
                    ui.close();
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every command in the shared spec must describe cleanly; this guards the
    /// dynamically indexed entries (templates, themes, grid presets).
    #[test]
    fn menu_spec_commands_are_describable() {
        fn walk(app: &PlotxApp, entries: &[MenuEntry]) {
            for entry in entries {
                match entry {
                    MenuEntry::Command(id) => {
                        assert!(!commands::describe(app, *id).label.is_empty());
                        assert!(!id.stable_id().is_empty());
                    }
                    MenuEntry::Submenu(title, children) => {
                        assert!(!title.is_empty());
                        walk(app, children);
                    }
                    MenuEntry::Separator => {}
                    MenuEntry::RecentFiles => {
                        for index in 0..app.session.recent_files.len() {
                            let id = commands::CommandId::OpenRecent(index);
                            assert!(!commands::describe(app, id).label.is_empty());
                            assert!(!id.stable_id().is_empty());
                        }
                    }
                }
            }
        }
        let mut app = PlotxApp::new_with_settings(plotx_core::settings::Settings::default());
        app.session.recent_files = vec![
            std::path::PathBuf::from("C:/data/project.plotx"),
            std::path::PathBuf::from("C:/other/project.plotx"),
        ];
        for (title, entries) in menu_bar_spec() {
            assert!(!title.is_empty());
            walk(&app, &entries);
        }
    }
}
