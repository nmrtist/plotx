use plotx_render::screen::ScreenRenderDetail;
use std::time::Duration;

const STATE_ID: &str = "plotx.workspace_interactive_render";
const INTERACTIVE_SECONDS: f64 = 0.2;

#[derive(Clone, Copy, Default)]
struct InteractiveRenderState {
    until: f64,
}

pub(super) fn mark_workspace_navigation(ctx: &egui::Context, now: f64) {
    ctx.data_mut(|data| {
        data.insert_temp(
            egui::Id::new(STATE_ID),
            InteractiveRenderState {
                until: now + INTERACTIVE_SECONDS,
            },
        );
    });
    ctx.request_repaint_after(Duration::from_millis(200));
}

pub(super) fn workspace_render_detail(ctx: &egui::Context, now: f64) -> ScreenRenderDetail {
    let id = egui::Id::new(STATE_ID);
    let state = ctx.data_mut(|data| data.get_temp::<InteractiveRenderState>(id));
    let Some(state) = state else {
        return ScreenRenderDetail::Full;
    };
    if now < state.until {
        ctx.request_repaint_after(Duration::from_secs_f64(state.until - now));
        ScreenRenderDetail::Interactive
    } else {
        ctx.data_mut(|data| data.remove_temp::<InteractiveRenderState>(id));
        ScreenRenderDetail::Full
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workspace_navigation_detail_expires_after_quiet_period() {
        let ctx = egui::Context::default();
        assert_eq!(
            workspace_render_detail(&ctx, 10.0),
            ScreenRenderDetail::Full
        );
        mark_workspace_navigation(&ctx, 10.0);
        assert_eq!(
            workspace_render_detail(&ctx, 10.199),
            ScreenRenderDetail::Interactive
        );
        assert_eq!(
            workspace_render_detail(&ctx, 10.2),
            ScreenRenderDetail::Full
        );
    }
}
