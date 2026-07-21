/// Task-oriented top-level workspace used by the desktop Ribbon. It is
/// transient chrome state: switching tasks changes command discovery, not the
/// scientific document or the active tool by itself.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum WorkflowTab {
    Data,
    Process,
    #[default]
    Analyze,
    Figure,
    Arrange,
    View,
}

impl WorkflowTab {
    /// Pipeline order: data -> process -> analyze -> figure -> arrange, with
    /// View last as the meta tab (matching the Office convention).
    pub const ALL: [Self; 6] = [
        Self::Data,
        Self::Process,
        Self::Analyze,
        Self::Figure,
        Self::Arrange,
        Self::View,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::View => "View",
            Self::Data => "Data",
            Self::Process => "Process",
            Self::Analyze => "Analyze",
            Self::Figure => "Figure",
            Self::Arrange => "Arrange",
        }
    }
}
