use super::commands::{CommandDescriptor, CommandExecutionClass};
use super::*;
use egui::{Align2, FontId, Key, TextEdit, vec2};

const PANEL_WIDTH: f32 = 540.0;
const LIST_HEIGHT: f32 = 320.0;
const ROW_HEIGHT: f32 = 26.0;

pub(super) fn command_palette_window(
    app: &mut PlotxApp,
    clipboard: &mut clipboard_table::ClipboardTablePaste,
    ctx: &egui::Context,
) {
    let Some(state) = app.session.ui.command_palette.as_ref() else {
        return;
    };
    let (mut query, mut selected) = (state.query.clone(), state.selected);

    let commands = commands::catalog(app);
    let (up, down, enter) = ctx.input(|input| {
        (
            input.key_pressed(Key::ArrowUp),
            input.key_pressed(Key::ArrowDown),
            input.key_pressed(Key::Enter),
        )
    });

    let mut run: Option<usize> = None;
    let modal = super::modal(ctx, "command_palette", ModalKind::Palette).show(ctx, |ui| {
        ui.set_width(PANEL_WIDTH);
        let response = ui.add(
            TextEdit::singleline(&mut query)
                .hint_text("Type a command…")
                .desired_width(f32::INFINITY),
        );
        if !response.has_focus() {
            response.request_focus();
        }
        if response.changed() {
            selected = 0;
        }

        let filtered = filter(&commands, &query);
        if filtered
            .get(selected)
            .is_none_or(|&index| !commands[index].enabled)
        {
            selected = filtered
                .iter()
                .position(|&index| commands[index].enabled)
                .unwrap_or(0);
        }
        let moved = up || down;
        if down {
            selected = step(&commands, &filtered, selected, 1);
        } else if up {
            selected = step(&commands, &filtered, selected, -1);
        }
        if enter
            && let Some(&index) = filtered
                .get(selected)
                .filter(|&&index| commands[index].enabled)
        {
            run = Some(index);
        }

        ui.add_space(4.0);
        egui::ScrollArea::vertical()
            .max_height(LIST_HEIGHT)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                if filtered.is_empty() {
                    ui.add_space(12.0);
                    ui.vertical_centered(|ui| ui.weak("No matching command"));
                    ui.add_space(12.0);
                    return;
                }
                for (position, &index) in filtered.iter().enumerate() {
                    let command = &commands[index];
                    let response = row(ui, command, position == selected && command.enabled);
                    let clicked = response.clicked();
                    if position == selected && moved {
                        response.scroll_to_me(None);
                    }
                    if !command.enabled
                        && let Some(reason) = command.disabled_reason
                    {
                        response.on_hover_text(reason);
                    }
                    if command.enabled && clicked {
                        run = Some(index);
                    }
                }
            });
    });

    if let Some(state) = app.session.ui.command_palette.as_mut() {
        state.query = query;
        state.selected = selected;
    }
    if run.is_some() || modal.should_close() {
        app.session.ui.command_palette = None;
    }
    if let Some(index) = run {
        commands::execute(commands[index].id, app, clipboard, ctx);
    }
}

fn filter(commands: &[CommandDescriptor], query: &str) -> Vec<usize> {
    let terms: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
    commands
        .iter()
        .enumerate()
        .filter(|(_, command)| {
            let name = command.label.to_lowercase();
            terms.iter().all(|term| name.contains(term))
        })
        .map(|(index, _)| index)
        .collect()
}

fn step(
    commands: &[CommandDescriptor],
    filtered: &[usize],
    from: usize,
    direction: isize,
) -> usize {
    let count = filtered.len() as isize;
    if count == 0 {
        return 0;
    }
    let mut index = from as isize;
    for _ in 0..count {
        index = (index + direction).rem_euclid(count);
        if commands[filtered[index as usize]].enabled {
            return index as usize;
        }
    }
    from
}

fn row(ui: &mut Ui, command: &CommandDescriptor, selected: bool) -> Response {
    let width = ui.available_width();
    let sense = if command.enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(vec2(width, ROW_HEIGHT), sense);
    if !ui.is_rect_visible(rect) {
        return response;
    }
    let visuals = ui.visuals();
    if selected {
        ui.painter()
            .rect_filled(rect, 4.0, visuals.selection.bg_fill);
    } else if command.enabled && response.hovered() {
        ui.painter()
            .rect_filled(rect, 4.0, visuals.widgets.hovered.bg_fill);
    }
    let color = if !command.enabled {
        visuals.weak_text_color()
    } else if selected {
        visuals.strong_text_color()
    } else {
        visuals.text_color()
    };
    let prefix = if command.checked == Some(true) {
        egui_phosphor::regular::CHECK
    } else if matches!(
        command.execution_class,
        CommandExecutionClass::ToolEditor | CommandExecutionClass::ToolBacked
    ) {
        egui_phosphor::regular::WRENCH
    } else {
        ""
    };
    ui.painter().text(
        egui::pos2(rect.left() + 10.0, rect.center().y),
        Align2::LEFT_CENTER,
        format!("{prefix}  {}", command.label),
        FontId::proportional(14.0),
        color,
    );
    if let Some(shortcut) = &command.shortcut {
        ui.painter().text(
            egui::pos2(rect.right() - 10.0, rect.center().y),
            Align2::RIGHT_CENTER,
            shortcut,
            FontId::proportional(12.0),
            visuals.weak_text_color(),
        );
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filter_matches_all_terms() {
        let app = PlotxApp::new_with_settings(plotx_core::settings::Settings::default());
        let commands = commands::catalog(&app);
        let matches = filter(&commands, "toggle snapping");
        assert!(
            matches
                .iter()
                .any(|&index| commands[index].id == commands::CommandId::ToggleSnap)
        );
    }

    #[test]
    fn empty_state_is_constructible() {
        let state = plotx_core::state::CommandPaletteState::default();
        assert!(state.query.is_empty());
    }
}
