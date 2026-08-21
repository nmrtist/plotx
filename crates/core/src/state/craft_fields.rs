use super::*;
use plotx_data::{
    TraceCollectionCatalog, TraceCollectionId, TraceItemDescriptor, TraceItemId,
    TraceItemParameter, TraceParameterValue,
};
use plotx_figure::{Axis, Color, Figure, RangeAnnotation, Series};
use plotx_processing::craft::{CraftRegionId, synthesize_craft_fid};
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Default)]
pub(crate) struct CraftSpectrumCache {
    models: HashMap<(CraftRunId, Option<CraftRegionId>), plotx_processing::Spectrum>,
    residuals: HashMap<CraftRunId, plotx_processing::Spectrum>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CraftFieldKind {
    Overview,
    Groups,
    Residual,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct CraftFieldSpec {
    pub run: CraftRunId,
    pub kind: CraftFieldKind,
    pub channel: CraftSpectrumChannel,
}

impl CraftFieldSpec {
    pub(crate) fn key(self) -> String {
        format!(
            "nmr.craft.run.{}.{}.{}",
            self.run.0,
            match self.kind {
                CraftFieldKind::Overview => "overview",
                CraftFieldKind::Groups => "groups",
                CraftFieldKind::Residual => "residual",
            },
            match self.channel {
                CraftSpectrumChannel::Magnitude => "magnitude",
                CraftSpectrumChannel::Real => "real",
                CraftSpectrumChannel::Imaginary => "imaginary",
            }
        )
    }
}

impl NmrDataset {
    pub(crate) fn clear_craft_spectrum_cache(&self) {
        if let Ok(mut cache) = self.craft_spectrum_cache.lock() {
            *cache = CraftSpectrumCache::default();
        }
    }

    pub(crate) fn craft_field_specs(&self) -> impl Iterator<Item = CraftFieldSpec> + '_ {
        self.craft_runs.iter().flat_map(|run| {
            [
                CraftSpectrumChannel::Magnitude,
                CraftSpectrumChannel::Real,
                CraftSpectrumChannel::Imaginary,
            ]
            .into_iter()
            .flat_map(move |channel| {
                [
                    CraftFieldKind::Overview,
                    CraftFieldKind::Groups,
                    CraftFieldKind::Residual,
                ]
                .into_iter()
                .map(move |kind| CraftFieldSpec {
                    run: run.id,
                    kind,
                    channel,
                })
            })
        })
    }

    pub(crate) fn craft_field_spec(&self, field: FieldId) -> Option<CraftFieldSpec> {
        self.craft_field_specs()
            .find(|spec| self.field_catalog.id_for_key(&spec.key()) == Some(field))
    }

    pub(crate) fn craft_field_id(
        &self,
        run: CraftRunId,
        kind: CraftFieldKind,
        channel: CraftSpectrumChannel,
    ) -> Option<FieldId> {
        self.field_catalog
            .id_for_key(&CraftFieldSpec { run, kind, channel }.key())
    }

    pub(crate) fn reconcile_craft_fields(&mut self) {
        let keys = std::iter::once("nmr.real".to_owned())
            .chain(self.craft_field_specs().map(CraftFieldSpec::key))
            .collect::<Vec<_>>();
        self.field_catalog
            .reconcile_keys(keys, &self.data.source, None);
        self.attach_craft_trace_collections();
    }

    fn attach_craft_trace_collections(&mut self) {
        let specs = self
            .craft_field_specs()
            .filter(|spec| spec.kind == CraftFieldKind::Groups)
            .collect::<Vec<_>>();
        for spec in specs {
            let key = spec.key();
            let Some(field) = self.field_catalog.id_for_key(&key) else {
                continue;
            };
            let Some(run) = self.craft_run(spec.run) else {
                continue;
            };
            let collection =
                TraceCollectionId::derived(self.data.source.as_bytes(), key.as_bytes());
            let items = run
                .region_summaries
                .iter()
                .enumerate()
                .map(|(index, summary)| TraceItemDescriptor {
                    id: TraceItemId::derived(collection, &summary.region.0.to_le_bytes()),
                    parameters: vec![
                        TraceItemParameter {
                            key: "signal_group".into(),
                            name: "Signal group".into(),
                            value: TraceParameterValue::Text {
                                value: format!("Signal {}", index + 1),
                            },
                        },
                        TraceItemParameter {
                            key: "start_ppm".into(),
                            name: "Start".into(),
                            value: TraceParameterValue::Number {
                                value: summary.start_ppm,
                                unit: "ppm".into(),
                            },
                        },
                        TraceItemParameter {
                            key: "end_ppm".into(),
                            name: "End".into(),
                            value: TraceParameterValue::Number {
                                value: summary.end_ppm,
                                unit: "ppm".into(),
                            },
                        },
                    ],
                    primary_label_parameter: "signal_group".into(),
                    label_override: Some(format!(
                        "Signal {} · {:.4}–{:.4} ppm",
                        index + 1,
                        summary.start_ppm,
                        summary.end_ppm
                    )),
                })
                .collect();
            self.field_catalog.set_trace_collection(
                field,
                TraceCollectionCatalog {
                    id: collection,
                    axis_quantity: "Signal group".into(),
                    axis_unit: String::new(),
                    items,
                },
            );
        }
    }

    pub(crate) fn craft_curve(&self, spec: CraftFieldSpec) -> Option<(Vec<f64>, Vec<f64>)> {
        self.craft_run(spec.run)?;
        let spectrum = match spec.kind {
            CraftFieldKind::Overview => self.spectrum()?.clone(),
            CraftFieldKind::Groups => self.cached_model_spectrum(spec.run, None)?,
            CraftFieldKind::Residual => self.cached_residual_spectrum(spec.run)?,
        };
        let values = channel_values(&spectrum.values, spec.channel);
        Some((spectrum.ppm, values))
    }

    pub(crate) fn craft_field_figure(&self, spec: CraftFieldSpec) -> Option<Figure> {
        let run = self.craft_run(spec.run)?;
        match spec.kind {
            CraftFieldKind::Overview => {
                let observed = self.spectrum()?;
                let model = self.cached_model_spectrum(spec.run, None)?;
                let observed_values = channel_values(&observed.values, spec.channel);
                let model_values = channel_values(&model.values, spec.channel);
                let (y0, y1) = finite_range(observed_values.iter().chain(&model_values).copied());
                let mut figure = Figure::new(
                    "",
                    Axis::new(
                        crate::figures::axis_label(&self.data.nucleus),
                        observed.ppm_bounds().0,
                        observed.ppm_bounds().1,
                    )
                    .reversed(true),
                    Axis::new("Intensity (a.u.)", y0, y1),
                )
                .with_series(
                    Series::line("Observed", points(&observed.ppm, &observed_values))
                        .colored(Color::AXIS),
                )
                .with_series(
                    Series::line("Reconstruction", points(&model.ppm, &model_values))
                        .colored(Color::rgb(0x2b, 0x7d, 0xc0)),
                )
                .with_series(
                    Series::sticks(
                        "Retained components",
                        run.components
                            .iter()
                            .map(|component| [component.chemical_shift_ppm, y1])
                            .collect(),
                    )
                    .colored(Color::rgb(0xd9, 0x7a, 0x12)),
                );
                figure.series_colors_are_semantic = true;
                figure.range_annotations = run
                    .region_summaries
                    .iter()
                    .enumerate()
                    .map(|(index, summary)| {
                        let [red, green, blue] = region_color(index);
                        RangeAnnotation {
                            source_id: summary.region.0,
                            x0: summary.start_ppm,
                            x1: summary.end_ppm,
                            label: format!("Signal {}", index + 1),
                            label_position: None,
                            color: Color::rgb(red, green, blue),
                            fill_opacity: 0.10,
                            width: 1.0,
                        }
                    })
                    .collect();
                Some(figure)
            }
            CraftFieldKind::Residual | CraftFieldKind::Groups => {
                let (x, y) = self.craft_curve(spec)?;
                Some(single_curve_figure(
                    &self.data.nucleus,
                    if spec.kind == CraftFieldKind::Residual {
                        "Complex residual"
                    } else {
                        "Reconstruction"
                    },
                    x,
                    y,
                    spec.kind == CraftFieldKind::Residual,
                ))
            }
        }
    }

    pub(crate) fn craft_group_figure(
        &self,
        spec: CraftFieldSpec,
        region: CraftRegionId,
        label: String,
    ) -> Option<Figure> {
        self.craft_run(spec.run)?;
        let spectrum = self.cached_model_spectrum(spec.run, Some(region))?;
        single_curve_figure(
            &self.data.nucleus,
            &label,
            spectrum.ppm,
            channel_values(&spectrum.values, spec.channel),
            false,
        )
        .into()
    }

    fn cached_model_spectrum(
        &self,
        run: CraftRunId,
        region: Option<CraftRegionId>,
    ) -> Option<plotx_processing::Spectrum> {
        if let Ok(cache) = self.craft_spectrum_cache.lock()
            && let Some(spectrum) = cache.models.get(&(run, region))
        {
            return Some(spectrum.clone());
        }
        let stored = self.craft_run(run)?;
        let components = stored
            .components
            .iter()
            .filter(|component| region.is_none_or(|region| component.region == region))
            .cloned()
            .collect::<Vec<_>>();
        let spectrum = transformed_fid(
            self,
            &components,
            stored
                .provenance
                .invocation
                .derived_plan
                .reconstruction_points
                .max(1),
        );
        if let Ok(mut cache) = self.craft_spectrum_cache.lock() {
            cache.models.insert((run, region), spectrum.clone());
        }
        Some(spectrum)
    }

    fn cached_residual_spectrum(&self, run: CraftRunId) -> Option<plotx_processing::Spectrum> {
        if let Ok(cache) = self.craft_spectrum_cache.lock()
            && let Some(spectrum) = cache.residuals.get(&run)
        {
            return Some(spectrum.clone());
        }
        let stored = self.craft_run(run)?;
        let model = synthesize_craft_fid(
            &stored.components,
            self.data.points.len(),
            self.data.spectral_width_hz,
        );
        let residual = self
            .data
            .points
            .iter()
            .zip(model)
            .map(|(observed, model)| observed - model)
            .collect();
        let spectrum = transformed_points(self, residual);
        if let Ok(mut cache) = self.craft_spectrum_cache.lock() {
            cache.residuals.insert(run, spectrum.clone());
        }
        Some(spectrum)
    }
}

fn transformed_fid(
    dataset: &NmrDataset,
    components: &[plotx_processing::craft::CraftComponent],
    point_count: usize,
) -> plotx_processing::Spectrum {
    transformed_points(
        dataset,
        synthesize_craft_fid(components, point_count, dataset.data.spectral_width_hz),
    )
}

fn transformed_points(
    dataset: &NmrDataset,
    points: Vec<num_complex::Complex64>,
) -> plotx_processing::Spectrum {
    let mut data = dataset.data.clone();
    data.points = points;
    let base =
        plotx_processing::transform_base(&data, dataset.pipeline(), dataset.group_delay_correct);
    plotx_processing::reapply(&base, dataset.pipeline())
}

fn channel_values(values: &[num_complex::Complex64], channel: CraftSpectrumChannel) -> Vec<f64> {
    values
        .iter()
        .map(|value| match channel {
            CraftSpectrumChannel::Magnitude => value.norm(),
            CraftSpectrumChannel::Real => value.re,
            CraftSpectrumChannel::Imaginary => value.im,
        })
        .collect()
}

fn points(x: &[f64], y: &[f64]) -> Vec<[f64; 2]> {
    x.iter()
        .copied()
        .zip(y.iter().copied())
        .map(|(x, y)| [x, y])
        .collect()
}

fn finite_range(values: impl IntoIterator<Item = f64>) -> (f64, f64) {
    let (mut lo, mut hi) = values
        .into_iter()
        .filter(|value| value.is_finite())
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), value| {
            (lo.min(value), hi.max(value))
        });
    if !lo.is_finite() || !hi.is_finite() {
        return (-0.5, 0.5);
    }
    if lo == hi {
        let pad = lo.abs().max(1.0) * 0.05;
        lo -= pad;
        hi += pad;
    }
    let pad = (hi - lo) * 0.04;
    (lo - pad, hi + pad)
}

