use super::band_editor::{BandHit, band_hit, edited_band_bounds};
use super::*;
use plotx_processing::craft::{CraftRegion, CraftRegionId};

pub(crate) fn handle_craft_region_drag(
    app: &mut PlotxApp,
    ci: usize,
    object_id: ObjectId,
    dataset: usize,
    plot: PlotRect,
    ui: &Ui,
) {
    let target = app.session.ui.craft_task_dataset;
    let Some(nmr) = app.doc.datasets.get(dataset).and_then(Dataset::as_nmr) else {
        return;
    };
    if target != Some(nmr.resource_id) || nmr.spectrum().is_none() {
        return;
    }
    let acquired_bounds = nmr.spectrum().unwrap().ppm_bounds();
    let dataset_id = nmr.resource_id;
    let observe_freq = nmr.data.observe_freq_mhz.max(f64::MIN_POSITIVE);
    let point_step =
        nmr.data.spectral_width_hz.abs() / observe_freq / nmr.data.points.len().max(1) as f64;
    let suggestions = app
        .session
        .ui
        .craft_resolution_cache
        .as_ref()
        .filter(|cache| cache.dataset == dataset_id)
        .map(|cache| cache.invocation.assessment.clear_signals.clone())
        .unwrap_or_default();

    let (hover, primary_down, primary_pressed, primary_released, double_clicked, esc, del, alt) =
        ui.input(|input| {
            (
                input.pointer.hover_pos(),
                input.pointer.primary_down(),
                input.pointer.primary_pressed(),
                input.pointer.primary_released(),
                input
                    .pointer
                    .button_double_clicked(egui::PointerButton::Primary),
                input.key_pressed(egui::Key::Escape),
                input.key_pressed(egui::Key::Delete) || input.key_pressed(egui::Key::Backspace),
                input.modifiers.alt,
            )
        });
    let Some((xmin, xspan, xrev)) = app.doc.canvases[ci]
        .object(object_id)
        .and_then(|object| object.plot())
        .map(|plot| {
            (
                plot.figure().x.min,
                plot.figure().x.span(),
                plot.figure().x.reversed,
            )
        })
    else {
        return;
    };

    if esc {
        if let Interaction::CraftRegion(drag) = app.take_interaction() {
            app.session.ui.craft_overrides.regions = Some(drag.before);
        }
        return;
    }
    if del && let Some(id) = app.session.ui.craft_selected_region.take() {
        let mut regions = explicit_regions(app);
        regions.retain(|region| region.id != id);
        app.session.ui.craft_overrides.regions = (!regions.is_empty()).then_some(regions);
        app.session.ui.craft_resolution_cache = None;
        return;
    }
    let (left, right, shift) = ui.input(|input| {
        (
            input.key_pressed(egui::Key::ArrowLeft),
            input.key_pressed(egui::Key::ArrowRight),
            input.modifiers.shift,
        )
    });
    if (left || right)
        && app.doc.canvases[ci].selected_object == Some(object_id)
        && let Some(id) = app.session.ui.craft_selected_region
    {
        let direction = if right { 1.0 } else { -1.0 } * if xrev { -1.0 } else { 1.0 };
        let delta = direction * point_step * if shift { 10.0 } else { 1.0 };
        let mut regions = explicit_regions(app);
        if let Some(region) = regions.iter_mut().find(|region| region.id == id) {
            let width = (region.end_ppm - region.start_ppm).abs();
            let lower = acquired_bounds.0.min(acquired_bounds.1);
            let upper = acquired_bounds.0.max(acquired_bounds.1);
            let mut lo = region.start_ppm.min(region.end_ppm) + delta;
            lo = lo.clamp(lower, (upper - width).max(lower));
            region.start_ppm = lo;
            region.end_ppm = lo + width;
            let mut ordered = regions
                .iter()
                .map(|region| region.normalized())
                .collect::<Vec<_>>();
            ordered.sort_by(|left, right| left.start_ppm.total_cmp(&right.start_ppm));
            if !ordered
                .windows(2)
                .any(|pair| pair[0].end_ppm > pair[1].start_ppm)
            {
                app.session.ui.craft_overrides.regions = Some(regions);
                app.session.ui.craft_resolution_cache = None;
            }
        }
        return;
    }

    if let Interaction::CraftRegion(drag) = app.interaction()
        && (drag.canvas != ci || drag.object != object_id)
    {
        return;
    }
    if matches!(app.interaction(), Interaction::CraftRegion(_)) {
        if let Some(pointer) = hover {
            let raw = screen_to_x(
                pointer.x.clamp(plot.left, plot.right()),
                plot,
                xmin,
                xspan,
                xrev,
            );
            let ppm = if alt {
                raw
            } else {
                snap_center(raw, pointer.x, &suggestions, plot, xmin, xspan, xrev)
            };
            if let Interaction::CraftRegion(drag) = &mut app.session.ui.interaction {
                drag.current_ppm = ppm;
            }
        }
        if primary_released || !primary_down {
            finish_craft_drag(app, point_step, xspan, plot.width, acquired_bounds);
        }
        return;
    }

    let Some(pointer) = hover.filter(|pointer| plot_contains(plot, *pointer)) else {
        return;
    };
    let regions = explicit_regions(app);
    let hit = band_hit(
        regions
            .iter()
            .map(|region| (region.id, region.start_ppm, region.end_ppm)),
        plot,
        xmin,
        xspan,
        xrev,
        pointer.x,
    );
    match hit {
        Some(BandHit::Edge { .. }) => ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal),
        Some(BandHit::Inside { .. }) => ui.ctx().set_cursor_icon(egui::CursorIcon::Grab),
        None => ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair),
    }

    if double_clicked
        && let Some(signal) = suggestions.iter().min_by(|left, right| {
            let left =
                (x_to_screen(left.chemical_shift_ppm, plot, xmin, xspan, xrev) - pointer.x).abs();
            let right =
                (x_to_screen(right.chemical_shift_ppm, plot, xmin, xspan, xrev) - pointer.x).abs();
            left.total_cmp(&right)
        })
        && (x_to_screen(signal.chemical_shift_ppm, plot, xmin, xspan, xrev) - pointer.x).abs()
            <= 8.0
    {
        let half_width = 45.0 / observe_freq;
        let id = next_region_id(&regions);
        let mut after = regions;
        after.push(CraftRegion::new(
            id,
            signal.chemical_shift_ppm - half_width,
            signal.chemical_shift_ppm + half_width,
        ));
        app.session.ui.craft_overrides.regions = Some(after);
        app.session.ui.craft_selected_region = Some(id);
        app.session.ui.craft_resolution_cache = None;
        return;
    }

    if primary_pressed {
        let ppm = screen_to_x(pointer.x, plot, xmin, xspan, xrev);
        let mut drag = plotx_core::state::CraftRegionDrag {
            canvas: ci,
            object: object_id,
            dataset: dataset_id,
            kind: RegionDragKind::NewBand,
            region_id: None,
            before: regions,
            anchor_ppm: ppm,
            grab_lo: ppm,
            grab_hi: ppm,
            current_ppm: ppm,
        };
        match hit {
            Some(BandHit::Edge { id, lo_edge }) => {
                let region = drag.before.iter().find(|region| region.id == id).unwrap();
                drag.kind = if lo_edge {
                    RegionDragKind::EdgeLo
                } else {
                    RegionDragKind::EdgeHi
                };
                drag.region_id = Some(id);
                drag.grab_lo = region.start_ppm;
                drag.grab_hi = region.end_ppm;
                app.session.ui.craft_selected_region = Some(id);
            }
            Some(BandHit::Inside { id }) => {
                let region = drag.before.iter().find(|region| region.id == id).unwrap();
                drag.kind = RegionDragKind::Move;
                drag.region_id = Some(id);
                drag.grab_lo = region.start_ppm;
                drag.grab_hi = region.end_ppm;
                app.session.ui.craft_selected_region = Some(id);
            }
            None => app.session.ui.craft_selected_region = None,
        }
        app.begin_interaction(Interaction::CraftRegion(drag));
    }
}

