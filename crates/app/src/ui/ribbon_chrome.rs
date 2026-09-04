use egui::{TextStyle, TextWrapMode, Ui, Vec2, WidgetText};
use egui_phosphor::regular as icon;
use plotx_core::state::{PlotxApp, WorkflowTab};

const INLINE_TITLE_MAX_WIDTH: f32 = 240.0;
const INLINE_TITLE_MIN_WIDTH: f32 = 72.0;

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct RibbonChrome {
    pub(super) macos_traffic_lights: Option<Vec2>,
}

impl RibbonChrome {
    #[cfg(target_os = "macos")]
    pub(crate) fn macos(traffic_lights_size: Vec2) -> Self {
        Self {
            macos_traffic_lights: Some(traffic_lights_size),
        }
    }
}

#[cfg(target_os = "macos")]
pub(crate) fn current(ctx: &egui::Context, frame: &eframe::Frame) -> RibbonChrome {
    use raw_window_handle::HasWindowHandle;

    let Some(size) = frame
        .window_handle()
        .ok()
        .and_then(|handle| eframe::WindowChromeMetrics::from_window_handle(&handle.as_raw()))
        .map(|metrics| metrics.traffic_lights_size / ctx.zoom_factor())
    else {
        return RibbonChrome::macos(egui::vec2(76.0, 28.0));
    };
    RibbonChrome::macos(size)
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn current(_ctx: &egui::Context, _frame: &eframe::Frame) -> RibbonChrome {
    RibbonChrome::default()
}

pub(crate) fn configure_viewport(viewport: egui::ViewportBuilder) -> egui::ViewportBuilder {
    // Windows and Linux draw a VS Code style title bar inside the content area;
    // macOS keeps the native traffic lights and system menu.
    #[cfg(not(target_os = "macos"))]
    let viewport = viewport.with_decorations(false);
    #[cfg(target_os = "macos")]
    let viewport = viewport
        .with_fullsize_content_view(true)
        .with_titlebar_shown(false)
        .with_title_shown(false)
        .with_titlebar_buttons_shown(true);
    viewport
}

pub(super) fn inline_project_title(app: &PlotxApp) -> Option<String> {
    let project = app
        .doc
        .project_path
        .as_deref()
        .and_then(std::path::Path::file_name)
        .map(|name| name.to_string_lossy().into_owned())
        .or_else(|| app.session.project_present.then(|| "Untitled".to_owned()))?;
    Some(if app.doc.dirty {
        format!("* {project}")
    } else {
        project
    })
}

pub(super) fn controls_need_compacting(
    app: &PlotxApp,
    ui: &Ui,
    leading: f32,
    row_width: f32,
) -> bool {
    leading + task_tabs_width(ui) + controls_width(app, ui, false) + 16.0 > row_width
}

pub(super) fn available_title_width(
    app: &PlotxApp,
    ui: &Ui,
    leading: f32,
    row_width: f32,
    compact_controls: bool,
) -> Option<f32> {
    let remaining = row_width
        - leading
        - task_tabs_width(ui)
        - controls_width(app, ui, compact_controls)
        - 24.0;
    (remaining >= INLINE_TITLE_MIN_WIDTH).then(|| remaining.min(INLINE_TITLE_MAX_WIDTH))
}

fn task_tabs_width(ui: &Ui) -> f32 {
    WorkflowTab::ALL
        .iter()
        .map(|tab| {
            text_width(
                ui,
                crate::typography::headline(tab.label()),
                TextStyle::Button,
            )
        })
        .sum::<f32>()
        + TASK_TAB_SPACING * (WorkflowTab::ALL.len().saturating_sub(1) as f32)
}

fn controls_width(app: &PlotxApp, ui: &Ui, compact: bool) -> f32 {
    use plotx_core::update::UpdateStatus;

    let collapse = if compact {
        icon::CARET_UP.to_owned()
    } else if app.session.ui.ribbon_expanded {
        format!("{} Collapse ribbon", icon::CARET_UP)
    } else {
        format!("{} Expand ribbon", icon::CARET_DOWN)
    };
    let search = if compact {
        icon::MAGNIFYING_GLASS.to_owned()
    } else {
        format!("{} Search commands", icon::MAGNIFYING_GLASS)
    };
    let update = match app.session.updates.status() {
        UpdateStatus::Downloading { percent, .. } if compact => {
            percent.map_or_else(|| icon::ARROW_CLOCKWISE.to_owned(), |p| format!("{p}%"))
        }
        UpdateStatus::Downloading { percent, .. } => {
            percent.map_or_else(|| "Updating…".to_owned(), |p| format!("Updating… {p}%"))
        }
        UpdateStatus::Installed { .. } if compact => icon::ARROW_CLOCKWISE.to_owned(),
        UpdateStatus::Installed { .. } => {
            format!("{} Restart to update", icon::ARROW_CLOCKWISE)
        }
        _ => String::new(),
    };
    let spacing = CONTROL_SPACING;
    [collapse, search, update]
        .into_iter()
        .filter(|text| !text.is_empty())
        .map(|text| text_width(ui, text, TextStyle::Button))
        .sum::<f32>()
        // The two sidebar layout toggles, the separator before them, and
        // their share of the item spacing.
        + 2.0 * SIDEBAR_TOGGLE_WIDTH
        + 6.0
        + 5.0 * spacing
}

/// Fixed width of one sidebar layout toggle; shared with the width estimate
/// in `controls_width` so compaction accounts for the pair.
pub(super) const SIDEBAR_TOGGLE_WIDTH: f32 = 30.0;

/// Fixed gap between task tabs. Command density applies only below this row,
/// so switching tabs cannot move the task buttons or trailing chrome.
pub(super) const TASK_TAB_SPACING: f32 = 8.0;

/// Gap between the trailing chrome controls.
pub(super) const CONTROL_SPACING: f32 = 8.0;

/// Always-visible sidebar toggle for the task row. The glyph mirrors the
/// window layout: the band marks which side the command controls and is
/// filled while that sidebar is visible, so the pair doubles as a live
/// layout indicator.
pub(super) fn sidebar_toggle_button(
    app: &mut PlotxApp,
    clipboard: &mut super::clipboard_table::ClipboardTablePaste,
    ui: &mut Ui,
    id: super::commands::CommandId,
) {
    use super::commands;

    let command = commands::describe(app, id);
    let sidebar_visible = command.checked == Some(true);
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(SIDEBAR_TOGGLE_WIDTH, ui.spacing().interact_size.y),
        egui::Sense::click(),
    );
    if response.clicked() {
        commands::execute(id, app, clipboard, ui.ctx());
    }
    let visuals = ui.style().interact(&response);
    if response.hovered() || response.is_pointer_button_down_on() {
        // Match the neighbouring frameless chrome buttons: a quiet fill that
        // appears only under the pointer.
        ui.painter()
            .rect_filled(rect, visuals.corner_radius, visuals.weak_bg_fill);
    }
    let color = if sidebar_visible {
        visuals.text_color()
    } else {
        ui.visuals().weak_text_color()
    };
    paint_sidebar_glyph(
        ui,
        rect,
        id == commands::CommandId::TogglePrimarySidebar,
        sidebar_visible,
        color,
    );
    let tip = match &command.shortcut {
        Some(shortcut) => format!("{} ({shortcut})", command.label),
        None => command.label.clone(),
    };
    response.on_hover_text(tip);
}

