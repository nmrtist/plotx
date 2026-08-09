//! UI-independent loading, export, and default-layout workflows shared by all frontends.
use crate::actions::ProcessingStateError;
use crate::export::{
    DEFAULT_BITMAP_DPI, ExportError, ExportFormat, ExportPageScope, ExportSettings, export_canvases,
};
use crate::state::{
    AxisOverrides, AxisProjections, CanvasDocument, CanvasObject, CanvasObjectKind, CanvasViewport,
    ChartSpec, DEFAULT_CANVAS_SIZE_MM, DataBinding, Dataset, MM_TO_PT, Nmr2DDataset, NmrDataset,
    ObjectFrame, ObjectId, PanelMeta, PlotObject, PlotxApp, StackMode, StackSpec,
    default_chart_type,
};
use plotx_figure::{Axis, Figure};
use plotx_io::{Acquisition, DataFormat, Domain, LoadWarning, LoadWarningCode, Provenance};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::time::Duration;
#[path = "workflow/mass_spec_layout.rs"]
mod mass_spec_layout;
#[path = "workflow/trace_collection.rs"]
mod trace_collection;
#[path = "workflow/xps.rs"]
mod xps;
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mass_spectrometry: Option<MassSpecReport>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub xrd: Option<XrdReport>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub xps: Option<XpsReport>,
}

#[derive(Clone, Debug, Serialize)]
pub struct XrdReport {
    pub instrument: Option<String>,
    pub target: Option<String>,
    pub wavelength_angstrom: Option<f64>,
    pub two_theta_range_deg: [f64; 2],
    pub point_count: usize,
}