fn explicit_regions(app: &PlotxApp) -> Vec<CraftRegion> {
    app.session
        .ui
        .craft_overrides
        .regions
        .clone()
        .unwrap_or_default()
}

fn finish_craft_drag(
    app: &mut PlotxApp,
    point_step: f64,
    xspan: f64,
    plot_width: f32,
    acquired_bounds: (f64, f64),
) {
    let Interaction::CraftRegion(drag) = app.take_interaction() else {
        return;
    };
    let mut after = drag.before.clone();
    let (mut lo, mut hi) = edited_band_bounds(
        drag.kind,
        drag.anchor_ppm,
        drag.grab_lo,
        drag.grab_hi,
        drag.current_ppm,
    );
    if lo > hi {
        std::mem::swap(&mut lo, &mut hi);
    }
    let lower = acquired_bounds.0.min(acquired_bounds.1);
    let upper = acquired_bounds.0.max(acquired_bounds.1);
    if drag.kind == RegionDragKind::Move {
        let width = hi - lo;
        lo = lo.clamp(lower, (upper - width).max(lower));
        hi = lo + width;
    } else {
        lo = lo.clamp(lower, upper);
        hi = hi.clamp(lower, upper);
    }
    let min_width = (point_step * 2.0)
        .max(xspan.abs() * 4.0 / f64::from(plot_width.max(1.0)))
        .max(f64::MIN_POSITIVE);
    if hi - lo < min_width {
        app.session.status = "CRAFT signal groups must span at least two spectral points.".into();
        return;
    }
    let id = if let Some(id) = drag.region_id {
        let Some(region) = after.iter_mut().find(|region| region.id == id) else {
            return;
        };
        region.start_ppm = lo;
        region.end_ppm = hi;
        id
    } else {
        let id = next_region_id(&after);
        after.push(CraftRegion::new(id, lo, hi));
        id
    };
    let mut normalized = after
        .iter()
        .map(|region| region.normalized())
        .collect::<Vec<_>>();
    normalized.sort_by(|left, right| left.start_ppm.total_cmp(&right.start_ppm));
    if normalized
        .windows(2)
        .any(|pair| pair[0].end_ppm > pair[1].start_ppm)
    {
        app.session.status = "CRAFT signal groups cannot overlap; the edit was reverted.".into();
        return;
    }
    app.session.ui.craft_overrides.regions = Some(after);
    app.session.ui.craft_selected_region = Some(id);
    app.session.ui.craft_resolution_cache = None;
}

