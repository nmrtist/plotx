use super::*;

/// The meaningful, linear axis a semantic plot interaction addresses.
///
/// This deliberately describes only presentation geometry.  It does not name a
/// scientific operation: domain state decides what a cursor or range means.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlotInteractionAxis {
    X,
}

/// Gestures a bound plot has elected to expose to semantic dispatch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlotInteractionGesture {
    Cursor,
    Range,
}

/// A bound, physical, linear axis which accepts semantic input.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlotInteractionDescriptor {
    pub dataset: DatasetId,
    pub canvas: CanvasId,
    pub object: ObjectId,
    pub field: FieldId,
    pub axis: PlotInteractionAxis,
    pub gestures: &'static [PlotInteractionGesture],
    /// The field's existing physical unit, not a display-label guess.
    pub unit: String,
}

/// Unit-aware input derived from a canvas hit.  This is transient and is never
/// part of a project payload or the undo history.
#[derive(Clone, Debug, PartialEq)]
pub enum PlotInteractionRequest {
    Cursor {
        target: PlotInteractionDescriptor,
        value: f64,
    },
    Range {
        target: PlotInteractionDescriptor,
        range: AxisRange,
    },
}

impl PlotInteractionDescriptor {
    pub fn accepts(&self, gesture: PlotInteractionGesture) -> bool {
        self.gestures.contains(&gesture)
    }

    pub fn cursor(&self, value: f64) -> Option<PlotInteractionRequest> {
        (self.accepts(PlotInteractionGesture::Cursor) && value.is_finite()).then(|| {
            PlotInteractionRequest::Cursor {
                target: self.clone(),
                value,
            }
        })
    }

    pub fn range(&self, start: f64, end: f64) -> Option<PlotInteractionRequest> {
        (self.accepts(PlotInteractionGesture::Range)
            && start.is_finite()
            && end.is_finite()
            && start != end)
            .then(|| PlotInteractionRequest::Range {
                target: self.clone(),
                range: AxisRange::new(start, end),
            })
    }
}
