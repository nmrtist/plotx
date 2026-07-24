use super::*;
use plotx_figure::{Axis, AxisFrame, HeatmapGrid, HeatmapSpec, SeriesEncoding};

#[cfg(test)]
thread_local! {
    static SYNCHRONOUS_CONTOUR_BUILDS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
pub(crate) fn reset_synchronous_contour_builds() {
    SYNCHRONOUS_CONTOUR_BUILDS.with(|builds| builds.set(0));
}

#[cfg(test)]
pub(crate) fn synchronous_contour_builds() -> usize {
    SYNCHRONOUS_CONTOUR_BUILDS.with(std::cell::Cell::get)
}

impl Nmr2DDataset {
    pub fn figure(&self) -> Figure {
        match &self.processed {
            Processed2D::Ft(_) => (*self.processed_figure).clone(),
            Processed2D::Stack(stack) => match self.display {
                PseudoDisplay::DosyMap => match self.dosy_method {
                    DosyMethod::Ilt(_) => match &self.ilt_map {
                        Some(map) => self.ilt_figure.as_ref().map_or_else(
                            || build_ilt_figure(map, &self.data.direct.nucleus, &stack.source),
                            |figure| (**figure).clone(),
                        ),
                        None => build_stack_figure(stack),
                    },
                    DosyMethod::MonoExp => match &self.dosy_map {
                        Some(map) => self.dosy_figure.as_ref().map_or_else(
                            || build_dosy_figure(map, &self.data.direct.nucleus, &stack.source),
                            |figure| (**figure).clone(),
                        ),
                        None => build_stack_figure(stack),
                    },
                },
                PseudoDisplay::Stack => (*self.processed_figure).clone(),
            },
        }
    }

    pub fn summary(&self) -> String {
        format!(
            "{}–{} · {}×{} · {}",
            self.data.direct.nucleus,
            self.data.indirect.nucleus,
            self.data.cols,
            self.data.rows,
            self.preset.label(),
        )
    }

    pub(crate) fn encoded_field_figure(
        &self,
        field: &FieldDescriptor,
        encoding: &SeriesEncoding,
    ) -> Option<Figure> {
        let Processed2D::Ft(spectrum) = &self.processed else {
            return None;
        };
        match (field.local_id.as_str(), encoding) {
            ("nmr.real", SeriesEncoding::Contour(contour)) => {
                let cached = default_contour_spec(&field.capabilities);
                if *contour == cached {
                    Some((*self.processed_figure).clone())
                } else {
                    #[cfg(test)]
                    SYNCHRONOUS_CONTOUR_BUILDS.with(|builds| builds.set(builds.get() + 1));
                    Some(crate::build_figure_2d(spectrum, self.preset, contour))
                }
            }
            ("nmr.real", SeriesEncoding::Heatmap(heatmap)) => Some(nmr_scalar_heatmap(
                spectrum,
                spectrum.real(),
                "Real",
                heatmap,
            )),
            ("nmr.magnitude", SeriesEncoding::Heatmap(heatmap)) => Some(nmr_scalar_heatmap(
                spectrum,
                spectrum.magnitude(),
                "Magnitude",
                heatmap,
            )),
            ("nmr.magnitude", SeriesEncoding::Contour(contour)) => {
                Some(nmr_magnitude_contour(spectrum, contour))
            }
            _ => None,
        }
    }
}

fn nmr_axes(spectrum: &plotx_processing::Spectrum2D) -> (Axis, Axis) {
    let (f2_lo, f2_hi) = spectrum.f2_bounds();
    let (f1_lo, f1_hi) = spectrum.f1_bounds();
    (
        Axis::new(
            format!("{} chemical shift (ppm)", spectrum.direct.nucleus),
            f2_lo,
            f2_hi,
        )
        .reversed(true),
        Axis::new(
            format!("{} chemical shift (ppm)", spectrum.indirect.nucleus),
            f1_lo,
            f1_hi,
        )
        .reversed(true),
    )
}

fn nmr_scalar_heatmap(
    spectrum: &plotx_processing::Spectrum2D,
    values: Vec<f32>,
    field_name: &str,
    heatmap: &HeatmapSpec,
) -> Figure {
    let (x, y) = nmr_axes(spectrum);
    let mut finite = values.iter().copied().filter(|value| value.is_finite());
    let Some(first) = finite.next() else {
        return Figure::new(field_name, x, y).with_axis_frame(AxisFrame::Box);
    };
    let (minimum, maximum) = finite.fold((first, first), |(minimum, maximum), value| {
        (minimum.min(value), maximum.max(value))
    });
    let mut figure = Figure::new(format!("{field_name} — {}", spectrum.source), x, y)
        .with_axis_frame(AxisFrame::Box);
    figure.lock_aspect = spectrum.direct.nucleus == spectrum.indirect.nucleus;
    figure.heatmap = Some(HeatmapGrid {
        rows: spectrum.f1_size,
        cols: spectrum.f2_size,
        values,
        x_bounds: [
            spectrum.f2_ppm.first().copied().unwrap_or(0.0),
            spectrum.f2_ppm.last().copied().unwrap_or(0.0),
        ],
        y_bounds: [
            spectrum.f1_ppm.first().copied().unwrap_or(0.0),
            spectrum.f1_ppm.last().copied().unwrap_or(0.0),
        ],
        colormap: heatmap.colormap,
        value_range: heatmap
            .value_range
            .map(|range| [range[0], range[1]])
            .unwrap_or([minimum, maximum]),
    });
    figure
}

fn nmr_magnitude_contour(
    spectrum: &plotx_processing::Spectrum2D,
    contour: &plotx_figure::ContourSpec,
) -> Figure {
    let mut figure = nmr_scalar_heatmap(
        spectrum,
        spectrum.magnitude(),
        "Magnitude",
        &HeatmapSpec::default(),
    );
    figure.heatmap = None;
    figure.contours = crate::figures::scalar_contour_overlays(
        &spectrum.magnitude(),
        spectrum.f1_size,
        spectrum.f2_size,
        [
            spectrum.f2_ppm.first().copied().unwrap_or(0.0),
            spectrum.f2_ppm.last().copied().unwrap_or(0.0),
            spectrum.f1_ppm.first().copied().unwrap_or(0.0),
            spectrum.f1_ppm.last().copied().unwrap_or(0.0),
        ],
        contour,
    );
    figure
}

pub(crate) fn build_processed_figure(processed: &Processed2D, preset: Preset2D) -> Figure {
    build_processed_figure_cancellable(processed, preset, &|| false)
        .expect("non-cancelling processed figure")
}

pub(crate) fn build_processed_figure_cancellable(
    processed: &Processed2D,
    preset: Preset2D,
    cancelled: &impl Fn() -> bool,
) -> Option<Figure> {
    if cancelled() {
        return None;
    }
    match processed {
        Processed2D::Ft(spectrum) => {
            let capabilities = scalar_grid_capabilities(
                axis_is_linear(&spectrum.f1_ppm) && axis_is_linear(&spectrum.f2_ppm),
                &[
                    crate::automation::CAP_FIELD_SIGNED,
                    crate::automation::CAP_FIELD_NOISE_SCALE,
                ],
            );
            let contour = default_contour_spec(&capabilities);
            build_figure_2d_cancellable(spectrum, preset, &contour, cancelled)
        }
        Processed2D::Stack(stack) => {
            let figure = build_stack_figure(stack);
            (!cancelled()).then_some(figure)
        }
    }
}
