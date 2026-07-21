//! Figure builders: turning processed 1D/2D and pseudo-2D data into renderable [`Figure`]s.

use plotx_analysis::diffusion::DiffusionMap;
use plotx_analysis::ilt::IltResult;
use plotx_figure::{Annotation, Axis, AxisFrame, Color, Contour, Figure, Series};
use plotx_io::NmrData;
use plotx_processing::{Preset2D, Spectrum, Spectrum2D, StackSpectrum};

use crate::state::ResolvedPeak;

pub fn build_figure(data: &NmrData, spec: &Spectrum, peaks: &[ResolvedPeak]) -> Figure {
    let (ppm_lo, ppm_hi) = spec.ppm_bounds();
    let (i_lo, i_hi) = spec.intensity_bounds();
    let range = (i_hi - i_lo).max(f64::MIN_POSITIVE);
    // Pad the intensity range, with extra headroom on top for peak labels.
    let y = Axis::new("Intensity (a.u.)", i_lo - 0.05 * range, i_hi + 0.08 * range);
    // NMR convention: chemical shift increases to the left.
    let x = Axis::new(axis_label(&data.nucleus), ppm_lo, ppm_hi).reversed(true);

    let fig = Figure::new(format!("{} spectrum — {}", data.nucleus, data.source), x, y)
        .with_series(Series::line("real", spec.real_points()).colored(Color::TRACE));

    apply_peak_labels(fig, peaks)
}

pub fn apply_peak_labels(mut fig: Figure, peaks: &[ResolvedPeak]) -> Figure {
    for peak in peaks {
        fig = fig.with_annotation(Annotation {
            text: peak.label.clone(),
            at: [peak.x, peak.y],
            color: Color::rgb(0x8a, 0x1c, 0x1c),
            size: 12.0,
        });
    }
    fig
}

/// Contour lowest level as a fraction of the peak, and the number/ratio of
/// geometric levels drawn above it.
pub const CONTOUR_BASE_FRAC: f64 = 0.04;
pub const CONTOUR_LEVELS: usize = 14;
pub const CONTOUR_RATIO: f64 = 1.35;

/// Build a contour figure from a processed true-2D spectrum. F2 (direct) is the
/// x-axis with high ppm on the left; F1 (indirect) is the y-axis with high ppm
/// at the bottom (low ppm at the top) — the standard 2D NMR orientation.
pub fn build_figure_2d(spec: &Spectrum2D, preset: Preset2D) -> Figure {
    build_figure_2d_cancellable(spec, preset, &|| false).expect("non-cancelling contour figure")
}

pub fn build_figure_2d_cancellable(
    spec: &Spectrum2D,
    preset: Preset2D,
    cancelled: &impl Fn() -> bool,
) -> Option<Figure> {
    if cancelled() {
        return None;
    }
    let (f2_lo, f2_hi) = spec.f2_bounds();
    let (f1_lo, f1_hi) = spec.f1_bounds();
    let x = Axis::new(axis_label(&spec.direct.nucleus), f2_lo, f2_hi).reversed(true);
    let y = Axis::new(axis_label(&spec.indirect.nucleus), f1_lo, f1_hi).reversed(true);

    let mut fig = Figure::new(format!("{} — {}", preset.label(), spec.source), x, y)
        .with_axis_frame(AxisFrame::Box);
    // Homonuclear spectra share a nucleus/range on both axes; render them square.
    fig.lock_aspect = preset.homonuclear();

    if spec.f2_size >= 2 && spec.f1_size >= 2 {
        let z = spec.real();
        // The real (absorption) plane carries signed lobes, so mirror the positive
        // geometric levels to negative.
        let peak = z.iter().fold(0.0f32, |m, &v| m.max(v.abs())) as f64;
        let positive = plotx_render::contour::geometric_levels(
            peak * CONTOUR_BASE_FRAC,
            peak,
            CONTOUR_LEVELS,
            CONTOUR_RATIO,
        );
        let levels: Vec<f64> = positive
            .iter()
            .rev()
            .map(|l| -l)
            .chain(positive.iter().copied())
            .collect();
        // Grid columns run low→high ppm (index 0 = f2_ppm[0]); rows likewise.
        let segments = plotx_render::contour::segments_cancellable(
            &z,
            spec.f1_size,
            spec.f2_size,
            spec.f2_ppm[0],
            spec.f2_ppm[spec.f2_size - 1],
            spec.f1_ppm[0],
            spec.f1_ppm[spec.f1_size - 1],
            &levels,
            cancelled,
        )?;
        fig = fig.with_contour(Contour {
            segments,
            color: Color::TRACE,
            width: 0.7,
        });
    }
    Some(fig)
}

