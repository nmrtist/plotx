use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CanvasSizeUnit {
    Mm,
    Cm,
    Inch,
    Pixel,
}

impl CanvasSizeUnit {
    pub fn all() -> &'static [Self] {
        &[Self::Mm, Self::Cm, Self::Inch, Self::Pixel]
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Mm => "mm",
            Self::Cm => "cm",
            Self::Inch => "in",
            Self::Pixel => "px",
        }
    }

    // Symmetric counterpart to `to_mm`: converts a value expressed in mm into this unit.
    #[allow(clippy::wrong_self_convention)]
    pub fn from_mm(self, value_mm: f32) -> f32 {
        match self {
            Self::Mm => value_mm,
            Self::Cm => value_mm / 10.0,
            Self::Inch => value_mm / MM_PER_IN,
            Self::Pixel => value_mm / MM_PER_IN * PX_PER_IN,
        }
    }

    pub fn to_mm(self, value: f32) -> f32 {
        match self {
            Self::Mm => value,
            Self::Cm => value * 10.0,
            Self::Inch => value * MM_PER_IN,
            Self::Pixel => value / PX_PER_IN * MM_PER_IN,
        }
    }

    pub fn drag_range(self) -> std::ops::RangeInclusive<f32> {
        self.from_mm(10.0)..=self.from_mm(1000.0)
    }

    pub fn drag_speed(self) -> f64 {
        match self {
            Self::Mm => 1.0,
            Self::Cm => 0.1,
            Self::Inch => 0.05,
            Self::Pixel => 10.0,
        }
    }

    pub fn decimals(self) -> usize {
        match self {
            Self::Mm | Self::Pixel => 1,
            Self::Cm | Self::Inch => 3,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PrimaryView {
    Canvas,
    Data,
}

impl PrimaryView {
    pub fn all() -> &'static [Self] {
        &[Self::Canvas, Self::Data]
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Canvas => "Canvas",
            Self::Data => "Data",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    Select,
    BrowseZoom,
    ManualPhase,
    SelectRegion,
    Regions,
    Integrate,
    Peaks,
    Slice,
    LineFit,
    Annotate,
    PeakAnalysis,
    Text,
    PanelLabel,
    Rect,
    Ellipse,
    Line,
    Arrow,
}

/// The three tool families. Navigate + Author are universal (they live in the
/// top toolbar); Analyze tools are domain-specific and surface in the Secondary
/// Side Bar for a focused figure.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolAxis {
    Navigate,
    Author,
    Analyze,
    Layout,
}

impl Tool {
    pub fn label(self) -> &'static str {
        match self {
            Self::Select => "Select",
            Self::BrowseZoom => "Browse / zoom",
            Self::ManualPhase => "Manual phase",
            Self::SelectRegion => "Analysis range",
            Self::Regions => "Regions",
            Self::Integrate => "Integrate",
            Self::Peaks => "Peaks",
            Self::Slice => "Slice",
            Self::LineFit => "Peak fit",
            Self::Annotate => "Annotate",
            Self::PeakAnalysis => "Peak analysis",
            Self::Text => "Text",
            Self::PanelLabel => "Panel label",
            Self::Rect => "Rectangle",
            Self::Ellipse => "Ellipse",
            Self::Line => "Line",
            Self::Arrow => "Arrow",
        }
    }

    pub fn axis(self) -> ToolAxis {
        match self {
            Self::Select => ToolAxis::Layout,
            Self::BrowseZoom => ToolAxis::Navigate,
            Self::Annotate
            | Self::Text
            | Self::PanelLabel
            | Self::Rect
            | Self::Ellipse
            | Self::Line
            | Self::Arrow => ToolAxis::Author,
            Self::ManualPhase
            | Self::SelectRegion
            | Self::Regions
            | Self::Integrate
            | Self::Peaks
            | Self::Slice
            | Self::LineFit
            | Self::PeakAnalysis => ToolAxis::Analyze,
        }
    }

    /// True for the page-space authoring tools that stamp a new canvas object on
    /// click (everything in the Author axis except `Annotate`, which is data
    /// space). A click with one of these active creates rather than selects.
    pub fn creates_object(self) -> bool {
        matches!(
            self,
            Self::Text | Self::PanelLabel | Self::Rect | Self::Ellipse | Self::Line | Self::Arrow
        )
    }

    pub fn is_data_tool(self) -> bool {
        matches!(
            self,
            Tool::BrowseZoom
                | Tool::ManualPhase
                | Tool::SelectRegion
                | Tool::Regions
                | Tool::Integrate
                | Tool::Peaks
                | Tool::Slice
                | Tool::LineFit
                | Tool::PeakAnalysis
                | Tool::Annotate
        )
    }

    pub fn is_layout_tool(self) -> bool {
        matches!(self, Tool::Select)
    }

    pub fn shape_kind(self) -> Option<ShapeKind> {
        match self {
            Self::Rect => Some(ShapeKind::Rect),
            Self::Ellipse => Some(ShapeKind::Ellipse),
            Self::Line => Some(ShapeKind::Line),
            Self::Arrow => Some(ShapeKind::Arrow),
            _ => None,
        }
    }
}

/// A phaseable/processable frequency dimension. `Direct` is the 1D axis (and the
/// direct axis of a pseudo-2D stack); `F2`/`F1` are the direct/indirect axes of a
/// true-2D spectrum. Lets the phase panel and canvas drag address any dimension
/// through one generic accessor rather than per-domain code.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PhaseAxis {
    Direct,
    F2,
    F1,
}

/// Which screen direction an axis's pivot line runs on the canvas: F1 (the y
/// dimension of a 2D contour) is horizontal, everything else vertical.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PhaseOrient {
    Vertical,
    Horizontal,
}

impl PhaseAxis {
    pub fn orient(self) -> PhaseOrient {
        match self {
            PhaseAxis::F1 => PhaseOrient::Horizontal,
            _ => PhaseOrient::Vertical,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            PhaseAxis::Direct => "Direct",
            PhaseAxis::F2 => "F2 (direct)",
            PhaseAxis::F1 => "F1 (indirect)",
        }
    }
}

/// A processing tool group a dataset's domain exposes. The Secondary Side Bar
/// renders a dataset's groups in order rather than hard-coding a panel per
/// format, so adding a data type only means implementing `Dataset::tool_groups`.
/// Processing is domain-neutral (it dispatches on the dataset kind at render
/// time) so 1D and 2D expose it in the same slot.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ToolGroup {
    Processing,
    Nmr1dAnalysis,
    Nmr2dExperiment,
    RegionAnalysis,
    Peaks,
    CurveFit,
    LineFit,
    Statistics,
    Electrophysiology,
}

impl ToolGroup {
    pub fn title(self) -> &'static str {
        match self {
            ToolGroup::Processing => "Processing",
            ToolGroup::Nmr1dAnalysis => "Analysis",
            ToolGroup::Nmr2dExperiment => "Experiment",
            ToolGroup::RegionAnalysis => "Region analysis",
            ToolGroup::Peaks => "Peaks",
            ToolGroup::CurveFit => "Curve Fit",
            ToolGroup::LineFit => "Peak Fit",
            ToolGroup::Statistics => "Statistics",
            ToolGroup::Electrophysiology => "Patch clamp",
        }
    }
}
