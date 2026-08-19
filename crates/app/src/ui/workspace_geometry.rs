use egui::{Id, Rect};
use plotx_core::state::PlotxApp;

const GEOMETRY_ID: &str = "plotx.workspace_geometry";
const SIDEBAR_RECTS_ID: &str = "plotx.workspace_sidebar_rects";
const OCCLUDER_CLEARANCE: f32 = 12.0;
const SIDEBAR_GAP: f32 = 8.0;

#[derive(Clone, Copy, Debug, Default)]
struct SidebarRects {
    primary: Option<Rect>,
    secondary: Option<Rect>,
}

/// Authoritative geometry snapshot for one workspace frame. Persistent panels
/// are already removed from `board_rect`. Floating task cards contribute only
/// their actual bounds, and only to viewport-fit avoidance.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct WorkspaceGeometry {
    pub board_rect: Rect,
    pub fit_occluders: Vec<Rect>,
    pub revision: u64,
}

impl WorkspaceGeometry {
    /// Rectangles in which a fitted target can remain wholly unobscured. There
    /// is at most one workflow task card, so this remains constant-time.
    pub(crate) fn fit_candidates(&self) -> impl Iterator<Item = Rect> {
        let board = self.board_rect;
        let candidates = if let Some(occluder) = self.fit_occluders.first() {
            [
                Rect::from_min_max(board.min, egui::pos2(occluder.left(), board.bottom())),
                Rect::from_min_max(egui::pos2(board.left(), occluder.bottom()), board.max),
                Rect::from_min_max(board.min, egui::pos2(board.right(), occluder.top())),
                Rect::from_min_max(egui::pos2(occluder.right(), board.top()), board.max),
            ]
        } else {
            [board, Rect::NOTHING, Rect::NOTHING, Rect::NOTHING]
        };
        candidates
            .into_iter()
            .filter(|rect| rect.width() > 1.0 && rect.height() > 1.0)
    }
}

pub(crate) fn set_sidebar_rects(
    ctx: &egui::Context,
    primary: Option<Rect>,
    secondary: Option<Rect>,
) {
    ctx.data_mut(|data| {
        data.insert_temp(
            Id::new(SIDEBAR_RECTS_ID),
            SidebarRects { primary, secondary },
        );
    });
}

pub(super) fn resolve(app: &PlotxApp, host_rect: Rect, ctx: &egui::Context) -> WorkspaceGeometry {
    let sidebars = ctx
        .data(|data| data.get_temp::<SidebarRects>(Id::new(SIDEBAR_RECTS_ID)))
        .unwrap_or_default();
    let mut board_rect = host_rect;
    if let Some(primary) = sidebars.primary {
        board_rect.min.x = board_rect.min.x.max(primary.right() + SIDEBAR_GAP);
    }
    if let Some(secondary) = sidebars.secondary {
        board_rect.max.x = board_rect.max.x.min(secondary.left() - SIDEBAR_GAP);
    }
    if board_rect.min.x >= board_rect.max.x {
        board_rect = host_rect;
    }
    let fit_occluders = super::tools::task_card::visible_area_id(app)
        .and_then(|id| ctx.memory(|memory| memory.area_rect(id)))
        .map(|rect| rect.expand(OCCLUDER_CLEARANCE).intersect(board_rect))
        .filter(|rect| rect.width() > 1.0 && rect.height() > 1.0)
        .into_iter()
        .collect();
    let id = Id::new(GEOMETRY_ID);
    let previous = ctx.data(|data| data.get_temp::<WorkspaceGeometry>(id));
    let changed = previous
        .as_ref()
        .is_none_or(|old| old.board_rect != board_rect || old.fit_occluders != fit_occluders);
    let revision = previous
        .as_ref()
        .map_or(0, |old| old.revision.saturating_add(u64::from(changed)));
    let geometry = WorkspaceGeometry {
        board_rect,
        fit_occluders,
        revision,
    };
    ctx.data_mut(|data| data.insert_temp(id, geometry.clone()));
    geometry
}

pub(crate) fn board_rect(ctx: &egui::Context) -> Option<Rect> {
    ctx.data(|data| {
        data.get_temp::<WorkspaceGeometry>(Id::new(GEOMETRY_ID))
            .map(|geometry| geometry.board_rect)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{Pos2, Vec2};

    #[test]
    fn revision_changes_only_when_workspace_bounds_change() {
        let app = PlotxApp::new_with_settings(plotx_core::settings::Settings::default());
        let ctx = egui::Context::default();
        let first_rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(1200.0, 800.0));
        let first = resolve(&app, first_rect, &ctx);
        let same = resolve(&app, first_rect, &ctx);
        assert_eq!(same.revision, first.revision);

        let changed_rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(900.0, 800.0));
        let changed = resolve(&app, changed_rect, &ctx);
        assert_eq!(changed.revision, first.revision + 1);
        assert_eq!(changed.board_rect, changed_rect);
    }

    #[test]
    fn candidates_preserve_the_space_below_a_floating_card() {
        let board = Rect::from_min_size(Pos2::ZERO, Vec2::new(1000.0, 700.0));
        let card = Rect::from_min_max(egui::pos2(680.0, 20.0), egui::pos2(990.0, 420.0));
        let geometry = WorkspaceGeometry {
            board_rect: board,
            fit_occluders: vec![card],
            revision: 0,
        };
        let candidates = geometry.fit_candidates().collect::<Vec<_>>();

        assert!(candidates.iter().any(|rect| {
            rect.left() == board.left()
                && rect.right() == board.right()
                && rect.top() == card.bottom()
                && rect.bottom() == board.bottom()
        }));
        assert!(candidates.iter().all(|rect| {
            let overlap = rect.intersect(card);
            overlap.width() <= 0.0 || overlap.height() <= 0.0
        }));
    }

    #[test]
    fn persistent_sidebars_are_hard_board_boundaries() {
        let app = PlotxApp::new();
        let ctx = egui::Context::default();
        let host = Rect::from_min_size(Pos2::ZERO, Vec2::new(1200.0, 700.0));
        let primary = Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(220.0, 700.0));
        let secondary = Rect::from_min_max(egui::pos2(900.0, 0.0), egui::pos2(1200.0, 700.0));
        set_sidebar_rects(&ctx, Some(primary), Some(secondary));

        let geometry = resolve(&app, host, &ctx);

        assert_eq!(geometry.board_rect.left(), primary.right() + SIDEBAR_GAP);
        assert_eq!(geometry.board_rect.right(), secondary.left() - SIDEBAR_GAP);
    }
}
