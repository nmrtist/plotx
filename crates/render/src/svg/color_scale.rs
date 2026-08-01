use crate::{Rect, color_scale_rect};
use plotx_figure::Figure;
use std::fmt::Write;

pub(super) fn write(s: &mut String, fig: &Figure, plot: Rect) {
    let (Some(heatmap), Some(rect)) = (&fig.heatmap, color_scale_rect(fig, plot, 1.0)) else {
        return;
    };
    let horizontal = rect.width > rect.height;
    const STEPS: usize = 64;
    for step in 0..STEPS {
        let q0 = step as f32 / STEPS as f32;
        let q1 = (step + 1) as f32 / STEPS as f32;
        let color = heatmap.colormap.sample((q0 + q1) * 0.5).to_hex();
        let (x, y, width, height) = if horizontal {
            (
                rect.left + rect.width * q0,
                rect.top,
                rect.width * (q1 - q0) + 0.1,
                rect.height,
            )
        } else {
            (
                rect.left,
                rect.top + rect.height * (1.0 - q1),
                rect.width,
                rect.height * (q1 - q0) + 0.1,
            )
        };
        let _ = write!(
            s,
            r#"<rect x="{x:.2}" y="{y:.2}" width="{width:.2}" height="{height:.2}" fill="{color}"/>"#,
        );
    }
    let _ = write!(
        s,
        r#"<rect x="{x:.2}" y="{y:.2}" width="{w:.2}" height="{h:.2}" fill="none" stroke="{axis}" stroke-width="0.75"/>"#,
        x = rect.left,
        y = rect.top,
        w = rect.width,
        h = rect.height,
        axis = plotx_figure::Color::AXIS.to_hex(),
    );
    let [min, max] = heatmap.value_range;
    let font = fig.typography.legend_pt;
    let color = fig.typography.legend_color.to_hex();
    if horizontal {
        let y = rect.bottom() + font + 2.0;
        let _ = write!(
            s,
            r#"<text x="{x0:.2}" y="{y:.2}" text-anchor="start" font-size="{font:.2}" fill="{color}">{min}</text><text x="{x1:.2}" y="{y:.2}" text-anchor="end" font-size="{font:.2}" fill="{color}">{max}</text>"#,
            x0 = rect.left,
            x1 = rect.right(),
            min = format_value(min),
            max = format_value(max),
        );
    } else {
        let x = rect.right() + 3.0;
        let _ = write!(
            s,
            r#"<text x="{x:.2}" y="{yt:.2}" dominant-baseline="hanging" font-size="{font:.2}" fill="{color}">{max}</text><text x="{x:.2}" y="{yb:.2}" dominant-baseline="auto" font-size="{font:.2}" fill="{color}">{min}</text>"#,
            yt = rect.top,
            yb = rect.bottom(),
            min = format_value(min),
            max = format_value(max),
        );
    }
}

fn format_value(value: f32) -> String {
    if value.abs() >= 10_000.0 || (value != 0.0 && value.abs() < 0.001) {
        format!("{value:.2e}")
    } else {
        format!("{value:.3}")
            .trim_end_matches('0')
            .trim_end_matches('.')
            .to_owned()
    }
}
