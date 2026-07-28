//! Catalog-driven property sections and section rendering.

use super::control::{RowEdits, property_row, property_row_inline};
use super::*;
use plotx_core::properties::{PropertyAddress, app_preferences, baseline};

#[derive(Clone, Copy, PartialEq, Eq)]
enum SectionLayout {
    Standard,
    Inline,
    Menu,
}

/// Render the contour section for the current selection. Returns `false` when
/// nothing in the selection draws a contour, in which case the caller draws no
/// heading either.
pub(crate) fn contour_section(
    app: &mut PlotxApp,
    canvas: usize,
    objects: &[ObjectId],
    ui: &mut Ui,
) -> bool {
    let targets: Vec<TargetRef> = objects
        .iter()
        .flat_map(|&object| app.series_targets(canvas, object))
        .collect();
    render_section(
        app,
        CONTOUR_SECTION,
        "Contour",
        SectionNoun::new("contour series", "contour series"),
        &targets,
        Some(EncodingKind::Contour),
        SectionLayout::Standard,
        ui,
    )
}

/// Render scalar heatmap display-range properties over the current selection.
pub(crate) fn heatmap_section(
    app: &mut PlotxApp,
    canvas: usize,
    objects: &[ObjectId],
    ui: &mut Ui,
) -> bool {
    let targets: Vec<TargetRef> = objects
        .iter()
        .flat_map(|&object| app.series_targets(canvas, object))
        .collect();
    render_section(
        app,
        HEATMAP_SECTION,
        "Heatmap",
        SectionNoun::new("heatmap series", "heatmap series"),
        &targets,
        Some(EncodingKind::Heatmap),
        SectionLayout::Standard,
        ui,
    )
}

/// Render line properties over the current plot selection.
pub(crate) fn line_section(
    app: &mut PlotxApp,
    canvas: usize,
    objects: &[ObjectId],
    ui: &mut Ui,
) -> bool {
    let targets: Vec<TargetRef> = objects
        .iter()
        .flat_map(|&object| app.series_targets(canvas, object))
        .collect();
    render_section(
        app,
        LINE_SECTION,
        "Line",
        SectionNoun::new("line series", "line series"),
        &targets,
        None,
        SectionLayout::Standard,
        ui,
    )
}

pub(crate) fn axis_section(
    app: &mut PlotxApp,
    canvas: usize,
    objects: &[ObjectId],
    ui: &mut Ui,
) -> bool {
    let targets: Vec<TargetRef> = objects
        .iter()
        .filter_map(|&object| app.object_target(canvas, object))
        .collect();
    render_section(
        app,
        AXIS_SECTION,
        "Axes",
        SectionNoun::new("plot", "plots"),
        &targets,
        None,
        SectionLayout::Standard,
        ui,
    )
}

pub(crate) fn stack_section(
    app: &mut PlotxApp,
    canvas: usize,
    objects: &[ObjectId],
    ui: &mut Ui,
) -> bool {
    object_section(app, canvas, objects, STACK_SECTION, "Stack", ui)
}

pub(crate) fn chart_section(
    app: &mut PlotxApp,
    canvas: usize,
    objects: &[ObjectId],
    ui: &mut Ui,
) -> bool {
    object_section(app, canvas, objects, CHART_SECTION, "Chart", ui)
}

pub(crate) fn text_section(
    app: &mut PlotxApp,
    canvas: usize,
    objects: &[ObjectId],
    ui: &mut Ui,
) -> bool {
    object_section(app, canvas, objects, TEXT_SECTION, "Text", ui)
}

pub(crate) fn shape_section(
    app: &mut PlotxApp,
    canvas: usize,
    objects: &[ObjectId],
    ui: &mut Ui,
) -> bool {
    object_section(app, canvas, objects, SHAPE_SECTION, "Shape", ui)
}

pub(crate) fn panel_section(
    app: &mut PlotxApp,
    canvas: usize,
    objects: &[ObjectId],
    ui: &mut Ui,
) -> bool {
    object_section(app, canvas, objects, PANEL_SECTION, "Panel", ui)
}

pub(crate) fn general_object_section(
    app: &mut PlotxApp,
    canvas: usize,
    objects: &[ObjectId],
    ui: &mut Ui,
) -> bool {
    object_section(app, canvas, objects, OBJECT_SECTION, "Object", ui)
}

