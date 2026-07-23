use super::*;
use plotx_core::state::Tool;

/// A key chord expressed with egui types. `command` means Cmd on macOS and
/// Ctrl elsewhere; chords without `command` ignore Shift (tool keys) but never
/// match while Cmd/Ctrl/Alt are held.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Chord {
    pub command: bool,
    pub shift: bool,
    pub key: egui::Key,
}

/// The single source of truth for a command's key bindings. The display label
/// (`shortcut_label`), the egui dispatcher (`handle_command_shortcuts`) and the
/// macOS menu accelerators (`native_menu`) are all derived from this table, so
/// a rebinding is one edit that cannot drift between surfaces.
pub(super) struct CommandBinding {
    pub id: commands::CommandId,
    /// Shown in labels and registered as the macOS accelerator.
    pub primary: Chord,
    /// Extra chords accepted by the dispatcher only.
    pub aliases: &'static [Chord],
    /// Matched by `handle_command_shortcuts`. Off for bindings owned by a
    /// focused handler with special semantics (palette toggle, board fit).
    pub dispatch: bool,
    /// Registered as a macOS menu key equivalent. Only meaningful for
    /// `command` chords; off for chords the system and text fields must keep
    /// (e.g. Cmd+C is text copy first, figure copy only via the in-app path).
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub menu_accelerator: bool,
}

const MAC: bool = cfg!(target_os = "macos");

const fn cmd(key: egui::Key) -> Chord {
    Chord {
        command: true,
        shift: false,
        key,
    }
}
const fn cmd_shift(key: egui::Key) -> Chord {
    Chord {
        command: true,
        shift: true,
        key,
    }
}
const fn plain(key: egui::Key) -> Chord {
    Chord {
        command: false,
        shift: false,
        key,
    }
}
const fn bound(id: commands::CommandId, primary: Chord) -> CommandBinding {
    CommandBinding {
        id,
        primary,
        aliases: &[],
        dispatch: true,
        menu_accelerator: primary.command,
    }
}
const fn tool_key(tool: Tool, key: egui::Key) -> CommandBinding {
    CommandBinding {
        id: commands::CommandId::Tool(tool),
        primary: plain(key),
        aliases: &[],
        dispatch: true,
        menu_accelerator: false,
    }
}

static BINDINGS: &[CommandBinding] = &[
    bound(commands::CommandId::SaveProject, cmd(egui::Key::S)),
    bound(commands::CommandId::Undo, cmd(egui::Key::Z)),
    CommandBinding {
        id: commands::CommandId::Redo,
        // macOS convention is Cmd+Shift+Z; elsewhere Ctrl+Y is the shown
        // binding. Both chords are accepted everywhere.
        primary: if MAC {
            cmd_shift(egui::Key::Z)
        } else {
            cmd(egui::Key::Y)
        },
        aliases: if MAC {
            &[cmd(egui::Key::Y)]
        } else {
            &[cmd_shift(egui::Key::Z)]
        },
        dispatch: true,
        menu_accelerator: true,
    },
    CommandBinding {
        id: commands::CommandId::CopyFigure,
        primary: cmd(egui::Key::C),
        aliases: &[],
        dispatch: true,
        // Cmd+C must stay free for text-field copy; the in-app handler also
        // routes the platform Copy event to this command.
        menu_accelerator: false,
    },
    bound(commands::CommandId::SelectAll, cmd(egui::Key::A)),
    bound(commands::CommandId::Group, cmd(egui::Key::G)),
    bound(commands::CommandId::Ungroup, cmd_shift(egui::Key::G)),
    bound(commands::CommandId::Preferences, cmd(egui::Key::Comma)),
    CommandBinding {
        id: commands::CommandId::UiScaleUp,
        primary: cmd(egui::Key::Plus),
        // "+" often needs Shift (Ctrl+Shift+=), and some layouts report the
        // key as Equals; accept every spelling.
        aliases: &[
            cmd_shift(egui::Key::Plus),
            cmd(egui::Key::Equals),
            cmd_shift(egui::Key::Equals),
        ],
        dispatch: true,
        menu_accelerator: true,
    },
    CommandBinding {
        id: commands::CommandId::UiScaleDown,
        primary: cmd(egui::Key::Minus),
        aliases: &[cmd_shift(egui::Key::Minus)],
        dispatch: true,
        menu_accelerator: true,
    },
    bound(commands::CommandId::UiScaleReset, cmd(egui::Key::Num0)),
    CommandBinding {
        id: commands::CommandId::CommandPalette,
        primary: cmd(egui::Key::K),
        aliases: &[cmd_shift(egui::Key::P)],
        // Owned by `handle_palette_shortcut`, which must consume the chord
        // before the keyboard-focus gate so it can close an open palette.
        dispatch: false,
        menu_accelerator: true,
    },
    CommandBinding {
        id: commands::CommandId::ZoomToSelection,
        primary: plain(egui::Key::F),
        aliases: &[],
        // Owned by `handle_fit_shortcut`, which adds selection-aware status.
        dispatch: false,
        menu_accelerator: false,
    },
    tool_key(Tool::Select, egui::Key::V),
    tool_key(Tool::BrowseZoom, egui::Key::Z),
    tool_key(Tool::Text, egui::Key::T),
    tool_key(Tool::Rect, egui::Key::R),
    tool_key(Tool::Ellipse, egui::Key::O),
    tool_key(Tool::Line, egui::Key::L),
    tool_key(Tool::Integrate, egui::Key::I),
    tool_key(Tool::Peaks, egui::Key::P),
    tool_key(Tool::Slice, egui::Key::S),
    tool_key(Tool::LineFit, egui::Key::D),
];

