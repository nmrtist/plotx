use plotx_analysis::series::ReduceOp;
use serde::{Deserialize, Serialize};

use super::DatasetId;

/// Stable identity of one analysis region within its source field.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
#[serde(transparent)]
pub struct RegionId(u64);

impl RegionId {
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    pub const fn get(self) -> u64 {
        self.0
    }

    pub fn checked_next(self) -> Option<Self> {
        self.0.checked_add(1).map(Self)
    }
}

impl std::fmt::Display for RegionId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(formatter)
    }
}

/// Owner-scoped identity of a selected analysis region.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RegionSelection {
    pub dataset: DatasetId,
    pub region: RegionId,
}

impl RegionSelection {
    pub const fn new(dataset: DatasetId, region: RegionId) -> Self {
        Self { dataset, region }
    }

    pub fn in_dataset(self, dataset: DatasetId) -> Option<RegionId> {
        (self.dataset == dataset).then_some(self.region)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Region {
    pub id: RegionId,
    pub lo: f64,
    pub hi: f64,
    pub name: String,
    /// Manually placed label center as fractions of the plot rectangle.
    pub label_position: Option<[f32; 2]>,
    pub color: [u8; 3],
    /// `None` follows the dataset's default metric.
    pub metric: Option<RegionMetric>,
}

impl Region {
    pub fn lo_min(&self) -> f64 {
        self.lo.min(self.hi)
    }

    pub fn hi_max(&self) -> f64 {
        self.lo.max(self.hi)
    }

    pub fn center(&self) -> f64 {
        0.5 * (self.lo + self.hi)
    }

    pub fn column_name(&self, unit: &str) -> String {
        if self.name.trim().is_empty() {
            if unit.is_empty() {
                format!("{:.3}", self.center())
            } else {
                format!("{:.3} {unit}", self.center())
            }
        } else {
            self.name.clone()
        }
    }
}

/// Persistent state shared by every field that exposes ordered 1D members.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RegionAnalysisState {
    pub regions: Vec<Region>,
    pub default_metric: RegionMetric,
    pub next_region_id: RegionId,
    pub show_annotations: bool,
}

impl Default for RegionAnalysisState {
    fn default() -> Self {
        Self {
            regions: Vec::new(),
            default_metric: RegionMetric::Height,
            next_region_id: RegionId::default(),
            show_annotations: true,
        }
    }
}

impl RegionAnalysisState {
    pub fn allocate_region_id(&mut self) -> Option<RegionId> {
        let id = self.next_region_id;
        self.next_region_id = id.checked_next()?;
        Some(id)
    }

    pub fn validate(&self) -> Result<(), String> {
        let mut ids = std::collections::HashSet::with_capacity(self.regions.len());
        for region in &self.regions {
            if !ids.insert(region.id) {
                return Err(format!("duplicate region id {}", region.id));
            }
            if !region.lo.is_finite() || !region.hi.is_finite() {
                return Err(format!("region {} has non-finite bounds", region.id));
            }
            if region.label_position.is_some_and(|[x, y]| {
                !x.is_finite()
                    || !y.is_finite()
                    || !(0.0..=1.0).contains(&x)
                    || !(0.0..=1.0).contains(&y)
            }) {
                return Err(format!(
                    "region {} has an invalid label position",
                    region.id
                ));
            }
            if region.id >= self.next_region_id {
                return Err(format!(
                    "next region id {} does not follow region {}",
                    self.next_region_id, region.id
                ));
            }
        }
        if self.next_region_id.checked_next().is_none() {
            return Err("region id space is exhausted".to_owned());
        }
        Ok(())
    }
}

/// Serializable UI mirror of the analysis [`ReduceOp`]; keep in sync with it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum RegionMetric {
    #[default]
    Height,
    Area,
    Max,
    Min,
    Mean,
}

impl RegionMetric {
    pub fn label(self) -> &'static str {
        match self {
            Self::Height => "Height",
            Self::Area => "Area",
            Self::Max => "Max",
            Self::Min => "Min",
            Self::Mean => "Mean",
        }
    }

    pub fn all() -> &'static [RegionMetric] {
        &[Self::Height, Self::Area, Self::Max, Self::Min, Self::Mean]
    }
}

