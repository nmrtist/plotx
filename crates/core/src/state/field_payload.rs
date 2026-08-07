//! Provider adapters that turn dataset internals into representation-neutral
//! field payloads, plus the cheap representation query capability derivation
//! uses. Renderers and derived-data workers only ever see the results.

use super::field_runtime::*;
use super::{FieldCatalog, FieldId};
use std::sync::Arc;

impl super::Dataset {
    /// Return an owned, representation-neutral payload for one concrete field.
    /// The provider boundary is the only place that knows dataset internals.
    ///
    /// This materializes values and is therefore reserved for work that needs
    /// them (worker snapshots, summaries). Capability and descriptor queries
    /// use [`Self::field_representation`] instead.
    pub fn field_payload(&self, id: FieldId) -> Option<FieldPayload> {
        #[cfg(test)]
        crate::contour_probe::record_field_payload();
        match self {
            Self::Nmr(nmr) => (nmr.field_catalog.id_for_key("nmr.real") == Some(id)).then(|| {
                let (x, values) = match &nmr.processed {
                    plotx_processing::Processed1D::Time(trace) => (&trace.time_s, &trace.values),
                    plotx_processing::Processed1D::Frequency(spectrum) => {
                        (&spectrum.ppm, &spectrum.values)
                    }
                };
                FieldPayload::Curve1D(Curve1D {
                    x: Arc::from(x.clone()),
                    values: Arc::from(
                        values
                            .iter()
                            .map(|value| value.re as f32)
                            .collect::<Vec<_>>(),
                    ),
                })
            }),
            Self::Nmr2D(nmr) => nmr_field_payload(nmr, id),
            Self::Table(table) => {
                (table.field_catalog.id_for_key("table.default_series") == Some(id)).then(|| {
                    let figure = table.figure();
                    let points = figure
                        .series
                        .first()
                        .map(|series| series.points.as_slice())
                        .unwrap_or_default();
                    FieldPayload::Curve1D(Curve1D {
                        x: Arc::from(points.iter().map(|point| point[0]).collect::<Vec<_>>()),
                        values: Arc::from(
                            points
                                .iter()
                                .map(|point| point[1] as f32)
                                .collect::<Vec<_>>(),
                        ),
                    })
                })
            }
            Self::Electrophysiology(recording) => {
                let channel = (0..recording.data.channels.len()).find(|&index| {
                    recording
                        .field_key(index)
                        .and_then(|key| recording.field_catalog.id_for_key(key))
                        == Some(id)
                })?;
                let values = recording
                    .data
                    .sweeps
                    .first()
                    .and_then(|sweep| sweep.channels.get(channel))
                    .cloned()
                    .unwrap_or_default();
                let dt = if recording.data.sample_rate_hz.is_finite()
                    && recording.data.sample_rate_hz > 0.0
                {
                    1.0 / recording.data.sample_rate_hz
                } else {
                    1.0
                };
                Some(FieldPayload::Curve1D(Curve1D {
                    x: Arc::from(
                        (0..values.len())
                            .map(|index| index as f64 * dt)
                            .collect::<Vec<_>>(),
                    ),
                    values: Arc::from(
                        values
                            .into_iter()
                            .map(|value| value as f32)
                            .collect::<Vec<_>>(),
                    ),
                }))
            }
            Self::Afm(afm) => {
                if let Some(channel) = afm
                    .data
                    .images
                    .iter()
                    .zip(afm.image_field_keys.iter())
                    .find_map(|(channel, key)| {
                        (afm.field_catalog.id_for_key(key) == Some(id)).then_some(channel)
                    })
                {
                    return Some(FieldPayload::ScalarGrid2D(ScalarGrid2D {
                        values: Arc::from(
                            channel
                                .raw
                                .iter()
                                .map(|value| channel.scale.apply(*value) as f32)
                                .collect::<Vec<_>>(),
                        ),
                        rows: channel.height,
                        cols: channel.width,
                        x: linear_or_explicit_axis(0.0, channel.scan_size_x, channel.width),
                        y: linear_or_explicit_axis(0.0, channel.scan_size_y, channel.height),
                    }));
                }
                (afm.field_catalog.id_for_key("afm.force_curve") == Some(id)).then(|| {
                    let values = afm
                        .data
                        .forces
                        .as_ref()
                        .and_then(|forces| {
                            forces.curve_raw(afm.selected_pixel[0], afm.selected_pixel[1])
                        })
                        .map(|curve| curve.iter().map(|value| *value as f32).collect::<Vec<_>>())
                        .unwrap_or_default();
                    FieldPayload::Curve1D(Curve1D {
                        x: Arc::from(
                            (0..values.len())
                                .map(|index| index as f64)
                                .collect::<Vec<_>>(),
                        ),
                        values: Arc::from(values),
                    })
                })
            }
            Self::Xps(dataset) => {
                let region = dataset.region_for_field(id)?;
                let processed = dataset.displayed_region(region.id)?;
                Some(FieldPayload::Curve1D(Curve1D {
                    x: Arc::from(processed.binding_energy_ev),
                    values: Arc::from(
                        processed
                            .intensity
                            .into_iter()
                            .map(|value| value as f32)
                            .collect::<Vec<_>>(),
                    ),
                }))
            }
            Self::MassSpec(dataset) => dataset.field_values(id).map(|(_, _, _, points, _)| {
                FieldPayload::Curve1D(Curve1D {
                    x: Arc::from(points.iter().map(|point| point[0]).collect::<Vec<_>>()),
                    values: Arc::from(
                        points
                            .iter()
                            .map(|point| point[1] as f32)
                            .collect::<Vec<_>>(),
                    ),
                })
            }),
            Self::Xrd(dataset) => (dataset.field_id() == Some(id)).then(|| {
                FieldPayload::Curve1D(Curve1D {
                    x: Arc::from(dataset.data.two_theta_deg.clone()),
                    values: Arc::from(
                        dataset
                            .processed
                            .intensity
                            .iter()
                            .map(|value| *value as f32)
                            .collect::<Vec<_>>(),
                    ),
                })
            }),
        }
    }

