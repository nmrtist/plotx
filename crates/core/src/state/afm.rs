use super::*;
use plotx_figure::{AxisFrame, ColormapId, Contour, ContourStyle, HeatmapGrid, Series};
use std::sync::Arc;

#[derive(Clone)]
pub struct AfmDataset {
    pub resource_id: DatasetId,
    /// Persisted mapping from stable channel keys to dataset-local field ids.
    pub field_catalog: FieldCatalog,
    pub data: Arc<AfmData>,
    /// Immutable keys calculated with the loaded raster data, never serialized.
    pub(crate) image_field_keys: Arc<[String]>,
    pub name: Option<String>,
    pub selected_pixel: [usize; 2],
    pub lineage: Option<DatasetLineage>,
}

impl AfmDataset {
    pub fn load(data: AfmData) -> Self {
        let selected_pixel = data.forces.as_ref().map_or([0, 0], |forces| {
            [forces.grid_width / 2, forces.grid_height / 2]
        });
        let image_field_keys = crate::state::afm_channel_keys(&data);
        let mut field_catalog = crate::state::afm_field_catalog_for_keys(&data, &image_field_keys);
        field_catalog.attach_provenance(&data.source, None);
        Self {
            resource_id: DatasetId::new(),
            field_catalog,
            data: Arc::new(data),
            image_field_keys,
            name: None,
            selected_pixel,
            lineage: None,
        }
    }

    pub fn map_figure(&self, field: FieldId, colormap: ColormapId) -> Option<Figure> {
        let channel = self.image_for_field(field)?;
        let values: Vec<f32> = channel
            .raw
            .iter()
            .map(|value| channel.scale.apply(*value) as f32)
            .collect();
        let (minimum, maximum) = values
            .iter()
            .copied()
            .filter(|v| v.is_finite())
            .fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), value| {
                (lo.min(value), hi.max(value))
            });
        let mut figure = Figure::new(
            &channel.name,
            Axis::new(&channel.lateral_unit, 0.0, channel.scan_size_x),
            Axis::new(&channel.lateral_unit, 0.0, channel.scan_size_y),
        );
        figure.heatmap = Some(HeatmapGrid {
            rows: channel.height,
            cols: channel.width,
            values,
            x_bounds: [0.0, channel.scan_size_x],
            y_bounds: [0.0, channel.scan_size_y],
            colormap,
            value_range: [minimum, maximum],
        });
        figure.lock_aspect = true;
        figure.axis_frame = AxisFrame::Box;
        Some(figure)
    }

    pub(crate) fn contour_base_figure(&self, field: FieldId) -> Option<Figure> {
        let channel = self.image_for_field(field)?;
        let mut figure = Figure::new(
            &channel.name,
            Axis::new(&channel.lateral_unit, 0.0, channel.scan_size_x),
            Axis::new(&channel.lateral_unit, 0.0, channel.scan_size_y),
        );
        figure.lock_aspect = true;
        figure.axis_frame = AxisFrame::Box;
        Some(figure)
    }

    pub(crate) fn contour_figure_from_geometry(
        &self,
        field: FieldId,
        geometry: &ContourGeometry,
        style: &ContourStyle,
    ) -> Option<Figure> {
        let mut figure = self.contour_base_figure(field)?;
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
        Some(figure)
    }

    pub fn force_figure(&self, field: FieldId) -> Option<Figure> {
        (self.field_catalog.id_for_key("afm.force_curve") == Some(field)).then_some(())?;
        let forces = self.data.forces.as_ref()?;
        let curve = forces.curve_raw(self.selected_pixel[0], self.selected_pixel[1])?;
        let x_value = |display_index: usize| {
            forces
                .z_positions
                .as_ref()
                .and_then(|axis| axis.get(display_index).copied())
                .or_else(|| {
                    let branch_index = if display_index < forces.approach_samples {
                        display_index
                    } else {
                        display_index - forces.approach_samples
                    };
                    forces
                        .sample_period_s
                        .map(|period| branch_index as f64 * period)
                })
                .unwrap_or_else(|| {
                    if display_index < forces.approach_samples {
                        display_index as f64
                    } else {
                        (display_index - forces.approach_samples) as f64
                    }
                })
        };
        let displacement_factor = physical_length_factor(&forces.signal_scale.unit)
            .or(forces.deflection_sensitivity_m_per_v);
        let spring_constant = forces
            .spring_constant_n_per_m
            .filter(|value| value.is_finite() && *value > 0.0);
        let calibrated = |raw: i32| {
            let stored = forces.signal_scale.apply(raw);
            match (displacement_factor, spring_constant) {
                (Some(to_metres), Some(spring)) => stored * to_metres * spring * 1.0e9,
                (Some(to_metres), None) => stored * to_metres * 1.0e9,
                (None, _) => stored,
            }
        };
        let ordered: Vec<[f64; 2]> = forces
            .display_order
            .iter()
            .enumerate()
            .filter_map(|(display_index, &raw_index)| {
                curve
                    .get(raw_index)
                    .map(|&raw| [x_value(display_index), calibrated(raw)])
            })
            .collect();
        let split = forces.approach_samples.min(ordered.len());
        let x_label = if forces.z_positions.is_some() {
            "Z"
        } else {
            "Time"
        };
        let y_label = if displacement_factor.is_some() && spring_constant.is_some() {
            "Force (nN)"
        } else if displacement_factor.is_some() {
            "Deflection (nm)"
        } else {
            &forces.signal_scale.unit
        };
        let (x_min, x_max) = ordered
            .iter()
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), point| {
                (lo.min(point[0]), hi.max(point[0]))
            });
        let (y_min, y_max) = ordered
            .iter()
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), point| {
                (lo.min(point[1]), hi.max(point[1]))
            });
        let mut figure = Figure::new(
            "Force Curve",
            Axis::new(x_label, x_min, x_max),
            Axis::new(y_label, y_min, y_max),
        );
        figure
            .series
            .push(Series::line("Approach", ordered[..split].to_vec()));
        figure.series.push(
            Series::line("Retract", ordered[split..].to_vec())
                .colored(Color::rgb(0xd1, 0x24, 0x2a)),
        );
        Some(figure)
    }

    fn image_for_field(&self, field: FieldId) -> Option<&plotx_io::AfmImageChannel> {
        self.data
            .images
            .iter()
            .zip(self.image_field_keys.iter())
            .find(|(_, key)| self.field_catalog.id_for_key(key) == Some(field))
            .map(|(channel, _)| channel)
    }
}

fn physical_length_factor(unit: &str) -> Option<f64> {
    let normalized = unit
        .split('/')
        .next()?
        .trim()
        .to_ascii_lowercase()
        .replace(['µ', '~'], "u");
    match normalized.as_str() {
        "m" => Some(1.0),
        "mm" => Some(1.0e-3),
        "um" => Some(1.0e-6),
        "nm" => Some(1.0e-9),
        "pm" => Some(1.0e-12),
        _ => None,
    }
}
