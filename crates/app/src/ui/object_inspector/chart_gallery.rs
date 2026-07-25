//! The Chart type gallery: chart selection chips plus per-chart options
//! (bins, stacking, colormap, 3D view), committing through undoable actions.

use egui::{DragValue, Ui};
use plotx_core::actions::Action;
use plotx_core::state::{
    ChartSpec, Dataset, ObjectId, PlotxApp, PresentationProfile, RequestedChart, chart_type,
    chart_types_for_capabilities, default_chart_type, default_encoding, encoding_descriptors_for,
    field_peak_magnitude,
};

pub(super) fn chart_gallery(app: &mut PlotxApp, ci: usize, object: ObjectId, ui: &mut Ui) {
    let Some(plot) = app.doc.canvases[ci].object(object).and_then(|o| o.plot()) else {
        return;
    };
    let current = plot.chart.clone();
    let binding = plot.binding.clone();
    let Some(primary) = plot
        .binding
        .primary_dataset()
        .and_then(|id| app.doc.dataset_index(id))
    else {
        return;
    };
    let domain = app.doc.datasets[primary].domain();
    let Some(primary_series) = binding.series.first() else {
        return;
    };
    let Some(field) = app.doc.datasets[primary].field_descriptor(primary_series.source.field)
    else {
        return;
    };
    let types = chart_types_for_capabilities(&field.capabilities, domain);
    let current_id = if chart_type(&current.type_id)
        .is_some_and(|chart| chart.is_applicable_to(&field.capabilities))
    {
        current.type_id.clone()
    } else {
        default_chart_type(domain).id.to_owned()
    };

    ui.separator();
    ui.strong("Chart type");

    let mut next: Option<ChartSpec> = None;
    if types.len() > 1 {
        ui.horizontal_wrapped(|ui| {
            for ct in &types {
                if ui.selectable_label(current_id == ct.id, ct.name).clicked() {
                    next = Some(ChartSpec {
                        type_id: ct.id.to_owned(),
                        ..current.clone()
                    });
                }
            }
        });
    } else if let Some(ct) = types.first() {
        ui.weak(ct.name);
    }

    let needs_column = chart_type(&current_id)
        .map(|c| c.needs_column)
        .unwrap_or(false);
    if needs_column {
        let columns: Vec<(plotx_core::data::ColumnId, String)> = app
            .doc
            .datasets
            .get(primary)
            .and_then(Dataset::as_table)
            .map(|table| {
                table
                    .series_bindings
                    .iter()
                    .filter_map(|binding| {
                        let column = table
                            .typed_state
                            .envelope
                            .revision
                            .snapshot
                            .schema
                            .column(binding.value_column)?;
                        Some((binding.value_column, column.name.clone()))
                    })
                    .collect()
            })
            .unwrap_or_default();
        if !columns.is_empty() {
            let mut sel = current
                .column
                .and_then(|id| columns.iter().position(|(candidate, _)| *candidate == id))
                .unwrap_or(0);
            egui::ComboBox::from_id_salt(("chart_column", object))
                .selected_text(
                    columns
                        .get(sel)
                        .map(|(_, name)| name.clone())
                        .unwrap_or_default(),
                )
                .show_ui(ui, |ui| {
                    for (i, (_, name)) in columns.iter().enumerate() {
                        ui.selectable_value(&mut sel, i, name);
                    }
                });
            let selected = columns.get(sel).map(|(id, _)| *id);
            if selected != current.column {
                next = Some(ChartSpec {
                    type_id: current_id.clone(),
                    column: selected,
                    ..current.clone()
                });
            }
        }
    }

    chart_options(&current, &current_id, &mut next, ui);

    if let Some(after) = next
        && after != current
    {
        app.execute_action(Action::set_chart_type(ci, object, current, after));
        app.session.status = "Changed chart type.".to_owned();
    }

    let encodings = visual_encodings(&field.capabilities);
    if encodings.len() > 1 {
        ui.separator();
        ui.strong("Visual encoding");
        ui.horizontal_wrapped(|ui| {
            for descriptor in encodings {
                let selected = matches!(
                    (descriptor.id, &primary_series.encoding),
                    ("line", plotx_figure::SeriesEncoding::Line(_))
                        | ("contour", plotx_figure::SeriesEncoding::Contour(_))
                        | ("heatmap", plotx_figure::SeriesEncoding::Heatmap(_))
                        | ("image", plotx_figure::SeriesEncoding::Image(_))
                );
                if ui.selectable_label(selected, descriptor.id).clicked() {
                    let requested = match descriptor.id {
                        "line" => RequestedChart::Line,
                        "contour" => RequestedChart::Contour,
                        "heatmap" => RequestedChart::Heatmap,
                        "image" => RequestedChart::Image,
                        _ => continue,
                    };
                    let mut after = binding.clone();
                    if let Some(series) = after.series.first_mut() {
                        let source_field = series.source.field;
                        let dataset = &app.doc.datasets[primary];
                        series.encoding = default_encoding(
                            &field.capabilities,
                            &field.metadata,
                            requested,
                            &PresentationProfile::default(),
                            &|| field_peak_magnitude(dataset, source_field),
                        );
                    }
                    app.execute_action(Action::set_data_binding(
                        ci,
                        object,
                        binding.clone(),
                        after,
                    ));
                    app.session.status = "Changed visual encoding.".to_owned();
                }
            }
        });
    }
}

fn visual_encodings(
    capabilities: &plotx_core::state::FieldCapabilities,
) -> Vec<&'static plotx_core::state::EncodingDescriptor> {
    encoding_descriptors_for(capabilities)
}