pub(crate) fn panel_inline_section(
    app: &mut PlotxApp,
    canvas: usize,
    object: ObjectId,
    ui: &mut Ui,
) -> bool {
    let Some(target) = app.object_target(canvas, object) else {
        return false;
    };
    render_section(
        app,
        PANEL_SECTION,
        "Panel",
        SectionNoun::new("panel", "panels"),
        std::slice::from_ref(&target),
        None,
        SectionLayout::Inline,
        ui,
    )
}

fn object_section(
    app: &mut PlotxApp,
    canvas: usize,
    objects: &[ObjectId],
    section: &'static str,
    title: &'static str,
    ui: &mut Ui,
) -> bool {
    let targets: Vec<TargetRef> = objects
        .iter()
        .filter_map(|&object| app.object_target(canvas, object))
        .collect();
    render_section(
        app,
        section,
        title,
        SectionNoun::new("object", "objects"),
        &targets,
        None,
        SectionLayout::Standard,
        ui,
    )
}

/// Render document-owned typography without requiring any canvas object.
pub(crate) fn typography_section(app: &mut PlotxApp, ui: &mut Ui) -> bool {
    let target = app.document_target();
    render_section(
        app,
        TYPOGRAPHY_SECTION,
        "Figure typography",
        SectionNoun::new("document", "documents"),
        std::slice::from_ref(&target),
        None,
        SectionLayout::Standard,
        ui,
    )
}

/// Render one settings sub-struct against the singleton application target.
/// Preferences use menu layout because the rail page already supplies the
/// surrounding title and scroll container.
pub(crate) fn preferences_section(app: &mut PlotxApp, section: &'static str, ui: &mut Ui) -> bool {
    let target = app.app_target();
    render_section(
        app,
        section,
        "Preferences",
        SectionNoun::new("preference", "preferences"),
        std::slice::from_ref(&target),
        None,
        SectionLayout::Menu,
        ui,
    )
}

pub(crate) fn canvas_margins_section(app: &mut PlotxApp, target: &TargetRef, ui: &mut Ui) -> bool {
    canvas_section(
        app,
        CANVAS_MARGINS_SECTION,
        "Margins and spacing",
        target,
        ui,
    )
}

pub(crate) fn canvas_grid_section(app: &mut PlotxApp, target: &TargetRef, ui: &mut Ui) -> bool {
    canvas_section(app, CANVAS_GRID_SECTION, "Layout grid", target, ui)
}

pub(crate) fn canvas_size_section(app: &mut PlotxApp, target: &TargetRef, ui: &mut Ui) -> bool {
    canvas_section(app, CANVAS_SIZE_SECTION, "Page size", target, ui)
}

pub(crate) fn canvas_caption_section(app: &mut PlotxApp, target: &TargetRef, ui: &mut Ui) -> bool {
    canvas_section(
        app,
        CANVAS_CAPTION_SECTION,
        "Caption and labels",
        target,
        ui,
    )
}

fn canvas_section(
    app: &mut PlotxApp,
    section: &'static str,
    title: &'static str,
    target: &TargetRef,
    ui: &mut Ui,
) -> bool {
    render_section(
        app,
        section,
        title,
        SectionNoun::new("canvas", "canvases"),
        std::slice::from_ref(target),
        None,
        SectionLayout::Standard,
        ui,
    )
}

/// Render the catalog rows for one expanded apodization step in the existing
/// processing editor. The step list supplies the stable component target; this
/// panel supplies the same schema, reset and action path as every other scope.
pub(crate) fn apodization_section(app: &mut PlotxApp, target: &TargetRef, ui: &mut Ui) -> bool {
    render_section(
        app,
        APODIZATION_SECTION,
        "Apodization",
        SectionNoun::new("processing step", "processing steps"),
        std::slice::from_ref(target),
        None,
        SectionLayout::Standard,
        ui,
    )
}

pub(crate) fn zero_fill_section(app: &mut PlotxApp, target: &TargetRef, ui: &mut Ui) -> bool {
    processing_parameter_section(app, ZERO_FILL_SECTION, "Zero fill", target, ui)
}