impl From<RegionMetric> for ReduceOp {
    fn from(m: RegionMetric) -> Self {
        match m {
            RegionMetric::Height => ReduceOp::Height,
            RegionMetric::Area => ReduceOp::Area,
            RegionMetric::Max => ReduceOp::Max,
            RegionMetric::Min => ReduceOp::Min,
            RegionMetric::Mean => ReduceOp::Mean,
        }
    }
}

pub const REGION_PALETTE: [[u8; 3]; 12] = [
    [0x1a, 0x7f, 0x37],
    [0x2b, 0x6c, 0xb0],
    [0xc0, 0x4a, 0x2b],
    [0x7a, 0x4f, 0xa3],
    [0xb8, 0x8a, 0x1e],
    [0x2f, 0x8f, 0x8f],
    [0xd9, 0x5f, 0x02],
    [0x56, 0xb4, 0xe9],
    [0xcc, 0x79, 0xa7],
    [0x6f, 0x4e, 0x37],
    [0x00, 0x70, 0x73],
    [0xe3, 0x77, 0xc2],
];

pub fn region_color(i: usize) -> [u8; 3] {
    if let Some(color) = REGION_PALETTE.get(i) {
        return *color;
    }
    // Golden-angle hue spacing avoids a visible cycle when a dataset has more
    // regions than the curated palette. Alternating saturation/value bands keep
    // later hues distinguishable from earlier hues with a similar angle.
    let hue = ((i as f64) * 0.618_033_988_749_894_9).fract();
    let band = (i / REGION_PALETTE.len()) % 3;
    let saturation = 0.58 + band as f64 * 0.08;
    let value = 0.72 + ((i / (REGION_PALETTE.len() * 3)) % 2) as f64 * 0.14;
    hsv_to_rgb(hue, saturation, value)
}

fn hsv_to_rgb(hue: f64, saturation: f64, value: f64) -> [u8; 3] {
    let sector = hue * 6.0;
    let index = sector.floor() as u8;
    let fraction = sector - f64::from(index);
    let p = value * (1.0 - saturation);
    let q = value * (1.0 - saturation * fraction);
    let t = value * (1.0 - saturation * (1.0 - fraction));
    let (red, green, blue) = match index {
        0 => (value, t, p),
        1 => (q, value, p),
        2 => (p, value, t),
        3 => (p, q, value),
        4 => (t, p, value),
        _ => (value, p, q),
    };
    [
        (red * 255.0).round() as u8,
        (green * 255.0).round() as u8,
        (blue * 255.0).round() as u8,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validation_rejects_duplicate_non_finite_and_stale_identity_state() {
        let region = Region {
            id: RegionId::new(2),
            lo: 0.1,
            hi: 0.2,
            name: String::new(),
            label_position: None,
            color: region_color(0),
            metric: None,
        };
        let mut state = RegionAnalysisState {
            regions: vec![region.clone(), region],
            default_metric: RegionMetric::Height,
            next_region_id: RegionId::new(3),
            show_annotations: true,
        };
        assert!(state.validate().unwrap_err().contains("duplicate"));

        state.regions.truncate(1);
        state.regions[0].lo = f64::NAN;
        assert!(state.validate().unwrap_err().contains("non-finite"));

        state.regions[0].lo = 0.1;
        state.regions[0].label_position = Some([1.2, 0.5]);
        assert!(
            state
                .validate()
                .unwrap_err()
                .contains("invalid label position")
        );

        state.regions[0].label_position = None;
        state.next_region_id = RegionId::new(2);
        assert!(state.validate().unwrap_err().contains("does not follow"));
    }

    #[test]
    fn allocator_never_wraps_region_identity() {
        let mut state = RegionAnalysisState {
            next_region_id: RegionId::new(u64::MAX),
            ..RegionAnalysisState::default()
        };
        assert_eq!(state.allocate_region_id(), None);
        assert_eq!(state.next_region_id, RegionId::new(u64::MAX));
    }

    #[test]
    fn selection_is_only_visible_in_its_owner_dataset() {
        let owner = DatasetId::new();
        let other = DatasetId::new();
        let id = RegionId::new(0);
        let selection = RegionSelection::new(owner, id);

        assert_eq!(selection.in_dataset(owner), Some(id));
        assert_eq!(selection.in_dataset(other), None);
    }

    #[test]
    fn practical_region_counts_do_not_repeat_colors() {
        let colors = (0..256)
            .map(region_color)
            .collect::<std::collections::HashSet<_>>();
        assert_eq!(colors.len(), 256);
    }
}
