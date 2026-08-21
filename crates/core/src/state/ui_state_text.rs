use super::ObjectId;

pub struct PanelNoteEditState {
    pub canvas: usize,
    pub object: ObjectId,
    pub buffer: String,
    pub focus: bool,
}

pub struct TextEditState {
    pub canvas: usize,
    pub object: ObjectId,
    pub buffer: String,
    pub focus: bool,
}