fn single_curve_figure(
    nucleus: &str,
    label: &str,
    x: Vec<f64>,
    y: Vec<f64>,
    zero_line: bool,
) -> Figure {
    let bounds = x
        .iter()
        .copied()
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), value| {
            (lo.min(value), hi.max(value))
        });
    let (y0, y1) = finite_range(y.iter().copied().chain(zero_line.then_some(0.0)));
    let mut figure = Figure::new(
        "",
        Axis::new(crate::figures::axis_label(nucleus), bounds.0, bounds.1).reversed(true),
        Axis::new("Intensity (a.u.)", y0, y1),
    )
    .with_series(Series::line(label, points(&x, &y)));
    if zero_line {
        figure.series.push(
            Series::line("Zero", vec![[bounds.0, 0.0], [bounds.1, 0.0]])
                .colored(Color::rgb(0x99, 0x99, 0x99)),
        );
        figure.series_colors_are_semantic = true;
    }
    figure
}

pub(crate) fn craft_curve_payload(dataset: &NmrDataset, field: FieldId) -> Option<FieldPayload> {
    let spec = dataset.craft_field_spec(field)?;
    let (x, values) = dataset.craft_curve(spec)?;
    Some(FieldPayload::Curve1D(Curve1D {
        x: Arc::from(x),
        values: Arc::from(
            values
                .into_iter()
                .map(|value| value as f32)
                .collect::<Vec<_>>(),
        ),
    }))
}
