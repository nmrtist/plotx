use super::*;

#[derive(Serialize, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct AssetEntry {
    pub id: String,
    pub sha256: String,
    pub path: String,
    pub format: String,
    pub byte_len: u64,
    pub pixel_size: [u32; 2],
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ParentPanelDto {
    Loose,
    Panel { id: String },
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct RasterImageDto {
    pub asset: String,
    pub page_index: u32,
    pub crop: [f32; 4],
    pub fit: String,
    pub rotation: u16,
    pub opacity: f32,
    pub interpolation: String,
    pub preserve_aspect: bool,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct ViewPanel {
    pub id: String,
    pub name: String,
    pub frame: FrameDto,
    pub item_order: Vec<String>,
    pub label: PanelLabelDto,
    pub note: String,
    pub visible: bool,
    pub locked: bool,
    pub clip_children: bool,
    pub layout: PanelLayoutDto,
    pub layout_gap: f32,
    pub layout_padding: f32,
    pub layout_alignment: String,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
pub enum PanelLabelDto {
    Auto {
        slot: u64,
        visible: bool,
        participates: bool,
        position: [f32; 2],
        font_size: f32,
    },
    LockedAuto {
        value: String,
        visible: bool,
        participates: bool,
        position: [f32; 2],
        font_size: f32,
    },
    Manual {
        value: String,
        visible: bool,
        participates: bool,
        position: [f32; 2],
        font_size: f32,
    },
}

#[derive(Serialize, Deserialize, Clone, Copy)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
pub enum PanelLayoutDto {
    Free,
    VerticalStack,
    HorizontalStack,
    Grid { rows: u32, cols: u32 },
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct ViewGroup {
    pub id: u64,
    pub members: Vec<ViewGroupMember>,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ViewGroupMember {
    Panel { id: String },
    Content { id: String },
}
