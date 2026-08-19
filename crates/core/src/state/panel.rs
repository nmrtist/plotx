use super::{CanvasDocument, ContentId, ObjectFrame, PanelId};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PanelLabelMode {
    Auto { slot: u64 },
    LockedAuto { value: String },
    Manual { value: String },
}

#[derive(Clone, Debug, PartialEq)]
pub struct PanelLabelSpec {
    pub mode: PanelLabelMode,
    pub visible: bool,
    pub participates_in_sequence: bool,
    pub position: [f32; 2],
    pub font_size: f32,
}

impl PanelLabelSpec {
    pub fn auto(slot: u64) -> Self {
        Self {
            mode: PanelLabelMode::Auto { slot },
            visible: true,
            participates_in_sequence: true,
            position: [6.0, 5.0],
            font_size: 8.0,
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        if !self.position.into_iter().all(f32::is_finite)
            || !self.font_size.is_finite()
            || self.font_size <= 0.0
        {
            return Err("panel label geometry must be finite and positive".to_owned());
        }
        match &self.mode {
            PanelLabelMode::Manual { value } | PanelLabelMode::LockedAuto { value }
                if value.trim().is_empty() =>
            {
                Err("manual and locked panel labels must not be empty".to_owned())
            }
            _ => Ok(()),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PanelLayout {
    #[default]
    Free,
    VerticalStack,
    HorizontalStack,
    Grid {
        rows: u32,
        cols: u32,
    },
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PanelAlignment {
    #[default]
    Stretch,
    Start,
    Center,
    End,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Panel {
    pub id: PanelId,
    pub name: String,
    pub frame: ObjectFrame,
    pub item_order: Vec<ContentId>,
    pub label: PanelLabelSpec,
    pub note: String,
    pub visible: bool,
    pub locked: bool,
    pub clip_children: bool,
    pub layout: PanelLayout,
    pub layout_gap: f32,
    pub layout_padding: f32,
    pub layout_alignment: PanelAlignment,
}

impl Panel {
    pub fn new(name: String, frame: ObjectFrame, slot: u64) -> Self {
        Self {
            id: PanelId::new(),
            name,
            frame,
            item_order: Vec::new(),
            label: PanelLabelSpec::auto(slot),
            note: String::new(),
            visible: true,
            locked: false,
            clip_children: false,
            layout: PanelLayout::Free,
            layout_gap: 6.0,
            layout_padding: 6.0,
            layout_alignment: PanelAlignment::Stretch,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum GroupMember {
    Panel(PanelId),
    Content(ContentId),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LayoutGroup {
    pub id: u64,
    pub members: Vec<GroupMember>,
}

pub fn validate_panel_structure(
    panels: &[Panel],
    content_ids: impl IntoIterator<Item = ContentId>,
    groups: &[LayoutGroup],
) -> Result<(), String> {
    let content_ids: BTreeSet<_> = content_ids.into_iter().collect();
    let mut panel_ids = BTreeSet::new();
    let mut parents = BTreeMap::new();
    for panel in panels {
        if !panel_ids.insert(panel.id) {
            return Err(format!("duplicate panel id {}", panel.id));
        }
        validate_frame(panel.frame, "panel")?;
        panel.label.validate()?;
        let mut order = BTreeSet::new();
        for &content in &panel.item_order {
            if !content_ids.contains(&content) {
                return Err(format!(
                    "panel {} references missing content {content}",
                    panel.id
                ));
            }
            if !order.insert(content) {
                return Err(format!("panel {} repeats content {content}", panel.id));
            }
            if let Some(first) = parents.insert(content, panel.id) {
                return Err(format!(
                    "content {content} belongs to both panel {first} and {}",
                    panel.id
                ));
            }
        }
        if let PanelLayout::Grid { rows, cols } = panel.layout
            && (rows == 0 || cols == 0)
        {
            return Err(format!("panel {} has an empty grid dimension", panel.id));
        }
        if !panel.layout_gap.is_finite()
            || panel.layout_gap < 0.0
            || !panel.layout_padding.is_finite()
            || panel.layout_padding < 0.0
        {
            return Err(format!("panel {} has invalid layout spacing", panel.id));
        }
    }
    let mut group_ids = BTreeSet::new();
    let mut grouped_members = BTreeMap::new();
    for group in groups {
        if !group_ids.insert(group.id) {
            return Err(format!("duplicate group id {}", group.id));
        }
        let mut members = BTreeSet::new();
        let mut kind = None;
        let mut content_scope = None;
        if group.members.len() < 2 {
            return Err(format!(
                "group {} must contain at least two members",
                group.id
            ));
        }
        for member in &group.members {
            if !members.insert(*member) {
                return Err(format!("group {} repeats a member", group.id));
            }
            if let Some(first_group) = grouped_members.insert(*member, group.id) {
                return Err(format!(
                    "member belongs to both group {first_group} and {}",
                    group.id
                ));
            }
            let member_kind = match member {
                GroupMember::Panel(id) => {
                    if !panel_ids.contains(id) {
                        return Err(format!("group {} references missing panel {id}", group.id));
                    }
                    0
                }
                GroupMember::Content(id) => {
                    if !content_ids.contains(id) {
                        return Err(format!(
                            "group {} references missing content {id}",
                            group.id
                        ));
                    }
                    let scope = parents.get(id).copied();
                    if content_scope
                        .replace(scope)
                        .is_some_and(|prior| prior != scope)
                    {
                        return Err(format!(
                            "group {} contains content from different scopes",
                            group.id
                        ));
                    }
                    1
                }
            };
            if kind
                .replace(member_kind)
                .is_some_and(|prior| prior != member_kind)
            {
                return Err(format!("group {} mixes panels and content", group.id));
            }
        }
        if group.members.iter().any(|member| match member {
            GroupMember::Panel(panel) => panels
                .iter()
                .find(|candidate| candidate.id == *panel)
                .is_some_and(|panel| {
                    panel
                        .item_order
                        .iter()
                        .any(|content| group.members.contains(&GroupMember::Content(*content)))
                }),
            GroupMember::Content(_) => false,
        }) {
            return Err(format!(
                "group {} contains a panel and its content",
                group.id
            ));
        }
    }
    Ok(())
}

pub fn validate_frame(frame: ObjectFrame, what: &str) -> Result<(), String> {
    if [frame.x, frame.y, frame.width, frame.height]
        .into_iter()
        .all(f32::is_finite)
        && frame.width > 0.0
        && frame.height > 0.0
    {
        Ok(())
    } else {
        Err(format!("{what} frame must be finite and positive"))
    }
}

impl CanvasDocument {
    /// Automatic labels distinguish multi-panel pages; explicit labels remain
    /// visible even when the page has only one panel.
    pub fn panel_label_is_displayed(&self, panel: PanelId) -> bool {
        let Some(panel) = self.panel(panel) else {
            return false;
        };
        if !panel.visible || !panel.label.visible {
            return false;
        }
        if !matches!(panel.label.mode, PanelLabelMode::Auto { .. }) {
            return true;
        }
        self.panels
            .iter()
            .filter(|candidate| {
                candidate.visible
                    && candidate.label.visible
                    && candidate.label.participates_in_sequence
            })
            .take(2)
            .count()
            >= 2
    }

    /// Page-space frame manipulated by the canvas authoring UI. Parented
    /// content is represented by its semantic panel; loose content uses its
    /// own frame. This is the single coordinate contract for selection,
    /// snapping, arranging and drag actions.
    pub fn layout_frame(&self, content: ContentId) -> Option<ObjectFrame> {
        self.parent_panel(content)
            .and_then(|id| self.panel(id))
            .map(|panel| panel.frame)
            .or_else(|| self.object(content).map(|item| item.frame))
    }

    /// Apply a page-space authoring frame without mixing it with a content
    /// item's panel-local coordinates. Resizing a multi-content panel scales
    /// its children proportionally; moving it preserves their local layout.
    pub fn set_layout_frame(&mut self, content: ContentId, frame: ObjectFrame) -> bool {
        let Some(panel_id) = self.parent_panel(content) else {
            let Some(item) = self.object_mut(content) else {
                return false;
            };
            item.frame = frame;
            return true;
        };
        let Some(before) = self.panel(panel_id).map(|panel| panel.frame) else {
            return false;
        };
        let scale_x = frame.width / before.width;
        let scale_y = frame.height / before.height;
        let children = self
            .panel(panel_id)
            .map(|panel| panel.item_order.clone())
            .unwrap_or_default();
        if scale_x != 1.0 || scale_y != 1.0 {
            for id in children {
                if let Some(item) = self.object_mut(id) {
                    item.frame.x *= scale_x;
                    item.frame.y *= scale_y;
                    item.frame.width *= scale_x;
                    item.frame.height *= scale_y;
                }
            }
        }
        if let Some(panel) = self.panel_mut(panel_id) {
            panel.frame = frame;
            true
        } else {
            false
        }
    }

    /// Give loose panel-capable content its default one-item semantic panel.
    pub fn create_panel_for_content(&mut self, content: ContentId) -> Option<PanelId> {
        if let Some(panel) = self.parent_panel(content) {
            return Some(panel);
        }
        let (name, frame) = {
            let item = self.object(content)?;
            if !matches!(
                item.kind,
                super::CanvasObjectKind::Plot(_) | super::CanvasObjectKind::RasterImage(_)
            ) {
                return None;
            }
            (item.name.clone(), item.frame)
        };
        let panel_id = self.create_panel(name.clone(), frame);
        if let Some(item) = self.object_mut(content) {
            item.frame.x = 0.0;
            item.frame.y = 0.0;
        }
        let panel = self.panel_mut(panel_id)?;
        panel.item_order.push(content);
        Some(panel_id)
    }

    pub fn create_panel_for_plot(&mut self, content: ContentId) -> Option<PanelId> {
        self.object(content)?.plot()?;
        self.create_panel_for_content(content)
    }
}

#[cfg(test)]
#[path = "panel_tests.rs"]
mod tests;
