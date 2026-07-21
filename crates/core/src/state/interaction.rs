use super::*;

/// A live drag of the Peaks tool's threshold line: `y` is the previewed detection
/// floor in data coordinates, committed as one `SetPeaks` on release.
pub struct PeakThresholdDrag {
    pub canvas: usize,
    pub object: ObjectId,
    pub dataset: usize,
    pub y: f64,
}

/// A live drag of the Peaks tool's pick band: on release, every peak inside
/// `[anchor_x, current_x]` is added. A zero-width band is a plain click-to-place.
pub struct PeakBandDrag {
    pub canvas: usize,
    pub object: ObjectId,
    pub dataset: usize,
    pub anchor_x: f64,
    pub current_x: f64,
}

/// The single in-flight canvas gesture. At most one direct-manipulation gesture
/// runs at a time: a gesture start replaces any prior one and every clear funnels
/// through [`PlotxApp::reset_interaction`]. The `tile_drop` and `snap_guides`
/// previews stay separate fields — they are derived views of an `Object` drag,
/// valid only for its lifetime.
pub enum Interaction {
    Idle,
    Object(ObjectDrag),
    Marquee(MarqueeDrag),
    PanelLabel(PanelLabelDrag),
    Frame(FrameDrag),
    Author(AuthorDrag),
    Zoom(ZoomDrag),
    Selection(SelectionDrag),
    Pan(PanDrag),
    Phase(PhaseDrag),
    Region(RegionDrag),
    Integral(IntegralDrag),
    Integral2D(Integral2DDrag),
    PeakThreshold(PeakThresholdDrag),
    PeakBand(PeakBandDrag),
}

/// The tool family a gesture belongs to: layout gestures manipulate whole objects
/// or frames (the Select tool and the board); data gestures act inside a plot
/// (the data tools).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GestureFamily {
    Idle,
    Layout,
    Data,
}

impl Interaction {
    pub fn is_active(&self) -> bool {
        !matches!(self, Interaction::Idle)
    }

    pub fn family(&self) -> GestureFamily {
        match self {
            Interaction::Idle => GestureFamily::Idle,
            Interaction::Object(_)
            | Interaction::Marquee(_)
            | Interaction::PanelLabel(_)
            | Interaction::Frame(_)
            | Interaction::Author(_) => GestureFamily::Layout,
            Interaction::Zoom(_)
            | Interaction::Selection(_)
            | Interaction::Pan(_)
            | Interaction::Phase(_)
            | Interaction::Region(_)
            | Interaction::Integral(_)
            | Interaction::Integral2D(_)
            | Interaction::PeakThreshold(_)
            | Interaction::PeakBand(_) => GestureFamily::Data,
        }
    }

    /// The canvas this gesture targets, when it names one. A `Frame` drag is keyed
    /// by `FrameRef` and a `Phase` drag by dataset, so both are canvas-agnostic.
    pub fn canvas(&self) -> Option<usize> {
        match self {
            Interaction::Object(d) => Some(d.canvas),
            Interaction::Marquee(d) => Some(d.canvas),
            Interaction::PanelLabel(d) => Some(d.canvas),
            Interaction::Author(d) => Some(d.canvas),
            Interaction::Zoom(d) => Some(d.canvas),
            Interaction::Selection(d) => Some(d.canvas),
            Interaction::Pan(d) => Some(d.canvas),
            Interaction::Region(d) => Some(d.canvas),
            Interaction::Integral(d) => Some(d.canvas),
            Interaction::Integral2D(d) => Some(d.canvas),
            Interaction::PeakThreshold(d) => Some(d.canvas),
            Interaction::PeakBand(d) => Some(d.canvas),
            Interaction::Idle | Interaction::Frame(_) | Interaction::Phase(_) => None,
        }
    }

    /// Sanity check for [`PlotxApp::begin_interaction`]: the gesture's family fits
    /// the active tool and, when it names one, its canvas is the active canvas.
    pub fn belongs_to(&self, tool: Tool, active_canvas: Option<usize>) -> bool {
        // Ambient navigation gestures ride under any tool: a data-pan and the
        // single-axis strip zoom target the plot under the cursor directly.
        if matches!(self, Interaction::Pan(_))
            || matches!(self, Interaction::Zoom(z) if z.axis != ZoomAxis::Box)
        {
            return active_canvas.is_none_or(|c| self.canvas() == Some(c));
        }
        let family_ok = match self.family() {
            GestureFamily::Idle => true,
            // A frame drag rides the board under any tool; other layout gestures
            // belong to the Select tool or an authoring create-tool.
            GestureFamily::Layout => {
                matches!(self, Interaction::Frame(_))
                    || tool.is_layout_tool()
                    || tool.creates_object()
            }
            GestureFamily::Data => tool.is_data_tool(),
        };
        let canvas_ok = self.canvas().is_none_or(|c| active_canvas == Some(c));
        family_ok && canvas_ok
    }
}
