use super::{Action, PanelState};
use crate::state::{
    ContentId, GroupMember, LayoutGroup, ObjectFrame, Panel, PanelId, PanelLabelMode, PlotxApp,
};

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum PanelActionError {
    #[error("canvas {0} does not exist")]
    MissingCanvas(usize),
    #[error("panel {0} does not exist")]
    MissingPanel(PanelId),
    #[error("content {0} does not exist")]
    MissingContent(ContentId),
    #[error("content {0} is already assigned to a panel")]
    AlreadyAssigned(ContentId),
    #[error("at least one content item is required")]
    EmptySelection,
    #[error("panel operation is invalid: {0}")]
    Invalid(String),
}

impl PlotxApp {
    pub fn create_panel_action(
        &self,
        canvas: usize,
        name: String,
        frame: ObjectFrame,
    ) -> Result<(PanelId, Action), PanelActionError> {
        let before = self.panel_state(canvas)?;
        let mut page = self.doc.canvases[canvas].clone();
        let id = page.create_panel(name, frame);
        page.validate_structure()
            .map_err(PanelActionError::Invalid)?;
        Ok((
            id,
            Action::ReplacePanelState {
                canvas,
                before,
                after: PanelState::of(&page),
            },
        ))
    }

    pub fn compose_panel_action(
        &self,
        canvas: usize,
        name: String,
        contents: &[ContentId],
        padding: f32,
    ) -> Result<(PanelId, Action), PanelActionError> {
        if contents.is_empty() {
            return Err(PanelActionError::EmptySelection);
        }
        let before = self.panel_state(canvas)?;
        let mut page = self.doc.canvases[canvas].clone();
        for &content in contents {
            if page.object(content).is_none() {
                return Err(PanelActionError::MissingContent(content));
            }
            if page.parent_panel(content).is_some() {
                return Err(PanelActionError::AlreadyAssigned(content));
            }
        }
        let frame = bounds(
            contents
                .iter()
                .filter_map(|id| page.object(*id).map(|v| v.frame)),
        )
        .ok_or(PanelActionError::EmptySelection)?;
        let frame = ObjectFrame::new(
            frame.x - padding,
            frame.y - padding,
            frame.width + padding * 2.0,
            frame.height + padding * 2.0,
        );
        let id = page.create_panel(name, frame);
        for &content in contents {
            let item = page
                .object_mut(content)
                .ok_or(PanelActionError::MissingContent(content))?;
            item.frame.x -= frame.x;
            item.frame.y -= frame.y;
        }
        page.panel_mut(id).expect("new panel exists").item_order = contents.to_vec();
        page.reconcile_content_group_scopes();
        page.validate_structure()
            .map_err(PanelActionError::Invalid)?;
        Ok((
            id,
            Action::ReplacePanelState {
                canvas,
                before,
                after: PanelState::of(&page),
            },
        ))
    }

    pub fn dissolve_panel_action(
        &self,
        canvas: usize,
        panel: PanelId,
    ) -> Result<Action, PanelActionError> {
        let before = self.panel_state(canvas)?;
        let mut page = self.doc.canvases[canvas].clone();
        let index = page
            .panels
            .iter()
            .position(|value| value.id == panel)
            .ok_or(PanelActionError::MissingPanel(panel))?;
        let removed = page.panels.remove(index);
        for content in removed.item_order {
            let item = page
                .object_mut(content)
                .ok_or(PanelActionError::MissingContent(content))?;
            item.frame.x += removed.frame.x;
            item.frame.y += removed.frame.y;
        }
        page.groups.retain_mut(|group| {
            group
                .members
                .retain(|member| *member != crate::state::GroupMember::Panel(panel));
            group.members.len() > 1
        });
        page.reconcile_content_group_scopes();
        Ok(Action::ReplacePanelState {
            canvas,
            before,
            after: PanelState::of(&page),
        })
    }

    pub fn delete_panel_action(
        &self,
        canvas: usize,
        panel: PanelId,
    ) -> Result<Action, PanelActionError> {
        let before = self.panel_state(canvas)?;
        let mut page = self.doc.canvases[canvas].clone();
        let index = page
            .panels
            .iter()
            .position(|value| value.id == panel)
            .ok_or(PanelActionError::MissingPanel(panel))?;
        let removed = page.panels.remove(index);
        page.objects
            .retain(|item| !removed.item_order.contains(&item.id));
        page.groups.retain_mut(|group| {
            group.members.retain(|member| match member {
                crate::state::GroupMember::Panel(id) => *id != panel,
                crate::state::GroupMember::Content(id) => !removed.item_order.contains(id),
            });
            group.members.len() > 1
        });
        Ok(Action::ReplacePanelState {
            canvas,
            before,
            after: PanelState::of(&page),
        })
    }

