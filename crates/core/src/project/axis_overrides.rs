use super::dto::RangeDto;
use crate::state::AxisOverrides;
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize)]
pub struct AxisOverridesDto {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    x_label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    y_label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    x_range: Option<RangeDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    y_range: Option<RangeDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    x_show_tick_labels: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    x_show_label: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    y_show_tick_labels: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    y_show_label: Option<bool>,
}

impl AxisOverridesDto {
    pub(super) fn from_overrides(overrides: &AxisOverrides) -> Option<Self> {
        (overrides != &AxisOverrides::default()).then(|| Self {
            x_label: overrides.x_label.clone(),
            y_label: overrides.y_label.clone(),
            x_range: overrides.x_range.map(RangeDto::from_range),
            y_range: overrides.y_range.map(RangeDto::from_range),
            x_show_tick_labels: overrides.x_show_tick_labels,
            x_show_label: overrides.x_show_label,
            y_show_tick_labels: overrides.y_show_tick_labels,
            y_show_label: overrides.y_show_label,
        })
    }

    pub(super) fn to_overrides(&self) -> AxisOverrides {
        AxisOverrides {
            x_label: self.x_label.clone(),
            y_label: self.y_label.clone(),
            x_range: self.x_range.map(RangeDto::into_range),
            y_range: self.y_range.map(RangeDto::into_range),
            x_show_tick_labels: self.x_show_tick_labels,
            x_show_label: self.x_show_label,
            y_show_tick_labels: self.y_show_tick_labels,
            y_show_label: self.y_show_label,
        }
        .normalized()
    }
}
