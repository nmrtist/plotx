//! The unified search (`Cmd+K`).
//!
//! Its search set is commands ∪ properties ∪ resources. Matching a command by
//! its label alone was the visible half of the gap the property catalog closes:
//! a setting the user could see on screen was not findable by name, because
//! only verbs were indexed. Every entry now contributes a set of terms — for a
//! property, its id tokens, canonical label and aliases, and the active
//! locale's label and aliases — and a query term has to appear in one of them.

use super::commands::{CommandDescriptor, CommandExecutionClass, CommandId};
use super::properties::{self, PanelRoute};
use super::*;
use egui::{Align2, FontId, Key, TextEdit, vec2};
use plotx_core::properties::PropertyId;
use plotx_core::state::{ObjectId, PropertyFocus, ToolGroup};

const PANEL_WIDTH: f32 = 540.0;
const LIST_HEIGHT: f32 = 320.0;
const ROW_HEIGHT: f32 = 26.0;

/// What activating a search hit does.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum PaletteAction {
    Command(CommandId),
    /// Reveal a property at its canonical home.
    Property(PropertyId),
    Dataset(usize),
    Canvas(usize),
    Object(usize, ObjectId),
}

/// One row of the unified search.
pub(super) struct PaletteItem {
    label: String,
    /// Right-hand hint: a shortcut for a command, the home panel for a
    /// property, the owning page for a plot.
    detail: String,
    /// Lower-cased text a query is matched against.
    haystack: String,
    prefix: &'static str,
    enabled: bool,
    disabled_reason: Option<String>,
    pub(super) action: PaletteAction,
}

impl PaletteItem {
    fn from_command(command: CommandDescriptor) -> Self {
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
        Self {
            haystack: command.label.to_lowercase(),
            label: command.label,
            detail: command.shortcut.unwrap_or_default(),
            prefix,
            enabled: command.enabled,
            disabled_reason: command.disabled_reason.map(str::to_owned),
            action: PaletteAction::Command(command.id),
        }
    }
}

pub(super) fn command_palette_window(
    app: &mut PlotxApp,
    clipboard: &mut clipboard_table::ClipboardTablePaste,
    ctx: &egui::Context,
) {
    let Some(state) = app.session.ui.command_palette.as_ref() else {
        return;
    };
    let (mut query, mut selected) = (state.query.clone(), state.selected);

    let items = search_set(app);
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
                .hint_text("Search commands, settings and data…")
                .desired_width(f32::INFINITY),
        );
        if !response.has_focus() {
            response.request_focus();
        }
        if response.changed() {
            selected = 0;
        }

        let filtered = filter(&items, &query);
        if filtered
            .get(selected)
            .is_none_or(|&index| !items[index].enabled)
        {
            selected = filtered
                .iter()
                .position(|&index| items[index].enabled)
                .unwrap_or(0);
        }
        let moved = up || down;
        if down {
            selected = step(&items, &filtered, selected, 1);
        } else if up {
            selected = step(&items, &filtered, selected, -1);
        }
        if enter
            && let Some(&index) = filtered
                .get(selected)
                .filter(|&&index| items[index].enabled)
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
                    ui.vertical_centered(|ui| ui.weak("No matching command, setting or data"));
                    ui.add_space(12.0);
                    return;
                }
                for (position, &index) in filtered.iter().enumerate() {
                    let item = &items[index];
                    let response = row(ui, item, position == selected && item.enabled);
                    let clicked = response.clicked();
                    if position == selected && moved {
                        response.scroll_to_me(None);
                    }
                    if !item.enabled
                        && let Some(reason) = item.disabled_reason.as_deref()
                    {
                        response.on_hover_text(reason);
                    }
                    if item.enabled && clicked {
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
        activate(items[index].action, app, clipboard, ctx);
    }
}

