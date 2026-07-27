//! Axis presentation captured before author overrides are applied.

use plotx_figure::Figure;

#[derive(Clone, Debug, PartialEq)]
pub struct DerivedAxes {
    pub x_label: String,
    pub y_label: String,
    pub x_show_tick_labels: bool,
    pub x_show_label: bool,
    pub y_show_tick_labels: bool,
    pub y_show_label: bool,
}

impl DerivedAxes {
    pub fn from_figure(figure: &Figure) -> Self {
        Self {
            x_label: figure.x.label.clone(),
            y_label: figure.y.label.clone(),
            x_show_tick_labels: figure.x.show_tick_labels,
            x_show_label: figure.x.show_label,
            y_show_tick_labels: figure.y.show_tick_labels,
            y_show_label: figure.y.show_label,
        }
    }
}
