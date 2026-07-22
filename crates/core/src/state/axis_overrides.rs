use super::visible_y_range;
use plotx_figure::{Axis, Figure};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AxisRange {
    pub min: f64,
    pub max: f64,
}

impl AxisRange {
    pub fn new(min: f64, max: f64) -> Self {
        if min <= max {
            Self { min, max }
        } else {
            Self { min: max, max: min }
        }
    }

    pub fn from_axis(axis: &Axis) -> Self {
        Self::new(axis.min, axis.max)
    }

    pub fn span(self) -> f64 {
        (self.max - self.min).max(f64::MIN_POSITIVE)
    }

    pub fn contains(self, value: f64) -> bool {
        self.min <= value && value <= self.max
    }

    pub fn is_valid(self) -> bool {
        self.min.is_finite() && self.max.is_finite() && self.min < self.max
    }

    pub fn clamp_to(self, full: Self) -> Self {
        let full_span = full.span();
        let span = self.span().min(full_span);
        if span >= full_span {
            return full;
        }

        let mut min = self.min.max(full.min).min(full.max - span);
        let mut max = min + span;
        if max > full.max {
            max = full.max;
            min = max - span;
        }
        Self { min, max }
    }

    pub fn zoom_around(self, full: Self, anchor: f64, scale: f64) -> Self {
        let span = self.span();
        let full_span = full.span();
        let new_span = (span * scale).clamp(full_span * 1e-6, full_span);
        if new_span >= full_span {
            return full;
        }

        let anchor = anchor.clamp(self.min, self.max);
        let frac = ((anchor - self.min) / span).clamp(0.0, 1.0);
        let min = anchor - frac * new_span;
        let max = min + new_span;
        Self { min, max }.clamp_to(full)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct CanvasViewport {
    pub full_x: AxisRange,
    pub full_y: AxisRange,
    pub view_x: AxisRange,
    pub view_y: AxisRange,
    pub auto_y: bool,
}

impl CanvasViewport {
    pub fn from_figure(fig: &Figure) -> Self {
        let full_x = AxisRange::from_axis(&fig.x);
        let full_y = AxisRange::from_axis(&fig.y);
        Self {
            full_x,
            full_y,
            view_x: full_x,
            view_y: full_y,
            auto_y: true,
        }
    }

    pub fn sync_full_from(&mut self, fig: &Figure) {
        self.full_x = AxisRange::from_axis(&fig.x);
        self.full_y = AxisRange::from_axis(&fig.y);
        self.view_x = self.view_x.clamp_to(self.full_x);
        if self.auto_y {
            self.refresh_auto_y(fig);
        } else {
            self.view_y = self.view_y.clamp_to(self.full_y);
        }
    }

    pub fn apply_to(&self, fig: &mut Figure) {
        fig.x.min = self.view_x.min;
        fig.x.max = self.view_x.max;
        fig.y.min = self.view_y.min;
        fig.y.max = self.view_y.max;
    }

    pub fn reset_all(&mut self) {
        self.view_x = self.full_x;
        self.view_y = self.full_y;
        self.auto_y = true;
    }

    pub fn reset_x(&mut self, fig: &Figure) {
        self.view_x = self.full_x;
        if self.auto_y {
            self.refresh_auto_y(fig);
        }
    }

    pub fn reset_y(&mut self, fig: &Figure) {
        self.auto_y = true;
        self.refresh_auto_y(fig);
    }

    pub fn zoom_x(&mut self, fig: &Figure, anchor: f64, scale: f64) {
        self.view_x = self.view_x.zoom_around(self.full_x, anchor, scale);
        if self.auto_y {
            self.refresh_auto_y(fig);
        }
    }

    pub fn zoom_y(&mut self, anchor: f64, scale: f64) {
        self.view_y = self.view_y.zoom_around(self.full_y, anchor, scale);
        self.auto_y = false;
    }

    pub fn select(&mut self, fig: &Figure, x: Option<AxisRange>, y: Option<AxisRange>) {
        if let Some(x) = x {
            self.view_x = x.clamp_to(self.full_x);
        }
        if let Some(y) = y {
            self.view_y = y.clamp_to(self.full_y);
            self.auto_y = false;
        } else if self.auto_y {
            self.refresh_auto_y(fig);
        }
    }

    fn refresh_auto_y(&mut self, fig: &Figure) {
        self.view_y = visible_y_range(fig, self.view_x).unwrap_or(self.full_y);
    }
}

/// Per-plot author overrides applied after rebuilding a figure from its data.
/// `None` keeps the corresponding value derived by the chart builder.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AxisOverrides {
    pub x_label: Option<String>,
    pub y_label: Option<String>,
    pub x_range: Option<AxisRange>,
    pub y_range: Option<AxisRange>,
    pub x_show_tick_labels: Option<bool>,
    pub x_show_label: Option<bool>,
    pub y_show_tick_labels: Option<bool>,
    pub y_show_label: Option<bool>,
}

impl AxisOverrides {
    pub fn apply_to(&self, figure: &mut Figure) {
        if let Some(label) = &self.x_label {
            figure.x.label.clone_from(label);
        }
        if let Some(label) = &self.y_label {
            figure.y.label.clone_from(label);
        }
        if let Some(range) = self.x_range
            && figure.x.categories.is_none()
        {
            figure.x.min = range.min;
            figure.x.max = range.max;
        }
        if let Some(range) = self.y_range
            && figure.y.categories.is_none()
        {
            figure.y.min = range.min;
            figure.y.max = range.max;
        }
        if let Some(show) = self.x_show_tick_labels {
            figure.x.show_tick_labels = show;
        }
        if let Some(show) = self.x_show_label {
            figure.x.show_label = show;
        }
        if let Some(show) = self.y_show_tick_labels {
            figure.y.show_tick_labels = show;
        }
        if let Some(show) = self.y_show_label {
            figure.y.show_label = show;
        }
    }