fn snap_center(
    raw: f64,
    pointer_x: f32,
    suggestions: &[plotx_processing::craft::CraftSignalSuggestion],
    plot: PlotRect,
    xmin: f64,
    xspan: f64,
    xrev: bool,
) -> f64 {
    suggestions
        .iter()
        .filter_map(|signal| {
            let distance =
                (x_to_screen(signal.chemical_shift_ppm, plot, xmin, xspan, xrev) - pointer_x).abs();
            (distance <= 8.0).then_some((distance, signal.chemical_shift_ppm))
        })
        .min_by(|left, right| left.0.total_cmp(&right.0))
        .map_or(raw, |(_, ppm)| ppm)
}

fn next_region_id(regions: &[CraftRegion]) -> CraftRegionId {
    CraftRegionId(
        regions
            .iter()
            .map(|region| region.id.0)
            .max()
            .unwrap_or(0)
            .saturating_add(1),
    )
}

pub(crate) fn craft_preview_regions(
    app: &PlotxApp,
    dataset: plotx_core::state::DatasetId,
) -> Vec<CraftRegion> {
    let Interaction::CraftRegion(drag) = app.interaction() else {
        return explicit_regions(app);
    };
    if drag.dataset != dataset {
        return explicit_regions(app);
    }
    let mut regions = drag.before.clone();
    let (lo, hi) = edited_band_bounds(
        drag.kind,
        drag.anchor_ppm,
        drag.grab_lo,
        drag.grab_hi,
        drag.current_ppm,
    );
    if let Some(id) = drag.region_id {
        if let Some(region) = regions.iter_mut().find(|region| region.id == id) {
            region.start_ppm = lo;
            region.end_ppm = hi;
        }
    } else {
        regions.push(CraftRegion::new(next_region_id(&regions), lo, hi));
    }
    regions
}