pub(super) fn binding(id: commands::CommandId) -> Option<&'static CommandBinding> {
    BINDINGS.iter().find(|binding| binding.id == id)
}

/// The user-facing binding label for a command, derived from `BINDINGS`.
pub(super) fn shortcut_label(id: commands::CommandId) -> Option<String> {
    let chord = binding(id)?.primary;
    Some(if chord.command {
        format!(
            "{}{}+{}",
            if MAC { "Cmd" } else { "Ctrl" },
            if chord.shift { "+Shift" } else { "" },
            key_name(chord.key)
        )
    } else {
        key_name(chord.key).to_owned()
    })
}

fn key_name(key: egui::Key) -> &'static str {
    // egui names punctuation keys verbosely ("Comma"); labels use the glyph.
    match key {
        egui::Key::Comma => ",",
        egui::Key::Plus => "+",
        egui::Key::Minus => "-",
        egui::Key::Num0 => "0",
        _ => key.name(),
    }
}

fn chord_pressed(input: &egui::InputState, chord: Chord) -> bool {
    let command = input.modifiers.command || input.modifiers.ctrl;
    if chord.command {
        command && input.modifiers.shift == chord.shift && input.key_pressed(chord.key)
    } else {
        !command && !input.modifiers.alt && input.key_pressed(chord.key)
    }
}

/// Sole owner of the command-palette bindings. Runs before the modal and
/// keyboard-focus gates so the shortcut can toggle the palette closed while
/// its search field holds focus; `consume_key` keeps the event from reaching
/// any other reader in the same frame, and `execute` toggles open/close and
/// enforces the command's `enabled` rules.
pub(super) fn handle_palette_shortcut(
    app: &mut PlotxApp,
    clipboard: &mut clipboard_table::ClipboardTablePaste,
    ctx: &egui::Context,
) {
    let Some(binding) = binding(commands::CommandId::CommandPalette) else {
        return;
    };
    let pressed = ctx.input_mut(|input| {
        std::iter::once(binding.primary)
            .chain(binding.aliases.iter().copied())
            .any(|chord| {
                let modifiers = if chord.shift {
                    egui::Modifiers::COMMAND | egui::Modifiers::SHIFT
                } else {
                    egui::Modifiers::COMMAND
                };
                input.consume_key(modifiers, chord.key)
            })
    });
    if pressed {
        commands::execute(commands::CommandId::CommandPalette, app, clipboard, ctx);
    }
}

/// Route global bindings through the same command dispatcher used by menus,
/// the Ribbon and the command palette. Direct-manipulation-only keys remain in
/// their focused handlers below.
pub(super) fn handle_command_shortcuts(
    app: &mut PlotxApp,
    clipboard: &mut clipboard_table::ClipboardTablePaste,
    ctx: &egui::Context,
) {
    if ctx.egui_wants_keyboard_input() {
        return;
    }
    let command = ctx.input(|i| {
        // The platform Copy event has no chord of its own; route it like Cmd+C.
        if i.events
            .iter()
            .any(|event| matches!(event, egui::Event::Copy))
        {
            return Some(commands::CommandId::CopyFigure);
        }
        BINDINGS
            .iter()
            .filter(|binding| binding.dispatch)
            .find_map(|binding| {
                std::iter::once(binding.primary)
                    .chain(binding.aliases.iter().copied())
                    .any(|chord| chord_pressed(i, chord))
                    .then_some(binding.id)
            })
    });
    if let Some(command) = command {
        commands::execute(command, app, clipboard, ctx);
    }
}

pub(super) fn handle_escape_shortcut(app: &mut PlotxApp, ctx: &egui::Context) {
    if ctx.egui_wants_keyboard_input() {
        return;
    }
    let (escape, now) = ctx.input(|i| (i.key_pressed(egui::Key::Escape), i.time));
    if !escape {
        return;
    }
    handle_escape(app, now);
}

