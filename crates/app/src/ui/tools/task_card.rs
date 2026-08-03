//! Shared geometry for the canvas task cards (Regions, Curve Fit). Both anchor
//! to the same corner of the canvas, so the sizing rules live in one place.

use egui::{Pos2, RichText, Ui};
use egui_phosphor::regular as icon;
use plotx_core::state::{PlotxApp, TaskDockTab, Tool};

/// Width shared by every task card, and the gap it keeps from the canvas edges.
const WIDTH: f32 = 310.0;
const MARGIN: f32 = 12.0;
const TOP_OFFSET: f32 = 8.0;
/// Room the card header, frame and footprint need outside the resizable body.
const CHROME: f32 = 64.0;
/// Below this the body is useless anyway; the card is allowed to overhang.
const FLOOR: f32 = 120.0;
const COLLAPSED_HEIGHT: f32 = 96.0;
const MIN_SAFE_EDGE: f32 = 72.0;
const MIN_CARD_WIDTH: f32 = 120.0;

pub(super) struct TaskCardGeometry {
    pub pos: Pos2,
    pub width: f32,
    pub min_body_height: f32,
    pub max_body_height: f32,
}

/// Anchors a card to the host's top-right corner and sizes its body to the
/// height the canvas actually has.
///
/// `preferred_min_body` is honoured only while it fits: `egui::Resize` applies
/// `at_least(min).at_most(max)`, so a min taller than the host would win over
/// the fitted max and force the card past the canvas. `Area` then constrains it
/// to the screen and slides it up over the Ribbon, hiding the very buttons that
/// opened it. Clamping the min keeps a short window shrinking instead.
pub(super) fn geometry(host: &Ui, preferred_min_body: f32) -> TaskCardGeometry {
    let host_rect = host.max_rect();
    let width = card_width(host_rect);
    let pos = host_rect.right_top() + egui::vec2(-width - MARGIN, TOP_OFFSET);
    let max_body_height = (host_rect.bottom() - pos.y - CHROME).max(FLOOR);
    TaskCardGeometry {
        pos,
        width,
        min_body_height: preferred_min_body.min(max_body_height),
        max_body_height,
    }
}

fn card_width(host: egui::Rect) -> f32 {
    WIDTH.min(
        (host.width() - MIN_SAFE_EDGE - MARGIN * 3.0)
            .max(MIN_CARD_WIDTH)
            .min((host.width() - MARGIN * 2.0).max(1.0)),
    )
}

fn visible_card_collapsed(app: &PlotxApp) -> Option<bool> {
    let active = app.active_dataset();
    match app.session.ui.task_dock_active? {
        TaskDockTab::Processing => app
            .session
            .ui
            .processing_task_dataset
            .and_then(|id| app.doc.dataset_index(id))
            .filter(|dataset| Some(*dataset) == active)
            .map(|_| app.session.ui.processing_task_collapsed),
        TaskDockTab::Regions => app
            .session
            .ui
            .region_task_dataset
            .and_then(|id| app.doc.dataset_index(id))
            .filter(|dataset| Some(*dataset) == active)
            .map(|_| app.session.ui.region_task_collapsed),
        TaskDockTab::CurveFit => app
            .session
            .ui
            .curve_fit_task_dataset
            .filter(|dataset| Some(*dataset) == active)
            .map(|_| app.session.ui.curve_fit_task_collapsed),
        TaskDockTab::Statistics => app
            .session
            .ui
            .stat_task_dataset
            .filter(|dataset| Some(*dataset) == active)
            .map(|_| app.session.ui.stat_task_collapsed),
    }
}

/// Largest practical board-fit rectangle not covered by the visible task card.
/// Persistent sidebars are already excluded from `host`; expanded cards reserve
/// the right dock strip, while a collapsed one leaves the full-width band below
/// its header available.
pub(crate) fn safe_fit_rect(app: &PlotxApp, host: egui::Rect) -> egui::Rect {
    let Some(collapsed) = visible_card_collapsed(app) else {
        return host;
    };
    if collapsed {
        if host.height() <= MIN_SAFE_EDGE {
            return host;
        }
        let top = (host.top() + TOP_OFFSET + COLLAPSED_HEIGHT + MARGIN)
            .min(host.bottom() - MIN_SAFE_EDGE);
        return egui::Rect::from_min_max(egui::pos2(host.left(), top), host.max);
    }
    if host.width() <= MIN_SAFE_EDGE {
        return host;
    }
    let right = (host.right() - card_width(host) - MARGIN * 2.0)
        .clamp(host.left() + MIN_SAFE_EDGE, host.right());
    egui::Rect::from_min_max(host.min, egui::pos2(right, host.bottom()))
}

pub(super) fn is_active(app: &PlotxApp, tab: TaskDockTab) -> bool {
    app.session.ui.task_dock_active == Some(tab)
}

