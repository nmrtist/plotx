use plotx_analysis::series::ReduceOp;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Region {
    pub id: u64,
    pub lo: f64,
    pub hi: f64,
    #[serde(default)]
    pub name: String,
    #[serde(default = "default_region_color")]
    pub color: [u8; 3],
    /// `None` follows the dataset's default metric.
    #[serde(default)]
    pub metric: Option<RegionMetric>,
}

fn default_region_color() -> [u8; 3] {
    REGION_PALETTE[0]
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

    pub fn column_name(&self) -> String {
        if self.name.trim().is_empty() {
            format!("{:.3} ppm", self.center())
        } else {
            self.name.clone()
        }
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

pub const REGION_PALETTE: [[u8; 3]; 6] = [
    [0x1a, 0x7f, 0x37],
    [0x2b, 0x6c, 0xb0],
    [0xc0, 0x4a, 0x2b],
    [0x7a, 0x4f, 0xa3],
    [0xb8, 0x8a, 0x1e],
    [0x2f, 0x8f, 0x8f],
];

pub fn region_color(i: usize) -> [u8; 3] {
    REGION_PALETTE[i % REGION_PALETTE.len()]
}
