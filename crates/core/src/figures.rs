//! Figure builders: turning processed 1D/2D and pseudo-2D data into renderable [`Figure`]s.

use plotx_analysis::diffusion::DiffusionMap;
use plotx_analysis::ilt::IltResult;
use plotx_analysis::robust::{deplaned_location_scale, robust_difference_mad};
use plotx_figure::{
    Annotation, Axis, AxisFrame, Color, Contour, ContourBasePolicy, ContourLevelSpec, ContourSpec,
    Figure, Series,
};
use plotx_io::NmrData;
use plotx_processing::{Preset2D, Processed1D, Spectrum, Spectrum2D, StackSpectrum, TimeTrace};

use crate::state::{
    EstimatedScale, FieldSummary, FiniteF64, ResolvedPeak, default_contour_spec,
    scalar_grid_capabilities,
};

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

pub fn build_time_figure(data: &NmrData, trace: &TimeTrace) -> Figure {
    let (time_lo, time_hi) = trace.time_bounds();
    let mut intensity = trace.values.iter().map(|value| value.re);
    let first = intensity.next().unwrap_or(0.0);
    let (minimum, maximum) = intensity.fold((first, first), |(minimum, maximum), value| {
        (minimum.min(value), maximum.max(value))
    });
    let range = (maximum - minimum).max(f64::MIN_POSITIVE);
    let x = Axis::new("Acquisition time (s)", time_lo, time_hi);
    let y = Axis::new(
        "Signal (a.u.)",
        minimum - 0.05 * range,
        maximum + 0.05 * range,
    );
    Figure::new(format!("{} FID — {}", data.nucleus, data.source), x, y)
        .with_series(Series::line("real", trace.real_points()).colored(Color::TRACE))
}

