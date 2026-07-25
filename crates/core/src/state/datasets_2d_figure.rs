use super::*;
use plotx_figure::{
    Axis, AxisFrame, Contour, ContourStyle, HeatmapGrid, HeatmapSpec, SeriesEncoding,
};

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
            ("nmr.real", SeriesEncoding::Contour(_)) => {
                Some(nmr_contour_base(spectrum, self.preset, "Real"))
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
            ("nmr.magnitude", SeriesEncoding::Contour(_)) => {
                Some(nmr_contour_base(spectrum, self.preset, "Magnitude"))
            }
            _ => None,
        }
    }

    pub(crate) fn contour_figure_from_geometry(
        &self,
        field: FieldId,
        geometry: &ContourGeometry,
        style: &ContourStyle,
    ) -> Option<Figure> {
        let Processed2D::Ft(spectrum) = &self.processed else {
            return None;
        };
        let name = if self.field_catalog.id_for_key("nmr.real") == Some(field) {
            "Real"
        } else if self.field_catalog.id_for_key("nmr.magnitude") == Some(field) {
            "Magnitude"
        } else {
            return None;
        };
        Some(apply_contour_geometry(
            nmr_contour_base(spectrum, self.preset, name),
            geometry,
            style,
        ))
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

fn nmr_contour_base(
    spectrum: &plotx_processing::Spectrum2D,
    preset: Preset2D,
    field_name: &str,
) -> Figure {
    let (x, y) = nmr_axes(spectrum);
    let mut figure = Figure::new(
        format!("{field_name} — {} — {}", preset.label(), spectrum.source),
        x,
        y,
    )
    .with_axis_frame(AxisFrame::Box);
    figure.lock_aspect = preset.homonuclear();
    figure
}

pub(crate) fn apply_contour_geometry(
    mut figure: Figure,
    geometry: &ContourGeometry,
    style: &ContourStyle,
) -> Figure {
    if !geometry.positive.is_empty() {
        figure.contours.push(Contour {
            segments: geometry.positive.as_ref().to_vec(),
            color: style.positive_color.resolve(),
            width: style.width.get(),
        });
    }
    if !geometry.negative.is_empty() {
        figure.contours.push(Contour {
            segments: geometry.negative.as_ref().to_vec(),
            color: style.negative_color.resolve(),
            width: style.width.get(),
        });
    }
    figure
}

pub(crate) fn build_processed_figure(processed: &Processed2D, preset: Preset2D) -> Figure {
    match processed {
        Processed2D::Ft(spectrum) => nmr_contour_base(spectrum, preset, "Real"),
        Processed2D::Stack(stack) => build_stack_figure(stack),
    }
}