    /// The cheap counterpart of [`Self::field_payload`]: what the payload for
    /// `id` *would* be, without materializing a single value. Capability
    /// derivation runs on the UI thread for every descriptor lookup, so it must
    /// never allocate an O(rows × cols) buffer just to learn that a grid is
    /// regularly sampled.
    ///
    /// It must answer `Some` for exactly the ids [`Self::field_payload`] does,
    /// and describe the same representation;
    /// `cheap_representation_matches_the_materialized_payload` locks both in
    /// for every dataset variant so a second, drifting source of capability
    /// truth cannot reappear.
    pub fn field_representation(&self, id: FieldId) -> Option<FieldRepresentation> {
        match self {
            Self::Nmr(nmr) => (nmr.field_catalog.id_for_key("nmr.real") == Some(id))
                .then_some(FieldRepresentation::Curve1D),
            Self::Nmr2D(nmr) => nmr_field_representation(nmr, id),
            Self::Table(table) => (table.field_catalog.id_for_key("table.default_series")
                == Some(id))
            .then_some(FieldRepresentation::Curve1D),
            Self::Electrophysiology(recording) => (0..recording.data.channels.len())
                .any(|index| {
                    recording
                        .field_key(index)
                        .and_then(|key| recording.field_catalog.id_for_key(key))
                        == Some(id)
                })
                .then_some(FieldRepresentation::Curve1D),
            Self::Afm(afm) => {
                if let Some(channel) = afm
                    .data
                    .images
                    .iter()
                    .zip(afm.image_field_keys.iter())
                    .find_map(|(channel, key)| {
                        (afm.field_catalog.id_for_key(key) == Some(id)).then_some(channel)
                    })
                {
                    return Some(FieldRepresentation::ScalarGrid2D {
                        rows: channel.height,
                        cols: channel.width,
                        values: channel.raw.len(),
                        x_linear: spanned_axis_is_linear(0.0, channel.scan_size_x),
                        y_linear: spanned_axis_is_linear(0.0, channel.scan_size_y),
                    });
                }
                (afm.field_catalog.id_for_key("afm.force_curve") == Some(id))
                    .then_some(FieldRepresentation::Curve1D)
            }
            Self::MassSpec(dataset) => dataset.field_representation(id),
            Self::Xrd(dataset) => {
                (dataset.field_id() == Some(id)).then_some(FieldRepresentation::Curve1D)
            }
            Self::Xps(dataset) => dataset
                .region_for_field(id)
                .map(|_| FieldRepresentation::Curve1D),
        }
    }