/// Commands ∪ properties ∪ resources, in that order: verbs first, then the
/// settings that describe how things look, then the things themselves.
pub(super) fn search_set(app: &PlotxApp) -> Vec<PaletteItem> {
    let mut items: Vec<PaletteItem> = commands::catalog(app)
        .into_iter()
        .map(PaletteItem::from_command)
        .collect();

    for hit in properties::property_hits() {
        let targets = properties::discovery::targets_for_property(app, hit.id);
        let unavailable = property_unavailable_reason(app, hit.id, &targets);
        items.push(PaletteItem {
            haystack: hit.terms.join(" "),
            label: hit.label,
            detail: hit.home.to_owned(),
            prefix: egui_phosphor::regular::SLIDERS_HORIZONTAL,
            enabled: unavailable.is_none(),
            disabled_reason: unavailable,
            action: PaletteAction::Property(hit.id),
        });
    }

    for (index, dataset) in app.doc.datasets.iter().enumerate() {
        let label = dataset.display_name();
        let kind = dataset.kind_label();
        items.push(PaletteItem {
            haystack: format!("{} {}", label.to_lowercase(), kind.to_lowercase()),
            label,
            detail: kind.to_owned(),
            prefix: egui_phosphor::regular::DATABASE,
            enabled: true,
            disabled_reason: None,
            action: PaletteAction::Dataset(index),
        });
    }

    for (index, canvas) in app.doc.canvases.iter().enumerate() {
        items.push(PaletteItem {
            haystack: canvas.name.to_lowercase(),
            label: canvas.name.clone(),
            detail: "Page".to_owned(),
            prefix: egui_phosphor::regular::FILE,
            enabled: true,
            disabled_reason: None,
            action: PaletteAction::Canvas(index),
        });
        for object in &canvas.objects {
            items.push(PaletteItem {
                haystack: format!(
                    "{} {}",
                    object.name.to_lowercase(),
                    canvas.name.to_lowercase()
                ),
                label: object.name.clone(),
                detail: canvas.name.clone(),
                prefix: egui_phosphor::regular::SELECTION,
                enabled: true,
                disabled_reason: None,
                action: PaletteAction::Object(index, object.id),
            });
        }
    }
    items
}

/// Why a property hit cannot be revealed right now, or `None` when it can.
///
/// Activating a hit only asks the panel to reveal a row, and the panel resolves
/// its rows against the selection. A hit that applies to nothing therefore has
/// no row to scroll to: the focus would be requested and then hang there, having
/// moved nothing on screen. The gate is the catalog's own applicability answer —
/// capability and encoding gates included — resolved against the same targets,
/// so the palette reports applicability rather than deciding it a second time.
///
/// The entry stays visible and disabled rather than disappearing, per the
/// crate's hide-vs-disable rule: a setting the user is searching for by name
/// must be findable even when the current selection cannot receive it.
fn property_unavailable_reason(
    app: &PlotxApp,
    property: PropertyId,
    targets: &[plotx_core::automation::TargetRef],
) -> Option<String> {
    let resolved = app.resolve_property_set(property, targets);
    if !resolved.applicable_targets.is_empty() {
        // The setting applies. Whether it can be *changed* is a separate
        // question with its own answer: a locked plot is still described, still
        // read out on the canvas, and still findable by name here.
        let editable = properties::discovery::editable_targets_for_property(app, property);
        if app
            .resolve_property_set(property, &editable)
            .applicable_targets
            .is_empty()
        {
            return Some(properties::discovery::LOCKED_REASON.to_owned());
        }
        return None;
    }
    // Nothing was even a candidate: the group already declares the sentence
    // that names the fix, and the catalog has nothing more specific to add.
    let Some(skip) = resolved.skipped_targets.first() else {
        return Some(
            properties::presentation(property)
                .and_then(|entry| properties::discovery::group(entry.home_route.section))
                .map(|group| group.unavailable_reason)
                .unwrap_or("Select an object that has this setting.")
                .to_owned(),
        );
    };
    Some(format!(
        "Select a series this setting applies to: {}",
        skip.message
    ))
}

fn activate(
    action: PaletteAction,
    app: &mut PlotxApp,
    clipboard: &mut clipboard_table::ClipboardTablePaste,
    ctx: &egui::Context,
) {
    match action {
        PaletteAction::Command(id) => commands::execute(id, app, clipboard, ctx),
        PaletteAction::Property(id) => {
            reveal_property(app, id, ctx.input(|input| input.time));
            ctx.request_repaint();
        }
        PaletteAction::Dataset(index) => app.focus_single(index),
        // Navigation goes through the one path that also brings the selection
        // and the data focus with it. A page switch that only moved
        // `active_canvas` would leave the previous page's selection in place,
        // and object ids restart at one on every page, so it would resolve to an
        // unrelated object here.
        PaletteAction::Canvas(index) => app.activate_canvas(index),
        PaletteAction::Object(canvas, object) => app.reveal_object(canvas, object),
    }
}

