use super::*;

pub(crate) const BAND_EDGE_PX: f32 = 8.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BandHit<Id> {
    Edge { id: Id, lo_edge: bool },
    Inside { id: Id },
}

impl<Id: Copy> BandHit<Id> {
    pub(crate) fn id(self) -> Id {
        match self {
            Self::Edge { id, .. } | Self::Inside { id } => id,
        }
    }
}

/// Hit-test one-dimensional bands with their resize edges taking priority.
pub(crate) fn band_hit<Id: Copy>(
    bands: impl IntoIterator<Item = (Id, f64, f64)> + Clone,
    plot: PlotRect,
    xmin: f64,
    xspan: f64,
    xrev: bool,
    px: f32,
) -> Option<BandHit<Id>> {
    for (id, lo, hi) in bands.clone() {
        let sxlo = x_to_screen(lo, plot, xmin, xspan, xrev);
        let sxhi = x_to_screen(hi, plot, xmin, xspan, xrev);
        if (px - sxlo).abs() <= BAND_EDGE_PX {
            return Some(BandHit::Edge { id, lo_edge: true });
        }
        if (px - sxhi).abs() <= BAND_EDGE_PX {
            return Some(BandHit::Edge { id, lo_edge: false });
        }
    }
    bands.into_iter().find_map(|(id, lo, hi)| {
        let a = x_to_screen(lo, plot, xmin, xspan, xrev);
        let b = x_to_screen(hi, plot, xmin, xspan, xrev);
        (px >= a.min(b) && px <= a.max(b)).then_some(BandHit::Inside { id })
    })
}

pub(crate) fn edited_band_bounds(
    kind: RegionDragKind,
    anchor: f64,
    grab_lo: f64,
    grab_hi: f64,
    current: f64,
) -> (f64, f64) {
    match kind {
        RegionDragKind::NewBand => (anchor.min(current), anchor.max(current)),
        RegionDragKind::EdgeLo => (current, grab_hi),
        RegionDragKind::EdgeHi => (grab_lo, current),
        RegionDragKind::Move => {
            let delta = current - anchor;
            (grab_lo + delta, grab_hi + delta)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edges_win_over_band_interiors_on_a_reversed_axis() {
        let plot = PlotRect::new(0.0, 0.0, 100.0, 20.0);
        let bands = [(7_u64, 2.0, 4.0), (9, 4.0, 6.0)];
        assert_eq!(
            band_hit(bands, plot, 0.0, 10.0, true, 60.0),
            Some(BandHit::Edge {
                id: 7,
                lo_edge: false,
            })
        );
    }

    #[test]
    fn move_is_absolute_from_the_grab_snapshot() {
        assert_eq!(
            edited_band_bounds(RegionDragKind::Move, 5.0, 4.0, 6.0, 7.0),
            (6.0, 8.0)
        );
    }
}