    /// Construct a worker-owned field snapshot from this dataset and a version
    /// assigned by `ComputeService`. The version is intentionally supplied by
    /// the runtime owner instead of being stored in the document.
    ///
    /// `cached_summary` is the runtime's summary for this exact
    /// `(field, version)`; passing it skips a full min/max scan of the payload.
    pub fn field_snapshot(
        &self,
        id: FieldId,
        version: FieldVersion,
        cached_summary: Option<FieldSummary>,
    ) -> Option<FieldSnapshot> {
        let payload = self.field_payload(id)?;
        let field = FieldRef {
            resource: self.resource_id(),
            field: id,
        };
        Some(FieldSnapshot::new(
            VersionedFieldRef { field, version },
            payload,
            self.field_provenance(id),
            cached_summary,
        ))
    }

    fn field_provenance(&self, id: FieldId) -> FieldProvenance {
        // Catalog provenance is populated at dataset construction/load and is
        // serialized with the stable field identity map. A deterministic
        // fallback keeps transient test fixtures that bypass constructors
        // meaningful; ordinary project data is validated to contain every entry.
        self.field_catalog()
            .provenance_for(id)
            .cloned()
            .unwrap_or_else(|| {
                let (source, algorithm) = match self {
                    Self::Nmr(dataset) => (dataset.data.source.as_str(), None),
                    Self::Nmr2D(dataset) => (
                        dataset.data.source.as_str(),
                        Some(FieldAlgorithmProvenance {
                            algorithm: "process_2d".to_owned(),
                            version: 1,
                        }),
                    ),
                    Self::Table(dataset) => (dataset.name.as_deref().unwrap_or("table"), None),
                    Self::Electrophysiology(dataset) => (dataset.data.source.as_str(), None),
                    Self::Afm(dataset) => (dataset.data.source.as_str(), None),
                    Self::MassSpec(dataset) => (dataset.run.source.as_str(), None),
                    Self::Xrd(dataset) => (
                        dataset.data.source.as_str(),
                        Some(FieldAlgorithmProvenance {
                            algorithm: "xrd.process".to_owned(),
                            version: 1,
                        }),
                    ),
                    Self::Xps(dataset) => (dataset.experiment.source.as_str(), None),
                };
                FieldCatalog::make_provenance(source, id, algorithm)
            })
    }
}