    pub fn move_content_to_panel_action(
        &self,
        canvas: usize,
        content: ContentId,
        target: Option<PanelId>,
        target_index: usize,
    ) -> Result<Action, PanelActionError> {
        let before = self.panel_state(canvas)?;
        let mut page = self.doc.canvases[canvas].clone();
        let page_frame = page
            .content_page_frame(content)
            .ok_or(PanelActionError::MissingContent(content))?;
        for panel in &mut page.panels {
            panel.item_order.retain(|id| *id != content);
        }
        let target_frame = target
            .map(|id| {
                page.panel(id)
                    .map(|v| v.frame)
                    .ok_or(PanelActionError::MissingPanel(id))
            })
            .transpose()?;
        let item = page
            .object_mut(content)
            .ok_or(PanelActionError::MissingContent(content))?;
        item.frame = match target_frame {
            Some(panel) => ObjectFrame {
                x: page_frame.x - panel.x,
                y: page_frame.y - panel.y,
                ..page_frame
            },
            None => page_frame,
        };
        if let Some(target) = target {
            let order = &mut page
                .panel_mut(target)
                .ok_or(PanelActionError::MissingPanel(target))?
                .item_order;
            order.insert(target_index.min(order.len()), content);
        }
        page.reconcile_content_group_scopes();
        page.validate_structure()
            .map_err(PanelActionError::Invalid)?;
        Ok(Action::ReplacePanelState {
            canvas,
            before,
            after: PanelState::of(&page),
        })
    }

    pub fn split_panel_action(
        &self,
        canvas: usize,
        source: PanelId,
        contents: &[ContentId],
        name: String,
    ) -> Result<(PanelId, Action), PanelActionError> {
        if contents.is_empty() {
            return Err(PanelActionError::EmptySelection);
        }
        let mut page = self
            .doc
            .canvases
            .get(canvas)
            .cloned()
            .ok_or(PanelActionError::MissingCanvas(canvas))?;
        let source_panel = page
            .panel(source)
            .cloned()
            .ok_or(PanelActionError::MissingPanel(source))?;
        if contents
            .iter()
            .any(|id| !source_panel.item_order.contains(id))
        {
            return Err(PanelActionError::Invalid(
                "split content must belong to the source panel".to_owned(),
            ));
        }
        let page_frames: Vec<_> = contents
            .iter()
            .map(|id| {
                page.content_page_frame(*id)
                    .ok_or(PanelActionError::MissingContent(*id))
            })
            .collect::<Result<_, _>>()?;
        let frame = bounds(page_frames).ok_or(PanelActionError::EmptySelection)?;
        let before = PanelState::of(&page);
        page.panel_mut(source)
            .expect("source checked")
            .item_order
            .retain(|id| !contents.contains(id));
        let new_id = page.create_panel(name, frame);
        page.panel_mut(new_id).expect("new panel exists").item_order = contents.to_vec();
        for &id in contents {
            let absolute = ObjectFrame {
                x: source_panel.frame.x + page.object(id).expect("checked").frame.x,
                y: source_panel.frame.y + page.object(id).expect("checked").frame.y,
                ..page.object(id).expect("checked").frame
            };
            let item = page.object_mut(id).expect("checked");
            item.frame = ObjectFrame {
                x: absolute.x - frame.x,
                y: absolute.y - frame.y,
                ..absolute
            };
        }
        page.reconcile_content_group_scopes();
        page.validate_structure()
            .map_err(PanelActionError::Invalid)?;
        Ok((
            new_id,
            Action::ReplacePanelState {
                canvas,
                before,
                after: PanelState::of(&page),
            },
        ))
    }