/// Build a waterfall figure from a pseudo-2D stack: the direct-dimension
/// spectrum of each increment, offset vertically. Increments are strided so at
/// most `MAX_STACK_TRACES` are drawn.
pub fn build_stack_figure(stack: &StackSpectrum) -> Figure {
    const MAX_STACK_TRACES: usize = 48;
    let (lo, hi) = stack.ppm_bounds();
    let n = stack.increments();
    let peak = stack.max_magnitude().max(f64::MIN_POSITIVE);
    let dy = peak * 0.12;
    let y_top = peak + n as f64 * dy;

    let x = Axis::new(axis_label(&stack.direct.nucleus), lo, hi).reversed(true);
    // The stack is phased to absorptive, so traces carry the signed real part:
    // short-τ relaxation increments dip below their baseline (inverted peaks).
    let y = Axis::new("Increment (offset)", -1.1 * peak, y_top * 1.02);
    let mut fig = Figure::new(format!("Pseudo-2D stack — {}", stack.source), x, y);

    let step = (n / MAX_STACK_TRACES).max(1);
    for i in (0..n).step_by(step) {
        let offset = i as f64 * dy;
        let pts: Vec<[f64; 2]> = stack
            .ppm
            .iter()
            .zip(&stack.traces[i])
            .map(|(&p, c)| [p, c.re + offset])
            .collect();
        fig = fig.with_series(Series::line(format!("{i}"), pts).colored(Color::TRACE));
    }
    fig
}

fn axis_label(nucleus: &str) -> String {
    let mut formatted = String::new();
    let mut chars = nucleus.chars().peekable();
    while chars.peek().is_some_and(char::is_ascii_digit) {
        let digit = chars.next().expect("peeked digit must exist");
        formatted.push(match digit {
            '0' => '⁰',
            '1' => '¹',
            '2' => '²',
            '3' => '³',
            '4' => '⁴',
            '5' => '⁵',
            '6' => '⁶',
            '7' => '⁷',
            '8' => '⁸',
            '9' => '⁹',
            _ => digit,
        });
    }
    formatted.extend(chars);
    format!("{formatted} chemical shift (ppm)")
}

/// Build a DOSY contour figure from a per-column diffusion map: x = chemical
/// shift (reversed), y = log₁₀(D). Fitted columns deposit a Gaussian blob on an
/// intensity grid that is then contoured.
pub fn build_dosy_figure(map: &DiffusionMap, nucleus: &str, source: &str) -> Figure {
    build_dosy_figure_cancellable(map, nucleus, source, &|| false)
        .expect("non-cancelling DOSY figure")
}

