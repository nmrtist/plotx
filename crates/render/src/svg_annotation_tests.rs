use super::export;
use plotx_figure::{Axis, Color, ErrorBar, Figure, RangeAnnotation, Series};

#[test]
fn exports_wellformed_ish_svg_with_polyline() {
    let figure = Figure::new(
        "Demo",
        Axis::new("ppm", 0.0, 10.0).reversed(true),
        Axis::new("intensity", 0.0, 1.0),
    )
    .with_series(Series::line(
        "trace",
        vec![[0.0, 0.0], [5.0, 1.0], [10.0, 0.0]],
    ));
    let output = export(&figure);
    assert!(output.starts_with("<svg"));
    assert!(output.trim_end().ends_with("</svg>"));
    assert!(output.contains("<polyline"));
    assert!(output.contains("Demo"));
}

#[test]
fn dense_line_series_keeps_distinct_projected_x_coordinates() {
    let count = 100_000usize;
    let denominator = (count - 1) as f64;
    let points = (0..count)
        .map(|index| {
            let noise = if index % 2 == 0 { -0.00001 } else { 0.00001 };
            [index as f64 / denominator, 0.5 + noise]
        })
        .collect();
    let figure = Figure::new("", Axis::new("x", 0.0, 1.0), Axis::new("y", 0.0, 1.0))
        .with_series(Series::line("", points));

    let output = export(&figure);
    let serialized = output
        .split_once("<polyline points=\"")
        .expect("line series polyline")
        .1
        .split_once('"')
        .expect("points attribute")
        .0;
    let xs: Vec<f32> = serialized
        .split_ascii_whitespace()
        .map(|point| {
            point
                .split_once(',')
                .expect("x,y pair")
                .0
                .parse()
                .expect("finite x coordinate")
        })
        .collect();

    assert_eq!(xs.len(), count);
    assert!(xs.windows(2).all(|pair| pair[0] < pair[1]));
}

#[test]
fn escapes_xml_special_chars() {
    let figure = Figure::new(
        "A & B <test>",
        Axis::categorical("x", vec!["A & B".into(), "<ctrl>".into()]),
        Axis::categorical("y", vec!["north & south".into(), "<root>".into()]),
    );
    let output = export(&figure);
    assert!(output.contains("A &amp; B &lt;test&gt;"));
    assert!(output.contains("&lt;ctrl&gt;"));
    assert!(output.contains("north &amp; south"));
    assert!(output.contains("&lt;root&gt;"));
    assert!(!output.contains(">A & B<"));
    assert!(!output.contains("><ctrl><"));
}

#[test]
fn exports_range_annotation_band_and_escaped_label() {
    let mut figure = Figure::new(
        "",
        Axis::new("Time (s)", 0.0, 1.0),
        Axis::new("Current (pA)", -2.0, 2.0),
    );
    figure.range_annotations.push(RangeAnnotation {
        source_id: 1,
        x0: 0.97,
        x1: 0.99,
        label: "Peak < 1 & 2".to_owned(),
        label_position: None,
        color: Color::rgb(0x2b, 0x6c, 0xb0),
        fill_opacity: 0.12,
        width: 1.0,
    });

    let output = export(&figure);
    assert_eq!(output.matches("class=\"range-annotation\"").count(), 1);
    assert_eq!(
        output.matches("class=\"range-annotation-label\"").count(),
        1
    );
    assert!(output.contains("#2b6cb0"));
    assert!(output.contains("Peak &lt; 1 &amp; 2"));
    let label = output
        .split("class=\"range-annotation-label\"")
        .nth(1)
        .expect("range label is present")
        .split("</text>")
        .next()
        .unwrap();
    assert!(!label.contains("dominant-baseline"));
    assert!(label.contains("text-anchor=\"end\""));
}

#[test]
fn omits_a_range_label_wholly_outside_the_visible_x_axis() {
    let mut figure = Figure::new(
        "",
        Axis::new("Time (s)", 0.0, 1.0),
        Axis::new("Current (pA)", -2.0, 2.0),
    );
    figure.range_annotations.push(RangeAnnotation {
        source_id: 1,
        x0: 2.0,
        x1: 3.0,
        label: "outside".to_owned(),
        label_position: None,
        color: Color::AXIS,
        fill_opacity: 0.12,
        width: 1.0,
    });

    let output = export(&figure);
    assert!(!output.contains("class=\"range-annotation-label\""));
    assert!(!output.contains(">outside</text>"));
}

#[test]
fn exports_a_manually_positioned_range_label_at_its_normalized_center() {
    let mut figure = Figure::new(
        "",
        Axis::new("Time (s)", 0.0, 1.0),
        Axis::new("Current (pA)", -2.0, 2.0),
    );
    figure.range_annotations.push(RangeAnnotation {
        source_id: 7,
        x0: 0.2,
        x1: 0.3,
        label: "moved".to_owned(),
        label_position: Some([0.5, 0.75]),
        color: Color::AXIS,
        fill_opacity: 0.12,
        width: 1.0,
    });

    let output = export(&figure);
    let label = output
        .split("class=\"range-annotation-label\"")
        .nth(1)
        .unwrap()
        .split("</text>")
        .next()
        .unwrap();
    assert!(label.contains("text-anchor=\"middle\""));
    assert!(label.contains(">moved"));
}

#[test]
fn exports_error_bar_stem_and_caps_inside_the_plot_clip() {
    let figure = Figure::new("", Axis::new("x", 0.0, 1.0), Axis::new("y", 0.0, 2.0))
        .with_error_bar(ErrorBar::symmetric([0.5, 1.0], 0.25));
    let output = export(&figure);
    assert_eq!(output.matches("class=\"error-bar\"").count(), 1);
    assert!(output.contains("clip-path=\"url(#plot)\""));
}