#[derive(Clone, Debug, Serialize)]
pub struct XpsReport {
    pub measurement_count: usize,
    pub region_count: usize,
    pub point_count: usize,
    pub binding_energy_region_count: usize,
    pub kinetic_only_region_count: usize,
    pub regions: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct MassSpecReport {
    pub instrument: Option<String>,
    pub stream_count: usize,
    pub ms_scan_count: usize,
    pub chromatograms: Vec<String>,
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
    #[error("field runtime setup failed: {0}")]
    FieldRuntime(String),
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
    // Headless exports use the same worker-only contour path as the desktop
    // app. This waits for queued jobs rather than reintroducing a synchronous
    // marching-squares shortcut in the export caller.
    let mut app = PlotxApp::new_with_settings(crate::settings::Settings::default());
    app.session
        .compute
        .register_loaded_dataset_fields(&loaded.dataset)
        .map_err(|error| {
            WorkflowError::FieldRuntime(match error {
                crate::state::FieldEnqueueError::WorkersUnavailable => {
                    "background workers are unavailable".to_owned()
                }
                crate::state::FieldEnqueueError::VersionExhausted => {
                    "FieldVersion allocator is exhausted".to_owned()
                }
            })
        })?;
    app.doc.datasets.push(loaded.dataset);
    let canvas = build_default_canvas(&app.doc.datasets[0], &loaded.source);
    app.doc.canvases.push(canvas);
    app.rebuild_canvases_for(0);
    while app.compute_busy() {
        app.poll_compute();
        std::thread::sleep(Duration::from_millis(5));
    }
    let settings = ExportSettings {
        format,
        scope: ExportPageScope::Current,
        dpi: DEFAULT_BITMAP_DPI,
        target_width_mm: None,
        trim_to_visible_content: false,
    };
    let output_paths = export_canvases(&app.doc.canvases, Some(0), &settings, output)?;
    Ok(ProcessResult {
        inspection: loaded.inspection,
        output_paths,
    })
}

/// The only acquisition-to-dataset conversion path. Loading frontends retain
/// provenance separately and hand the neutral acquisition to this function.
pub fn dataset_from_acquisition(acquisition: Acquisition) -> (Dataset, String) {
    dataset_from_acquisition_with_equal_scale_preference(acquisition, true)
}

pub fn dataset_from_acquisition_with_equal_scale_preference(
    acquisition: Acquisition,
    equal_scale_homonuclear_2d_imports: bool,
) -> (Dataset, String) {
    match acquisition {
        Acquisition::D1(data) => {
            let source = data.source.clone();
            (Dataset::Nmr(Box::new(NmrDataset::load(data))), source)
        }
        Acquisition::D2(data) => {
            let source = data.source.clone();
            (
                Dataset::Nmr2D(Box::new(Nmr2DDataset::load_with_equal_scale_preference(
                    *data,
                    equal_scale_homonuclear_2d_imports,
                ))),
                source,
            )
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
        Acquisition::MassSpec(data) => {
            let source = data.source.clone();
            (
                Dataset::MassSpec(Box::new(crate::state::MassSpecDataset::load(*data))),
                source,
            )
        }
        Acquisition::Xrd(data) => {
            let source = data.source.clone();
            (
                Dataset::Xrd(Box::new(crate::state::XrdDataset::load(*data))),
                source,
            )
        }
        Acquisition::Xps(data) => {
            let source = data.source.clone();
            (
                Dataset::Xps(Box::new(crate::state::XpsDataset::load(*data))),
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
        Dataset::MassSpec(data) => data
            .name
            .clone()
            .unwrap_or_else(|| short_name(&data.run.source)),
        Dataset::Xrd(data) => data
            .name
            .clone()
            .unwrap_or_else(|| short_name(&data.data.source)),
        Dataset::Xps(data) => data
            .name
            .clone()
            .unwrap_or_else(|| short_name(&data.experiment.source)),
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

fn default_binding(dataset: &Dataset) -> DataBinding {
    let fields = match dataset {
        Dataset::Nmr2D(data) if !data.is_true_2d() => ["nmr.stack", "nmr.dosy_map", "nmr.ilt_map"]
            .into_iter()
            .filter_map(|key| data.field_catalog.id_for_key(key))
            .collect::<Vec<_>>(),
        Dataset::Electrophysiology(_) => dataset
            .field_descriptors()
            .into_iter()
            .map(|field| field.id)
            .collect(),
        _ => return DataBinding::single(dataset),
    };
    let mut series = fields
        .into_iter()
        .flat_map(|field| crate::state::SeriesBinding::from_field_all(dataset, field))
        .collect::<Vec<_>>();
    for (index, binding) in series.iter_mut().enumerate() {
        binding.id = crate::state::SeriesId::new(index as u64);
    }
    DataBinding { series }
}

pub fn build_plot_object(
    dataset: &Dataset,
    _dataset_index: usize,
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
    let binding = default_binding(dataset);
    let figure = trace_collection::initial_figure(
        dataset,
        &binding,
        size_mm,
        build_dataset_figure(dataset, &chart, size_mm),
    );
    let viewport = CanvasViewport::from_figure(&figure);
    let panel = PanelMeta::new(dataset_title(dataset), frame.width);
    let axis_overrides = AxisOverrides {
        lock_aspect: matches!(dataset, Dataset::Nmr2D(dataset) if dataset.is_true_2d())
            .then_some(figure.lock_aspect),
        guide_visibility: (matches!(dataset, Dataset::Electrophysiology(_) | Dataset::Nmr2D(_)))
            .then_some(plotx_figure::GuideVisibility::Hide),
        ..AxisOverrides::default()
    };
    let stack = if binding
        .series
        .iter()
        .any(|series| series.source.item.is_some())
    {
        StackSpec {
            mode: StackMode::Offset,
            ..StackSpec::default()
        }
    } else {
        StackSpec::default()
    };
    CanvasObject {
        id,
        name,
        frame,
        locked: false,
        visible: true,
        kind: CanvasObjectKind::Plot(Box::new(PlotObject::new(
            Some(dataset.resource_id()),
            crate::state::SeriesId::new(binding.series.len() as u64),
            binding,
            chart,
            stack,
            AxisProjections::default(),
            axis_overrides,
            figure,
            viewport,
            panel,
        ))),
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

/// Build the canonical initial layout used by GUI insertion, CLI, automation,
/// and export; callers supply only the document-local dataset and canvas identity.
pub fn build_default_canvas_for_dataset(
    dataset: &Dataset,
    dataset_index: usize,
    canvas_name: String,
    size_mm: [f32; 2],
) -> CanvasDocument {
    let has_map_and_force = matches!(dataset, Dataset::Afm(afm) if !afm.data.images.is_empty() && afm.data.forces.is_some());
    let optical_fields = dataset
        .as_mass_spec()
        .map_or_else(Vec::new, mass_spec_layout::optical_fields);
    let has_uv = !optical_fields.is_empty();
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
    let first_height = if has_uv { height / 2.0 } else { height };
    let mut first = build_plot_object(
        dataset,
        dataset_index,
        ObjectFrame::new(0.0, 0.0, first_width, first_height),
        id,
        if has_uv {
            "UV Chromatogram".to_owned()
        } else if matches!(dataset, Dataset::MassSpec(_)) {
            "Total Ion Chromatogram".to_owned()
        } else {
            dataset_title(dataset)
        },
    );
    if let Dataset::MassSpec(mass_spec) = dataset {
        if has_uv {
            mass_spec_layout::configure_fields(
                &mut first,
                dataset,
                &optical_fields,
                "mass_chromatogram",
            );
            if first.plot().is_some() {
                first.name = format!(
                    "UV chromatogram{} — {}",
                    if optical_fields.len() == 1 { "" } else { "s" },
                    optical_fields
                        .iter()
                        .map(|item| item.1.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                );
            }
        } else if first.plot().is_some() {
            first.name = mass_spec.tic_panel_note();
        }
    }
    canvas.objects.push(first);
    canvas
        .create_panel_for_plot(id)
        .expect("the default object is a plot");
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
            if let Some(series) = plot.binding.series.first_mut()
                && let Some(field) = dataset
                    .field_descriptors()
                    .into_iter()
                    .find(|field| field.local_id == "afm.force_curve")
            {
                series.source.field = field.id;
                series.encoding = plotx_figure::SeriesEncoding::default();
            }
            let figure = build_dataset_figure(
                dataset,
                &plot.chart,
                [width / 2.0 / MM_TO_PT, height / MM_TO_PT],
            );
            plot.adopt_rebuilt_figure(figure);
        }
        canvas.objects.push(second);
        canvas
            .create_panel_for_plot(second_id)
            .expect("the AFM companion object is a plot");
    }
    if let Dataset::MassSpec(mass_spec) = dataset
        && has_uv
    {
        let second_id = canvas.allocate_object_id();
        let mut second = build_plot_object(
            dataset,
            dataset_index,
            ObjectFrame::new(0.0, height / 2.0, width, height / 2.0),
            second_id,
            "Total Ion Chromatogram".to_owned(),
        );
        if let CanvasObjectKind::Plot(plot) = &mut second.kind {
            plot.chart.type_id = "mass_chromatogram".to_owned();
            if let Some(series) = plot.binding.series.first_mut()
                && let Some(field) = mass_spec
                    .field_catalog
                    .id_for_key(&crate::state::stream_tic_key(mass_spec.active_stream))
            {
                series.source.field = field;
                series.encoding = plotx_figure::SeriesEncoding::default();
            }
            second.name = mass_spec.tic_panel_note();
            let figure = build_dataset_figure(
                dataset,
                &plot.chart,
                [width / MM_TO_PT, height / 2.0 / MM_TO_PT],
            );
            plot.adopt_rebuilt_figure(figure);
        }
        canvas.objects.push(second);
        canvas
            .create_panel_for_plot(second_id)
            .expect("the mass spectrum companion object is a plot");
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
                mass_spectrometry: None,
                xrd: None,
                xps: None,
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
                mass_spectrometry: None,
                xrd: None,
                xps: None,
            };
        }
        Acquisition::MassSpec(run) => {
            let ms_scan_count = run
                .streams
                .iter()
                .filter(|stream| stream.role == plotx_io::StreamRole::Primary)
                .map(|stream| stream.spectra.len())
                .sum();
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
                    shape: vec![run.streams.len(), ms_scan_count, run.chromatograms.len()],
                },
                domain: "mass_spectrometry".to_owned(),
                warnings: warnings.iter().map(warning_report).collect(),
                electrophysiology: None,
                afm: None,
                mass_spectrometry: Some(MassSpecReport {
                    instrument: run.instrument.clone(),
                    stream_count: run.streams.len(),
                    ms_scan_count,
                    chromatograms: run
                        .chromatograms
                        .iter()
                        .map(|channel| channel.description.clone())
                        .collect(),
                }),
                xrd: None,
                xps: None,
            };
        }
        Acquisition::Xrd(data) => {
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
                    count: 1,
                    shape: vec![data.len()],
                },
                domain: "xrd".to_owned(),
                warnings: warnings.iter().map(warning_report).collect(),
                electrophysiology: None,
                afm: None,
                mass_spectrometry: None,
                xrd: Some(XrdReport {
                    instrument: data.instrument.clone(),
                    target: data.target.clone(),
                    wavelength_angstrom: data.wavelength_angstrom,
                    two_theta_range_deg: [
                        data.two_theta_deg.first().copied().unwrap_or(0.0),
                        data.two_theta_deg.last().copied().unwrap_or(0.0),
                    ],
                    point_count: data.len(),
                }),
                xps: None,
            };
        }
        Acquisition::Xps(experiment) => {
            return xps::inspection_report(format, provenance, warnings, experiment);
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
        mass_spectrometry: None,
        xrd: None,
        xps: None,
    }
}

pub(super) fn warning_report(warning: &LoadWarning) -> WarningReport {
    let code = match warning.code {
        LoadWarningCode::ArchiveEntryFailed => "archive-entry-failed",
        LoadWarningCode::OptionalImaginaryMissing => "optional-imaginary-missing",
        LoadWarningCode::MissingStimulus => "missing-stimulus",
        LoadWarningCode::InvalidMetadata => "invalid-metadata",
        LoadWarningCode::MissingCalibration => "missing-calibration",
        LoadWarningCode::MissingCompanion => "missing-companion",
        LoadWarningCode::CompanionMismatch => "companion-mismatch",
        LoadWarningCode::OptionalChannelSkipped => "optional-channel-skipped",
        LoadWarningCode::UnsupportedFunction => "unsupported-function",
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
#[path = "workflow_tests.rs"]
mod tests;
