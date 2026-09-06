use super::{Action, PanelActionError, PanelState};
use crate::state::{CanvasDocument, CanvasId, GroupMember, PanelId, PlotxApp};

impl PlotxApp {
    /// Move a complete Panel into a fitted page as one reversible transaction.
    pub fn detach_panel_action(
        &self,
        canvas: CanvasId,
        panel: PanelId,
        board_pos: [f32; 2],
    ) -> Result<Action, PanelActionError> {
        let ci = self
            .doc
            .canvas_index(canvas)
            .ok_or_else(|| PanelActionError::Invalid("the canvas no longer exists".to_owned()))?;
        if !board_pos.into_iter().all(f32::is_finite) {
            return Err(PanelActionError::Invalid(
                "invalid board position".to_owned(),
            ));
        }
        let source = &self.doc.canvases[ci];
        let mut moved = source
            .panel(panel)
            .cloned()
            .ok_or(PanelActionError::MissingPanel(panel))?;
        if moved.locked || !moved.visible {
            return Err(PanelActionError::Invalid(
                "the panel must be visible and unlocked".to_owned(),
            ));
        }
        source
            .validate_structure()
            .map_err(PanelActionError::Invalid)?;
        let ids = moved.item_order.clone();
        let mut destination = CanvasDocument::new(
            moved.name.clone(),
            [
                moved.frame.width * 25.4 / 72.0,
                moved.frame.height * 25.4 / 72.0,
            ],
        );
        destination.board_pos = board_pos;
        destination.background = source.background;
        destination.panel_label_style = source.panel_label_style;
        destination.next_panel_label_slot = source.next_panel_label_slot;
        destination.next_object_id = source.next_object_id;
        destination.next_group_id = source.next_group_id;
        destination.objects = source
            .objects
            .iter()
            .filter(|item| ids.contains(&item.id))
            .cloned()
            .collect();
        destination.groups = source
            .groups
            .iter()
            .filter(|group| {
                group
                    .members
                    .iter()
                    .all(|member| matches!(member, GroupMember::Content(id) if ids.contains(id)))
            })
            .cloned()
            .collect();
        destination.x_viewport_links = source.x_viewport_links.clone();
        destination.x_viewport_links.retain_mut(|group| {
            group.members.retain(|id| ids.contains(id));
            group.members.len() >= 2
        });
        moved.frame.x = 0.0;
        moved.frame.y = 0.0;
        destination.panels.push(moved);
        destination
            .validate_structure()
            .map_err(PanelActionError::Invalid)?;

        let before = PanelState::of(source);
        let mut after = before.clone();
        after.panels.retain(|item| item.id != panel);
        after.objects.retain(|item| !ids.contains(&item.id));
        after.groups.retain_mut(|group| {
            group.members.retain(|member| match member {
                GroupMember::Panel(id) => *id != panel,
                GroupMember::Content(id) => !ids.contains(id),
            });
            group.members.len() >= 2
        });
        let mut source_links = source.x_viewport_links.clone();
        source_links.retain_mut(|group| {
            group.members.retain(|id| !ids.contains(id));
            group.members.len() >= 2
        });
        // Undo must restore the source contents before reactivating its dataset context.
        Ok(Action::Composite(vec![
            Action::InsertCanvas {
                index: self.doc.canvases.len(),
                canvas: Box::new(destination),
                active_before: self.session.active_canvas,
                auto_place: false,
            },
            Action::ReplacePanelState {
                canvas: ci,
                before,
                after,
            },
            Action::SetXViewportLinks {
                canvas: ci,
                before: source.x_viewport_links.clone(),
                after: source_links,
            },
        ]))
    }
}

#[cfg(test)]
#[path = "panel_detach_tests.rs"]
mod tests;
