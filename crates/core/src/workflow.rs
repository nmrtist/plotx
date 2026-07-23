//! UI-independent loading, processing and export workflows.
//!
//! This module owns the canonical path from a detected acquisition to a
//! [`Dataset`], and the default figure/canvas construction used by both the GUI
//! and automation. It intentionally contains no session, status or undo state.

use crate::actions::ProcessingStateError;
use crate::export::{
    DEFAULT_BITMAP_DPI, ExportError, ExportFormat, ExportPageScope, ExportSettings, export_canvases,
};
use crate::state::{
    AxisOverrides, AxisProjections, CanvasDocument, CanvasObject, CanvasObjectKind, CanvasViewport,
    ChartSpec, DEFAULT_CANVAS_SIZE_MM, DataBinding, Dataset, MM_TO_PT, Nmr2DDataset, NmrDataset,
    ObjectFrame, ObjectId, PanelMeta, PlotObject, StackSpec, default_chart_type,
};
use plotx_figure::{Axis, Figure};
use plotx_io::{Acquisition, DataFormat, Domain, LoadWarning, LoadWarningCode, Provenance};
use serde::Serialize;
use std::path::{Path, PathBuf};

pub const INSPECTION_SCHEMA: &str = "plotx.inspect.v1";

#[derive(Clone, Debug, Serialize)]
pub struct InspectionReport {
    pub schema: &'static str,
    pub format: String,
    pub provenance: ProvenanceReport,
    pub dimension: DimensionReport,
    pub domain: String,
    pub warnings: Vec<WarningReport>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub electrophysiology: Option<ElectrophysiologyReport>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub afm: Option<AfmReport>,
}