pub fn build_dosy_figure_cancellable(
    map: &DiffusionMap,
    nucleus: &str,
    source: &str,
    cancelled: &impl Fn() -> bool,
) -> Option<Figure> {
    const NX: usize = 512;
    const NY: usize = 300;
    let fitted: Vec<(f64, f64, f64)> = map
        .ppm
        .iter()
        .zip(&map.d)
        .zip(&map.amp)
        .filter_map(|((&p, &d), &a)| (d.is_finite() && d > 0.0).then_some((p, d.log10(), a)))
        .collect();

    let (ppm_lo, ppm_hi) = map
        .ppm
        .iter()
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), &p| {
            (lo.min(p), hi.max(p))
        });
    let x = Axis::new(axis_label(nucleus), ppm_lo, ppm_hi).reversed(true);

    if fitted.is_empty() {
        let y = Axis::new("log₁₀(D / (m²/s))", -10.5, -8.5);
        return Some(Figure::new(format!("DOSY — {source}"), x, y).with_axis_frame(AxisFrame::Box));
    }
    let (mut logd_lo, mut logd_hi) = (f64::INFINITY, f64::NEG_INFINITY);
    for &(_, ld, _) in &fitted {
        logd_lo = logd_lo.min(ld);
        logd_hi = logd_hi.max(ld);
    }
    // Pad the D axis by half a decade each side.
    logd_lo -= 0.5;
    logd_hi += 0.5;
    let y = Axis::new("log₁₀(D / (m²/s))", logd_lo, logd_hi).reversed(true);

    // Accumulate Gaussian blobs onto the grid (row-major, NY rows × NX cols).
    let mut grid = vec![0.0f32; NX * NY];
    let sx = (ppm_hi - ppm_lo).max(f64::MIN_POSITIVE);
    let sy = (logd_hi - logd_lo).max(f64::MIN_POSITIVE);
    let sig_x = 1.5f64; // px in ppm direction
    let sig_y = 3.0f64; // px in log-D direction
    for &(ppm, logd, amp) in &fitted {
        if cancelled() {
            return None;
        }
        let cx = ((ppm - ppm_lo) / sx * (NX - 1) as f64).round() as isize;
        let cy = ((logd - logd_lo) / sy * (NY - 1) as f64).round() as isize;
        let rx = (sig_x * 3.0) as isize;
        let ry = (sig_y * 3.0) as isize;
        for dy in -ry..=ry {
            let yy = cy + dy;
            if yy < 0 || yy >= NY as isize {
                continue;
            }
            for dx in -rx..=rx {
                let xx = cx + dx;
                if xx < 0 || xx >= NX as isize {
                    continue;
                }
                let g = (-(dx as f64).powi(2) / (2.0 * sig_x * sig_x)
                    - (dy as f64).powi(2) / (2.0 * sig_y * sig_y))
                    .exp();
                grid[yy as usize * NX + xx as usize] += (amp * g) as f32;
            }
        }
    }
    let peak = grid.iter().cloned().fold(0.0f32, f32::max) as f64;
    let mut fig = Figure::new(format!("DOSY — {source}"), x, y).with_axis_frame(AxisFrame::Box);
    if peak > 0.0 {
        let levels = plotx_render::contour::geometric_levels(
            peak * CONTOUR_BASE_FRAC,
            peak,
            CONTOUR_LEVELS,
            CONTOUR_RATIO,
        );
        // Grid rows map onto [logd_lo, logd_hi], cols onto [ppm_lo, ppm_hi].
        let segments = plotx_render::contour::segments_cancellable(
            &grid, NY, NX, ppm_lo, ppm_hi, logd_lo, logd_hi, &levels, cancelled,
        )?;
        fig = fig.with_contour(Contour {
            segments,
            color: Color::TRACE,
            width: 0.7,
        });
    }
    Some(fig)
}

/// Build a DOSY contour figure from a full ILT/CONTIN inversion: x = chemical
/// shift (reversed), y = log₁₀(D). `amp[c]` is column `c`'s D distribution over
/// the shared, log-spaced `d_grid`, so its rows map linearly onto log₁₀(D) and
/// its columns onto the ppm axis — contoured directly without re-binning.
pub fn build_ilt_figure(result: &IltResult, nucleus: &str, source: &str) -> Figure {
    build_ilt_figure_cancellable(result, nucleus, source, &|| false)
        .expect("non-cancelling ILT figure")
}