fn paint_sidebar_glyph(ui: &Ui, rect: egui::Rect, left: bool, filled: bool, color: egui::Color32) {
    let painter = ui.painter();
    let outer = egui::Rect::from_center_size(rect.center(), egui::vec2(16.0, 12.0));
    painter.rect_stroke(
        outer,
        3.0,
        egui::Stroke::new(1.2_f32, color),
        egui::StrokeKind::Inside,
    );
    let band = if left {
        egui::Rect::from_min_max(
            outer.min + egui::vec2(2.0, 2.0),
            egui::pos2(outer.min.x + 7.0, outer.max.y - 2.0),
        )
    } else {
        egui::Rect::from_min_max(
            egui::pos2(outer.max.x - 7.0, outer.min.y + 2.0),
            outer.max - egui::vec2(2.0, 2.0),
        )
    };
    if filled {
        painter.rect_filled(band, 1.5, color);
    } else {
        painter.rect_stroke(
            band,
            1.5,
            egui::Stroke::new(1.0_f32, color),
            egui::StrokeKind::Inside,
        );
    }
}

fn text_width(ui: &Ui, text: impl Into<WidgetText>, fallback: TextStyle) -> f32 {
    text.into()
        .into_galley(ui, Some(TextWrapMode::Extend), f32::INFINITY, fallback)
        .size()
        .x
        + 2.0 * ui.spacing().button_padding.x
}

pub(super) fn frame(dark: bool) -> egui::Frame {
    #[cfg(target_os = "macos")]
    {
        // The task row occupies the native title-bar area, so its surface must
        // meet the window edges instead of floating below them as a card.
        super::card_frame(
            dark,
            egui::Margin {
                left: 0,
                right: 0,
                top: 0,
                bottom: 4,
            },
        )
        .corner_radius(0)
        .inner_margin(egui::Margin {
            left: 8,
            right: 8,
            top: 0,
            bottom: 8,
        })
        .shadow(egui::epaint::Shadow::NONE)
    }
    #[cfg(not(target_os = "macos"))]
    {
        super::card_frame(
            dark,
            egui::Margin {
                left: 8,
                right: 8,
                top: 4,
                bottom: 4,
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app() -> PlotxApp {
        PlotxApp::new_with_settings(plotx_core::settings::Settings::default())
    }

    #[test]
    fn inline_title_only_describes_a_present_project() {
        let mut app = app();
        assert_eq!(inline_project_title(&app), None);

        app.session.project_present = true;
        assert_eq!(inline_project_title(&app).as_deref(), Some("Untitled"));

        app.doc.dirty = true;
        assert_eq!(inline_project_title(&app).as_deref(), Some("* Untitled"));

        app.doc.project_path = Some(std::path::PathBuf::from("/tmp/report.plotx"));
        assert_eq!(
            inline_project_title(&app).as_deref(),
            Some("* report.plotx")
        );
    }

    #[test]
    fn task_tabs_width_uses_the_fixed_tab_spacing() {
        let ctx = crate::typography::test_context();
        let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
            let labels_width = WorkflowTab::ALL
                .iter()
                .map(|tab| {
                    text_width(
                        ui,
                        crate::typography::headline(tab.label()),
                        TextStyle::Button,
                    )
                })
                .sum::<f32>();
            let gaps = WorkflowTab::ALL.len().saturating_sub(1) as f32;

            assert_eq!(task_tabs_width(ui), labels_width + gaps * 8.0);
            assert_eq!(TASK_TAB_SPACING, 8.0);
        });
    }
}
