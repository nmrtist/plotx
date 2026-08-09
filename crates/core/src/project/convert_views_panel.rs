use super::*;

pub(super) fn panel_to_dto(panel: &Panel) -> ViewPanel {
    let (participates, position, font_size) = (
        panel.label.participates_in_sequence,
        panel.label.position,
        panel.label.font_size,
    );
    let label = match &panel.label.mode {
        PanelLabelMode::Auto { slot } => PanelLabelDto::Auto {
            slot: *slot,
            visible: panel.label.visible,
            participates,
            position,
            font_size,
        },
        PanelLabelMode::LockedAuto { value } => PanelLabelDto::LockedAuto {
            value: value.clone(),
            visible: panel.label.visible,
            participates,
            position,
            font_size,
        },
        PanelLabelMode::Manual { value } => PanelLabelDto::Manual {
            value: value.clone(),
            visible: panel.label.visible,
            participates,
            position,
            font_size,
        },
    };
    ViewPanel {
        id: panel.id.to_string(),
        name: panel.name.clone(),
        frame: FrameDto::from_frame(panel.frame),
        item_order: panel.item_order.iter().map(ToString::to_string).collect(),
        label,
        note: panel.note.clone(),
        visible: panel.visible,
        locked: panel.locked,
        clip_children: panel.clip_children,
        layout: match panel.layout {
            PanelLayout::Free => PanelLayoutDto::Free,
            PanelLayout::VerticalStack => PanelLayoutDto::VerticalStack,
            PanelLayout::HorizontalStack => PanelLayoutDto::HorizontalStack,
            PanelLayout::Grid { rows, cols } => PanelLayoutDto::Grid { rows, cols },
        },
    }
}

pub(super) fn group_to_dto(group: &LayoutGroup) -> ViewGroup {
    ViewGroup {
        id: group.id,
        members: group
            .members
            .iter()
            .map(|member| match member {
                GroupMember::Panel(id) => ViewGroupMember::Panel { id: id.to_string() },
                GroupMember::Content(id) => ViewGroupMember::Content { id: id.to_string() },
            })
            .collect(),
    }
}

pub(super) fn panel_from_dto(dto: &ViewPanel) -> Result<Panel> {
    let (mode, visible, participates, position, font_size) = match &dto.label {
        PanelLabelDto::Auto {
            slot,
            visible,
            participates,
            position,
            font_size,
        } => (
            PanelLabelMode::Auto { slot: *slot },
            *visible,
            *participates,
            *position,
            *font_size,
        ),
        PanelLabelDto::LockedAuto {
            value,
            visible,
            participates,
            position,
            font_size,
        } => (
            PanelLabelMode::LockedAuto {
                value: value.clone(),
            },
            *visible,
            *participates,
            *position,
            *font_size,
        ),
        PanelLabelDto::Manual {
            value,
            visible,
            participates,
            position,
            font_size,
        } => (
            PanelLabelMode::Manual {
                value: value.clone(),
            },
            *visible,
            *participates,
            *position,
            *font_size,
        ),
    };
    Ok(Panel {
        id: dto
            .id
            .parse::<PanelId>()
            .map_err(|_| ProjectError::Invalid(format!("invalid panel id {}", dto.id)))?,
        name: dto.name.clone(),
        frame: dto.frame.into_frame(),
        item_order: dto
            .item_order
            .iter()
            .map(|id| {
                id.parse::<ObjectId>()
                    .map_err(|_| ProjectError::Invalid(format!("invalid panel content id {id}")))
            })
            .collect::<Result<_>>()?,
        label: PanelLabelSpec {
            mode,
            visible,
            participates_in_sequence: participates,
            position,
            font_size,
        },
        note: dto.note.clone(),
        visible: dto.visible,
        locked: dto.locked,
        clip_children: dto.clip_children,
        layout: match dto.layout {
            PanelLayoutDto::Free => PanelLayout::Free,
            PanelLayoutDto::VerticalStack => PanelLayout::VerticalStack,
            PanelLayoutDto::HorizontalStack => PanelLayout::HorizontalStack,
            PanelLayoutDto::Grid { rows, cols } => PanelLayout::Grid { rows, cols },
        },
    })
}

pub(super) fn group_from_dto(dto: &ViewGroup) -> Result<LayoutGroup> {
    Ok(LayoutGroup {
        id: dto.id,
        members: dto
            .members
            .iter()
            .map(|member| match member {
                ViewGroupMember::Panel { id } => id
                    .parse::<PanelId>()
                    .map(GroupMember::Panel)
                    .map_err(|_| ProjectError::Invalid(format!("invalid group panel id {id}"))),
                ViewGroupMember::Content { id } => id
                    .parse::<ObjectId>()
                    .map(GroupMember::Content)
                    .map_err(|_| ProjectError::Invalid(format!("invalid group content id {id}"))),
            })
            .collect::<Result<_>>()?,
    })
}