pub fn build_ilt_figure_cancellable(
    result: &IltResult,
    nucleus: &str,
    source: &str,
    cancelled: &impl Fn() -> bool,
) -> Option<Figure> {
    let nx = result.ppm.len();
    let ny = result.d_grid.len();
    let (ppm_lo, ppm_hi) = result
        .ppm
        .iter()
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), &p| {
            (lo.min(p), hi.max(p))
        });
    let x = Axis::new(axis_label(nucleus), ppm_lo, ppm_hi).reversed(true);

    let logd: Vec<f64> = result
        .d_grid
        .iter()
        .map(|&d| d.max(f64::MIN_POSITIVE).log10())
        .collect();
    if nx < 2 || ny < 2 {
        let y = Axis::new("log₁₀(D / (m²/s))", -10.5, -8.5);
        return Some(
            Figure::new(format!("DOSY (ILT) — {source}"), x, y).with_axis_frame(AxisFrame::Box),
        );
    }
    let (logd_lo, logd_hi) = (logd[0], logd[ny - 1]);
    let y = Axis::new(
        "log₁₀(D / (m²/s))",
        logd_lo.min(logd_hi),
        logd_lo.max(logd_hi),
    )
    .reversed(true);

    // Row-major NY×NX grid: row = D index, col = ppm column.
    let mut grid = vec![0.0f32; nx * ny];
    for (c, col) in result.amp.iter().enumerate().take(nx) {
        if cancelled() {
            return None;
        }
        for (r, &a) in col.iter().enumerate().take(ny) {
            grid[r * nx + c] = a as f32;
        }
    }
    let peak = grid.iter().cloned().fold(0.0f32, f32::max) as f64;
    let mut fig =
        Figure::new(format!("DOSY (ILT) — {source}"), x, y).with_axis_frame(AxisFrame::Box);
    if peak > 0.0 {
        let levels = plotx_render::contour::geometric_levels(
            peak * CONTOUR_BASE_FRAC,
            peak,
            CONTOUR_LEVELS,
            CONTOUR_RATIO,
        );
        let segments = plotx_render::contour::segments_cancellable(
            &grid,
            ny,
            nx,
            result.ppm[0],
            result.ppm[nx - 1],
            logd_lo,
            logd_hi,
            &levels,
            cancelled,
        )?;
        fig = fig.with_contour(Contour {
            segments,
            color: Color::TRACE,
            width: 0.7,
        });
    }
    Some(fig)
}

#[cfg(test)]
mod tests {
    use super::*;
    use num_complex::Complex64;
    use plotx_processing::AxisMeta;
    use plotx_render::{Margins, Projector, Rect};

    fn spectrum_2d() -> Spectrum2D {
        let f2_ppm = vec![0.0, 1.0, 2.0, 3.0];
        let f1_ppm = vec![0.0, 1.0, 2.0, 3.0];
        let (f2_size, f1_size) = (f2_ppm.len(), f1_ppm.len());
        Spectrum2D {
            data: vec![Complex64::new(1.0, 0.0); f1_size * f2_size],
            f2_ppm,
            f1_ppm,
            f2_size,
            f1_size,
            direct: AxisMeta {
                nucleus: "1H".to_owned(),
                observe_freq_mhz: 400.0,
            },
            indirect: AxisMeta {
                nucleus: "13C".to_owned(),
                observe_freq_mhz: 100.0,
            },
            source: "test".to_owned(),
        }
    }

    // The 2D NMR convention places low chemical shift at the top of the plot. The
    // F1 axis is built `reversed`, which — with the projector's own y-flip — maps
    // low F1 ppm to `plot.top`. Guards against "un-reversing" it (screen and export
    // share the projector, so a wrong flip is invisible in preview).
    #[test]
    fn contour_places_low_f1_ppm_near_the_top() {
        let fig = build_figure_2d(&spectrum_2d(), Preset2D::Hsqc);
        assert_eq!(fig.axis_frame, AxisFrame::Box);
        let proj = Projector::new(&fig, Rect::new(0.0, 0.0, 400.0, 300.0), &Margins::default());
        let (_, py_low_ppm) = proj.project([1.5, 0.0]);
        let (_, py_high_ppm) = proj.project([1.5, 3.0]);
        // Screen y grows downward, so the smaller py is higher on the page.
        assert!(
            py_low_ppm < py_high_ppm,
            "low F1 ppm ({py_low_ppm}) should sit above high F1 ppm ({py_high_ppm})"
        );
    }

    #[test]
    fn axis_label_formats_isotope_mass_as_superscript() {
        assert_eq!(axis_label("13C"), "¹³C chemical shift (ppm)");
        assert_eq!(axis_label("1H"), "¹H chemical shift (ppm)");
        assert_eq!(axis_label("F"), "F chemical shift (ppm)");
    }
}