/// Open the property's home panel and ask it to expand, scroll and highlight.
/// The route is data: this reads it rather than knowing where anything lives.
pub(super) fn reveal_property(app: &mut PlotxApp, property: PropertyId, now: f64) {
    let Some(presentation) = properties::presentation(property) else {
        return;
    };
    let route = presentation.home_route;
    if !route.panel.sections().contains(&route.section) {
        // Opening a panel that will never scroll anywhere is worse than saying
        // nothing happened, so report it instead of pretending to navigate.
        app.session.status = format!("No panel currently hosts {property}.");
        return;
    }
    match route.panel {
        PanelRoute::SecondarySidebar => app.session.secondary_sidebar_visible = true,
        PanelRoute::Processing => {
            app.session.secondary_sidebar_visible = true;
            app.session.ui.requested_tool_group = Some(ToolGroup::Processing);
        }
    }
    // A property owned by an owner-local component needs that component opened,
    // or the row it names is not on screen to scroll to. The step is chosen here
    // — once, from the user's activation — rather than recomputed every frame
    // while the panel renders.
    if route.panel == PanelRoute::Processing
        && let Some(step) = revealed_step(app, property)
    {
        app.session.ui.proc_expanded_step = Some(step);
    }
    app.session.ui.property_focus = Some(PropertyFocus::request(property, now));
}

/// The first processing step that actually carries `property`, addressed the way
/// the catalog addresses it. Applicability is the catalog's answer, so a step
/// whose current settings do not expose the property is passed over rather than
/// opened onto a row that is not there.
fn revealed_step(
    app: &PlotxApp,
    property: PropertyId,
) -> Option<(plotx_core::state::DatasetId, plotx_processing::StepId)> {
    use plotx_core::automation::ComponentRef;
    use plotx_core::properties::PropertyAddress;

    properties::discovery::targets_for_property(app, property)
        .into_iter()
        .find(|target| {
            app.resolve_property(&PropertyAddress::new(target.clone(), property))
                .is_ok()
        })
        .and_then(|target| match target.component {
            Some(ComponentRef::ProcessingStep(step)) => {
                plotx_core::state::DatasetId::try_from(&target.resource)
                    .ok()
                    .map(|dataset| (dataset, step))
            }
            _ => None,
        })
}

fn filter(items: &[PaletteItem], query: &str) -> Vec<usize> {
    let terms: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
    items
        .iter()
        .enumerate()
        .filter(|(_, item)| terms.iter().all(|term| item.haystack.contains(term)))
        .map(|(index, _)| index)
        .collect()
}

fn step(items: &[PaletteItem], filtered: &[usize], from: usize, direction: isize) -> usize {
    let count = filtered.len() as isize;
    if count == 0 {
        return 0;
    }
    let mut index = from as isize;
    for _ in 0..count {
        index = (index + direction).rem_euclid(count);
        if items[filtered[index as usize]].enabled {
            return index as usize;
        }
    }
    from
}

fn row(ui: &mut Ui, item: &PaletteItem, selected: bool) -> Response {
    let width = ui.available_width();
    let sense = if item.enabled {
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
    } else if item.enabled && response.hovered() {
        ui.painter()
            .rect_filled(rect, 4.0, visuals.widgets.hovered.bg_fill);
    }
    let color = if !item.enabled {
        visuals.weak_text_color()
    } else if selected {
        visuals.strong_text_color()
    } else {
        visuals.text_color()
    };
    ui.painter().text(
        egui::pos2(rect.left() + 10.0, rect.center().y),
        Align2::LEFT_CENTER,
        format!("{}  {}", item.prefix, item.label),
        FontId::proportional(14.0),
        color,
    );
    if !item.detail.is_empty() {
        ui.painter().text(
            egui::pos2(rect.right() - 10.0, rect.center().y),
            Align2::RIGHT_CENTER,
            &item.detail,
            FontId::proportional(12.0),
            visuals.weak_text_color(),
        );
    }
    response
}

#[cfg(test)]
#[path = "command_palette_tests.rs"]
mod tests;
