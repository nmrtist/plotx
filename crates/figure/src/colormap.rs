//! Named colormaps for value-mapped fills (heatmap cells, surface facets).
//! Sampling lives here rather than in the render crate so figure builders can
//! also bake per-facet colors (e.g. the 3D surface chart) from the same tables.

use crate::Color;

/// A perceptually ordered colormap identified by a stable string id (persisted
/// in `.plotx` chart specs, so variants must never be renamed).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ColormapId {
    #[default]
    Viridis,
    Plasma,
    Inferno,
    Magma,
    Turbo,
    Coolwarm,
    Grays,
}

impl ColormapId {
    pub const ALL: [ColormapId; 7] = [
        ColormapId::Viridis,
        ColormapId::Plasma,
        ColormapId::Inferno,
        ColormapId::Magma,
        ColormapId::Turbo,
        ColormapId::Coolwarm,
        ColormapId::Grays,
    ];

    /// Map a normalized value in `[0, 1]` to a color. Out-of-range and
    /// non-finite inputs clamp so callers can feed raw normalized data.
    pub fn sample(self, t: f32) -> Color {
        let t = if t.is_finite() {
            t.clamp(0.0, 1.0) as f64
        } else {
            0.0
        };
        let c = match self {
            ColormapId::Viridis => colorous::VIRIDIS.eval_continuous(t),
            ColormapId::Plasma => colorous::PLASMA.eval_continuous(t),
            ColormapId::Inferno => colorous::INFERNO.eval_continuous(t),
            ColormapId::Magma => colorous::MAGMA.eval_continuous(t),
            ColormapId::Turbo => colorous::TURBO.eval_continuous(t),
            // RED_BLUE runs red→blue; flip so low = cool (blue), high = warm (red).
            ColormapId::Coolwarm => colorous::RED_BLUE.eval_continuous(1.0 - t),
            ColormapId::Grays => colorous::GREYS.eval_continuous(t),
        };
        Color::rgb(c.r, c.g, c.b)
    }

    /// Stable persistence id.
    pub fn id(self) -> &'static str {
        match self {
            ColormapId::Viridis => "viridis",
            ColormapId::Plasma => "plasma",
            ColormapId::Inferno => "inferno",
            ColormapId::Magma => "magma",
            ColormapId::Turbo => "turbo",
            ColormapId::Coolwarm => "coolwarm",
            ColormapId::Grays => "grays",
        }
    }

    /// Human-readable name for UI pickers.
    pub fn name(self) -> &'static str {
        match self {
            ColormapId::Viridis => "Viridis",
            ColormapId::Plasma => "Plasma",
            ColormapId::Inferno => "Inferno",
            ColormapId::Magma => "Magma",
            ColormapId::Turbo => "Turbo",
            ColormapId::Coolwarm => "Coolwarm",
            ColormapId::Grays => "Grays",
        }
    }

    /// Lenient id lookup: unknown ids (from a newer file) fall back to `None`
    /// so the loader can substitute the default.
    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|c| c.id() == id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_round_trip_and_stay_stable() {
        for cm in ColormapId::ALL {
            assert_eq!(ColormapId::from_id(cm.id()), Some(cm));
        }
        assert_eq!(ColormapId::from_id("viridis"), Some(ColormapId::Viridis));
        assert_eq!(ColormapId::from_id("not-a-map"), None);
    }

    #[test]
    fn sampling_clamps_and_orients_low_cool_high_warm() {
        let low = ColormapId::Coolwarm.sample(0.0);
        let high = ColormapId::Coolwarm.sample(1.0);
        assert!(low.b > low.r, "low end should be cool: {low:?}");
        assert!(high.r > high.b, "high end should be warm: {high:?}");
        assert_eq!(
            ColormapId::Viridis.sample(-3.0),
            ColormapId::Viridis.sample(0.0)
        );
        assert_eq!(
            ColormapId::Viridis.sample(f32::NAN),
            ColormapId::Viridis.sample(0.0)
        );
    }
}