    pub fn merge_panels_action(
        &self,
        canvas: usize,
        primary: PanelId,
        others: &[PanelId],
    ) -> Result<Action, PanelActionError> {
        let before = self.panel_state(canvas)?;
        let mut page = self.doc.canvases[canvas].clone();
        let mut ids = vec![primary];
        ids.extend_from_slice(others);
        ids.sort();
        ids.dedup();
        let selected: Vec<Panel> = ids
            .iter()
            .map(|id| {
                page.panel(*id)
                    .cloned()
                    .ok_or(PanelActionError::MissingPanel(*id))
            })
            .collect::<Result<_, _>>()?;
        if selected.len() < 2 {
            return Err(PanelActionError::Invalid(
                "merge requires two panels".to_owned(),
            ));
        }
        let frame = bounds(selected.iter().map(|panel| panel.frame)).expect("non-empty");
        let mut ordered: Vec<_> = selected
            .iter()
            .flat_map(|panel| {
                panel
                    .item_order
                    .iter()
                    .copied()
                    .map(move |content| (content, panel.frame))
            })
            .collect();
        ordered.sort_by_key(|(content, _)| {
            page.objects
                .iter()
                .position(|item| item.id == *content)
                .unwrap_or(usize::MAX)
        });
        for (content, old_panel) in &ordered {
            let item = page
                .object_mut(*content)
                .ok_or(PanelActionError::MissingContent(*content))?;
            item.frame.x += old_panel.x - frame.x;
            item.frame.y += old_panel.y - frame.y;
        }
        let primary_index = page
            .panels
            .iter()
            .position(|panel| panel.id == primary)
            .ok_or(PanelActionError::MissingPanel(primary))?;
        page.panels[primary_index].frame = frame;
        page.panels[primary_index].item_order = ordered.iter().map(|(id, _)| *id).collect();
        let notes: Vec<_> = selected
            .iter()
            .filter(|panel| panel.id != primary)
            .map(|panel| panel.note.trim())
            .filter(|note| !note.is_empty())
            .collect();
        if !notes.is_empty() {
            page.panels[primary_index]
                .note
                .push_str(&format!("\n\n{}", notes.join("\n\n")));
        }
        page.panels
            .retain(|panel| panel.id == primary || !ids.contains(&panel.id));
        let affected_group_ids: Vec<_> = page
            .groups
            .iter()
            .filter(|group| {
                group
                    .members
                    .iter()
                    .any(|member| matches!(member, GroupMember::Panel(id) if ids.contains(id)))
            })
            .map(|group| group.id)
            .collect();
        if let Some(&group_id) = affected_group_ids.first() {
            let mut members = std::collections::BTreeSet::new();
            for group in page
                .groups
                .iter()
                .filter(|group| affected_group_ids.contains(&group.id))
            {
                for member in &group.members {
                    members.insert(match member {
                        GroupMember::Panel(id) if ids.contains(id) => GroupMember::Panel(primary),
                        member => *member,
                    });
                }
            }
            page.groups
                .retain(|group| !affected_group_ids.contains(&group.id));
            if members.len() >= 2 {
                page.groups.push(LayoutGroup {
                    id: group_id,
                    members: members.into_iter().collect(),
                });
            }
        }
        page.reconcile_content_group_scopes();
        page.validate_structure()
            .map_err(PanelActionError::Invalid)?;
        Ok(Action::ReplacePanelState {
            canvas,
            before,
            after: PanelState::of(&page),
        })
    }

    pub fn duplicate_panel_action(
        &self,
        canvas: usize,
        panel: PanelId,
        offset: [f32; 2],
    ) -> Result<(PanelId, Action), PanelActionError> {
        let before = self.panel_state(canvas)?;
        let mut page = self.doc.canvases[canvas].clone();
        let source = page
            .panel(panel)
            .cloned()
            .ok_or(PanelActionError::MissingPanel(panel))?;
        let slot = page.next_panel_label_slot;
        page.next_panel_label_slot = slot.saturating_add(1);
        let mut copy = source.clone();
        copy.id = PanelId::new();
        copy.frame.x += offset[0];
        copy.frame.y += offset[1];
        copy.label.mode = PanelLabelMode::Auto { slot };
        copy.item_order.clear();
        for id in source.item_order {
            let mut item = page
                .object(id)
                .cloned()
                .ok_or(PanelActionError::MissingContent(id))?;
            item.id = page.allocate_object_id();
            copy.item_order.push(item.id);
            page.objects.push(item);
        }
        let id = copy.id;
        page.panels.push(copy);
        Ok((
            id,
            Action::ReplacePanelState {
                canvas,
                before,
                after: PanelState::of(&page),
            },
        ))
    }

    pub fn reorder_panel_labels_action(&self, canvas: usize) -> Result<Action, PanelActionError> {
        let before = self.panel_state(canvas)?;
        let mut page = self.doc.canvases[canvas].clone();
        let order = page.panel_reading_order();
        let mut slot = 0_u64;
        for id in order {
            let panel = page
                .panel_mut(id)
                .ok_or(PanelActionError::MissingPanel(id))?;
            if panel.label.participates_in_sequence {
                if matches!(panel.label.mode, PanelLabelMode::Auto { .. }) {
                    panel.label.mode = PanelLabelMode::Auto { slot };
                }
                slot = slot.saturating_add(1);
            }
        }
        page.next_panel_label_slot = page.next_panel_label_slot.max(slot);
        Ok(Action::ReplacePanelState {
            canvas,
            before,
            after: PanelState::of(&page),
        })
    }

    fn panel_state(&self, canvas: usize) -> Result<PanelState, PanelActionError> {
        self.doc
            .canvases
            .get(canvas)
            .map(PanelState::of)
            .ok_or(PanelActionError::MissingCanvas(canvas))
    }
}

fn bounds(frames: impl IntoIterator<Item = ObjectFrame>) -> Option<ObjectFrame> {
    let mut frames = frames.into_iter();
    let first = frames.next()?;
    let mut x0 = first.x;
    let mut y0 = first.y;
    let mut x1 = first.x + first.width;
    let mut y1 = first.y + first.height;
    for frame in frames {
        x0 = x0.min(frame.x);
        y0 = y0.min(frame.y);
        x1 = x1.max(frame.x + frame.width);
        y1 = y1.max(frame.y + frame.height);
    }
    Some(ObjectFrame::new(x0, y0, x1 - x0, y1 - y0))
}
