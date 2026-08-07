//! Dataset snapshots and table layouts for unified numerical export.

use crate::state::{Dataset, ElectrophysiologyAnalysisError, StoredCurveFitAnalysis};
use crate::{Integral2D, IntegralResult};
use num_complex::Complex64;
use plotx_io::delimited::{DelimitedWriter, Delimiter};
use plotx_processing::{Processed2D, Spectrum2D, StackSpectrum, StepKind};
use std::io::{self, Write};
use std::sync::Arc;

mod service;
pub use service::*;
mod write;
mod xlsx;
use write::{
    safe_name, write_1d, write_electrophysiology, write_fits, write_integrals_1d,
    write_integrals_2d, write_peaks, write_pseudo_2d, write_true_2d, write_xps, write_xps_fits,
    write_xrd,
};
pub use xlsx::delimited_sidecar_path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataExportContent {
    ProcessedData,
    TypedTable,
    Peaks,
    Integrals,
    CurveFits,
}

impl DataExportContent {
    pub const fn label(self) -> &'static str {
        match self {
            Self::ProcessedData => "Processed data",
            Self::TypedTable => "Complete typed table / series",
            Self::Peaks => "Peak table",
            Self::Integrals => "Integral table",
            Self::CurveFits => "Curve-fit parameters",
        }
    }

    const fn slug(self) -> &'static str {
        match self {
            Self::ProcessedData => "processed-data",
            Self::TypedTable => "data-table",
            Self::Peaks => "peaks",
            Self::Integrals => "integrals",
            Self::CurveFits => "fit-parameters",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntensityChannel {
    Real,
    Imaginary,
    Magnitude,
}

impl IntensityChannel {
    pub const ALL: [Self; 3] = [Self::Real, Self::Imaginary, Self::Magnitude];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Real => "Real",
            Self::Imaginary => "Imaginary",
            Self::Magnitude => "Magnitude",
        }
    }

    const fn slug(self) -> &'static str {
        match self {
            Self::Real => "real",
            Self::Imaginary => "imaginary",
            Self::Magnitude => "magnitude",
        }
    }

    fn reduce(self, value: Complex64) -> f64 {
        match self {
            Self::Real => value.re,
            Self::Imaginary => value.im,
            Self::Magnitude => value.norm(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TableShape {
    Matrix,
    Long,
}

impl TableShape {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Matrix => "Matrix",
            Self::Long => "Long",
        }
    }

    const fn slug(self) -> &'static str {
        match self {
            Self::Matrix => "matrix",
            Self::Long => "long",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DataExportRequest {
    pub content: DataExportContent,
    pub channel: IntensityChannel,
    pub shape: TableShape,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataExportAvailability {
    pub contents: Vec<DataExportContent>,
    pub has_channel_choice: bool,
    pub has_shape_choice: bool,
    pub default_channel: IntensityChannel,
}

impl DataExportAvailability {
    pub fn for_dataset(dataset: &Dataset) -> Self {
        let mut contents = Vec::new();
        if processed_data_available(dataset) {
            contents.push(DataExportContent::ProcessedData);
        }
        if dataset.as_table().is_some_and(|table| {
            let snapshot = &table.typed_state.envelope.revision.snapshot;
            snapshot.row_count > 0 && !snapshot.schema.columns.is_empty()
        }) {
            contents.push(DataExportContent::TypedTable);
        }
        if dataset.peaks().is_some_and(|peaks| !peaks.marks.is_empty()) {
            contents.push(DataExportContent::Peaks);
        }
        if dataset
            .as_nmr()
            .is_some_and(|nmr| !nmr.integrals.is_empty())
            || dataset
                .as_nmr2d()
                .is_some_and(|nmr| nmr.is_true_2d() && !nmr.integrals.is_empty())
        {
            contents.push(DataExportContent::Integrals);
        }
        if dataset.as_table().is_some_and(|table| {
            table
                .series_bindings
                .iter()
                .any(|binding| binding.fit.is_some())
        }) {
            contents.push(DataExportContent::CurveFits);
        }
        if dataset.as_xps().is_some_and(|xps| {
            let region = xps.active_region();
            region.imported_fit.is_some() || xps.current_fit(region.id).is_some()
        }) {
            contents.push(DataExportContent::CurveFits);
        }
        Self {
            has_channel_choice: matches!(dataset, Dataset::Nmr(_) | Dataset::Nmr2D(_)),
            has_shape_choice: matches!(dataset, Dataset::Nmr2D(_)),
            default_channel: displayed_channel(dataset),
            contents,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.contents.is_empty()
    }
}

fn processed_data_available(dataset: &Dataset) -> bool {
    match dataset {
        Dataset::Nmr(nmr) => !nmr.processed.is_empty(),
        Dataset::Nmr2D(nmr) => match &nmr.processed {
            Processed2D::Ft(spectrum) => !spectrum.is_empty(),
            Processed2D::Stack(stack) => {
                !stack.ppm.is_empty() && stack.traces.iter().any(|trace| !trace.is_empty())
            }
        },
        Dataset::Electrophysiology(recording) => {
            recording.data.sample_rate_hz.is_finite()
                && recording.data.sample_rate_hz > 0.0
                && recording
                    .data
                    .channels
                    .get(recording.selected_channel)
                    .is_some()
                && recording.selected_sweep_indices().into_iter().any(|index| {
                    recording.data.sweeps.get(index).is_some_and(|sweep| {
                        sweep
                            .channels
                            .get(recording.selected_channel)
                            .is_some_and(|trace| !trace.is_empty())
                    })
                })
        }
        Dataset::Table(_) => false,
        Dataset::Afm(_) => false,
        // LC–MS export snapshots are not implemented yet. Do not advertise an
        // option whose capture path can only return `ContentUnavailable`.
        Dataset::MassSpec(_) => false,
        Dataset::Xrd(xrd) => !xrd.processed.intensity.is_empty(),
        Dataset::Xps(xps) => xps
            .displayed_region(xps.active_region)
            .is_some_and(|region| !region.intensity.is_empty()),
    }
}

fn displayed_channel(dataset: &Dataset) -> IntensityChannel {
    let has_magnitude = match dataset {
        Dataset::Nmr(nmr) => [&nmr.pipeline].into_iter().any(pipeline_has_magnitude),
        Dataset::Nmr2D(nmr) => [&nmr.params.f2, &nmr.params.f1]
            .into_iter()
            .any(pipeline_has_magnitude),
        _ => false,
    };
    if has_magnitude {
        IntensityChannel::Magnitude
    } else {
        IntensityChannel::Real
    }
}

fn pipeline_has_magnitude(pipeline: &plotx_processing::AxisPipeline) -> bool {
    pipeline
        .steps
        .iter()
        .any(|step| step.enabled && matches!(step.kind, StepKind::Magnitude))
}

#[derive(Debug, thiserror::Error)]
pub enum DataExportError {
    #[error("no exportable data is available for the current dataset")]
    Unavailable,
    #[error("the selected export content is not available for this dataset")]
    ContentUnavailable,
    #[error("the selected electrophysiology traces could not be processed: {0}")]
    Electrophysiology(#[from] ElectrophysiologyAnalysisError),
    #[error("the exported text could not be written: {0}")]
    Write(#[from] io::Error),
    #[error("the exported text is not valid UTF-8: {0}")]
    Utf8(#[from] std::string::FromUtf8Error),
    #[error("the XLSX workbook could not be written: {0}")]
    Xlsx(#[from] plotx_io::xlsx::XlsxIoError),
    #[error("the typed table could not be read for XLSX export: {0}")]
    Typed(#[from] plotx_data::DataError),
}

impl DataExportError {
    pub const fn category(&self) -> &'static str {
        match self {
            Self::Unavailable => "unavailable",
            Self::ContentUnavailable => "content_unavailable",
            Self::Electrophysiology(_) => "serialization",
            Self::Write(_) => "write",
            Self::Utf8(_) => "serialization",
            Self::Xlsx(_) => "xlsx_write",
            Self::Typed(_) => "typed_read",
        }
    }
}

#[derive(Clone)]
pub struct DataExportSnapshot {
    dataset_name: String,
    request: DataExportRequest,
    data: SnapshotData,
}

#[derive(Clone)]
enum SnapshotData {
    Nmr1D {
        axis: Vec<f64>,
        axis_label: &'static str,
        values: Vec<Complex64>,
    },
    True2D(Arc<Spectrum2D>),
    Pseudo2D {
        spectrum: Arc<StackSpectrum>,
        ruler_name: String,
        ruler_unit: String,
        ruler: Vec<f64>,
    },
    Table(Box<crate::state::TypedTableState>),
    Peaks(Vec<crate::state::ResolvedPeak>),
    Integrals1D(Vec<IntegralResult>),
    Integrals2D(Vec<Integral2D>),
    Fits(Vec<StoredCurveFitAnalysis>),
    Electrophysiology {
        sample_rate_hz: f64,
        channel_label: String,
        traces: Vec<(usize, Vec<f64>)>,
    },
    Xrd {
        two_theta_deg: Vec<f64>,
        intensity: Vec<f64>,
    },
    Xps(Box<XpsDataSnapshot>),
    XpsFits(Vec<XpsFitParameterRow>),
}

#[derive(Clone)]
struct XpsDataSnapshot {
    native_energy_ev: Vec<f64>,
    binding_energy_ev: Option<Vec<f64>>,
    raw_cps: Vec<f64>,
    processed_energy_ev: Vec<f64>,
    processed_cps: Vec<f64>,
    fit_energy_ev: Vec<f64>,
    background: Vec<f64>,
    background_subtracted: Vec<f64>,
    envelope: Vec<f64>,
    residual: Vec<f64>,
    components: Vec<(String, Vec<f64>)>,
    background_model: Option<String>,
    background_window_ev: Option<[f64; 2]>,
    low_anchor_ev: Option<[f64; 2]>,
    high_anchor_ev: Option<[f64; 2]>,
}

#[derive(Clone)]
struct XpsFitParameterRow {
    provenance: &'static str,
    label: String,
    center_ev: f64,
    fwhm_ev: f64,
    area: f64,
    fraction: Option<f64>,
    r_squared: Option<f64>,
    rmse: Option<f64>,
    residual_lag1: Option<f64>,
    hit_position_bound: Option<bool>,
    hit_fwhm_bound: Option<bool>,
    hit_area_bound: Option<bool>,
    center_standard_error: Option<f64>,
    center_confidence_95: Option<[f64; 2]>,
    fwhm_standard_error: Option<f64>,
    fwhm_confidence_95: Option<[f64; 2]>,
    area_standard_error: Option<f64>,
    area_confidence_95: Option<[f64; 2]>,
    maximum_correlation: Option<f64>,
    bootstrap_center: Option<[f64; 3]>,
    bootstrap_fwhm: Option<[f64; 3]>,
    bootstrap_area: Option<[f64; 3]>,
    bootstrap_fraction: Option<[f64; 3]>,
}

impl DataExportSnapshot {
    pub fn capture(dataset: &Dataset, request: DataExportRequest) -> Result<Self, DataExportError> {
        let availability = DataExportAvailability::for_dataset(dataset);
        if availability.is_empty() {
            return Err(DataExportError::Unavailable);
        }
        if !availability.contents.contains(&request.content) {
            return Err(DataExportError::ContentUnavailable);
        }
        let dataset_name = dataset.display_name();
        let data = match request.content {
            DataExportContent::ProcessedData => capture_processed(dataset)?,
            DataExportContent::TypedTable => {
                let table = dataset
                    .as_table()
                    .ok_or(DataExportError::ContentUnavailable)?;
                SnapshotData::Table(Box::new(table.typed_state.clone()))
            }
            DataExportContent::Peaks => SnapshotData::Peaks(
                dataset
                    .peaks()
                    .ok_or(DataExportError::ContentUnavailable)?
                    .resolve(),
            ),
            DataExportContent::Integrals => {
                if let Some(nmr) = dataset.as_nmr() {
                    SnapshotData::Integrals1D(nmr.integrals.clone())
                } else if let Some(nmr) = dataset.as_nmr2d().filter(|nmr| nmr.is_true_2d()) {
                    SnapshotData::Integrals2D(nmr.integrals.clone())
                } else {
                    return Err(DataExportError::ContentUnavailable);
                }
            }
            DataExportContent::CurveFits => {
                if let Some(xps) = dataset.as_xps() {
                    SnapshotData::XpsFits(capture_xps_fits(xps)?)
                } else {
                    SnapshotData::Fits(
                        dataset
                            .as_table()
                            .ok_or(DataExportError::ContentUnavailable)?
                            .curve_fit_analyses
                            .clone(),
                    )
                }
            }
        };
        Ok(Self {
            dataset_name,
            request,
            data,
        })
    }

    pub fn default_file_name(&self, extension: &str) -> String {
        let mut parts = vec![
            safe_name(&self.dataset_name),
            self.request.content.slug().into(),
        ];
        if matches!(
            self.data,
            SnapshotData::Nmr1D { .. } | SnapshotData::True2D(_) | SnapshotData::Pseudo2D { .. }
        ) {
            parts.push(self.request.channel.slug().into());
        }
        if matches!(
            self.data,
            SnapshotData::True2D(_) | SnapshotData::Pseudo2D { .. }
        ) {
            parts.push(self.request.shape.slug().into());
        }
        format!("{}.{}", parts.join("-"), extension.trim_start_matches('.'))
    }

    pub fn write_to<W: Write>(
        &self,
        output: W,
        delimiter: Delimiter,
    ) -> Result<(), DataExportError> {
        let mut writer = DelimitedWriter::new(output, delimiter);
        match &self.data {
            SnapshotData::Nmr1D {
                axis,
                axis_label,
                values,
            } => write_1d(&mut writer, axis, axis_label, values, self.request.channel)?,
            SnapshotData::True2D(spectrum) => write_true_2d(&mut writer, spectrum, self.request)?,
            SnapshotData::Pseudo2D {
                spectrum,
                ruler_name,
                ruler_unit,
                ruler,
            } => write_pseudo_2d(
                &mut writer,
                spectrum,
                ruler_name,
                ruler_unit,
                ruler,
                self.request,
            )?,
            SnapshotData::Table(typed) => xlsx::write_typed_delimited(&mut writer, typed)?,
            SnapshotData::Peaks(values) => write_peaks(&mut writer, &self.dataset_name, values)?,
            SnapshotData::Integrals1D(values) => write_integrals_1d(&mut writer, values)?,
            SnapshotData::Integrals2D(values) => write_integrals_2d(&mut writer, values)?,
            SnapshotData::Fits(table) => write_fits(&mut writer, table)?,
            SnapshotData::Electrophysiology {
                sample_rate_hz,
                channel_label,
                traces,
            } => write_electrophysiology(&mut writer, *sample_rate_hz, channel_label, traces)?,
            SnapshotData::Xrd {
                two_theta_deg,
                intensity,
            } => write_xrd(&mut writer, two_theta_deg, intensity)?,
            SnapshotData::Xps(data) => write_xps(&mut writer, data)?,
            SnapshotData::XpsFits(rows) => write_xps_fits(&mut writer, rows)?,
        }
        Ok(())
    }

    pub fn to_text(&self, delimiter: Delimiter) -> Result<String, DataExportError> {
        let mut bytes = Vec::new();
        self.write_to(&mut bytes, delimiter)?;
        Ok(String::from_utf8(bytes)?)
    }
}

fn capture_processed(dataset: &Dataset) -> Result<SnapshotData, DataExportError> {
    match dataset {
        Dataset::Nmr(nmr) => {
            let (axis, axis_label) = match &nmr.processed {
                plotx_processing::Processed1D::Time(trace) => (trace.time_s.clone(), "time_s"),
                plotx_processing::Processed1D::Frequency(spectrum) => (spectrum.ppm.clone(), "ppm"),
            };
            Ok(SnapshotData::Nmr1D {
                axis,
                axis_label,
                values: nmr.processed.values().to_vec(),
            })
        }
        Dataset::Nmr2D(nmr) => match &nmr.processed {
            Processed2D::Ft(spectrum) => Ok(SnapshotData::True2D(Arc::clone(spectrum))),
            Processed2D::Stack(spectrum) => {
                let axis = nmr.data.pseudo_axis.as_ref();
                Ok(SnapshotData::Pseudo2D {
                    spectrum: Arc::clone(spectrum),
                    ruler_name: axis
                        .map(|axis| axis.name.clone())
                        .filter(|name| !name.is_empty())
                        .unwrap_or_else(|| "Ruler".into()),
                    ruler_unit: axis.map(|axis| axis.unit.clone()).unwrap_or_default(),
                    ruler: axis
                        .map(|axis| axis.values.clone())
                        .unwrap_or_else(|| (0..spectrum.increments()).map(|i| i as f64).collect()),
                })
            }
        },
        Dataset::Electrophysiology(recording) => {
            let channel = recording
                .data
                .channels
                .get(recording.selected_channel)
                .ok_or(DataExportError::ContentUnavailable)?;
            let channel_label = if channel.unit.symbol.is_empty() {
                channel.name.clone()
            } else {
                format!("{} ({})", channel.name, channel.unit.symbol)
            };
            let mut traces = Vec::new();
            for index in recording.selected_sweep_indices() {
                traces.push((
                    index + 1,
                    recording.processed_trace(index, recording.selected_channel)?,
                ));
            }
            Ok(SnapshotData::Electrophysiology {
                sample_rate_hz: recording.data.sample_rate_hz,
                channel_label,
                traces,
            })
        }
        Dataset::Table(_) => Err(DataExportError::ContentUnavailable),
        Dataset::Afm(_) => Err(DataExportError::ContentUnavailable),
        Dataset::MassSpec(_) => Err(DataExportError::ContentUnavailable),
        Dataset::Xrd(xrd) => Ok(SnapshotData::Xrd {
            two_theta_deg: xrd.data.two_theta_deg.clone(),
            intensity: xrd.processed.intensity.clone(),
        }),
        Dataset::Xps(xps) => {
            let region = xps.active_region();
            let processed = xps
                .displayed_region(region.id)
                .ok_or(DataExportError::ContentUnavailable)?;
            let current = xps.current_fit(region.id);
            let imported = current
                .is_none()
                .then(|| xps.imported_fit_for_processed_region(region.id))
                .flatten();
            let (fit_energy_ev, background, corrected, envelope, residual, components) =
                if let Some(fit) = current {
                    (
                        fit.result.energy_ev.clone(),
                        fit.result.background.clone(),
                        fit.result
                            .intensity
                            .iter()
                            .zip(&fit.result.background)
                            .map(|(y, bg)| y - bg)
                            .collect(),
                        fit.result.envelope.clone(),
                        fit.result.residual.clone(),
                        fit.result
                            .components
                            .iter()
                            .enumerate()
                            .map(|(index, values)| {
                                (
                                    fit.result.peaks.get(index).map_or_else(
                                        || format!("component_{}", index + 1),
                                        |peak| peak.label.clone(),
                                    ),
                                    values.clone(),
                                )
                            })
                            .collect(),
                    )
                } else if let Some(fit) = imported {
                    let shift = xps.energy_shift(region.measurement).unwrap_or(0.0);
                    (
                        region
                            .binding_energy_ev
                            .as_ref()
                            .map(|energy| energy.iter().map(|value| value + shift).collect())
                            .unwrap_or_default(),
                        fit.background_cps.clone(),
                        region
                            .intensity_cps
                            .iter()
                            .zip(&fit.background_cps)
                            .map(|(y, bg)| y - bg)
                            .collect(),
                        fit.envelope_cps.clone(),
                        region
                            .intensity_cps
                            .iter()
                            .zip(&fit.envelope_cps)
                            .map(|(observed, predicted)| observed - predicted)
                            .collect(),
                        fit.components_cps
                            .iter()
                            .enumerate()
                            .map(|(index, values)| {
                                (
                                    fit.peaks.get(index).map_or_else(
                                        || format!("imported_component_{}", index + 1),
                                        |peak| peak.label.clone(),
                                    ),
                                    values.clone(),
                                )
                            })
                            .collect(),
                    )
                } else {
                    let preview = xps.fit_workspaces.get(&region.id).and_then(|workspace| {
                        plotx_analysis::xps::compute_xps_background(
                            &processed.binding_energy_ev,
                            &processed.intensity,
                            &workspace.invocation.background,
                        )
                        .ok()
                    });
                    preview.map_or_else(
                        || {
                            (
                                Vec::new(),
                                Vec::new(),
                                Vec::new(),
                                Vec::new(),
                                Vec::new(),
                                Vec::new(),
                            )
                        },
                        |preview| {
                            (
                                preview.energy_ev,
                                preview.background,
                                preview.corrected,
                                Vec::new(),
                                Vec::new(),
                                Vec::new(),
                            )
                        },
                    )
                };
            let background_spec = current.map(|fit| &fit.invocation.background).or_else(|| {
                imported
                    .is_none()
                    .then(|| {
                        xps.fit_workspaces
                            .get(&region.id)
                            .map(|workspace| &workspace.invocation.background)
                    })
                    .flatten()
            });
            let background_model = if imported.is_some() {
                Some("Imported (CasaXPS)".to_owned())
            } else {
                background_spec.map(|spec| background_model_label(&spec.model))
            };
            Ok(SnapshotData::Xps(Box::new(XpsDataSnapshot {
                native_energy_ev: region.native_energy_ev.clone(),
                binding_energy_ev: region.binding_energy_ev.clone(),
                raw_cps: region.intensity_cps.clone(),
                processed_energy_ev: processed.binding_energy_ev,
                processed_cps: processed.intensity,
                fit_energy_ev,
                background,
                background_subtracted: corrected,
                envelope,
                residual,
                components,
                background_model,
                background_window_ev: background_spec.map(|spec| spec.window_ev),
                low_anchor_ev: background_spec.map(|spec| spec.low_anchor_ev),
                high_anchor_ev: background_spec.map(|spec| spec.high_anchor_ev),
            })))
        }
    }
}

fn capture_xps_fits(
    xps: &crate::state::XpsDataset,
) -> Result<Vec<XpsFitParameterRow>, DataExportError> {
    let region = xps.active_region();
    if let Some(fit) = xps.current_fit(region.id) {
        let maximum_correlation = fit
            .result
            .parameter_correlation
            .as_ref()
            .and_then(|matrix| {
                matrix
                    .iter()
                    .enumerate()
                    .flat_map(|(row, values)| {
                        values
                            .iter()
                            .enumerate()
                            .filter(move |(column, _)| *column != row)
                            .map(|(_, value)| value.abs())
                    })
                    .reduce(f64::max)
            });
        return Ok(fit
            .result
            .peaks
            .iter()
            .map(|peak| {
                let bootstrap = fit.bootstrap.as_ref().and_then(|result| {
                    result
                        .peaks
                        .iter()
                        .find(|candidate| candidate.id == peak.id)
                });
                XpsFitParameterRow {
                    provenance: "PlotX",
                    label: peak.label.clone(),
                    center_ev: peak.center_ev.value,
                    fwhm_ev: peak.fwhm_ev.value,
                    area: peak.area.value,
                    fraction: Some(peak.fraction.value),
                    r_squared: Some(fit.result.r_squared),
                    rmse: Some(fit.result.rmse),
                    residual_lag1: fit.result.residual_lag1,
                    hit_position_bound: Some(peak.hit_position_bound),
                    hit_fwhm_bound: Some(peak.hit_fwhm_bound),
                    hit_area_bound: Some(peak.hit_area_bound),
                    center_standard_error: peak.center_ev.standard_error,
                    center_confidence_95: peak.center_ev.confidence_95,
                    fwhm_standard_error: peak.fwhm_ev.standard_error,
                    fwhm_confidence_95: peak.fwhm_ev.confidence_95,
                    area_standard_error: peak.area.standard_error,
                    area_confidence_95: peak.area.confidence_95,
                    maximum_correlation,
                    bootstrap_center: bootstrap.map(|value| value.center_ev),
                    bootstrap_fwhm: bootstrap.map(|value| value.fwhm_ev),
                    bootstrap_area: bootstrap.map(|value| value.area),
                    bootstrap_fraction: bootstrap.map(|value| value.fraction),
                }
            })
            .collect());
    }
    let imported = region
        .imported_fit
        .as_ref()
        .ok_or(DataExportError::ContentUnavailable)?;
    let total = imported.peaks.iter().map(|peak| peak.area).sum::<f64>();
    Ok(imported
        .peaks
        .iter()
        .map(|peak| XpsFitParameterRow {
            provenance: "Imported (CasaXPS)",
            label: peak.label.clone(),
            center_ev: peak.position_ev,
            fwhm_ev: peak.fwhm_ev,
            area: peak.area,
            fraction: (total > 0.0).then_some(peak.area / total),
            r_squared: None,
            rmse: None,
            residual_lag1: None,
            hit_position_bound: None,
            hit_fwhm_bound: None,
            hit_area_bound: None,
            center_standard_error: None,
            center_confidence_95: None,
            fwhm_standard_error: None,
            fwhm_confidence_95: None,
            area_standard_error: None,
            area_confidence_95: None,
            maximum_correlation: None,
            bootstrap_center: None,
            bootstrap_fwhm: None,
            bootstrap_area: None,
            bootstrap_fraction: None,
        })
        .collect())
}

fn background_model_label(model: &plotx_analysis::xps::XpsBackgroundModel) -> String {
    match model {
        plotx_analysis::xps::XpsBackgroundModel::Linear => "Linear".into(),
        plotx_analysis::xps::XpsBackgroundModel::Shirley { .. } => "Shirley".into(),
        plotx_analysis::xps::XpsBackgroundModel::TougaardU2 { b_ev2, c_ev2 } => {
            format!("Tougaard U2 (B={b_ev2}, C={c_ev2})")
        }
    }
}

#[cfg(test)]
mod tests;