fn nmr_field_payload(dataset: &super::Nmr2DDataset, id: FieldId) -> Option<FieldPayload> {
    match &dataset.processed {
        plotx_processing::Processed2D::Ft(spectrum) => {
            if dataset.field_catalog.id_for_key("nmr.real") == Some(id) {
                return Some(FieldPayload::ScalarGrid2D(nmr_scalar_grid(
                    spectrum,
                    spectrum.real(),
                )));
            }
            if dataset.field_catalog.id_for_key("nmr.magnitude") == Some(id) {
                return Some(FieldPayload::ScalarGrid2D(nmr_scalar_grid(
                    spectrum,
                    spectrum.magnitude(),
                )));
            }
            None
        }
        plotx_processing::Processed2D::Stack(stack)
            if dataset.field_catalog.id_for_key("nmr.stack") == Some(id) =>
        {
            let values = stack
                .traces
                .first()
                .map(|trace| {
                    trace
                        .iter()
                        .map(|value| value.re as f32)
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            Some(FieldPayload::Curve1D(Curve1D {
                x: Arc::from(stack.ppm.clone()),
                values: Arc::from(values),
            }))
        }
        plotx_processing::Processed2D::Stack(_) => {
            pseudo_map_grid(dataset, id).map(FieldPayload::ScalarGrid2D)
        }
    }
}

fn nmr_field_representation(
    dataset: &super::Nmr2DDataset,
    id: FieldId,
) -> Option<FieldRepresentation> {
    match &dataset.processed {
        plotx_processing::Processed2D::Ft(spectrum) => {
            (dataset.field_catalog.id_for_key("nmr.real") == Some(id)
                || dataset.field_catalog.id_for_key("nmr.magnitude") == Some(id))
            .then(|| FieldRepresentation::ScalarGrid2D {
                rows: spectrum.f1_size,
                cols: spectrum.f2_size,
                // `real()`/`magnitude()` map over `data`, so the buffer a payload
                // would carry has exactly this length.
                values: spectrum.data.len(),
                x_linear: axis_is_linear(&spectrum.f2_ppm),
                y_linear: axis_is_linear(&spectrum.f1_ppm),
            })
        }
        plotx_processing::Processed2D::Stack(_)
            if dataset.field_catalog.id_for_key("nmr.stack") == Some(id) =>
        {
            Some(FieldRepresentation::Curve1D)
        }
        plotx_processing::Processed2D::Stack(_) => {
            pseudo_map_grid(dataset, id).map(|grid| FieldRepresentation::ScalarGrid2D {
                rows: grid.rows,
                cols: grid.cols,
                values: grid.values.len(),
                x_linear: matches!(grid.x, AxisSampling::Linear { .. }),
                y_linear: matches!(grid.y, AxisSampling::Linear { .. }),
            })
        }
    }
}

fn pseudo_map_grid(dataset: &super::Nmr2DDataset, id: FieldId) -> Option<ScalarGrid2D> {
    if dataset.field_catalog.id_for_key("nmr.ilt_map") == Some(id) {
        let map = dataset.ilt_map.as_ref()?;
        let rows = map.d_grid.len();
        let cols = map.ppm.len();
        let mut values = vec![0.0_f32; rows * cols];
        for (column, amplitudes) in map.amp.iter().enumerate().take(cols) {
            for (row, amplitude) in amplitudes.iter().enumerate().take(rows) {
                values[row * cols + column] = *amplitude as f32;
            }
        }
        return Some(ScalarGrid2D {
            values: Arc::from(values),
            rows,
            cols,
            x: axis_sampling(&map.ppm),
            y: axis_sampling(
                &map.d_grid
                    .iter()
                    .map(|value| value.max(f64::MIN_POSITIVE).log10())
                    .collect::<Vec<_>>(),
            ),
        });
    }
    if dataset.field_catalog.id_for_key("nmr.dosy_map") == Some(id) {
        let map = dataset.dosy_map.as_ref()?;
        return Some(super::dosy_scalar_grid(map));
    }
    None
}

pub(crate) fn nmr_scalar_grid(
    spectrum: &plotx_processing::Spectrum2D,
    values: Vec<f32>,
) -> ScalarGrid2D {
    ScalarGrid2D {
        values: Arc::from(values),
        rows: spectrum.f1_size,
        cols: spectrum.f2_size,
        x: axis_sampling(&spectrum.f2_ppm),
        y: axis_sampling(&spectrum.f1_ppm),
    }
}

fn axis_sampling(values: &[f64]) -> AxisSampling {
    if axis_is_linear(values) {
        let start = values.first().copied().unwrap_or(0.0);
        let end = values.last().copied().unwrap_or(start);
        AxisSampling::Linear { start, end }
    } else {
        AxisSampling::Explicit(Arc::from(values.to_vec()))
    }
}

fn linear_or_explicit_axis(start: f64, end: f64, count: usize) -> AxisSampling {
    if spanned_axis_is_linear(start, end) {
        AxisSampling::Linear { start, end }
    } else {
        AxisSampling::Explicit(Arc::from(
            (0..count).map(|index| index as f64).collect::<Vec<_>>(),
        ))
    }
}

/// Shared by `linear_or_explicit_axis` and the cheap representation query so
/// the two can never disagree about an evenly spanned axis.
fn spanned_axis_is_linear(start: f64, end: f64) -> bool {
    start.is_finite() && end.is_finite()
}

fn axis_is_linear(values: &[f64]) -> bool {
    let Some((&first, rest)) = values.split_first() else {
        return false;
    };
    if !first.is_finite() {
        return false;
    }
    if rest.is_empty() {
        return true;
    }
    let last = *rest.last().unwrap_or(&first);
    if !last.is_finite() {
        return false;
    }
    let step = (last - first) / (values.len() - 1) as f64;
    step.is_finite()
        && values.iter().enumerate().all(|(index, value)| {
            let expected = first + step * index as f64;
            value.is_finite() && (*value - expected).abs() <= 1e-9 * expected.abs().max(1.0)
        })
}
