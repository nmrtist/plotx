/// Which sidebar entry an in-progress inline rename targets.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RenameTarget {
    Canvas(usize),
    Data(usize),
}

/// An active inline rename: the entry being edited plus its working buffer.
/// `focus` requests keyboard focus for one frame after the edit box appears.
pub struct RenameState {
    pub target: RenameTarget,
    pub buffer: String,
    pub focus: bool,
}