#[derive(Clone, Debug, Serialize)]
pub struct AfmReport {
    pub channels: Vec<String>,
    pub grid: Option<[usize; 2]>,
    pub curve_count: usize,
    pub samples_per_curve: Option<usize>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ElectrophysiologyReport {
    pub abf_version: String,
    pub channels: Vec<String>,
    pub units: Vec<String>,
    pub sample_rate_hz: f64,
    pub sweep_count: usize,
    pub protocol: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ProvenanceReport {
    pub selected_path: PathBuf,
    pub data_path: PathBuf,
    pub parameter_paths: Vec<PathBuf>,
    pub companion_paths: Vec<PathBuf>,
}

#[derive(Clone, Debug, Serialize)]
pub struct DimensionReport {
    pub count: usize,
    /// Canonical storage order: `[points]` for 1D and `[indirect, direct]` for 2D.
    pub shape: Vec<usize>,
}

#[derive(Clone, Debug, Serialize)]
pub struct WarningReport {
    pub code: &'static str,
    pub message: String,
    pub path: Option<PathBuf>,
}

#[derive(Clone)]
pub struct LoadedDataset {
    pub dataset: Dataset,
    pub inspection: InspectionReport,
    pub source: String,
}

impl LoadedDataset {
    pub fn apply_scheme(
        &mut self,
        scheme: &crate::project::ProcessingScheme,
    ) -> Result<(), WorkflowError> {
        let state = crate::project::apply_scheme(scheme, &self.dataset)?;
        state.apply_to(&mut self.dataset)?;
        if let Some(dataset) = self.dataset.as_nmr2d_mut() {
            dataset.recompute_integrals()?;
        }
        Ok(())
    }

    pub fn apply_scheme_file(&mut self, path: &Path) -> Result<(), WorkflowError> {
        let scheme = crate::project::load_scheme(path)?;
        self.apply_scheme(&scheme)
    }

    pub fn default_canvas(&self) -> CanvasDocument {
        build_default_canvas(&self.dataset, &self.source)
    }
}

#[derive(Clone, Debug)]
pub struct ProcessResult {
    pub inspection: InspectionReport,
    pub output_paths: Vec<PathBuf>,
}

#[derive(Debug, thiserror::Error)]
pub enum WorkflowError {
    #[error("input load failed: {0}")]
    Load(#[from] plotx_io::IoError),
    #[error("processing scheme failed: {0}")]
    Scheme(#[from] crate::project::ProjectError),
    #[error("processing state failed: {0}")]
    Processing(#[from] ProcessingStateError),
    #[error("2D integral recompute failed: {0}")]
    Integration(#[from] plotx_analysis::integrate_2d::IntegrateError),
    #[error("default figure is unavailable for {0}")]
    FigureUnavailable(&'static str),
    #[error("export failed: {0}")]
    Export(#[from] ExportError),
}

pub fn load_dataset(path: &Path) -> Result<LoadedDataset, WorkflowError> {
    let loaded = plotx_io::load_path(path)?;
    let inspection = inspection_report(
        loaded.format,
        &loaded.provenance,
        &loaded.warnings,
        &loaded.acquisition,
    );
    let (dataset, source) = dataset_from_acquisition(loaded.acquisition);
    Ok(LoadedDataset {
        dataset,
        inspection,
        source,
    })
}

pub fn process_file(
    input: &Path,
    scheme: &Path,
    output: &Path,
    format: ExportFormat,
) -> Result<ProcessResult, WorkflowError> {
    let mut loaded = load_dataset(input)?;
    loaded.apply_scheme_file(scheme)?;
    let canvas = loaded.default_canvas();
    let settings = ExportSettings {
        format,
        scope: ExportPageScope::Current,
        dpi: DEFAULT_BITMAP_DPI,
        target_width_mm: None,
        trim_to_visible_content: false,
    };
    let output_paths = export_canvases(&[canvas], Some(0), &settings, output)?;
    Ok(ProcessResult {
        inspection: loaded.inspection,
        output_paths,
    })
}

/// The only acquisition-to-dataset conversion path. Loading frontends retain
/// provenance separately and hand the neutral acquisition to this function.
pub fn dataset_from_acquisition(acquisition: Acquisition) -> (Dataset, String) {
    match acquisition {
        Acquisition::D1(data) => {
            let source = data.source.clone();
            (Dataset::Nmr(Box::new(NmrDataset::load(data))), source)
        }
        Acquisition::D2(data) => {
            let source = data.source.clone();
            (Dataset::Nmr2D(Box::new(Nmr2DDataset::load(*data))), source)
        }
        Acquisition::Electrophysiology(data) => {
            let source = data.source.clone();
            (
                Dataset::Electrophysiology(Box::new(crate::state::ElectrophysiologyDataset::load(
                    *data,
                ))),
                source,
            )
        }
        Acquisition::Afm(data) => {
            let source = data.source.clone();
            (
                Dataset::Afm(Box::new(crate::state::AfmDataset::load(*data))),
                source,
            )
        }
    }
}

pub fn dataset_title(dataset: &Dataset) -> String {
    match dataset {
        Dataset::Nmr(nmr) => nmr
            .name
            .clone()
            .unwrap_or_else(|| short_name(&nmr.data.source)),
        Dataset::Nmr2D(nmr) => nmr
            .name
            .clone()
            .unwrap_or_else(|| short_name(&nmr.data.source)),
        Dataset::Table(table) => table.name.clone().unwrap_or_else(|| table.summary()),
        Dataset::Electrophysiology(data) => data
            .name
            .clone()
            .unwrap_or_else(|| short_name(&data.data.source)),
        Dataset::Afm(data) => data
            .name
            .clone()
            .unwrap_or_else(|| short_name(&data.data.source)),
    }
}

pub fn build_dataset_figure(dataset: &Dataset, chart: &ChartSpec, size_mm: [f32; 2]) -> Figure {
    let domain = dataset.domain();
    let context = chart.context(dataset);
    let selected = crate::state::resolved_chart_type(domain, &chart.type_id);
    let mut figure = (selected.build)(dataset, &context)
        .or_else(|| (default_chart_type(domain).build)(dataset, &context))
        .unwrap_or_else(|| Figure::new("", Axis::new("x", 0.0, 1.0), Axis::new("y", 0.0, 1.0)));
    figure.title.clear();
    figure.width = size_mm[0] * MM_TO_PT;
    figure.height = size_mm[1] * MM_TO_PT;
    if let Some(nmr) = dataset.as_nmr() {
        figure.integral_curves = nmr.integral_curves();
    }
    figure
}

pub fn build_plot_object(
    dataset: &Dataset,
    dataset_index: usize,
    frame: ObjectFrame,
    id: ObjectId,
    name: String,
) -> CanvasObject {
    let size_mm = [frame.width / MM_TO_PT, frame.height / MM_TO_PT];
    let mut chart = ChartSpec::default_for(dataset.domain());
    if matches!(dataset, Dataset::Afm(afm) if afm.data.images.is_empty() && afm.data.forces.is_some())
    {
        chart.type_id = "afm_force_curve".to_owned();
    }
    let figure = build_dataset_figure(dataset, &chart, size_mm);
    let viewport = CanvasViewport::from_figure(&figure);
    let panel = PanelMeta::new(dataset_title(dataset), frame.width);
    CanvasObject {
        id,
        name,
        frame,
        locked: false,
        visible: true,
        group: None,
        kind: CanvasObjectKind::Plot(Box::new(PlotObject {
            binding: DataBinding::single(dataset_index),
            chart,
            stack: StackSpec::default(),
            projections: AxisProjections::default(),
            axis_overrides: AxisOverrides::default(),
            figure,
            viewport,
            panel,
        })),
    }
}

pub fn build_default_canvas(dataset: &Dataset, source: &str) -> CanvasDocument {
    let title = short_name(source);
    build_default_canvas_for_dataset(
        dataset,
        0,
        format!("Canvas 1 - {title}"),
        DEFAULT_CANVAS_SIZE_MM,
    )
}

/// Build the canonical initial layout for one dataset. GUI insertion, CLI,
/// automation, and export use this same layout policy; callers supply only the
/// document-local dataset index and canvas identity.
pub fn build_default_canvas_for_dataset(
    dataset: &Dataset,
    dataset_index: usize,
    canvas_name: String,
    size_mm: [f32; 2],
) -> CanvasDocument {
    let has_map_and_force = matches!(dataset, Dataset::Afm(afm) if !afm.data.images.is_empty() && afm.data.forces.is_some());
    let size_mm = if has_map_and_force && size_mm == DEFAULT_CANVAS_SIZE_MM {
        [crate::state::NATURE_DOUBLE_COLUMN.width_mm, size_mm[1]]
    } else {
        size_mm
    };
    let mut canvas = CanvasDocument::new(canvas_name, size_mm);
    if has_map_and_force && size_mm[0] == crate::state::NATURE_DOUBLE_COLUMN.width_mm {
        canvas.size_preset_id = Some(crate::state::NATURE_DOUBLE_COLUMN.id.to_owned());
    }
    let [width, height] = canvas.size_pt();
    let id = canvas.allocate_object_id();
    let first_width = if has_map_and_force {
        width / 2.0
    } else {
        width
    };
    let first = build_plot_object(
        dataset,
        dataset_index,
        ObjectFrame::new(0.0, 0.0, first_width, height),
        id,
        "Plot 1".to_owned(),
    );
    canvas.objects.push(first);
    if has_map_and_force {
        let second_id = canvas.allocate_object_id();
        let mut second = build_plot_object(
            dataset,
            dataset_index,
            ObjectFrame::new(width / 2.0, 0.0, width / 2.0, height),
            second_id,
            "Force Curve".to_owned(),
        );
        if let CanvasObjectKind::Plot(plot) = &mut second.kind {
            plot.chart.type_id = "afm_force_curve".to_owned();
            plot.figure = build_dataset_figure(
                dataset,
                &plot.chart,
                [width / 2.0 / MM_TO_PT, height / MM_TO_PT],
            );
            plot.viewport = CanvasViewport::from_figure(&plot.figure);
        }
        canvas.objects.push(second);
    }
    canvas
}

fn inspection_report(
    format: DataFormat,
    provenance: &Provenance,
    warnings: &[LoadWarning],
    acquisition: &Acquisition,
) -> InspectionReport {
    let (count, shape, domain) = match acquisition {
        Acquisition::D1(data) => (1, vec![data.len()], data.domain),
        Acquisition::D2(data) => (2, vec![data.rows, data.cols], data.domain),
        Acquisition::Electrophysiology(data) => {
            let max_points = data
                .sweeps
                .iter()
                .filter_map(|s| s.channels.first())
                .map(Vec::len)
                .max()
                .unwrap_or(0);
            return InspectionReport {
                schema: INSPECTION_SCHEMA,
                format: format.as_str().to_owned(),
                provenance: ProvenanceReport {
                    selected_path: provenance.selected_path.clone(),
                    data_path: provenance.data_path.clone(),
                    parameter_paths: provenance.parameter_paths.clone(),
                    companion_paths: provenance.companion_paths.clone(),
                },
                dimension: DimensionReport {
                    count: 3,
                    shape: vec![data.sweeps.len(), data.channels.len(), max_points],
                },
                domain: "electrophysiology".to_owned(),
                warnings: warnings.iter().map(warning_report).collect(),
                electrophysiology: Some(ElectrophysiologyReport {
                    abf_version: data.abf_version.clone(),
                    channels: data
                        .channels
                        .iter()
                        .map(|channel| channel.name.clone())
                        .collect(),
                    units: data
                        .channels
                        .iter()
                        .map(|channel| channel.unit.symbol.clone())
                        .collect(),
                    sample_rate_hz: data.sample_rate_hz,
                    sweep_count: data.sweeps.len(),
                    protocol: data.protocol.clone(),
                }),
                afm: None,
            };
        }
        Acquisition::Afm(data) => {
            let force = data.forces.as_ref();
            let shape = force.map_or_else(
                || {
                    data.images
                        .first()
                        .map_or_else(Vec::new, |image| vec![image.height, image.width])
                },
                |force| vec![force.grid_height, force.grid_width, force.samples_per_curve],
            );
            return InspectionReport {
                schema: INSPECTION_SCHEMA,
                format: format.as_str().to_owned(),
                provenance: ProvenanceReport {
                    selected_path: provenance.selected_path.clone(),
                    data_path: provenance.data_path.clone(),
                    parameter_paths: provenance.parameter_paths.clone(),
                    companion_paths: provenance.companion_paths.clone(),
                },
                dimension: DimensionReport {
                    count: shape.len(),
                    shape,
                },
                domain: "afm".to_owned(),
                warnings: warnings.iter().map(warning_report).collect(),
                electrophysiology: None,
                afm: Some(AfmReport {
                    channels: data.images.iter().map(|image| image.name.clone()).collect(),
                    grid: force.map(|force| [force.grid_width, force.grid_height]),
                    curve_count: force.map_or(0, |force| {
                        force.grid_width.saturating_mul(force.grid_height)
                    }),
                    samples_per_curve: force.map(|force| force.samples_per_curve),
                }),
            };
        }
    };
    InspectionReport {
        schema: INSPECTION_SCHEMA,
        format: format.as_str().to_owned(),
        provenance: ProvenanceReport {
            selected_path: provenance.selected_path.clone(),
            data_path: provenance.data_path.clone(),
            parameter_paths: provenance.parameter_paths.clone(),
            companion_paths: provenance.companion_paths.clone(),
        },
        dimension: DimensionReport { count, shape },
        domain: domain_label(domain).to_owned(),
        warnings: warnings.iter().map(warning_report).collect(),
        electrophysiology: None,
        afm: None,
    }
}

fn warning_report(warning: &LoadWarning) -> WarningReport {
    let code = match warning.code {
        LoadWarningCode::ArchiveEntryFailed => "archive-entry-failed",
        LoadWarningCode::OptionalImaginaryMissing => "optional-imaginary-missing",
        LoadWarningCode::MissingStimulus => "missing-stimulus",
        LoadWarningCode::InvalidMetadata => "invalid-metadata",
        LoadWarningCode::MissingCalibration => "missing-calibration",
        LoadWarningCode::MissingCompanion => "missing-companion",
        LoadWarningCode::CompanionMismatch => "companion-mismatch",
        LoadWarningCode::OptionalChannelSkipped => "optional-channel-skipped",
    };
    WarningReport {
        code,
        message: warning.message.clone(),
        path: warning.path.clone(),
    }
}

fn domain_label(domain: Domain) -> &'static str {
    match domain {
        Domain::Time => "time",
        Domain::Frequency => "frequency",
    }
}

fn short_name(source: &str) -> String {
    Path::new(source)
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| source.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use num_complex::Complex64;

    fn acquisition() -> Acquisition {
        Acquisition::D1(plotx_io::NmrData {
            points: vec![Complex64::new(1.0, 0.0); 8],
            domain: Domain::Frequency,
            spectral_width_hz: 4_000.0,
            observe_freq_mhz: 400.0,
            carrier_ppm: 4.7,
            nucleus: "1H".to_owned(),
            source: "sample.dx".to_owned(),
            group_delay: 0.0,
        })
    }

    #[test]
    fn canonical_conversion_and_default_canvas_share_dataset_identity() {
        let (dataset, source) = dataset_from_acquisition(acquisition());
        assert_eq!(dataset.kind_label(), "NMR 1D");
        let canvas = build_default_canvas(&dataset, &source);
        assert_eq!(canvas.dataset_indices(), vec![0]);
        assert_eq!(canvas.objects.len(), 1);
    }

    #[test]
    fn inspection_contract_reports_canonical_shape_and_domain() {
        let report = inspection_report(
            DataFormat::JcampDx1D,
            &Provenance {
                selected_path: "sample.dx".into(),
                data_path: "sample.dx".into(),
                parameter_paths: Vec::new(),
                companion_paths: Vec::new(),
            },
            &[],
            &acquisition(),
        );
        assert_eq!(report.schema, INSPECTION_SCHEMA);
        assert_eq!(report.dimension.count, 1);
        assert_eq!(report.dimension.shape, vec![8]);
        assert_eq!(report.domain, "frequency");
    }
}