/// Per-chart-type options below the gallery. Drag edits only commit on
/// release/defocus so a slider gesture is one undo step, not dozens.
fn chart_options(current: &ChartSpec, current_id: &str, next: &mut Option<ChartSpec>, ui: &mut Ui) {
    match current_id {
        "table_histogram" => {
            ui.horizontal(|ui| {
                let mut auto = current.bins.is_none();
                if ui.checkbox(&mut auto, "Auto bins").changed() {
                    *next = Some(ChartSpec {
                        bins: if auto { None } else { Some(20) },
                        ..current.clone()
                    });
                }
                if let Some(bins) = current.bins
                    && let Some(value) = deferred_drag(ui, "chart_bins", bins, |ui, value| {
                        ui.add(DragValue::new(value).range(1..=512))
                    })
                    && value != bins
                {
                    *next = Some(ChartSpec {
                        bins: Some(value),
                        ..current.clone()
                    });
                }
            });
        }
        "table_bar_grouped" => {
            let mut stacked = current.stacked;
            if ui.checkbox(&mut stacked, "Stacked").changed() {
                *next = Some(ChartSpec {
                    stacked,
                    ..current.clone()
                });
            }
        }
        "table_heatmap" | "table_surface" => {
            ui.horizontal(|ui| {
                ui.label("Colormap");
                egui::ComboBox::from_id_salt(("chart_colormap", current_id))
                    .selected_text(current.colormap.name())
                    .show_ui(ui, |ui| {
                        for cm in plotx_figure::ColormapId::ALL {
                            if ui
                                .selectable_label(current.colormap == cm, cm.name())
                                .clicked()
                            {
                                *next = Some(ChartSpec {
                                    colormap: cm,
                                    ..current.clone()
                                });
                            }
                        }
                    });
            });
            if current_id == "table_surface" {
                ui.horizontal(|ui| {
                    ui.label("View");
                    if let Some(angles) =
                        deferred_drag(ui, "chart_view", current.view_angles, |ui, angles| {
                            let azimuth = ui.add(
                                DragValue::new(&mut angles[0])
                                    .range(-180.0..=180.0)
                                    .suffix("°"),
                            );
                            let elevation = ui
                                .add(DragValue::new(&mut angles[1]).range(5.0..=90.0).suffix("°"));
                            azimuth | elevation
                        })
                        && angles != current.view_angles
                    {
                        *next = Some(ChartSpec {
                            view_angles: angles,
                            ..current.clone()
                        });
                    }
                });
            }
        }
        _ => {}
    }
}

/// Drive drag-value widgets against a scratch copy held in egui temp memory,
/// so the committed model can stay untouched during the gesture (a live drag
/// would otherwise be reset by the unchanged model every frame). Returns the
/// edited value once the gesture ends (drag release / focus loss).
fn deferred_drag<T: Clone + Default + Send + Sync + 'static>(
    ui: &mut Ui,
    key: &'static str,
    committed: T,
    add_widgets: impl FnOnce(&mut Ui, &mut T) -> egui::Response,
) -> Option<T> {
    let id = ui.id().with(key);
    let mut value = ui
        .data_mut(|d| d.get_temp::<T>(id))
        .unwrap_or_else(|| committed.clone());
    let response = add_widgets(ui, &mut value);
    if response.drag_stopped() || response.lost_focus() {
        ui.data_mut(|d| d.remove_temp::<T>(id));
        return Some(value);
    }
    if response.dragged() || response.has_focus() || response.changed() {
        ui.data_mut(|d| d.insert_temp(id, value));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use plotx_core::state::{AfmDataset, Dataset, Nmr2DDataset};
    use std::sync::Arc;

    #[test]
    fn gallery_offers_contour_for_afm_height_and_heatmap_for_nmr_plane() {
        let afm = Dataset::Afm(Box::new(AfmDataset::load(plotx_io::AfmData {
            images: vec![plotx_io::AfmImageChannel {
                name: "Height".to_owned(),
                width: 2,
                height: 2,
                scan_size_x: 1.0,
                scan_size_y: 1.0,
                lateral_unit: "nm".to_owned(),
                scale: plotx_io::AfmScale {
                    multiplier: 1.0,
                    offset: 0.0,
                    unit: "nm".to_owned(),
                },
                raw: Arc::from(vec![0, 1, 2, 3]),
                frame_direction: plotx_io::AfmFrameDirection::Trace,
            }],
            forces: None,
            source: "gallery AFM".to_owned(),
            import_warnings: Vec::new(),
        })));
        let afm_height = afm.field_descriptors().into_iter().next().unwrap();
        assert!(
            visual_encodings(&afm_height.capabilities)
                .iter()
                .any(|encoding| encoding.id == "contour")
        );

        let dimension = |nucleus: &str| plotx_io::Dim {
            spectral_width_hz: 4_000.0,
            observe_freq_mhz: 400.0,
            carrier_ppm: 0.0,
            nucleus: nucleus.to_owned(),
            group_delay: 0.0,
        };
        let nmr = Dataset::Nmr2D(Box::new(Nmr2DDataset::load(plotx_io::NmrData2D {
            data: vec![num_complex::Complex64::new(1.0, 0.0); 4],
            rows: 2,
            cols: 2,
            domain: plotx_io::Domain::Frequency,
            direct: dimension("1H"),
            indirect: dimension("13C"),
            quad: plotx_io::QuadMode::Complex,
            indirect_conjugate: false,
            experiment: None,
            pseudo_axis: None,
            diffusion: None,
            nus: None,
            source: "gallery NMR".to_owned(),
        })));
        let nmr_plane = nmr
            .field_descriptors()
            .into_iter()
            .find(|field| field.local_id == "nmr.real")
            .unwrap();
        assert!(
            visual_encodings(&nmr_plane.capabilities)
                .iter()
                .any(|encoding| encoding.id == "heatmap")
        );
    }
}
