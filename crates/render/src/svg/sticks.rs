use crate::Projector;
use std::fmt::Write as _;

pub(super) fn write(output: &mut String, projector: &Projector<'_>, series: &plotx_figure::Series) {
    let mut path = String::new();
    for point in &series.points {
        let (x, baseline) = projector.project([point[0], 0.0]);
        let (_, y) = projector.project(*point);
        let _ = write!(path, "M{x:.2} {baseline:.2}V{y:.2}");
    }
    let _ = write!(
        output,
        r#"<path class="stick-series" d="{path}" fill="none" stroke="{color}" stroke-width="{width}"/>"#,
        color = series.color.to_hex(),
        width = series.width,
    );
}

#[cfg(test)]
mod tests {
    use plotx_figure::{Axis, Figure, Series};

    #[test]
    fn exports_each_stick_from_the_zero_baseline() {
        let figure = Figure::new(
            "",
            Axis::new("m/z", 10.0, 30.0),
            Axis::new("Intensity", 0.0, 5.0),
        )
        .with_series(Series::sticks("scan", vec![[12.0, 2.0], [25.0, 4.0]]));
        let output = crate::svg::export(&figure);
        assert!(output.contains("class=\"stick-series\""));
        let path = output.split("class=\"stick-series\"").nth(1).unwrap();
        assert_eq!(path.matches('M').count(), 2);
        assert_eq!(path.matches('V').count(), 2);
    }
}