pub(crate) fn phase_section(app: &mut PlotxApp, target: &TargetRef, ui: &mut Ui) -> bool {
    let rendered = processing_parameter_section(app, PHASE_SECTION, "Phase", target, ui);
    if rendered {
        ui.weak(format!(
            "{}  drag the spectrum to adjust",
            egui_phosphor::regular::HAND_POINTING
        ));
    }
    rendered
}

pub(crate) fn baseline_section(app: &mut PlotxApp, target: &TargetRef, ui: &mut Ui) -> bool {
    let rendered = processing_parameter_section(app, BASELINE_SECTION, "Baseline", target, ui);
    if rendered
        && app
            .resolve_property(&PropertyAddress::new(target.clone(), baseline::METHOD))
            .ok()
            .is_some_and(|resolved| {
                resolved.value.uniform()
                    == Some(&PropertyValue::Enum(baseline::ASYMMETRIC_LEAST_SQUARES))
            })
    {
        ui.small("AsLS estimates a smooth baseline while down-weighting positive peaks.");
    }
    rendered
}

pub(crate) fn reference_section(app: &mut PlotxApp, target: &TargetRef, ui: &mut Ui) -> bool {
    processing_parameter_section(app, REFERENCE_SECTION, "Reference", target, ui)
}

pub(crate) fn smooth_section(app: &mut PlotxApp, target: &TargetRef, ui: &mut Ui) -> bool {
    processing_parameter_section(app, SMOOTH_SECTION, "Smoothing", target, ui)
}

pub(crate) fn normalize_section(app: &mut PlotxApp, target: &TargetRef, ui: &mut Ui) -> bool {
    processing_parameter_section(app, NORMALIZE_SECTION, "Normalize", target, ui)
}

pub(crate) fn bin_section(app: &mut PlotxApp, target: &TargetRef, ui: &mut Ui) -> bool {
    processing_parameter_section(app, BIN_SECTION, "Binning", target, ui)
}

fn processing_parameter_section(
    app: &mut PlotxApp,
    section: &'static str,
    title: &'static str,
    target: &TargetRef,
    ui: &mut Ui,
) -> bool {
    render_section(
        app,
        section,
        title,
        SectionNoun::new("processing step", "processing steps"),
        std::slice::from_ref(target),
        None,
        SectionLayout::Standard,
        ui,
    )
}

/// Render the cross-step enabled property inside the step-list row. The same
/// Essential set used by standard sections and by the budget test selects the
/// row; only its chrome is compact.
pub(crate) fn processing_step_section(app: &mut PlotxApp, target: &TargetRef, ui: &mut Ui) -> bool {
    render_section(
        app,
        PROCESSING_STEP_SECTION,
        "Processing step",
        SectionNoun::new("processing step", "processing steps"),
        std::slice::from_ref(target),
        None,
        SectionLayout::Inline,
        ui,
    )
}

/// The processing menu is already titled Advanced, so its catalog section
/// renders all of its non-Essential rows directly instead of nesting another
/// Advanced disclosure.
pub(crate) fn processing_advanced_section(
    app: &mut PlotxApp,
    target: &TargetRef,
    ui: &mut Ui,
) -> bool {
    render_section(
        app,
        PROCESSING_ADVANCED_SECTION,
        "Advanced processing",
        SectionNoun::new("dataset", "datasets"),
        std::slice::from_ref(target),
        None,
        SectionLayout::Menu,
        ui,
    )
}