/// Labelled switcher shared by every page in the one top-right task dock.
/// Hidden task pages keep their state; selecting a tab also restores the
/// dataset that owns it so controls never edit a dataset other than the canvas.
pub(super) fn tab_bar(app: &mut PlotxApp, current: TaskDockTab, ui: &mut Ui) -> bool {
    let tabs = [
        (
            TaskDockTab::Processing,
            icon::FLOW_ARROW,
            "Process",
            app.session
                .ui
                .processing_task_dataset
                .and_then(|id| app.doc.dataset_index(id)),
        ),
        (
            TaskDockTab::Regions,
            icon::SELECTION,
            "Regions",
            app.session
                .ui
                .region_task_dataset
                .and_then(|id| app.doc.dataset_index(id)),
        ),
        (
            TaskDockTab::CurveFit,
            icon::CHART_LINE_UP,
            "Fit",
            app.session.ui.curve_fit_task_dataset,
        ),
        (
            TaskDockTab::Statistics,
            icon::FUNCTION,
            "Stats",
            app.session.ui.stat_task_dataset,
        ),
    ];
    let open = tabs
        .into_iter()
        .filter(|(_, _, _, dataset)| dataset.is_some())
        .collect::<Vec<_>>();
    if open.len() < 2 {
        return false;
    }
    let mut selected = None;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 3.0;
        for (tab, glyph, label, dataset) in &open {
            if ui
                .selectable_label(
                    *tab == current,
                    RichText::new(format!("{glyph} {label}")).small(),
                )
                .clicked()
            {
                selected = Some((*tab, *dataset));
            }
        }
    });
    if let Some((tab, dataset)) = selected {
        if app.session.ui.task_dock_active == Some(TaskDockTab::Regions)
            && tab != TaskDockTab::Regions
            && app.session.tool == Tool::Regions
        {
            app.set_tool(Tool::BrowseZoom);
        }
        app.session.ui.open_task_tab(tab);
        if let Some(dataset) = dataset {
            app.set_active_dataset(Some(dataset));
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use plotx_core::state::{CanvasDocument, Dataset, NmrDataset};
    use plotx_io::{Domain, NmrData};

    fn app_with_task(tab: TaskDockTab, collapsed: bool) -> PlotxApp {
        let mut app = PlotxApp::new();
        let data = NmrData {
            points: vec![num_complex::Complex64::new(0.0, 0.0); 2],
            domain: Domain::Time,
            spectral_width_hz: 1.0,
            observe_freq_mhz: 1.0,
            carrier_ppm: 0.0,
            nucleus: "1H".into(),
            source: "test".into(),
            group_delay: 0.0,
        };
        app.doc
            .datasets
            .push(Dataset::Nmr(Box::new(NmrDataset::load(data))));
        app.doc
            .canvases
            .push(CanvasDocument::new("p".into(), [100.0, 80.0]));
        app.focus_single(0);
        app.session.ui.task_dock_active = Some(tab);
        let id = app.doc.datasets[0].resource_id();
        match tab {
            TaskDockTab::Processing => {
                app.session.ui.processing_task_dataset = Some(id);
                app.session.ui.processing_task_collapsed = collapsed;
            }
            TaskDockTab::Regions => {
                app.session.ui.region_task_dataset = Some(id);
                app.session.ui.region_task_collapsed = collapsed;
            }
            _ => unreachable!(),
        }
        app
    }

    #[test]
    fn expanded_card_reserves_the_right_dock_strip() {
        let app = app_with_task(TaskDockTab::Processing, false);
        let host = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1000.0, 700.0));
        let safe = safe_fit_rect(&app, host);
        assert_eq!(safe.left(), host.left());
        assert!(safe.right() < host.right() - WIDTH);
        assert_eq!(safe.height(), host.height());
    }

    #[test]
    fn collapsed_card_uses_the_full_width_below_its_header() {
        let app = app_with_task(TaskDockTab::Regions, true);
        let host = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1000.0, 700.0));
        let safe = safe_fit_rect(&app, host);
        assert_eq!(safe.width(), host.width());
        assert!(safe.top() > host.top());
    }

    #[test]
    fn narrow_hosts_keep_valid_nonempty_fit_geometry() {
        let app = app_with_task(TaskDockTab::Processing, false);
        let host = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(250.0, 140.0));
        let safe = safe_fit_rect(&app, host);
        assert!(safe.is_finite());
        assert!(card_width(host) < WIDTH);
        assert!(safe.width() >= MIN_SAFE_EDGE && safe.height() > 0.0);
    }

    #[test]
    fn sub_minimum_width_expanded_card_uses_the_recoverable_host_rect() {
        let app = app_with_task(TaskDockTab::Processing, false);
        let host = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(48.0, 140.0));

        let safe = safe_fit_rect(&app, host);

        assert_eq!(safe, host);
        assert!(safe.is_finite());
    }

    #[test]
    fn sub_minimum_height_collapsed_card_uses_the_recoverable_host_rect() {
        let app = app_with_task(TaskDockTab::Regions, true);
        let host = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(250.0, 48.0));

        let safe = safe_fit_rect(&app, host);

        assert_eq!(safe, host);
        assert!(safe.is_finite());
    }
}