pub fn build_processed_1d_figure(
    data: &NmrData,
    processed: &Processed1D,
    peaks: &[ResolvedPeak],
) -> Figure {
    match processed {
        Processed1D::Time(trace) => build_time_figure(data, trace),
        Processed1D::Frequency(spectrum) => build_figure(data, spectrum, peaks),
    }
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

/// Build the non-geometric shell of a processed true-2D figure. Contour
/// geometry is supplied later by the versioned field cache; this convenience
/// builder intentionally never runs marching squares on the caller's thread.
pub fn build_figure_2d(spec: &Spectrum2D, preset: Preset2D) -> Figure {
    let (f2_lo, f2_hi) = spec.f2_bounds();
    let (f1_lo, f1_hi) = spec.f1_bounds();
    let x = Axis::new(axis_label(&spec.direct.nucleus), f2_lo, f2_hi).reversed(true);
    let y = Axis::new(axis_label(&spec.indirect.nucleus), f1_lo, f1_hi).reversed(true);

    let mut fig = Figure::new(format!("{} — {}", preset.label(), spec.source), x, y)
        .with_axis_frame(AxisFrame::Box);
    fig.lock_aspect = equal_scale_for_nmr_2d(spec);
    fig
}

/// Whether an imported true-2D spectrum has commensurate frequency axes whose
/// full ranges remain useful when rendered with equal data units per pixel.
pub(crate) fn equal_scale_for_nmr_2d(spec: &Spectrum2D) -> bool {
    if spec.f2_domain != plotx_io::Domain::Frequency
        || spec.f1_domain != plotx_io::Domain::Frequency
        || spec.direct.nucleus != spec.indirect.nucleus
    {
        return false;
    }
    let (f2_lo, f2_hi) = spec.f2_bounds();
    let (f1_lo, f1_hi) = spec.f1_bounds();
    let f2_span = (f2_hi - f2_lo).abs();
    let f1_span = (f1_hi - f1_lo).abs();
    let narrow = f2_span.min(f1_span);
    let wide = f2_span.max(f1_span);
    narrow.is_finite() && wide.is_finite() && narrow > 0.0 && wide / narrow <= 2.0
}

/// Resolve levels for the existing DOSY/ILT analysis-map workers. Ordinary
/// `FieldPayload::ScalarGrid2D` contours use the versioned field resolver and
/// never reach this legacy analysis-only helper on the UI thread.
fn contour_levels(
    values: &[f32],
    rows: usize,
    cols: usize,
    level: &ContourLevelSpec,
    negative: bool,
) -> Vec<f64> {
    let Some((minimum, maximum)) = finite_range(values) else {
        return Vec::new();
    };
    let peak = if negative {
        -minimum.min(0.0)
    } else {
        maximum.max(0.0)
    };
    if peak <= 0.0 {
        return Vec::new();
    }
    let base = match &level.base {
        ContourBasePolicy::Absolute(value) => value.get(),
        ContourBasePolicy::NoiseFloor {
            multiplier,
            peak_fraction,
            ..
        } => {
            // The floor decision is shared with the versioned resolver for the
            // same reason the ladder is: two copies of it would drift, and this
            // path and that one must agree on what a field's noise scale is.
            let Some(scale) = EstimatedScale::new(robust_difference_mad(values, rows, cols)) else {
                return Vec::new();
            };
            let (Some(min), Some(max)) = (FiniteF64::new(minimum), FiniteF64::new(maximum)) else {
                return Vec::new();
            };
            multiplier.get()
                * crate::state::resolved_noise_scale(
                    scale,
                    *peak_fraction,
                    FieldSummary { min, max },
                )
                .0
        }
        ContourBasePolicy::BackgroundScale { multiplier, .. } => {
            let (location, scale) = deplaned_location_scale(values, rows, cols);
            (location + multiplier.get() * scale).abs()
        }
        ContourBasePolicy::FractionOfRange(fraction) => {
            minimum + fraction.get() * (maximum - minimum)
        }
    };
    // The ladder — including which policies may be rewritten when their base is
    // unusable — is shared with the versioned field resolver, and works purely
    // in positive magnitudes; this half applies its own sign afterwards. These
    // analysis maps are `FractionOfRange` (see `bounded_scalar_contour_spec`),
    // so they never carry an explicit threshold for the ladder to report on.
    let levels = crate::contour_ladder::contour_level_ladder(base, peak, level).levels;
    if negative {
        levels.into_iter().map(|value| -value).collect()
    } else {
        levels
    }
}

fn finite_range(values: &[f32]) -> Option<(f64, f64)> {
    let mut finite = values.iter().copied().filter(|value| value.is_finite());
    let first = f64::from(finite.next()?);
    Some(finite.fold((first, first), |(minimum, maximum), value| {
        let value = f64::from(value);
        (minimum.min(value), maximum.max(value))
    }))
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

    let x = match stack.direct_domain {
        plotx_io::Domain::Time => Axis::new("Direct acquisition time (s)", lo, hi),
        plotx_io::Domain::Frequency => {
            Axis::new(axis_label(&stack.direct.nucleus), lo, hi).reversed(true)
        }
    };
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

pub(crate) fn axis_label(nucleus: &str) -> String {
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
    let mut fig = Figure::new(format!("DOSY — {source}"), x, y).with_axis_frame(AxisFrame::Box);
    let contour = bounded_scalar_contour_spec();
    let levels = contour_levels(&grid, NY, NX, &contour.positive, false);
    if !levels.is_empty() {
        // Grid rows map onto [logd_lo, logd_hi], cols onto [ppm_lo, ppm_hi].
        #[cfg(test)]
        crate::contour_probe::record_marching_squares();
        let segments = plotx_render::contour::segments_cancellable(
            &grid, NY, NX, ppm_lo, ppm_hi, logd_lo, logd_hi, &levels, cancelled,
        )?;
        fig = fig.with_contour(Contour {
            segments,
            color: contour.style.positive_color.resolve(),
            width: contour.style.width.get(),
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
    let mut fig =
        Figure::new(format!("DOSY (ILT) — {source}"), x, y).with_axis_frame(AxisFrame::Box);
    let contour = bounded_scalar_contour_spec();
    let levels = contour_levels(&grid, ny, nx, &contour.positive, false);
    if !levels.is_empty() {
        #[cfg(test)]
        crate::contour_probe::record_marching_squares();
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
            color: contour.style.positive_color.resolve(),
            width: contour.style.width.get(),
        });
    }
    Some(fig)
}

fn bounded_scalar_contour_spec() -> ContourSpec {
    let capabilities = scalar_grid_capabilities(true, &[crate::automation::CAP_FIELD_BOUNDED]);
    // `Bounded` anchors the base to the value range, so no peak is consulted.
    default_contour_spec(&capabilities, crate::state::NO_PEAK)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{
        ContourResolution, DatasetId, EstimateProvenance, EstimateResult, EstimatedScale, FieldId,
        FieldRef, FieldSummary, FieldVersion, FiniteF64, ScaleEstimate, VersionedFieldRef,
        resolve_contour_levels,
    };
    use num_complex::Complex64;
    use plotx_figure::ContourStyle;
    use plotx_processing::AxisMeta;
    use plotx_render::{Margins, Projector, Rect};

    /// A tilted plane: every first difference is identical, so its robust MAD is
    /// exactly zero — the degenerate estimate an ideal noiseless grid produces.
    fn planar_values() -> Vec<f32> {
        (0..4u8)
            .flat_map(|row| (0..4u8).map(move |col| 1.0 + f32::from(row) + f32::from(col)))
            .collect()
    }

    /// Resolve the same positive half through both contour paths: the legacy
    /// analysis-map helper, and the versioned field resolver fed the degenerate
    /// estimate its worker would produce for these values.
    fn both_paths(values: &[f32], level: &ContourLevelSpec) -> (Vec<f64>, Vec<f64>) {
        let legacy = contour_levels(values, 4, 4, level, false);
        let (minimum, maximum) = finite_range(values).expect("fixture values are finite");
        let spec = ContourSpec {
            positive: level.clone(),
            negative: None,
            style: ContourStyle::default(),
        };
        let source = VersionedFieldRef {
            field: FieldRef {
                resource: DatasetId::from_uuid(uuid::Uuid::from_u128(77)),
                field: FieldId::new(0),
            },
            version: FieldVersion(1),
        };
        let summary = FieldSummary {
            min: FiniteF64::new(minimum).expect("finite"),
            max: FiniteF64::new(maximum).expect("finite"),
        };
        let resolution = resolve_contour_levels(source, &spec, summary, |_| {
            Some(EstimateResult::Scale(ScaleEstimate {
                scale: EstimatedScale::Degenerate,
                provenance: EstimateProvenance {
                    estimator: plotx_analysis::robust::ROBUST_DIFFERENCE_MAD_ID.to_owned(),
                    version: plotx_analysis::robust::ROBUST_DIFFERENCE_MAD_VERSION,
                },
            }))
        });
        let ContourResolution::Ready {
            levels: resolved, ..
        } = resolution
        else {
            panic!("the estimate is supplied, so resolution is not pending");
        };
        (
            legacy,
            resolved.positive.iter().map(|level| level.get()).collect(),
        )
    }

    fn noise_sigma_level(count: u16) -> ContourLevelSpec {
        ContourLevelSpec {
            base: ContourBasePolicy::NoiseFloor {
                multiplier: plotx_figure::PositiveFiniteF64::new(5.0).unwrap(),
                peak_fraction: plotx_figure::UnitInterval::new(0.0).expect("a zero floor is valid"),
                estimator: plotx_figure::EstimatorSelection::FollowLatest,
            },
            count,
            ratio: plotx_figure::PositiveFiniteF64::new(1.5).unwrap(),
        }
    }

    // The whole point of extracting `contour_ladder`: there is one policy for
    // what an unusable base means, so the legacy ILT/DOSY path and the versioned
    // field resolver cannot drift apart before ILT/DOSY move onto
    // `FieldId`/`FieldVersion`.
    #[test]
    fn degenerate_bases_resolve_identically_on_both_contour_paths() {
        let planar = planar_values();

        // One level, degenerate (zero) base.
        let (legacy, resolved) = both_paths(&planar, &noise_sigma_level(1));
        assert_eq!(legacy, resolved);
        assert_eq!(legacy.len(), 1);

        // A ladder of levels, degenerate (zero) base.
        let (legacy, resolved) = both_paths(&planar, &noise_sigma_level(5));
        assert_eq!(legacy, resolved);
        assert!(!legacy.is_empty());

        // An explicit threshold beyond the peak has no crossing, and is obeyed
        // literally rather than rewritten — on both paths alike.
        let above_peak = ContourLevelSpec {
            base: ContourBasePolicy::Absolute(
                plotx_figure::PositiveFiniteF64::new(1_000.0).unwrap(),
            ),
            count: 4,
            ratio: plotx_figure::PositiveFiniteF64::new(1.5).unwrap(),
        };
        let (legacy, resolved) = both_paths(&planar, &above_peak);
        assert_eq!(legacy, resolved);
        assert!(legacy.is_empty());
    }

    #[test]
    fn one_level_specs_draw_exactly_one_level_on_both_paths() {
        let planar = planar_values();
        // A usable base stays exactly where the user put it.
        let usable = ContourLevelSpec {
            base: ContourBasePolicy::Absolute(plotx_figure::PositiveFiniteF64::new(2.0).unwrap()),
            count: 1,
            ratio: plotx_figure::PositiveFiniteF64::new(1.5).unwrap(),
        };
        let (legacy, resolved) = both_paths(&planar, &usable);
        assert_eq!(legacy, [2.0]);
        assert_eq!(resolved, [2.0]);

        // A degenerate one resolves halfway to the peak, on both paths.
        let (legacy, resolved) = both_paths(&planar, &noise_sigma_level(1));
        assert_eq!(legacy.len(), 1);
        assert_eq!(resolved.len(), 1);
        assert_eq!(legacy, [7.0 / 2.0]);
        assert_eq!(resolved, [7.0 / 2.0]);
    }

    fn spectrum_2d() -> Spectrum2D {
        let f2_ppm = vec![0.0, 1.0, 2.0, 3.0];
        let f1_ppm = vec![0.0, 1.0, 2.0, 3.0];
        let (f2_size, f1_size) = (f2_ppm.len(), f1_ppm.len());
        Spectrum2D {
            f2_domain: plotx_io::Domain::Frequency,
            f1_domain: plotx_io::Domain::Frequency,
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

    #[test]
    fn equal_scale_requires_matching_frequency_axes_and_ranges_within_twofold() {
        let mut spectrum = spectrum_2d();
        spectrum.indirect.nucleus = "1H".to_owned();
        assert!(equal_scale_for_nmr_2d(&spectrum));

        spectrum.f2_ppm = vec![0.0, 2.0, 4.0, 6.0];
        assert!(
            equal_scale_for_nmr_2d(&spectrum),
            "a range exactly twice as wide remains eligible"
        );

        spectrum.f2_ppm = vec![0.0, 2.1, 4.2, 6.3];
        assert!(!equal_scale_for_nmr_2d(&spectrum));

        spectrum.f2_ppm = vec![0.0, 1.0, 2.0, 3.0];
        spectrum.indirect.nucleus = "13C".to_owned();
        assert!(!equal_scale_for_nmr_2d(&spectrum));

        spectrum.indirect.nucleus = "1H".to_owned();
        spectrum.f1_domain = plotx_io::Domain::Time;
        assert!(!equal_scale_for_nmr_2d(&spectrum));
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

    // Positive control for the marching-squares probe. Every "no synchronous
    // contour build" assertion elsewhere reads zero from the same counter, so
    // that counter must be shown to move at least once: this test fails the
    // moment an increment is dropped from a call site.
    //
    // It also pins the remaining synchronous path: ILT/DOSY analysis maps still
    // contour on the caller's thread and have not been moved onto the versioned
    // field cache.
    #[test]
    fn ilt_figure_runs_marching_squares_on_the_calling_thread() {
        crate::contour_probe::reset();
        let result = IltResult {
            ppm: vec![0.0, 1.0, 2.0, 3.0],
            d_grid: vec![1.0e-10, 2.0e-10, 4.0e-10, 8.0e-10],
            amp: vec![vec![0.0, 0.5, 1.0, 0.5]; 4],
        };

        let figure = build_ilt_figure(&result, "1H", "probe");

        assert!(
            !figure.contours.is_empty(),
            "the fixture must actually reach contour extraction"
        );
        assert!(
            crate::contour_probe::marching_squares_on_this_thread() > 0,
            "the marching-squares probe must observe a build on the calling thread"
        );
    }

    #[test]
    fn axis_label_formats_isotope_mass_as_superscript() {
        assert_eq!(axis_label("13C"), "¹³C chemical shift (ppm)");
        assert_eq!(axis_label("1H"), "¹H chemical shift (ppm)");
        assert_eq!(axis_label("F"), "F chemical shift (ppm)");
    }

    #[test]
    fn background_scale_removes_a_planar_afm_tilt_before_measuring_mad() {
        let values = (0..4)
            .flat_map(|row| (0..4).map(move |col| 10.0 + 3.0 * col as f32 + 2.0 * row as f32))
            .collect::<Vec<_>>();
        let (location, scale) = deplaned_location_scale(&values, 4, 4);

        assert!((location - 17.5).abs() < 1e-10);
        assert!(scale < 1e-10, "a plane is not background roughness");
    }

    #[test]
    fn finite_range_preserves_signed_and_baselined_fields() {
        assert_eq!(finite_range(&[-40.0, -5.0]), Some((-40.0, -5.0)));
        assert_eq!(finite_range(&[500.0, 600.0]), Some((500.0, 600.0)));
        assert_eq!(finite_range(&[f32::NAN, f32::INFINITY]), None);
    }

    #[test]
    fn fraction_of_range_starts_from_the_field_minimum() {
        let level = ContourLevelSpec {
            base: ContourBasePolicy::FractionOfRange(
                plotx_figure::UnitInterval::new(0.04).unwrap(),
            ),
            count: 1,
            ratio: plotx_figure::PositiveFiniteF64::new(1.35).unwrap(),
        };
        assert_eq!(
            contour_levels(&[500.0, 600.0], 1, 2, &level, false),
            [504.0]
        );
    }

    #[test]
    fn all_negative_fields_get_negative_levels() {
        let level = ContourLevelSpec {
            base: ContourBasePolicy::Absolute(plotx_figure::PositiveFiniteF64::new(10.0).unwrap()),
            count: 1,
            ratio: plotx_figure::PositiveFiniteF64::new(1.35).unwrap(),
        };
        assert_eq!(contour_levels(&[-40.0, -5.0], 1, 2, &level, true), [-10.0]);
        assert!(contour_levels(&[-40.0, -5.0], 1, 2, &level, false).is_empty());

        // The shared ladder speaks only in positive magnitudes, so each path
        // must still apply its own half's sign. The versioned resolver agrees.
        let spec = ContourSpec {
            positive: level.clone(),
            negative: Some(level),
            style: ContourStyle::default(),
        };
        let source = VersionedFieldRef {
            field: FieldRef {
                resource: DatasetId::from_uuid(uuid::Uuid::from_u128(78)),
                field: FieldId::new(0),
            },
            version: FieldVersion(1),
        };
        let summary = FieldSummary {
            min: FiniteF64::new(-40.0).expect("finite"),
            max: FiniteF64::new(-5.0).expect("finite"),
        };
        let ContourResolution::Ready {
            levels: resolved,
            unreachable,
        } = resolve_contour_levels(source, &spec, summary, |_| None)
        else {
            panic!("an absolute contour needs no estimate");
        };
        assert!(
            unreachable.is_empty(),
            "an all-negative field has no positive signal at all; that is the \
             field's shape, not a mistyped threshold"
        );
        assert_eq!(
            resolved
                .negative
                .iter()
                .map(|level| level.get())
                .collect::<Vec<_>>(),
            [-10.0]
        );
        assert!(resolved.positive.is_empty());
    }

    #[test]
    fn one_level_contour_uses_an_interior_fallback_for_degenerate_mad() {
        let level = ContourLevelSpec {
            base: ContourBasePolicy::NoiseFloor {
                multiplier: plotx_figure::PositiveFiniteF64::new(5.0).unwrap(),
                peak_fraction: plotx_figure::UnitInterval::new(0.0).expect("a zero floor is valid"),
                estimator: plotx_figure::EstimatorSelection::FollowLatest,
            },
            count: 1,
            ratio: plotx_figure::PositiveFiniteF64::new(1.35).unwrap(),
        };
        assert_eq!(contour_levels(&[0.0, 2.0], 1, 2, &level, false), [1.0]);
    }
}
