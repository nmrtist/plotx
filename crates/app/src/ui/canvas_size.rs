//! The canvas-size UI: the searchable preset section of the Canvas settings
//! window, the on-page size chip with its overflow and column-suggestion
//! hints, and the preset-application path shared with the command catalog.

use egui::{Response, RichText, Ui};
use egui_phosphor::regular as icon;
use plotx_core::actions::{Action, PageSizeState, PendingCanvasSizeEdit};
use plotx_core::settings::{self, CanvasSizeDefaults, CustomSizePreset};
use plotx_core::state::{
    CanvasSizeUnit, MM_TO_PT, PlotxApp, SizePreset, SizePresetGroup, content_bounds_pt,
    content_overflows, matching_preset, size_display_name, size_presets, wider_preset_suggestion,
};

/// What the user picked in the preset list; resolved after the list closures
/// release their borrow of the app.
enum SizeChoice {
    Preset(&'static SizePreset),
    Custom([f32; 2]),
    DeleteCustom(usize),
    Swap,
}

pub(crate) fn size_section(app: &mut PlotxApp, ci: usize, ui: &mut Ui) {
    ui.strong("Canvas size");
    ui.add_space(6.0);

    let size = app.doc.canvases[ci].size_mm;
    let preset_id = app.doc.canvases[ci].size_preset_id.clone();
    let matched = matching_preset(size, preset_id.as_deref());
    let mut choice: Option<SizeChoice> = None;

    let search_id = ui.id().with(("canvas_size_search", ci));
    let mut search = ui
        .ctx()
        .data_mut(|d| d.get_temp::<String>(search_id))
        .unwrap_or_default();
    ui.horizontal(|ui| {
        ui.label("Preset");
        let resp = ui.add(
            egui::TextEdit::singleline(&mut search)
                .hint_text("Search presets…")
                .desired_width(200.0),
        );
        let clear = !search.is_empty() && ui.small_button(icon::X).clicked();
        if clear {
            search.clear();
        }
        if resp.changed() || clear {
            ui.ctx()
                .data_mut(|d| d.insert_temp(search_id, search.clone()));
        }
    });

    let filter = search.trim().to_lowercase();
    let defaults = canvas_size_defaults(ui.ctx());
    egui::ScrollArea::vertical()
        .max_height(170.0)
        .show(ui, |ui| {
            // Recency only helps while browsing; a search already names the target.
            if filter.is_empty() {
                let recent: Vec<_> = defaults
                    .recent_presets
                    .iter()
                    .filter_map(|id| plotx_core::state::preset_by_id(id))
                    .collect();
                if !recent.is_empty() {
                    ui.weak("Recent");
                    for preset in recent {
                        if preset_row(ui, preset, matched).clicked() {
                            choice = Some(SizeChoice::Preset(preset));
                        }
                    }
                    ui.add_space(4.0);
                }
            }
            for group in [
                SizePresetGroup::Journal,
                SizePresetGroup::Paper,
                SizePresetGroup::Presentation,
            ] {
                let items: Vec<_> = size_presets()
                    .iter()
                    .filter(|p| p.group == group && p.label.to_lowercase().contains(&filter))
                    .collect();
                if items.is_empty() {
                    continue;
                }
                ui.weak(group.title());
                for preset in items {
                    if preset_row(ui, preset, matched).clicked() {
                        choice = Some(SizeChoice::Preset(preset));
                    }
                }
                ui.add_space(4.0);
            }
            let customs: Vec<_> = defaults
                .custom_presets
                .iter()
                .enumerate()
                .filter(|(_, c)| c.name.to_lowercase().contains(&filter))
                .collect();
            if !customs.is_empty() {
                ui.weak("Custom");
                for (index, custom) in customs {
                    let custom_size = [custom.width_mm, custom.height_mm];
                    let selected = (size[0] - custom_size[0]).abs() < 0.01
                        && (size[1] - custom_size[1]).abs() < 0.01;
                    ui.horizontal(|ui| {
                        if ui.selectable_label(selected, &custom.name).clicked() {
                            choice = Some(SizeChoice::Custom(custom_size));
                        }
                        if ui
                            .small_button(icon::TRASH)
                            .on_hover_text("Remove this custom preset")
                            .clicked()
                        {
                            choice = Some(SizeChoice::DeleteCustom(index));
                        }
                    });
                }
            }
        });

    // Orientation applies to fixed rectangles; journal widths have no
    // landscape variant to rotate into.
    if matched.is_none_or(SizePreset::is_fixed) && (size[0] - size[1]).abs() > 0.01 {
        ui.horizontal(|ui| {
            ui.label("Orientation");
            let portrait = size[1] >= size[0];
            if ui.selectable_label(portrait, "Portrait").clicked() && !portrait {
                choice = Some(SizeChoice::Swap);
            }
            if ui.selectable_label(!portrait, "Landscape").clicked() && portrait {
                choice = Some(SizeChoice::Swap);
            }
        });
    }

    match choice {
        Some(SizeChoice::Preset(preset)) => apply_preset(app, ui.ctx(), ci, preset),
        Some(SizeChoice::Custom(after)) => apply_size(app, ui.ctx(), ci, after, None),
        Some(SizeChoice::Swap) => {
            let after = [size[1], size[0]];
            apply_size(app, ui.ctx(), ci, after, matched.filter(|p| p.is_fixed()));
        }
        Some(SizeChoice::DeleteCustom(index)) => {
            update_canvas_size_defaults(ui.ctx(), |d| {
                if index < d.custom_presets.len() {
                    d.custom_presets.remove(index);
                }
            });
        }
        None => {}
    }

    ui.horizontal(|ui| {
        ui.label("Unit");
        for unit in CanvasSizeUnit::all() {
            ui.selectable_value(&mut app.session.ui.canvas_size_unit, *unit, unit.label());
        }
    });

    let unit = app.session.ui.canvas_size_unit;
    let auto_height = app.doc.canvases[ci].auto_height;
    ui.horizontal(|ui| {
        ui.label("W");
        let before = app.doc.canvases[ci].size_mm;
        let mut width = unit.from_mm(before[0]);
        let resp = ui.add(
            egui::DragValue::new(&mut width)
                .speed(unit.drag_speed())
                .range(unit.drag_range())
                .max_decimals(unit.decimals()),
        );
        handle_canvas_dimension_response(app, ci, &resp, before, unit, 0, width);
        ui.label("H");
        let before = app.doc.canvases[ci].size_mm;
        let mut height = unit.from_mm(before[1]);
        let resp = ui.add_enabled(
            !auto_height,
            egui::DragValue::new(&mut height)
                .speed(unit.drag_speed())
                .range(unit.drag_range())
                .max_decimals(unit.decimals()),
        );
        if auto_height {
            resp.clone()
                .on_disabled_hover_text("Turn off Auto height to set the height manually.");
        }
        handle_canvas_dimension_response(app, ci, &resp, before, unit, 1, height);
        ui.label(unit.label());
    });

    let defaults = canvas_size_defaults(ui.ctx());
    let mut scale_content = defaults.scale_content;
    if ui
        .checkbox(&mut scale_content, "Scale content when applying sizes")
        .on_hover_text(
            "Presets and orientation changes scale objects uniformly by the width \
             ratio; font sizes keep their physical pt values. Manual W/H edits \
             never scale content.",
        )
        .changed()
    {
        update_canvas_size_defaults(ui.ctx(), |d| d.scale_content = scale_content);
    }

    let mut auto = auto_height;
    if ui
        .checkbox(&mut auto, "Auto height")
        .on_hover_text(
            "Keeps the width fixed while the page height follows the content's \
             depth, up to the journal's maximum figure depth.",
        )
        .changed()
    {
        app.doc.canvases[ci].auto_height = auto;
        app.doc.dirty = true;
    }

    let size = app.doc.canvases[ci].size_mm;
    if let Some(preset) = matched
        && let Some(max) = preset.max_height_mm
        && size[1] > max + 0.01
    {
        ui.colored_label(
            ui.visuals().warn_fg_color,
            format!(
                "{} allows at most {max:.0} mm of figure depth; the page is {:.0} mm tall.",
                preset.label, size[1]
            ),
        );
    }

    let width_in = CanvasSizeUnit::Inch.from_mm(size[0]);
    let height_in = CanvasSizeUnit::Inch.from_mm(size[1]);
    let width_px = CanvasSizeUnit::Pixel.from_mm(size[0]);
    let height_px = CanvasSizeUnit::Pixel.from_mm(size[1]);
    ui.weak(format!(
        "{:.1} x {:.1} mm | {:.3} x {:.3} in | {:.0} x {:.0} px at 96 px/in",
        size[0], size[1], width_in, height_in, width_px, height_px
    ));

    if ui
        .button(format!("{} Save as custom preset", icon::PLUS))
        .on_hover_text("Keep the current page size in the preset list, across sessions.")
        .clicked()
    {
        update_canvas_size_defaults(ui.ctx(), |d| {
            let exists = d.custom_presets.iter().any(|c| {
                (c.width_mm - size[0]).abs() < 0.01 && (c.height_mm - size[1]).abs() < 0.01
            });
            if !exists {
                d.custom_presets.push(CustomSizePreset {
                    name: format!("{:.0} × {:.0} mm", size[0], size[1]),
                    width_mm: size[0],
                    height_mm: size[1],
                });
            }
        });
    }
}

fn preset_row(ui: &mut Ui, preset: &'static SizePreset, matched: Option<&SizePreset>) -> Response {
    let selected = matched.is_some_and(|m| m.id == preset.id);
    let detail = if preset.is_fixed() {
        format!(
            "{:.0} × {:.0} mm",
            preset.width_mm, preset.default_height_mm
        )
    } else {
        format!("{:.0} mm wide", preset.width_mm)
    };
    ui.horizontal(|ui| {
        let resp = ui.selectable_label(selected, preset.label);
        ui.weak(detail);
        resp
    })
    .inner
}

/// Applies `preset` to page `ci`: fixed rectangles take their exact size; a
/// journal preset fixes the width and preserves the content-driven height
/// (scaled along when the sticky scale option is on; the default height is
/// used only for an empty page).
pub(crate) fn apply_preset(
    app: &mut PlotxApp,
    ctx: &egui::Context,
    ci: usize,
    preset: &'static SizePreset,
) {
    let Some(canvas) = app.doc.canvases.get(ci) else {
        return;
    };
    let current = canvas.size_mm;
    let after = if preset.is_fixed() || canvas.objects.is_empty() {
        preset.size_mm()
    } else if canvas_size_defaults(ctx).scale_content {
        [
            preset.width_mm,
            (current[1] * preset.width_mm / current[0]).clamp(10.0, 1000.0),
        ]
    } else {
        [preset.width_mm, current[1]]
    };
    apply_size(app, ctx, ci, after, Some(preset));
}

fn apply_size(
    app: &mut PlotxApp,
    ctx: &egui::Context,
    ci: usize,
    after_size: [f32; 2],
    preset: Option<&'static SizePreset>,
) {
    // Size and preset identity travel through the action together, so
    // undo/redo restores both and an ambiguous width (183 mm is Nature double
    // and Science full width) keeps the user's choice across history moves.
    let before = PageSizeState::of(&app.doc.canvases[ci]);
    let after = PageSizeState {
        size_mm: after_size,
        preset_id: preset.map(|p| p.id.to_owned()),
    };
    if before != after {
        let action = if canvas_size_defaults(ctx).scale_content {
            Action::set_canvas_size_scaling_content(app, ci, after)
        } else {
            Action::set_canvas_size(ci, before, after)
        };
        app.execute_action(action);
    }
    if let Some(preset) = preset {
        update_canvas_size_defaults(ctx, |d| d.note_recent(preset.id));
    }
}

/// The floating chip row above the active page: current size (click opens the
/// settings window), an overflow warning with a one-click fix, and the
/// dismissible wider-column suggestion. All fixes go through undoable actions;
/// nothing here resizes the page on its own.
pub(crate) fn page_size_chrome(
    app: &mut PlotxApp,
    ci: usize,
    page: egui::Rect,
    view: egui::Rect,
    host: &Ui,
) {
    if !view.intersects(page) {
        return;
    }
    let ctx = host.ctx().clone();
    let canvas = &app.doc.canvases[ci];
    let size = canvas.size_mm;
    let name = size_display_name(size, canvas.size_preset_id.as_deref());
    let mut label = format!("{} {:.0} × {:.0} mm", icon::FRAME_CORNERS, size[0], size[1]);
    if let Some(name) = &name {
        label = format!("{label} · {name}");
    }
    if canvas.auto_height {
        label = format!("{label} · auto");
    }
    let overflows = content_overflows(canvas);
    let resource_id = canvas.resource_id.clone();
    let suggestion =
        wider_preset_suggestion(canvas).filter(|s| !suggestion_dismissed(&ctx, &resource_id, s));

    // The chip is page chrome stacked above the frame-header strip (which sits
    // directly on the page's top edge), bottom-anchored so the row grows
    // upward from a known clearance. It scrolls and zooms away with the page:
    // pinning it inside the viewport would float it over unrelated content,
    // so once its row leaves the view it goes with the page — Canvas settings
    // stays reachable from the Ribbon, the page list, and the right-click
    // menu. Zoom-to-fit reserves this row's headroom (`FIT_CHROME_PX`).
    let mut pos = egui::pos2(
        page.left(),
        page.top() - super::canvas::FRAME_HEADER_PX - 4.0,
    );
    pos.x = pos.x.max(view.left() + 4.0);
    if pos.y - 22.0 < view.top() {
        return;
    }

    let mut open_settings = false;
    let mut fit = false;
    let mut apply: Option<&'static SizePreset> = None;
    let mut dismiss: Option<&'static SizePreset> = None;
    // Middle order keeps the chip above the board but in the same class as
    // windows, so any window the user touches stacks above it; Foreground
    // would pin it over popups and the settings window it opens.
    egui::Area::new(egui::Id::new(("page_size_chip", ci)))
        .order(egui::Order::Middle)
        .pivot(egui::Align2::LEFT_BOTTOM)
        .fixed_pos(pos)
        .show(&ctx, |ui| {
            ui.horizontal(|ui| {
                if ui
                    .small_button(label)
                    .on_hover_text("Canvas size — click to change it")
                    .clicked()
                {
                    open_settings = true;
                }
                if overflows
                    && ui
                        .small_button(
                            RichText::new(format!("{} Overflows — scale to fit", icon::WARNING))
                                .color(ui.visuals().error_fg_color),
                        )
                        .on_hover_text(
                            "Content extends past the page. Click to scale it down to \
                             fit (undoable); font sizes keep their pt values.",
                        )
                        .clicked()
                {
                    fit = true;
                }
                if let Some(preset) = suggestion {
                    if ui
                        .small_button(format!(
                            "{} {}-column grid {} {} ({:.0} mm)?",
                            icon::ARROWS_OUT_LINE_HORIZONTAL,
                            app.doc.canvases[ci].layout.cols,
                            icon::ARROW_RIGHT,
                            preset.label,
                            preset.width_mm
                        ))
                        .on_hover_text(
                            "The layout grid asks for multiple panel columns but the page \
                             is a single-column width. Click to switch (undoable).",
                        )
                        .clicked()
                    {
                        apply = Some(preset);
                    }
                    if ui
                        .small_button(icon::X)
                        .on_hover_text("Dismiss this suggestion for this page")
                        .clicked()
                    {
                        dismiss = Some(preset);
                    }
                }
            });
        });

    if open_settings {
        app.session.ui.canvas_settings = Some(ci);
        // The click just raised the chip's own layer; an already-open settings
        // window must still end up above it.
        ctx.move_to_top(super::windows::canvas_settings_layer());
    }
    if fit {
        scale_content_to_fit(app, ci);
    }
    if let Some(preset) = apply {
        apply_preset(app, &ctx, ci, preset);
    }
    if let Some(preset) = dismiss {
        ctx.data_mut(|d| d.insert_temp(suggestion_id(&resource_id, preset), true));
    }
}

/// Shrinks (never enlarges) all content uniformly so its bounding box fits the
/// page, translating it into the page first if it starts left of or above the
/// origin. One undoable step.
pub(crate) fn scale_content_to_fit(app: &mut PlotxApp, ci: usize) {
    let canvas = &app.doc.canvases[ci];
    let Some([min_x, min_y, max_x, max_y]) = content_bounds_pt(canvas) else {
        return;
    };
    let page_w = canvas.size_mm[0] * MM_TO_PT;
    let page_h = canvas.size_mm[1] * MM_TO_PT;
    let offset_x = min_x.min(0.0);
    let offset_y = min_y.min(0.0);
    let extent_w = (max_x - offset_x).max(1.0);
    let extent_h = (max_y - offset_y).max(1.0);
    let scale = (page_w / extent_w).min(page_h / extent_h).min(1.0);
    let before: Vec<_> = canvas.objects.iter().map(|o| (o.id, o.frame)).collect();
    let after: Vec<_> = before
        .iter()
        .map(|&(id, f)| {
            (
                id,
                plotx_core::state::ObjectFrame::new(
                    (f.x - offset_x) * scale,
                    (f.y - offset_y) * scale,
                    f.width * scale,
                    f.height * scale,
                ),
            )
        })
        .collect();
    if before.is_empty() {
        return;
    }
    app.execute_action(Action::set_object_frames(ci, before, after));
}

fn suggestion_id(resource_id: &str, preset: &SizePreset) -> egui::Id {
    egui::Id::new((
        "size_suggestion_dismissed",
        resource_id.to_owned(),
        preset.id,
    ))
}

fn suggestion_dismissed(ctx: &egui::Context, resource_id: &str, preset: &SizePreset) -> bool {
    ctx.data(|d| d.get_temp(suggestion_id(resource_id, preset)))
        .unwrap_or(false)
}

/// The sticky canvas-size choices, cached in egui memory so the per-frame UI
/// never re-reads the settings file; writes go through
/// [`update_canvas_size_defaults`], which refreshes the cache and persists.
fn canvas_size_defaults(ctx: &egui::Context) -> CanvasSizeDefaults {
    let id = egui::Id::new("canvas_size_defaults_cache");
    if let Some(cached) = ctx.data(|d| d.get_temp::<CanvasSizeDefaults>(id)) {
        return cached;
    }
    let loaded = settings::load().canvas_size;
    ctx.data_mut(|d| d.insert_temp(id, loaded.clone()));
    loaded
}

fn update_canvas_size_defaults(ctx: &egui::Context, f: impl FnOnce(&mut CanvasSizeDefaults)) {
    let mut defaults = canvas_size_defaults(ctx);
    f(&mut defaults);
    ctx.data_mut(|d| {
        d.insert_temp(
            egui::Id::new("canvas_size_defaults_cache"),
            defaults.clone(),
        )
    });
    settings::update(|s| s.canvas_size = defaults);
}

fn handle_canvas_dimension_response(
    app: &mut PlotxApp,
    ci: usize,
    resp: &Response,
    fallback_before: [f32; 2],
    unit: CanvasSizeUnit,
    axis: usize,
    value: f32,
) {
    if resp.drag_started() {
        // Capture the preset id along with the size before any mutation: the
        // per-frame reconciler may clear a stale id mid-drag, and undo must
        // restore the pre-drag identity.
        app.session.ui.canvas_size_edit = Some(PendingCanvasSizeEdit {
            canvas: ci,
            before: PageSizeState {
                size_mm: fallback_before,
                preset_id: app.doc.canvases[ci].size_preset_id.clone(),
            },
        });
    }
    if resp.changed() {
        app.doc.canvases[ci].size_mm[axis] = unit.to_mm(value).clamp(10.0, 1000.0);
        app.rebuild_canvas(ci);
        app.doc.dirty = true;
    }
    if resp.drag_stopped() {
        let before = app
            .session
            .ui
            .canvas_size_edit
            .take()
            .filter(|edit| edit.canvas == ci)
            .map(|edit| edit.before)
            .unwrap_or_else(|| PageSizeState {
                size_mm: fallback_before,
                preset_id: app.doc.canvases[ci].size_preset_id.clone(),
            });
        let after = manual_edit_state(&before, app.doc.canvases[ci].size_mm);
        app.execute_action(Action::set_canvas_size(ci, before, after));
    } else if resp.changed() && !resp.dragged() {
        let before = PageSizeState {
            size_mm: fallback_before,
            preset_id: app.doc.canvases[ci].size_preset_id.clone(),
        };
        let after = manual_edit_state(&before, app.doc.canvases[ci].size_mm);
        app.execute_action(Action::set_canvas_size(ci, before, after));
    }
}

/// The post-edit size state for a manual W/H change: the original preset id is
/// kept while the new size still matches it (a height tweak on a journal
/// preset), and dropped once it no longer does.
fn manual_edit_state(before: &PageSizeState, after_size: [f32; 2]) -> PageSizeState {
    let preset_id = before
        .preset_id
        .clone()
        .filter(|id| plotx_core::state::preset_by_id(id).is_some_and(|p| p.matches(after_size)));
    PageSizeState {
        size_mm: after_size,
        preset_id,
    }
}
