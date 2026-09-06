use super::{Action, PanelActionError, PanelState};
use crate::state::{CanvasId, ObjectFrame, PanelId, PlotxApp};

impl PlotxApp {
    /// Exchange occupied panel slots, preserving panel identities and local layout.
    pub fn swap_panels_action(
        &self,
        canvas: CanvasId,
        source: PanelId,
        target: PanelId,
    ) -> Result<Action, PanelActionError> {
        let canvas = self
            .doc
            .canvas_index(canvas)
            .ok_or_else(|| PanelActionError::Invalid("the canvas no longer exists".to_owned()))?;
        if source == target {
            return Err(PanelActionError::Invalid(
                "choose two different panels".to_owned(),
            ));
        }
        let mut page = self.doc.canvases[canvas].clone();
        let before = PanelState::of(&page);
        let frames = [source, target].map(|id| {
            let panel = page.panel(id).ok_or(PanelActionError::MissingPanel(id))?;
            if panel.locked || !panel.visible {
                return Err(PanelActionError::Invalid(
                    "both panels must be visible and unlocked".to_owned(),
                ));
            }
            Ok(panel.frame)
        });
        let [source_frame, target_frame] = frames;
        let (source_frame, target_frame) = (source_frame?, target_frame?);
        for (id, frame) in [(source, target_frame), (target, source_frame)] {
            let panel = page.panel(id).expect("panels checked above");
            let scale = [
                frame.width / panel.frame.width,
                frame.height / panel.frame.height,
            ];
            let children = panel.item_order.clone();
            for child in children {
                let item = page
                    .object_mut(child)
                    .ok_or(PanelActionError::MissingContent(child))?;
                item.frame = ObjectFrame::new(
                    item.frame.x * scale[0],
                    item.frame.y * scale[1],
                    item.frame.width * scale[0],
                    item.frame.height * scale[1],
                );
            }
            page.panel_mut(id).expect("panels checked above").frame = frame;
        }
        page.validate_structure()
            .map_err(PanelActionError::Invalid)?;
        Ok(Action::ReplacePanelState {
            canvas,
            before,
            after: PanelState::of(&page),
        })
    }
}