fn handle_escape(app: &mut PlotxApp, now: f64) {
    if app.interaction().is_active() {
        let phase = matches!(app.interaction(), Interaction::Phase(_));
        app.cancel_interaction();
        app.session.status = if phase {
            "Cancelled phase drag.".to_owned()
        } else {
            "Cancelled interaction.".to_owned()
        };
        return;
    }

    if app.session.ui.wheel_zoom.is_some() {
        app.finish_pending_wheel_zoom(now, true);
        app.session.status = "Cancelled interaction.".to_owned();
        return;
    }
    // Esc steps back one level: analysis region -> panel-letter sub-selection ->
    // selection -> active tool.
    if app.session.ui.analysis_selection.is_some() {
        if let Some(ci) = app.session.active_canvas {
            canvas::clear_canvas_interaction_state(
                app,
                ci,
                canvas::CanvasInteractionClearScope::All,
            );
        } else {
            app.session.ui.analysis_selection = None;
        }
        app.session.status = "Cleared analysis selection.".to_owned();
        return;
    }

    if app.session.ui.panel_label_selection.is_some() {
        app.session.ui.panel_label_selection = None;
        app.session.status = "Selection cleared.".to_owned();
        return;
    }

    if !matches!(app.session.ui.selection, Selection::None) {
        exit_to_page(app, "Selection cleared.");
        return;
    }

    let rest = app.session.tool.rest();
    if app.session.tool != rest {
        app.set_tool(rest);
        app.session.status = "Exited tool mode.".to_owned();
    }
}

fn exit_to_page(app: &mut PlotxApp, status: &str) {
    if let Some(ci) = app.session.active_canvas {
        canvas::clear_canvas_interaction_state(
            app,
            ci,
            canvas::CanvasInteractionClearScope::Selection,
        );
    } else {
        app.session.ui.selection = Selection::None;
    }
    app.session.status = status.to_owned();
}

/// F2 renames the selected entry in the active primary view — a dataset in the
/// Data view, a canvas in the Canvas view — mirroring the sidebar's inline edit.
pub(super) fn handle_rename_shortcut(app: &mut PlotxApp, ctx: &egui::Context) {
    if ctx.egui_wants_keyboard_input() {
        return;
    }
    if !ctx.input(|i| i.key_pressed(egui::Key::F2)) {
        return;
    }
    match app.session.view {
        plotx_core::state::PrimaryView::Data => {
            if let Some(di) = app
                .active_dataset()
                .filter(|&di| di < app.doc.datasets.len())
            {
                app.session.ui.rename = Some(plotx_core::state::RenameState {
                    target: plotx_core::state::RenameTarget::Data(di),
                    buffer: app.doc.datasets[di].display_name(),
                    focus: true,
                });
            }
        }
        plotx_core::state::PrimaryView::Canvas => {
            if let Some(ci) = app
                .session
                .active_canvas
                .filter(|&ci| ci < app.doc.canvases.len())
            {
                app.session.ui.rename = Some(plotx_core::state::RenameState {
                    target: plotx_core::state::RenameTarget::Canvas(ci),
                    buffer: app.doc.canvases[ci].name.clone(),
                    focus: true,
                });
            }
        }
    }
}

/// F zooms the board to fit the frame selection, or every frame when nothing is
/// selected.
pub(super) fn handle_fit_shortcut(app: &mut PlotxApp, ctx: &egui::Context) {
    if ctx.egui_wants_keyboard_input() {
        return;
    }
    let fit =
        ctx.input(|i| !i.modifiers.command && !i.modifiers.ctrl && i.key_pressed(egui::Key::F));
    if !fit {
        return;
    }
    canvas::zoom_to_selection(app, ctx);
    let n = app.session.ui.frame_selection.len();
    app.session.status = if n > 1 {
        format!("Zoomed to {n} selected frames.")
    } else {
        "Zoomed to fit.".to_owned()
    };
}

/// Enter springs the board to zoom-to-fit the active frame — the lone selected
/// page/sheet, or the active page when the selection is empty or multiple. `F`
/// still fits the whole frame selection.
pub(super) fn handle_focus_shortcut(app: &mut PlotxApp, ctx: &egui::Context) {
    if ctx.egui_wants_keyboard_input() {
        return;
    }
    if !ctx.input(|i| i.key_pressed(egui::Key::Enter)) {
        return;
    }
    let frame = match app.session.ui.frame_selection.as_slice() {
        [only] => Some(*only),
        _ => app
            .session
            .active_canvas
            .map(plotx_core::state::FrameRef::Page),
    };
    let Some(frame) = frame else {
        return;
    };
    canvas::request_board_fit(app, ctx, frame);
    app.session.status = "Focused frame.".to_owned();
}

