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
    write_integrals_2d, write_peaks, write_pseudo_2d, write_true_2d,
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
        Dataset::Nmr(nmr) => !nmr.spectrum.is_empty(),
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
                && recording
                    .selected_sweeps
                    .iter()
                    .enumerate()
                    .any(|(index, selected)| {
                        *selected
                            && recording.data.sweeps.get(index).is_some_and(|sweep| {
                                sweep
                                    .channels
                                    .get(recording.selected_channel)
                                    .is_some_and(|trace| !trace.is_empty())
                            })
                    })
        }
        Dataset::Table(_) => false,
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
        ppm: Vec<f64>,
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
            DataExportContent::CurveFits => SnapshotData::Fits(
                dataset
                    .as_table()
                    .ok_or(DataExportError::ContentUnavailable)?
                    .curve_fit_analyses
                    .clone(),
            ),
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
            SnapshotData::Nmr1D { ppm, values } => {
                write_1d(&mut writer, ppm, values, self.request.channel)?
            }
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
        Dataset::Nmr(nmr) => Ok(SnapshotData::Nmr1D {
            ppm: nmr.spectrum.ppm.clone(),
            values: nmr.spectrum.values.clone(),
        }),
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
            for (index, selected) in recording.selected_sweeps.iter().copied().enumerate() {
                if selected {
                    traces.push((
                        index + 1,
                        recording.processed_trace(index, recording.selected_channel)?,
                    ));
                }
            }
            Ok(SnapshotData::Electrophysiology {
                sample_rate_hz: recording.data.sample_rate_hz,
                channel_label,
                traces,
            })
        }
        Dataset::Table(_) => Err(DataExportError::ContentUnavailable),
    }
}

#[cfg(test)]
mod tests;