#[allow(clippy::too_many_arguments)]
fn render_section(
    app: &mut PlotxApp,
    section: &'static str,
    title: &'static str,
    status_noun: SectionNoun,
    targets: &[TargetRef],
    reset_encoding: Option<EncodingKind>,
    layout: SectionLayout,
    ui: &mut Ui,
) -> bool {
    if targets.is_empty() {
        return false;
    }
    let mut rows = resolve_rows_for(app, targets, section);
    if rows.is_empty() {
        return false;
    }
    if app.settings.appearance.canvas_accent.is_none()
        && let Some(row) = rows
            .iter_mut()
            .find(|row| row.presentation.id == app_preferences::ACCENT_COLOR)
    {
        let theme = ui.visuals().selection.bg_fill;
        let color = PropertyValue::Color(plotx_figure::Color::rgb(theme.r(), theme.g(), theme.b()));
        row.set.value = AggregateValue::Uniform(color.clone());
        row.representative.value = AggregateValue::Uniform(color.clone());
        row.representative.default_value = Some(color);
        row.representative.modified = Some(false);
    }

    let now = ui.input(|input| input.time);
    let focus = app.session.ui.property_focus;
    let focused_here =
        focus.is_some_and(|focus| rows.iter().any(|row| row.presentation.id == focus.property));

    // Every target in the selection is not what this section acts on: the
    // heading counts the ones that actually supply one of its rows, so a page
    // holding one contour plot and one line plot does not report two of each.
    let applicable = applicable_targets(&rows);

    if layout == SectionLayout::Standard {
        ui.separator();
        let response = ui
            .horizontal(|ui| {
                ui.strong(title);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.weak(status_noun.counted(applicable.len()));
                });
            })
            .response;
        if app.session.ui.requested_inspector_section.as_deref() == Some(section) {
            response.scroll_to_me(Some(egui::Align::Min));
            app.session.ui.requested_inspector_section = None;
        }
    }

    // The rows rendered without expanding anything are exactly the list the
    // budget check counts, so the check cannot pass while the panel shows more.
    let essential: Vec<PropertyId> = super::super::essential_in(section)
        .into_iter()
        .map(|entry| entry.id)
        .collect();
    let mut pending: Option<Pending> = None;
    let mut gesture: Option<(PropertyId, GestureEdge)> = None;
    let mut text_edits = std::mem::take(&mut app.session.ui.property_text_edits);
    let length_unit = app.session.ui.canvas_size_unit;
    let mut edits = RowEdits {
        pending: &mut pending,
        gesture: &mut gesture,
        text_edits: &mut text_edits,
    };
    for row in rows
        .iter()
        .filter(|row| essential.contains(&row.presentation.id))
    {
        if layout == SectionLayout::Inline {
            property_row_inline(row, focus, now, &mut edits, targets, length_unit, ui);
        } else {
            property_row(row, focus, now, &mut edits, targets, length_unit, ui);
        }
    }

    let advanced: Vec<&Row> = rows
        .iter()
        .filter(|row| !essential.contains(&row.presentation.id))
        .collect();
    if layout == SectionLayout::Menu {
        for row in advanced {
            property_row(row, focus, now, &mut edits, targets, length_unit, ui);
        }
    } else if !advanced.is_empty() {
        let id = ui.make_persistent_id(("property_section", section));
        let mut state =
            egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, false);
        if focused_here
            && focus.is_some_and(|focus| {
                advanced
                    .iter()
                    .any(|row| row.presentation.id == focus.property)
            })
        {
            state.set_open(true);
            state.store(ui.ctx());
        }
        egui::CollapsingHeader::new("Advanced")
            .id_salt(("property_section", section))
            .show(ui, |ui| {
                for row in advanced {
                    property_row(row, focus, now, &mut edits, targets, length_unit, ui);
                }
            });
    }

    if let Some(encoding) = reset_encoding {
        let label = match encoding {
            EncodingKind::Contour => "Reset contour",
            EncodingKind::Heatmap => "Reset heatmap",
            _ => "Reset series style",
        };
        if ui
            .small_button(label)
            .on_hover_text("Rebuild this series' encoding from its defaults")
            .clicked()
        {
            pending = Some(Pending::ResetEncoding(encoding));
        }
    }

    // The reveal is one-shot: once the section has been drawn with the row in
    // it, only the fading highlight remains.
    if let Some(focus) = app.session.ui.property_focus.as_mut()
        && focused_here
    {
        focus.pending = false;
    }
    if focus.is_some_and(|focus| now >= focus.highlight_until) {
        app.session.ui.property_focus = None;
    }
    app.session.ui.property_text_edits = text_edits;

    // Opened before the write and closed after it, so the frame that starts a
    // drag is already inside the gesture and the frame that ends one is the last
    // it records.
    if let Some((property, GestureEdge::Started)) = gesture {
        app.begin_property_gesture(property);
    }
    if let Some(pending) = pending {
        // Still the whole selection: a target this section cannot supply is
        // reported as a skip rather than quietly left out of the write.
        apply(app, targets, pending, status_noun);
    }
    if let Some((_, GestureEdge::Stopped)) = gesture {
        app.end_property_gesture();
    }
    true
}