pub(super) fn handle_delete_shortcut(app: &mut PlotxApp, ctx: &egui::Context) {
    if ctx.egui_wants_keyboard_input() {
        return;
    }
    let delete =
        ctx.input(|i| i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::Backspace));
    if !delete {
        return;
    }

    // Page-space authoring objects (text/shape/panel label) are deletable; plots
    // are not removed this way. Each selected authoring object is its own step.
    if let (Selection::Objects(ids), Some(ci)) =
        (&app.session.ui.selection, app.session.active_canvas)
    {
        let ids = ids.clone();
        let deletable: Vec<plotx_core::state::ObjectId> = ids
            .into_iter()
            .filter(|&id| {
                app.doc
                    .canvases
                    .get(ci)
                    .and_then(|c| c.object(id))
                    .map(|o| o.plot().is_none())
                    .unwrap_or(false)
            })
            .collect();
        if !deletable.is_empty() {
            let count = deletable.len();
            for id in deletable {
                if let Some(action) = Action::delete_object(app, ci, id) {
                    app.execute_action(action);
                }
            }
            app.session.status = if count == 1 {
                "Object deleted.".to_owned()
            } else {
                format!("Deleted {count} objects.")
            };
            return;
        }
    }

    let Some((ci, object_id)) = app.panel_label_selection() else {
        return;
    };
    let Some(before) = app
        .doc
        .canvases
        .get(ci)
        .and_then(|canvas| canvas.object(object_id))
        .and_then(|object| object.plot())
        .map(|plot| plot.panel.clone())
    else {
        app.session.ui.panel_label_selection = None;
        return;
    };
    let mut after = before.clone();
    after.visible = false;
    app.execute_action(Action::set_panel_meta(ci, object_id, before, after));
    app.select_object(ci, object_id);
    app.session.ui.panel_note_inline_edit = None;
    app.session.ui.panel_note_edit = None;
    app.session.status = "Panel letter hidden.".to_owned();
}

pub(super) fn handle_file_drop(app: &mut PlotxApp, ctx: &egui::Context) {
    let hovering = ctx.input(|i| !i.raw.hovered_files.is_empty());
    if hovering {
        let painter = ctx.layer_painter(egui::LayerId::new(
            egui::Order::Foreground,
            egui::Id::new("file_drop_overlay"),
        ));
        let rect = ctx.content_rect();
        painter.rect_filled(rect, 0.0, Color32::from_black_alpha(160));
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "Drop a .plotx project, a CSV/TSV table, an .abf/.jdf file, a Bruker TopSpin folder, or a .zip archive",
            egui::FontId::proportional(24.0),
            Color32::WHITE,
        );
    }

    let dropped: Vec<_> = ctx.input(|i| {
        i.raw
            .dropped_files
            .iter()
            .filter_map(|f| f.path.clone())
            .collect()
    });
    for path in dropped {
        // Route by shape through the same dispatcher as the recent list and the
        // welcome page, so a dropped CSV reaches the delimited importer instead
        // of failing through the acquisition loader. Notes each open as recent.
        super::file_dialogs::open_recent_path(app, &path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two dispatchable bindings must never share an effective chord. The
    /// matcher ignores Shift for plain keys, so those normalize shift away.
    #[test]
    fn dispatchable_chords_are_unambiguous() {
        let mut seen = std::collections::HashSet::new();
        for binding in BINDINGS.iter().filter(|binding| binding.dispatch) {
            for chord in std::iter::once(binding.primary).chain(binding.aliases.iter().copied()) {
                assert!(
                    seen.insert((chord.command, chord.command && chord.shift, chord.key)),
                    "chord {chord:?} bound twice"
                );
            }
        }
    }

    #[test]
    fn labels_derive_from_the_binding_table() {
        let label = shortcut_label(commands::CommandId::SaveProject).unwrap();
        assert!(label.ends_with("+S"));
        assert_eq!(
            shortcut_label(commands::CommandId::Tool(Tool::Select)).as_deref(),
            Some("V")
        );
        assert!(shortcut_label(commands::CommandId::About).is_none());
    }

    #[test]
    fn escape_exits_an_active_tool_after_other_fallbacks() {
        let mut app = PlotxApp::new_with_settings(plotx_core::settings::Settings::default());
        app.set_tool(Tool::Integrate);

        handle_escape(&mut app, 0.0);

        assert_eq!(app.session.tool, Tool::BrowseZoom);
        assert_eq!(app.session.status, "Exited tool mode.");
    }
}