    pub fn normalized(mut self) -> Self {
        self.x_label = normalize_label(self.x_label);
        self.y_label = normalize_label(self.y_label);
        self.x_range = self.x_range.filter(|range| range.is_valid());
        self.y_range = self.y_range.filter(|range| range.is_valid());
        self
    }
}

fn normalize_label(label: Option<String>) -> Option<String> {
    label.filter(|text| !text.trim().is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use plotx_figure::{Axis, Figure};

    #[test]
    fn empty_labels_and_invalid_ranges_normalize_to_auto() {
        let overrides = AxisOverrides {
            x_label: Some("  ".to_owned()),
            y_label: Some("Signal".to_owned()),
            x_range: Some(AxisRange { min: 1.0, max: 1.0 }),
            y_range: Some(AxisRange {
                min: 5.0,
                max: -5.0,
            }),
            ..AxisOverrides::default()
        }
        .normalized();
        assert_eq!(overrides.x_label, None);
        assert_eq!(overrides.y_label.as_deref(), Some("Signal"));
        assert_eq!(overrides.x_range, None);
        assert_eq!(overrides.y_range, None);
    }

    #[test]
    fn overrides_apply_without_changing_unspecified_axes() {
        let mut figure = Figure::new(
            "",
            Axis::new("automatic x", 0.0, 10.0),
            Axis::new("automatic y", -1.0, 1.0),
        );
        AxisOverrides {
            x_label: Some("Time".to_owned()),
            y_range: Some(AxisRange::new(-2.0, 2.0)),
            ..AxisOverrides::default()
        }
        .apply_to(&mut figure);
        assert_eq!(figure.x.label, "Time");
        assert_eq!(figure.y.label, "automatic y");
        assert_eq!(AxisRange::from_axis(&figure.x), AxisRange::new(0.0, 10.0));
        assert_eq!(AxisRange::from_axis(&figure.y), AxisRange::new(-2.0, 2.0));
    }

    #[test]
    fn numeric_ranges_do_not_replace_categorical_axis_windows() {
        let mut figure = Figure::new("", Axis::new("x", -0.5, 2.5), Axis::new("y", 0.0, 1.0));
        figure.x.categories = Some(vec!["A".to_owned(), "B".to_owned(), "C".to_owned()]);
        AxisOverrides {
            x_range: Some(AxisRange::new(1.0, 8.0)),
            ..AxisOverrides::default()
        }
        .apply_to(&mut figure);

        assert_eq!(AxisRange::from_axis(&figure.x), AxisRange::new(-0.5, 2.5));
    }
}
